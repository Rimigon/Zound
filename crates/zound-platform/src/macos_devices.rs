//! Прямой обход CoreAudio для энумерации устройств на macOS.
//!
//! Зачем: `cpal 0.15` фильтрует `output_devices()` через
//! `default_output_config().is_ok()`. Этот вызов на macOS падает для
//! устройств, которые сейчас не активны (DisplayPort-выходы без
//! текущего стрима, Built-in Speakers, когда выбран другой default,
//! и т. п.). В результате видны лишь USB/виртуальные выходы.
//!
//! Здесь мы спрашиваем CoreAudio напрямую: «дай все AudioObjectID,
//! из них возьми те, у которых есть каналы в нужном scope (Output/
//! Input)». Имя берём как `CFStringRef` через `kAudioObjectPropertyName`
//! — Unicode-устройства («Динамики MacBook Pro») остаются читаемыми.

use std::os::raw::c_void;

use core_foundation_sys::base::{CFRelease, CFTypeRef};
use core_foundation_sys::string::{
    kCFStringEncodingUTF8, CFStringGetCString, CFStringGetLength,
    CFStringGetMaximumSizeForEncoding, CFStringRef,
};
use coreaudio_sys::{
    kAudioDevicePropertyNominalSampleRate, kAudioDevicePropertyStreamConfiguration,
    kAudioHardwarePropertyDefaultInputDevice, kAudioHardwarePropertyDefaultOutputDevice,
    kAudioHardwarePropertyDevices, kAudioObjectPropertyName, kAudioObjectPropertyScopeGlobal,
    kAudioObjectPropertyScopeInput, kAudioObjectPropertyScopeOutput, kAudioObjectSystemObject,
    AudioBufferList, AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectID,
    AudioObjectPropertyAddress, AudioObjectPropertyElement, AudioObjectPropertyScope,
};

use zound_core::{DeviceId, DeviceInfo, DeviceKind, Error, Result, DEFAULT_SAMPLE_RATE};

/// kAudioObjectPropertyElementMaster (deprecated) == kAudioObjectPropertyElementMain == 0.
/// Хардкодим, чтобы не зависеть от того, какой алиас экспортирует bindgen
/// в конкретной версии coreaudio-sys.
const ELEMENT_MAIN: AudioObjectPropertyElement = 0;

pub fn enumerate_outputs() -> Result<Vec<DeviceInfo>> {
    enumerate(true)
}

pub fn enumerate_inputs() -> Result<Vec<DeviceInfo>> {
    enumerate(false)
}

/// Найти AudioObjectID устройства по его человеко-читаемому имени.
/// Нужно, чтобы `output.rs` мог достать частоту/каналы для тех
/// устройств, у которых cpal-овский `default_output_config` падает.
pub fn find_device_id_by_name(name: &str) -> Option<AudioObjectID> {
    let ids = list_all_device_ids().ok()?;
    for id in ids {
        if device_name(id).as_deref() == Some(name) {
            return Some(id);
        }
    }
    None
}

/// (sample_rate, channels) для output-стороны устройства. Возвращает
/// None, если устройство не имеет output-каналов вовсе.
pub fn output_format(device_id: AudioObjectID) -> Option<(u32, u16)> {
    let ch = device_channels(device_id, true);
    if ch == 0 {
        return None;
    }
    let sr = device_sample_rate(device_id).unwrap_or(DEFAULT_SAMPLE_RATE);
    Some((sr, ch))
}

/// Имя текущего системного дефолтного output. Нужно, чтобы блокировать
/// добавление этого устройства как Zound-output (иначе звук задвоится:
/// ScreenCaptureKit тапит весь системный звук, и системный дефолт уже
/// его проигрывает «своими» руками).
pub fn default_output_device_name() -> Option<String> {
    let id = default_device_id(true)?;
    device_name(id)
}

fn enumerate(is_output: bool) -> Result<Vec<DeviceInfo>> {
    let ids = list_all_device_ids()?;
    let default_id = default_device_id(is_output);

    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let channels = device_channels(id, is_output);
        if channels == 0 {
            continue;
        }
        let Some(name) = device_name(id) else {
            continue;
        };
        let sample_rate = device_sample_rate(id).unwrap_or(DEFAULT_SAMPLE_RATE);
        out.push(DeviceInfo {
            id: DeviceId::from(name.clone()),
            name,
            kind: DeviceKind::Unknown,
            sample_rate,
            channels,
            is_default: Some(id) == default_id,
        });
    }
    Ok(out)
}

fn list_all_device_ids() -> Result<Vec<AudioObjectID>> {
    let address = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDevices,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: ELEMENT_MAIN,
    };
    let mut size: u32 = 0;
    let status = unsafe {
        AudioObjectGetPropertyDataSize(
            kAudioObjectSystemObject,
            &address,
            0,
            std::ptr::null(),
            &mut size,
        )
    };
    if status != 0 {
        return Err(Error::Backend(format!(
            "AudioObjectGetPropertyDataSize(Devices): OSStatus {status}"
        )));
    }
    let count = size as usize / std::mem::size_of::<AudioObjectID>();
    if count == 0 {
        return Ok(Vec::new());
    }
    let mut ids = vec![0 as AudioObjectID; count];
    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject,
            &address,
            0,
            std::ptr::null(),
            &mut size,
            ids.as_mut_ptr() as *mut c_void,
        )
    };
    if status != 0 {
        return Err(Error::Backend(format!(
            "AudioObjectGetPropertyData(Devices): OSStatus {status}"
        )));
    }
    Ok(ids)
}

fn default_device_id(is_output: bool) -> Option<AudioObjectID> {
    let selector = if is_output {
        kAudioHardwarePropertyDefaultOutputDevice
    } else {
        kAudioHardwarePropertyDefaultInputDevice
    };
    let address = AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: ELEMENT_MAIN,
    };
    let mut id: AudioObjectID = 0;
    let mut size = std::mem::size_of::<AudioObjectID>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject,
            &address,
            0,
            std::ptr::null(),
            &mut size,
            &mut id as *mut AudioObjectID as *mut c_void,
        )
    };
    if status == 0 && id != 0 {
        Some(id)
    } else {
        None
    }
}

fn device_channels(device_id: AudioObjectID, is_output: bool) -> u16 {
    let scope: AudioObjectPropertyScope = if is_output {
        kAudioObjectPropertyScopeOutput
    } else {
        kAudioObjectPropertyScopeInput
    };
    let address = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyStreamConfiguration,
        mScope: scope,
        mElement: ELEMENT_MAIN,
    };
    let mut size: u32 = 0;
    let status = unsafe {
        AudioObjectGetPropertyDataSize(device_id, &address, 0, std::ptr::null(), &mut size)
    };
    if status != 0 || (size as usize) < std::mem::size_of::<u32>() {
        return 0;
    }
    // Vec<u64> вместо Vec<u8>: AudioBufferList требует 8-байтового
    // выравнивания (содержит указатель), а `Vec<u8>` его не гарантирует.
    let n_u64 = size.div_ceil(8) as usize;
    let mut buffer = vec![0u64; n_u64];
    let status = unsafe {
        AudioObjectGetPropertyData(
            device_id,
            &address,
            0,
            std::ptr::null(),
            &mut size,
            buffer.as_mut_ptr() as *mut c_void,
        )
    };
    if status != 0 {
        return 0;
    }
    // AudioBufferList: { u32 mNumberBuffers; AudioBuffer mBuffers[mNumberBuffers]; }
    // На 64-бит между mNumberBuffers и mBuffers есть 4 байта паддинга
    // (AudioBuffer выровнен по 8 из-за указателя). repr(C) bindgen-а
    // даёт корректный offset через `mBuffers.as_ptr()`.
    let abl = buffer.as_ptr() as *const AudioBufferList;
    let n = unsafe { (*abl).mNumberBuffers } as usize;
    let buffers = unsafe { (*abl).mBuffers.as_ptr() };
    let mut total: u16 = 0;
    for i in 0..n {
        let buf = unsafe { &*buffers.add(i) };
        total = total.saturating_add(buf.mNumberChannels as u16);
    }
    total
}

fn device_sample_rate(device_id: AudioObjectID) -> Option<u32> {
    let address = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyNominalSampleRate,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: ELEMENT_MAIN,
    };
    let mut sr: f64 = 0.0;
    let mut size = std::mem::size_of::<f64>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            device_id,
            &address,
            0,
            std::ptr::null(),
            &mut size,
            &mut sr as *mut f64 as *mut c_void,
        )
    };
    if status == 0 && sr > 0.0 {
        Some(sr.round() as u32)
    } else {
        None
    }
}

fn device_name(device_id: AudioObjectID) -> Option<String> {
    let address = AudioObjectPropertyAddress {
        mSelector: kAudioObjectPropertyName,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: ELEMENT_MAIN,
    };
    let mut name_ref: CFStringRef = std::ptr::null();
    let mut size = std::mem::size_of::<CFStringRef>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            device_id,
            &address,
            0,
            std::ptr::null(),
            &mut size,
            &mut name_ref as *mut CFStringRef as *mut c_void,
        )
    };
    if status != 0 || name_ref.is_null() {
        return None;
    }
    let result = cfstring_to_string(name_ref);
    unsafe { CFRelease(name_ref as CFTypeRef) };
    result
}

fn cfstring_to_string(s: CFStringRef) -> Option<String> {
    if s.is_null() {
        return None;
    }
    let len_chars = unsafe { CFStringGetLength(s) };
    let max_bytes =
        unsafe { CFStringGetMaximumSizeForEncoding(len_chars, kCFStringEncodingUTF8) } + 1;
    if max_bytes <= 0 {
        return None;
    }
    let mut buffer = vec![0i8; max_bytes as usize];
    let success =
        unsafe { CFStringGetCString(s, buffer.as_mut_ptr(), max_bytes, kCFStringEncodingUTF8) };
    if success == 0 {
        return None;
    }
    let bytes: Vec<u8> = buffer
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8(bytes).ok()
}
