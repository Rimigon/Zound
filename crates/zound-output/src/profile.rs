//! Сессионные профили — сериализация/десериализация состояния
//! пользователя между запусками.
//!
//! В файле хранится: список добавленных устройств, их per-device volume /
//! mute / balance / latency, плюс master gain / mute. Identification идёт
//! по имени устройства (см. ограничение в platform/output.rs).
//!
//! Формат — JSON, читаемый. Поле `version` нужно для будущей миграции:
//! пока всегда 1.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Текущая версия формата. Если будем менять breaking — поднимаем и
/// добавляем миграцию в [`SessionProfile::load_from`].
pub const PROFILE_VERSION: u32 = 1;

/// Состояние одного output-устройства в профиле.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DevicePreset {
    pub name: String,
    #[serde(default = "default_volume")]
    pub volume: f32,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub balance: f32,
    #[serde(default = "default_latency_ms")]
    pub latency_ms: u64,
}

fn default_volume() -> f32 {
    1.0
}
fn default_latency_ms() -> u64 {
    20
}

impl DevicePreset {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            volume: default_volume(),
            muted: false,
            balance: 0.0,
            latency_ms: default_latency_ms(),
        }
    }
}

/// Полная сессия Zound. Сохраняется в `<app_data>/session.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionProfile {
    pub version: u32,
    #[serde(default)]
    pub devices: Vec<DevicePreset>,
    #[serde(default = "default_master_gain")]
    pub master_gain: f32,
    #[serde(default)]
    pub master_muted: bool,
}

fn default_master_gain() -> f32 {
    1.0
}

impl Default for SessionProfile {
    fn default() -> Self {
        Self {
            version: PROFILE_VERSION,
            devices: Vec::new(),
            master_gain: default_master_gain(),
            master_muted: false,
        }
    }
}

impl SessionProfile {
    /// Прочитать профиль из файла. Возвращает `Ok(None)`, если файла нет
    /// (первый запуск). Любая другая ошибка (парс, IO) — `Err`.
    pub fn load_from(path: &Path) -> Result<Option<Self>, String> {
        match std::fs::read_to_string(path) {
            Ok(s) => {
                let profile: SessionProfile =
                    serde_json::from_str(&s).map_err(|e| format!("session profile parse: {e}"))?;
                if profile.version != PROFILE_VERSION {
                    tracing::warn!(
                        version = profile.version,
                        expected = PROFILE_VERSION,
                        "session profile version mismatch — using as-is"
                    );
                }
                Ok(Some(profile))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("session profile read: {e}")),
        }
    }

    /// Записать профиль в файл атомарно: сначала во временный файл, потом
    /// rename. Иначе при крэше посередине записи остаётся пустой/обрезанный
    /// JSON и при следующем старте session не восстановится.
    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("session profile mkdir: {e}"))?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("session profile serialize: {e}"))?;

        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| format!("session profile write tmp: {e}"))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("session profile rename: {e}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_default_profile() {
        let p = SessionProfile::default();
        let s = serde_json::to_string(&p).unwrap();
        let back: SessionProfile = serde_json::from_str(&s).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn roundtrip_with_devices() {
        let mut p = SessionProfile::default();
        p.devices.push(DevicePreset {
            name: "Speakers".into(),
            volume: 0.5,
            muted: true,
            balance: -0.25,
            latency_ms: 80,
        });
        p.master_gain = 0.7;
        p.master_muted = false;

        let s = serde_json::to_string(&p).unwrap();
        let back: SessionProfile = serde_json::from_str(&s).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn missing_fields_use_defaults() {
        let s = r#"{"version":1,"devices":[{"name":"X"}]}"#;
        let p: SessionProfile = serde_json::from_str(s).unwrap();
        assert_eq!(p.devices.len(), 1);
        assert_eq!(p.devices[0].volume, 1.0);
        assert_eq!(p.devices[0].latency_ms, 20);
        assert!(!p.devices[0].muted);
        assert_eq!(p.master_gain, 1.0);
    }

    #[test]
    fn save_load_atomic() {
        let dir = std::env::temp_dir().join(format!("zound-test-{}", std::process::id()));
        let path = dir.join("session.json");
        let _ = std::fs::remove_dir_all(&dir);

        // Несуществующий файл → None.
        std::fs::create_dir_all(&dir).unwrap();
        assert!(SessionProfile::load_from(&path).unwrap().is_none());

        // Сохранили — прочитали обратно.
        let mut p = SessionProfile::default();
        p.devices.push(DevicePreset::new("A"));
        p.save_to(&path).unwrap();
        let loaded = SessionProfile::load_from(&path).unwrap().unwrap();
        assert_eq!(p, loaded);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
