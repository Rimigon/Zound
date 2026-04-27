//! Tauri команды — тонкий фасад над AudioEngine и i18n.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tauri::State;

use zound_core::DeviceId;
use zound_output::{AudioEngine, DevicePreset, OutputManager, SessionProfile};
use zound_platform::{AudioBackend, CpalBackend, TestKind};
use zound_sync::SyncEngine;

use crate::i18n::I18n;

/// Состояние, которое Tauri-handler получает через `State<AppState>`.
pub struct AppState {
    pub engine: Arc<AudioEngine>,
    pub sync: Arc<SyncEngine>,
    /// Хранится для будущих master-related-команд, читается через
    /// `engine.master_*`. Поле пока не читается напрямую — `_` префикс
    /// сообщает clippy/dead_code что это намеренно.
    pub _outputs: Arc<OutputManager>,
    pub i18n: Arc<I18n>,
    /// Путь к файлу session.json. Заполняется в main после Tauri builder
    /// resolve-а app data dir, потому здесь — RwLock<Option<...>>.
    pub session_path: Arc<RwLock<Option<PathBuf>>>,
}

#[derive(Serialize)]
pub struct DeviceDto {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub is_default: bool,
    /// `true`, если устройство умеет только вход (микрофон, и т.п.).
    /// UI прячет такие в дефолтном виде и блокирует кнопку «Добавить».
    pub is_input_only: bool,
}

#[tauri::command]
pub fn list_outputs() -> Result<Vec<DeviceDto>, String> {
    let backend = CpalBackend::new();
    let devices = backend.enumerate_outputs().map_err(|e| e.to_string())?;
    Ok(devices
        .into_iter()
        .map(|d| DeviceDto {
            id: d.id.to_string(),
            name: d.name,
            kind: format!("{:?}", d.kind),
            sample_rate: d.sample_rate,
            channels: d.channels,
            is_default: d.is_default,
            is_input_only: false,
        })
        .collect())
}

/// Все аудио-устройства системы — outputs + inputs. Используется
/// настройкой UI «показать все устройства». Inputs приходят с флагом
/// `is_input_only=true` и без output-`kind`-а; добавлять их как Zound-
/// output нельзя (UI блокирует кнопку).
#[tauri::command]
pub fn list_all_devices() -> Result<Vec<DeviceDto>, String> {
    use std::collections::HashSet;
    let backend = CpalBackend::new();
    let outputs = backend.enumerate_outputs().map_err(|e| e.to_string())?;
    let inputs = backend.enumerate_inputs().map_err(|e| e.to_string())?;

    let output_names: HashSet<String> = outputs.iter().map(|d| d.name.clone()).collect();
    let mut result: Vec<DeviceDto> = outputs
        .into_iter()
        .map(|d| DeviceDto {
            id: d.id.to_string(),
            name: d.name,
            kind: format!("{:?}", d.kind),
            sample_rate: d.sample_rate,
            channels: d.channels,
            is_default: d.is_default,
            is_input_only: false,
        })
        .collect();

    // Дублирующие input-стороны duplex-устройств (одно и то же имя в обоих
    // списках) уже попали как output — их пропускаем. В UI остаётся одна
    // строка, она и так с кнопкой «Добавить».
    for d in inputs
        .into_iter()
        .filter(|d| !output_names.contains(&d.name))
    {
        result.push(DeviceDto {
            id: d.id.to_string(),
            name: d.name,
            kind: format!("{:?}", d.kind),
            sample_rate: d.sample_rate,
            channels: d.channels,
            is_default: d.is_default,
            is_input_only: true,
        });
    }
    Ok(result)
}

#[tauri::command]
pub fn start_engine(state: State<'_, AppState>) -> Result<(), String> {
    state.engine.start().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn stop_engine(state: State<'_, AppState>) {
    state.engine.stop();
}

#[tauri::command]
pub fn engine_status(state: State<'_, AppState>) -> EngineStatus {
    EngineStatus {
        running: state.engine.is_running(),
        loopback_source: state.engine.loopback_source(),
    }
}

#[derive(Serialize)]
pub struct EngineStatus {
    pub running: bool,
    pub loopback_source: Option<String>,
}

#[tauri::command]
pub fn add_output(state: State<'_, AppState>, device_name: String) -> Result<String, String> {
    state
        .engine
        .add_output(&device_name)
        .map(|id| id.to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_output(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.engine.remove_output(&DeviceId::from(id));
    Ok(())
}

#[tauri::command]
pub fn set_output_volume(
    state: State<'_, AppState>,
    id: String,
    volume: f32,
) -> Result<(), String> {
    state
        .engine
        .set_volume(&DeviceId::from(id), volume)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_output_muted(state: State<'_, AppState>, id: String, muted: bool) -> Result<(), String> {
    state
        .engine
        .set_muted(&DeviceId::from(id), muted)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_output_balance(
    state: State<'_, AppState>,
    id: String,
    balance: f32,
) -> Result<(), String> {
    state
        .engine
        .set_balance(&DeviceId::from(id), balance)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_output_eq(
    state: State<'_, AppState>,
    id: String,
    low_db: f32,
    mid_db: f32,
    high_db: f32,
) -> Result<(), String> {
    state
        .engine
        .set_eq(&DeviceId::from(id), low_db, mid_db, high_db)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn play_test_signal(
    state: State<'_, AppState>,
    device_name: String,
    kind: String,
    bpm: Option<u16>,
) -> Result<(), String> {
    let kind = match kind.as_str() {
        "click" => TestKind::Click,
        "sine" => TestKind::Sine1kHz,
        "metronome" => TestKind::Metronome {
            bpm: bpm.unwrap_or(120).clamp(40, 240),
        },
        other => return Err(format!("unknown test kind: {other}")),
    };
    state
        .engine
        .play_test_signal(&device_name, kind)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn stop_test_signal(state: State<'_, AppState>, device_name: String) {
    state.engine.stop_test_signal(&device_name);
}

#[derive(Serialize)]
pub struct SyncStatusDto {
    pub in_sync: bool,
    pub drift_ms: u32,
    pub active_count: u32,
}

#[tauri::command]
pub fn sync_status(state: State<'_, AppState>) -> SyncStatusDto {
    let snap = state.sync.drift_snapshot();
    SyncStatusDto {
        in_sync: snap.in_sync,
        drift_ms: snap.drift_ms,
        active_count: snap.active_count,
    }
}

#[tauri::command]
pub fn set_output_latency(
    state: State<'_, AppState>,
    id: String,
    latency_ms: u64,
) -> Result<(), String> {
    state
        .sync
        .set_device_latency(&DeviceId::from(id), Duration::from_millis(latency_ms))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn target_latency_ms(state: State<'_, AppState>) -> u64 {
    state.sync.target_latency().as_millis() as u64
}

/// Применить одинаковую задержку ко всем активным устройствам. Возвращает
/// сколько устройств затронуто. Используется UI-режимом «связанные
/// задержки».
#[tauri::command]
pub fn set_all_latencies(state: State<'_, AppState>, latency_ms: u64) -> usize {
    state
        .sync
        .set_all_latencies(Duration::from_millis(latency_ms))
}

// ---------- master controls ---------- //

#[derive(Serialize)]
pub struct MasterStateDto {
    pub gain: f32,
    pub muted: bool,
}

#[tauri::command]
pub fn master_state(state: State<'_, AppState>) -> MasterStateDto {
    MasterStateDto {
        gain: state.engine.master_gain(),
        muted: state.engine.master_muted(),
    }
}

#[tauri::command]
pub fn set_master_gain(state: State<'_, AppState>, gain: f32) {
    state.engine.set_master_gain(gain);
}

#[tauri::command]
pub fn set_master_muted(state: State<'_, AppState>, muted: bool) {
    state.engine.set_master_muted(muted);
}

// ---------- peak meters ---------- //

#[derive(Serialize)]
pub struct PeakDto {
    pub id: String,
    pub peak: f32,
}

#[tauri::command]
pub fn peaks(state: State<'_, AppState>) -> Vec<PeakDto> {
    state
        .engine
        .peaks_snapshot()
        .into_iter()
        .map(|(id, p)| PeakDto {
            id: id.to_string(),
            peak: p,
        })
        .collect()
}

// ---------- session profile ---------- //

#[derive(Serialize, Deserialize)]
pub struct ProfileDeviceDto {
    pub name: String,
    pub volume: f32,
    pub muted: bool,
    pub balance: f32,
    pub latency_ms: u64,
}

#[derive(Serialize)]
pub struct SessionProfileDto {
    pub devices: Vec<ProfileDeviceDto>,
    pub master_gain: f32,
    pub master_muted: bool,
}

impl From<SessionProfile> for SessionProfileDto {
    fn from(p: SessionProfile) -> Self {
        Self {
            devices: p
                .devices
                .into_iter()
                .map(|d| ProfileDeviceDto {
                    name: d.name,
                    volume: d.volume,
                    muted: d.muted,
                    balance: d.balance,
                    latency_ms: d.latency_ms,
                })
                .collect(),
            master_gain: p.master_gain,
            master_muted: p.master_muted,
        }
    }
}

#[tauri::command]
pub fn load_session_profile(state: State<'_, AppState>) -> Option<SessionProfileDto> {
    let path = state.session_path.read().clone()?;
    match SessionProfile::load_from(&path) {
        Ok(Some(p)) => Some(p.into()),
        Ok(None) => None,
        Err(e) => {
            tracing::warn!(?e, "session profile load failed");
            None
        }
    }
}

#[tauri::command]
pub fn save_session_profile(
    state: State<'_, AppState>,
    devices: Vec<ProfileDeviceDto>,
    master_gain: f32,
    master_muted: bool,
) -> Result<(), String> {
    let path = match state.session_path.read().clone() {
        Some(p) => p,
        None => return Err("session path not initialized".into()),
    };
    let profile = SessionProfile {
        version: zound_output::profile::PROFILE_VERSION,
        devices: devices
            .into_iter()
            .map(|d| DevicePreset {
                name: d.name,
                volume: d.volume,
                muted: d.muted,
                balance: d.balance,
                latency_ms: d.latency_ms,
            })
            .collect(),
        master_gain,
        master_muted,
    };
    profile.save_to(&path)
}

// ---------- latency calibration (заготовка) ---------- //

/// Сгенерировать chirp-сигнал для измерения. Возвращает f32-массив,
/// фронт может проиграть его на `device_name` через test-канал, или
/// записать через capture loopback и сравнить.
///
/// Pipeline play+record+correlate ещё не подключён (см.
/// `zound-sync::calibration` — модуль с алгоритмом). Эта команда дана,
/// чтобы UI и frontend смогли уже сейчас получить сигнал и запланировать
/// эксперимент.
#[tauri::command]
pub fn generate_calibration_chirp(sample_rate: u32, duration_ms: u32) -> Vec<f32> {
    let samples = (sample_rate as f32 * duration_ms as f32 / 1000.0) as usize;
    zound_sync::generate_chirp(sample_rate, samples, 0.5)
}

#[tauri::command]
pub fn load_dictionary(state: State<'_, AppState>, lang: String) -> HashMap<String, String> {
    state.i18n.set_language(&lang);
    state.i18n.dictionary(&lang)
}

#[tauri::command]
pub fn format_message(
    state: State<'_, AppState>,
    lang: String,
    key: String,
    args: HashMap<String, f64>,
) -> Option<String> {
    state.i18n.format(&lang, &key, &args)
}
