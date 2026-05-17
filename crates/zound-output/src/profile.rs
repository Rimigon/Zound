//! Сессионные профили — сериализация/десериализация состояния
//! пользователя между запусками.
//!
//! В файле хранится: список добавленных устройств, их per-device volume /
//! mute / balance / latency / EQ, плюс master gain / mute. Привязка к
//! устройству идёт по `endpoint_id` (стабильный backend-id, переживающий
//! переименование), с fallback на `name` — см. матчинг в
//! `platform::open_output_by_endpoint`.
//!
//! Формат — JSON, читаемый. Поле `version` нужно для будущей миграции:
//! пока всегда 1; новые поля добавляются с `#[serde(default)]`, поэтому
//! старые профили читаются без миграции.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Текущая версия формата. Если будем менять breaking — поднимаем и
/// добавляем миграцию в [`SessionProfile::load_from`].
pub const PROFILE_VERSION: u32 = 1;

/// Состояние одного output-устройства в профиле.
///
/// Используется и в Tauri-командах (для обмена с фронтом), и в
/// сериализации session.json: одна структура с
/// `serde(rename_all = "camelCase")` снимает дублирование, которое было
/// в `app/src/commands.rs::ProfileDeviceDto`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DevicePreset {
    pub name: String,
    /// Стабильный backend-id (WASAPI endpoint, CoreAudio AudioObjectID,
    /// и т.п.). При восстановлении профиля приоритетный матчинг идёт по
    /// нему; если backend на текущей системе вернул другой id или вообще
    /// не вернул — fallback на сравнение по `name`.
    #[serde(default)]
    pub endpoint_id: Option<String>,
    #[serde(default = "default_volume")]
    pub volume: f32,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub balance: f32,
    #[serde(default = "default_latency_ms")]
    pub latency_ms: u64,
    /// Усиление НЧ-полосы (low-shelf 100 Hz) в dB, диапазон ±12.
    #[serde(default)]
    pub eq_low_db: f32,
    /// Усиление СЧ (peak 1 kHz).
    #[serde(default)]
    pub eq_mid_db: f32,
    /// Усиление ВЧ (high-shelf 8 kHz).
    #[serde(default)]
    pub eq_high_db: f32,
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
            endpoint_id: None,
            volume: default_volume(),
            muted: false,
            balance: 0.0,
            latency_ms: default_latency_ms(),
            eq_low_db: 0.0,
            eq_mid_db: 0.0,
            eq_high_db: 0.0,
        }
    }
}

/// Полная сессия Zound. Сохраняется в `<app_data>/session.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionProfile {
    pub version: u32,
    #[serde(default)]
    pub devices: Vec<DevicePreset>,
    #[serde(default = "default_master_gain")]
    pub master_gain: f32,
    #[serde(default)]
    pub master_muted: bool,
    /// Алиасы устройств: endpoint_id → пользовательское имя. Применяются
    /// только при отображении в UI; системное имя устройства не меняется.
    /// Поле опциональное — старые профили без алиасов читаются как есть.
    #[serde(default)]
    pub device_aliases: BTreeMap<String, String>,
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
            device_aliases: BTreeMap::new(),
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
            endpoint_id: Some("wasapi:{0.0.0.00000000}.{abc}".into()),
            volume: 0.5,
            muted: true,
            balance: -0.25,
            latency_ms: 80,
            eq_low_db: 3.0,
            eq_mid_db: -2.0,
            eq_high_db: 1.5,
        });
        p.master_gain = 0.7;
        p.master_muted = false;

        let s = serde_json::to_string(&p).unwrap();
        let back: SessionProfile = serde_json::from_str(&s).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn missing_fields_use_defaults() {
        // Старые v0.3.0 профили (snake_case, без endpoint_id и EQ) должны
        // продолжать читаться: новые поля приходят со `default`.
        let s = r#"{"version":1,"devices":[{"name":"X","latencyMs":20}]}"#;
        let p: SessionProfile = serde_json::from_str(s).unwrap();
        assert_eq!(p.devices.len(), 1);
        assert_eq!(p.devices[0].volume, 1.0);
        assert_eq!(p.devices[0].latency_ms, 20);
        assert!(!p.devices[0].muted);
        assert_eq!(p.devices[0].eq_low_db, 0.0);
        assert!(p.devices[0].endpoint_id.is_none());
        assert_eq!(p.master_gain, 1.0);
    }

    #[test]
    fn device_aliases_roundtrip_and_default() {
        // Старый профиль без deviceAliases читается с пустой картой.
        let s = r#"{"version":1,"devices":[]}"#;
        let p: SessionProfile = serde_json::from_str(s).unwrap();
        assert!(p.device_aliases.is_empty());

        // Roundtrip с алиасами.
        let mut p = SessionProfile::default();
        p.device_aliases
            .insert("wasapi:{abc}".into(), "Колонки на кухне".into());
        p.device_aliases
            .insert("coreaudio:42".into(), "AirPods".into());
        let s = serde_json::to_string(&p).unwrap();
        assert!(s.contains("\"deviceAliases\""));
        let back: SessionProfile = serde_json::from_str(&s).unwrap();
        assert_eq!(p, back);
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
