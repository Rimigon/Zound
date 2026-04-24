use std::fmt;

/// Уникальный идентификатор устройства в рамках одного backend-а.
/// Строковый, чтобы не завязываться на конкретный тип ID (WASAPI endpoint id,
/// CoreAudio AudioObjectID, PipeWire node id — всё плоский UTF-8).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceId(pub String);

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for DeviceId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl From<String> for DeviceId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    /// Физически подключённый провод/внутренний динамик.
    Wired,
    /// Bluetooth (A2DP или LE Audio).
    Bluetooth,
    /// Виртуальное / loopback-устройство.
    Virtual,
    /// Сетевой клиент (пост-MVP).
    Network,
    /// Тип неизвестен — backend не смог определить.
    Unknown,
}

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub id: DeviceId,
    /// Человеко-читаемое имя (как показывает ОС).
    pub name: String,
    pub kind: DeviceKind,
    /// Частота сэмплирования, которую устройство умеет / использует сейчас.
    pub sample_rate: u32,
    pub channels: u16,
    /// Является ли дефолтным устройством системы.
    pub is_default: bool,
}
