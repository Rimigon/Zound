//! Tauri команды — тонкий фасад над AudioEngine и i18n.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::State;

use zound_core::DeviceId;
use zound_output::AudioEngine;
use zound_platform::{AudioBackend, CpalBackend, TestKind};
use zound_sync::SyncEngine;

use crate::i18n::I18n;

/// Состояние, которое Tauri-handler получает через `State<AppState>`.
pub struct AppState {
    pub engine: Arc<AudioEngine>,
    pub sync: Arc<SyncEngine>,
    pub i18n: Arc<I18n>,
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
    for d in inputs.into_iter().filter(|d| !output_names.contains(&d.name)) {
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
pub fn set_output_muted(
    state: State<'_, AppState>,
    id: String,
    muted: bool,
) -> Result<(), String> {
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
