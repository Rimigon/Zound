//! Output-поток на конкретное устройство.
//!
//! Держит cpal-стрим + producer ringbuf-а, в который AudioEngine пушит
//! сэмплы внутреннего формата (f32, 48 kHz, stereo). Внутри callback-а
//! они конвертируются в формат устройства.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use parking_lot::Mutex;
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};
use zound_core::{DeviceId, Error, Result};

use crate::sample_convert::FromF32;

/// Громкость устройства — атомарная, чтобы менять в любое время без лока.
/// Храним f32 как u32-биты.
#[derive(Clone)]
pub struct AtomicVolume(Arc<AtomicU32>);

impl AtomicVolume {
    pub fn new(v: f32) -> Self {
        Self(Arc::new(AtomicU32::new(v.to_bits())))
    }
    pub fn set(&self, v: f32) {
        self.0.store(v.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
}

impl Default for AtomicVolume {
    fn default() -> Self {
        Self::new(1.0)
    }
}

pub struct OutputSink {
    _stream: Stream,
    pub device_id: DeviceId,
    pub device_name: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub producer: HeapProd<f32>,
    pub volume: AtomicVolume,
    /// Заглушка звука. При `true` callback пишет тишину независимо от volume.
    /// Шарится с UI через `AudioEngine::set_muted`.
    pub muted: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
pub struct OutputOpts {
    /// Ёмкость ringbuf-а в interleaved-сэмплах.
    pub buffer_samples: usize,
    /// Предзаполнение тишиной (для компенсации задержки). В сэмплах.
    pub prefill_silence_samples: usize,
}

impl Default for OutputOpts {
    fn default() -> Self {
        Self {
            buffer_samples: 96_000,
            prefill_silence_samples: 0,
        }
    }
}

/// Открыть output на устройстве по его имени (то, что cpal отдаёт через
/// `DeviceTrait::name`). Мы используем имя как ID в MVP — см. `cpal_backend.rs`.
pub fn open_output_by_name(device_name: &str, opts: OutputOpts) -> Result<OutputSink> {
    let host = cpal::default_host();
    // На macOS используем `host.devices()` (без фильтра), потому что
    // cpal-овский `output_devices()` выкидывает idle-устройства
    // (DisplayPort, Built-in без активного стрима) — а мы их теперь
    // показываем в списке через прямой обход CoreAudio.
    #[cfg(target_os = "macos")]
    let mut devices = host
        .devices()
        .map_err(|e| Error::Backend(format!("devices: {e}")))?;
    #[cfg(not(target_os = "macos"))]
    let mut devices = host
        .output_devices()
        .map_err(|e| Error::Backend(format!("output_devices: {e}")))?;

    let device = devices
        .find(|d| d.name().map(|n| n == device_name).unwrap_or(false))
        .ok_or_else(|| Error::DeviceNotFound(device_name.to_string()))?;

    let (sample_rate, channels, format) = resolve_output_config(&device, device_name)?;

    let config = cpal::StreamConfig {
        channels,
        sample_rate: cpal::SampleRate(sample_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    let rb = HeapRb::<f32>::new(opts.buffer_samples);
    let (mut producer, consumer) = rb.split();

    if opts.prefill_silence_samples > 0 {
        let zeros = vec![0.0_f32; opts.prefill_silence_samples];
        let pushed = producer.push_slice(&zeros);
        tracing::debug!(pushed, "prefilled output with silence");
    }

    let consumer = Arc::new(Mutex::new(consumer));
    let volume = AtomicVolume::new(1.0);
    let muted = Arc::new(AtomicBool::new(false));

    let stream = build_output_stream(
        &device,
        &config,
        format,
        consumer,
        volume.clone(),
        muted.clone(),
    )?;
    stream
        .play()
        .map_err(|e| Error::Backend(format!("stream.play: {e}")))?;

    let resolved_name = device.name().unwrap_or_else(|_| device_name.to_string());

    tracing::info!(%sample_rate, %channels, ?format, name = %resolved_name, "output started");

    Ok(OutputSink {
        _stream: stream,
        device_id: DeviceId::from(resolved_name.clone()),
        device_name: resolved_name,
        sample_rate,
        channels,
        producer,
        volume,
        muted,
    })
}

/// Достаём (sample_rate, channels, sample_format) для устройства.
///
/// Обычно через `cpal::Device::default_output_config()`. На macOS он падает
/// для idle-устройств (DisplayPort, Built-in, когда выбран другой default) —
/// в этом случае идём в CoreAudio FFI и берём nominal sample rate +
/// количество каналов из stream configuration. Sample format фиксируем
/// `F32` — CoreAudio внутри сделает конверсию.
///
/// `pub(crate)` — нужен `test_signal.rs`, чтобы открывать второй stream
/// на устройстве с теми же параметрами.
pub(crate) fn resolve_output_config_pub(
    device: &cpal::Device,
    device_name: &str,
) -> Result<(u32, u16, SampleFormat)> {
    resolve_output_config(device, device_name)
}

fn resolve_output_config(
    device: &cpal::Device,
    device_name: &str,
) -> Result<(u32, u16, SampleFormat)> {
    match device.default_output_config() {
        Ok(supported) => Ok((
            supported.sample_rate().0,
            supported.channels(),
            supported.sample_format(),
        )),
        Err(err) => {
            #[cfg(target_os = "macos")]
            {
                if let Some(id) = crate::macos_devices::find_device_id_by_name(device_name) {
                    if let Some((sr, ch)) = crate::macos_devices::output_format(id) {
                        tracing::debug!(
                            device = %device_name, sr, ch,
                            "default_output_config failed; CoreAudio FFI fallback"
                        );
                        return Ok((sr, ch, SampleFormat::F32));
                    }
                }
            }
            let _ = device_name;
            Err(Error::Backend(format!("default_output_config: {err}")))
        }
    }
}

fn build_output_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    format: SampleFormat,
    consumer: Arc<Mutex<HeapCons<f32>>>,
    volume: AtomicVolume,
    muted: Arc<AtomicBool>,
) -> Result<Stream> {
    let err_cb = |e| tracing::error!(?e, "output stream error");

    macro_rules! build {
        ($ty:ty) => {{
            let cons = consumer.clone();
            let vol = volume.clone();
            let mute = muted.clone();
            device
                .build_output_stream(
                    config,
                    move |data: &mut [$ty], _info: &cpal::OutputCallbackInfo| {
                        // Mute-проверка одна на чанк: если стоит — gain=0,
                        // итог тот же (тишина), без branch'ей внутри loop.
                        let gain = if mute.load(Ordering::Relaxed) {
                            0.0
                        } else {
                            vol.get()
                        };
                        fill_output::<$ty>(cons.clone(), gain, data);
                    },
                    err_cb,
                    None,
                )
                .map_err(|e| Error::Backend(format!("build_output_stream: {e}")))
        }};
    }
    match format {
        SampleFormat::F32 => build!(f32),
        SampleFormat::I16 => build!(i16),
        SampleFormat::U16 => build!(u16),
        SampleFormat::I32 => build!(i32),
        other => Err(Error::Backend(format!(
            "unsupported output format: {other}"
        ))),
    }
}

fn fill_output<T: FromF32>(consumer: Arc<Mutex<HeapCons<f32>>>, gain: f32, dst: &mut [T]) {
    const CHUNK: usize = 1024;
    let mut tmp = [0.0_f32; CHUNK];
    let mut written = 0;
    {
        let mut cons = consumer.lock();
        while written < dst.len() {
            let n = (dst.len() - written).min(CHUNK);
            let popped = cons.pop_slice(&mut tmp[..n]);
            if popped == 0 {
                break;
            }
            for i in 0..popped {
                dst[written + i] = T::from_f32_clamped(tmp[i] * gain);
            }
            written += popped;
        }
    }
    if written < dst.len() {
        // Underrun — заполняем тишиной. Статично, чтобы не логировать часто.
        static UNDERRUN_COUNT: AtomicU32 = AtomicU32::new(0);
        let c = UNDERRUN_COUNT.fetch_add(1, Ordering::Relaxed);
        if c % 100 == 0 {
            tracing::warn!(
                missing = dst.len() - written,
                total = c + 1,
                "output underrun"
            );
        }
        let silence = T::from_f32_clamped(0.0);
        for s in &mut dst[written..] {
            *s = silence;
        }
    }
}
