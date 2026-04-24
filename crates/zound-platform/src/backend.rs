use zound_core::{DeviceInfo, Result};

/// Платформенный backend. Единый интерфейс над WASAPI / CoreAudio / PipeWire.
///
/// На MVP единственная реализация — [`crate::CpalBackend`]. Она даёт базовые
/// возможности; расширенные (per-app capture, системный loopback с
/// управлением процессом-источником) будут в отдельных backend-ах
/// в следующих итерациях.
pub trait AudioBackend: Send + Sync {
    /// Имя backend-а (для логов/UI). Например: "cpal/wasapi".
    fn name(&self) -> &'static str;

    /// Перечислить доступные output-устройства.
    fn enumerate_outputs(&self) -> Result<Vec<DeviceInfo>>;

    /// Перечислить доступные input-устройства (включая loopback на Windows).
    fn enumerate_inputs(&self) -> Result<Vec<DeviceInfo>>;

    /// Дефолтное output-устройство системы.
    fn default_output(&self) -> Result<DeviceInfo>;
}
