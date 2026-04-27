use zound_core::{DeviceInfo, Result};

/// Платформенный backend — единый интерфейс над WASAPI / CoreAudio /
/// PipeWire. Реализации запрашивают у ОС список устройств и дают
/// низкоуровневые операции для capture/output (через свободные функции
/// в [`crate::capture`] / [`crate::output`], не методы трейта — так
/// удобнее переиспользовать одну реализацию между платформами).
///
/// На MVP единственная реализация — [`crate::CpalBackend`] поверх крейта
/// `cpal`. Расширенные нативные backend-ы (per-app capture, системный
/// loopback с управлением процессом-источником, ASIO/JACK) могут быть
/// добавлены отдельными типами, реализующими этот же трейт.
///
/// # Контракт
///
/// - Реализация **должна** быть `Send + Sync` — `AudioEngine` хранит
///   её как `Arc<dyn AudioBackend>` или эквивалент и обращается из
///   нескольких потоков.
/// - Перечисление устройств — операция, которая может быть медленной
///   (десятки миллисекунд на macOS), поэтому UI вызывает её только при
///   явных событиях и периодическом refresh, не на каждый кадр.
/// - Имена устройств в `DeviceInfo::name` могут совпадать у разных
///   физических endpoint-ов; уникальность даёт `DeviceInfo::id`, но в
///   MVP мы используем имя как ключ — см. ограничение в
///   `output::open_output_by_name`.
pub trait AudioBackend: Send + Sync {
    /// Имя backend-а для логов и UI. Например: `"cpal/wasapi"`,
    /// `"cpal/coreaudio"`, `"cpal/alsa"`.
    fn name(&self) -> &'static str;

    /// Перечислить доступные **output**-устройства (динамики, наушники,
    /// HDMI/DisplayPort выходы). Включает default-устройство (с
    /// `is_default = true`).
    fn enumerate_outputs(&self) -> Result<Vec<DeviceInfo>>;

    /// Перечислить доступные **input**-устройства (микрофоны, line-in,
    /// loopback-источники на Windows). Используется UI-настройкой
    /// «показать все устройства».
    fn enumerate_inputs(&self) -> Result<Vec<DeviceInfo>>;

    /// Дефолтное output-устройство системы. Обычно совпадает с одним из
    /// элементов [`enumerate_outputs`](Self::enumerate_outputs), у
    /// которого `is_default = true`.
    fn default_output(&self) -> Result<DeviceInfo>;
}
