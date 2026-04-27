use std::collections::HashMap;

use cpal::traits::{DeviceTrait, HostTrait};
use zound_core::{DeviceId, DeviceInfo, DeviceKind, Error, Result};

use crate::backend::AudioBackend;

/// Backend поверх `cpal`. Кроссплатформенный, умеет:
/// - перечислять устройства ввода/вывода;
/// - открывать output-потоки;
/// - (на Windows) открывать loopback-потоки.
///
/// Ограничения: per-app capture и детальная работа с BT-кодеками — вне
/// `cpal`, будут отдельными backend-ами.
pub struct CpalBackend {
    host: cpal::Host,
}

impl CpalBackend {
    pub fn new() -> Self {
        Self {
            host: cpal::default_host(),
        }
    }

    fn describe(
        &self,
        device: &cpal::Device,
        is_default: bool,
        endpoint_lookup: Option<&HashMap<String, String>>,
    ) -> DeviceInfo {
        let name = device.name().unwrap_or_else(|_| "unknown".to_string());
        // cpal не даёт точный тип транспорта; маркируем всё как Unknown,
        // backend-ы поверх нативных API смогут различать Wired / Bluetooth.
        let (sample_rate, channels) = device
            .default_output_config()
            .or_else(|_| device.default_input_config())
            .map(|c| (c.sample_rate().0, c.channels()))
            .unwrap_or((
                zound_core::DEFAULT_SAMPLE_RATE,
                zound_core::DEFAULT_CHANNELS,
            ));

        let endpoint_id = endpoint_lookup.and_then(|m| m.get(&name).cloned());

        DeviceInfo {
            id: DeviceId::from(name.clone()),
            name,
            kind: DeviceKind::Unknown,
            sample_rate,
            channels,
            is_default,
            endpoint_id,
        }
    }
}

impl Default for CpalBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioBackend for CpalBackend {
    fn name(&self) -> &'static str {
        "cpal"
    }

    fn enumerate_outputs(&self) -> Result<Vec<DeviceInfo>> {
        // На macOS обходим cpal: его фильтр через `default_output_config`
        // выкидывает idle-устройства (DisplayPort, Built-in без активного
        // стрима). Спрашиваем CoreAudio напрямую — см. `macos_devices`.
        #[cfg(target_os = "macos")]
        {
            crate::macos_devices::enumerate_outputs()
        }

        #[cfg(not(target_os = "macos"))]
        {
            let default_name = self
                .host
                .default_output_device()
                .and_then(|d| d.name().ok());

            let devices = self
                .host
                .output_devices()
                .map_err(|e| Error::Backend(e.to_string()))?;

            #[cfg(target_os = "windows")]
            let endpoint_lookup = Some(crate::windows_endpoints::endpoint_id_map());
            #[cfg(not(target_os = "windows"))]
            let endpoint_lookup: Option<HashMap<String, String>> = None;

            Ok(devices
                // На Linux PipeWire/Pulse cpal иногда отдаёт monitor-source
                // в `output_devices()`. Это виртуальный input от существующего
                // sink — играть в него нельзя, и в UI он только мешает.
                .filter(|d| {
                    #[cfg(target_os = "linux")]
                    {
                        !d.name().map(|n| n.ends_with(".monitor")).unwrap_or(false)
                    }
                    #[cfg(not(target_os = "linux"))]
                    {
                        let _ = d;
                        true
                    }
                })
                .map(|d| {
                    let is_default = default_name
                        .as_deref()
                        .zip(d.name().ok().as_deref())
                        .map(|(a, b)| a == b)
                        .unwrap_or(false);
                    self.describe(&d, is_default, endpoint_lookup.as_ref())
                })
                .collect())
        }
    }

    fn enumerate_inputs(&self) -> Result<Vec<DeviceInfo>> {
        #[cfg(target_os = "macos")]
        {
            crate::macos_devices::enumerate_inputs()
        }

        #[cfg(not(target_os = "macos"))]
        {
            let default_name = self.host.default_input_device().and_then(|d| d.name().ok());

            let devices = self
                .host
                .input_devices()
                .map_err(|e| Error::Backend(e.to_string()))?;

            // Endpoint id есть только у render-устройств — на Windows
            // input — отдельный capture-flow в IMMDeviceEnumerator. Пока не
            // используется в profile (мы храним только outputs), — поэтому
            // оставляем None.
            let endpoint_lookup: Option<HashMap<String, String>> = None;

            Ok(devices
                .map(|d| {
                    let is_default = default_name
                        .as_deref()
                        .zip(d.name().ok().as_deref())
                        .map(|(a, b)| a == b)
                        .unwrap_or(false);
                    self.describe(&d, is_default, endpoint_lookup.as_ref())
                })
                .collect())
        }
    }

    fn default_output(&self) -> Result<DeviceInfo> {
        let device = self
            .host
            .default_output_device()
            .ok_or_else(|| Error::Backend("no default output device".into()))?;
        #[cfg(target_os = "windows")]
        let endpoint_lookup = Some(crate::windows_endpoints::endpoint_id_map());
        #[cfg(not(target_os = "windows"))]
        let endpoint_lookup: Option<HashMap<String, String>> = None;
        Ok(self.describe(&device, true, endpoint_lookup.as_ref()))
    }
}
