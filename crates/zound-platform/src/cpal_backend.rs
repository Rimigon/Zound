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

    fn describe(&self, device: &cpal::Device, is_default: bool) -> DeviceInfo {
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

        DeviceInfo {
            id: DeviceId::from(name.clone()),
            name,
            kind: DeviceKind::Unknown,
            sample_rate,
            channels,
            is_default,
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
            return crate::macos_devices::enumerate_outputs();
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

            Ok(devices
                .map(|d| {
                    let is_default = default_name
                        .as_deref()
                        .zip(d.name().ok().as_deref())
                        .map(|(a, b)| a == b)
                        .unwrap_or(false);
                    self.describe(&d, is_default)
                })
                .collect())
        }
    }

    fn enumerate_inputs(&self) -> Result<Vec<DeviceInfo>> {
        #[cfg(target_os = "macos")]
        {
            return crate::macos_devices::enumerate_inputs();
        }

        #[cfg(not(target_os = "macos"))]
        {
            let default_name = self.host.default_input_device().and_then(|d| d.name().ok());

            let devices = self
                .host
                .input_devices()
                .map_err(|e| Error::Backend(e.to_string()))?;

            Ok(devices
                .map(|d| {
                    let is_default = default_name
                        .as_deref()
                        .zip(d.name().ok().as_deref())
                        .map(|(a, b)| a == b)
                        .unwrap_or(false);
                    self.describe(&d, is_default)
                })
                .collect())
        }
    }

    fn default_output(&self) -> Result<DeviceInfo> {
        let device = self
            .host
            .default_output_device()
            .ok_or_else(|| Error::Backend("no default output device".into()))?;
        Ok(self.describe(&device, true))
    }
}
