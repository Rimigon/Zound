//! Tauri команды — тонкий фасад над AudioEngine и i18n.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use serde::Serialize;
use tauri::State;

use zound_core::DeviceId;
use zound_output::{AudioEngine, CommandError, DevicePreset, OutputManager, SessionProfile};
use zound_platform::{AudioBackend, CpalBackend, EndpointSpec, TestKind};
use zound_sync::SyncEngine;

use crate::i18n::I18n;

/// Тип Result для всех команд: ошибка идёт во фронт как структурированный
/// объект `{kind, message}` (P2.8). Frontend matches по `kind`.
type CmdResult<T> = Result<T, CommandError>;

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
#[serde(rename_all = "camelCase")]
pub struct DeviceDto {
    pub id: String,
    pub name: String,
    /// Стабильный backend-id (см. [`zound_core::DeviceInfo::endpoint_id`]).
    /// Frontend сохраняет его в session profile вместе с именем.
    pub endpoint_id: Option<String>,
    pub kind: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub is_default: bool,
    /// `true`, если устройство умеет только вход (микрофон, и т.п.).
    /// UI прячет такие в дефолтном виде и блокирует кнопку «Добавить».
    pub is_input_only: bool,
}

#[tauri::command]
pub fn list_outputs() -> CmdResult<Vec<DeviceDto>> {
    let backend = CpalBackend::new();
    let devices = backend
        .enumerate_outputs()
        .map_err(|e| CommandError::backend(e.to_string()))?;
    Ok(devices
        .into_iter()
        .map(|d| DeviceDto {
            id: d.id.to_string(),
            name: d.name,
            endpoint_id: d.endpoint_id,
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
pub fn list_all_devices() -> CmdResult<Vec<DeviceDto>> {
    use std::collections::HashSet;
    let backend = CpalBackend::new();
    let outputs = backend
        .enumerate_outputs()
        .map_err(|e| CommandError::backend(e.to_string()))?;
    let inputs = backend
        .enumerate_inputs()
        .map_err(|e| CommandError::backend(e.to_string()))?;

    let output_names: HashSet<String> = outputs.iter().map(|d| d.name.clone()).collect();
    let mut result: Vec<DeviceDto> = outputs
        .into_iter()
        .map(|d| DeviceDto {
            id: d.id.to_string(),
            name: d.name,
            endpoint_id: d.endpoint_id,
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
            endpoint_id: d.endpoint_id,
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
pub fn start_engine(state: State<'_, AppState>) -> CmdResult<()> {
    state
        .engine
        .start()
        .map_err(|e| CommandError::from_engine_string(e.to_string()))
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
        alive: state.engine.health().is_alive(),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub running: bool,
    pub loopback_source: Option<String>,
    /// `false`, если audio-thread поймал panic и больше не работает.
    /// UI показывает баннер и предлагает перезапустить приложение.
    pub alive: bool,
}

/// Эвенты от audio-thread (watchdog disconnect, panic, default-device
/// change). UI забирает batched через `engine_events` poll каждые ~200 мс.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EngineEventDto {
    OutputDisconnected { id: String, reason: String },
    EnginePanicked { message: String },
    DefaultDeviceChanged { new_endpoint_id: Option<String> },
}

#[tauri::command]
pub fn engine_events(state: State<'_, AppState>) -> Vec<EngineEventDto> {
    state
        .engine
        .take_events()
        .into_iter()
        .map(|ev| match ev {
            zound_output::engine::EngineEvent::OutputDisconnected { id, reason } => {
                EngineEventDto::OutputDisconnected {
                    id: id.to_string(),
                    reason,
                }
            }
            zound_output::engine::EngineEvent::EnginePanicked { message } => {
                EngineEventDto::EnginePanicked { message }
            }
            zound_output::engine::EngineEvent::DefaultDeviceChanged { new_endpoint_id } => {
                EngineEventDto::DefaultDeviceChanged { new_endpoint_id }
            }
        })
        .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddOutputResultDto {
    pub id: String,
    pub endpoint_id: Option<String>,
}

#[tauri::command]
pub fn add_output(
    state: State<'_, AppState>,
    device_name: String,
    endpoint_id: Option<String>,
) -> CmdResult<AddOutputResultDto> {
    let spec = EndpointSpec {
        name: device_name,
        endpoint_id,
    };
    state
        .engine
        .add_output_with_endpoint(spec)
        .map(|(id, eid)| AddOutputResultDto {
            id: id.to_string(),
            endpoint_id: eid,
        })
        .map_err(|e| CommandError::from_engine_string(e.to_string()))
}

#[tauri::command]
pub fn remove_output(state: State<'_, AppState>, id: String) {
    state.engine.remove_output(&DeviceId::from(id));
}

#[tauri::command]
pub fn set_output_volume(state: State<'_, AppState>, id: String, volume: f32) {
    state.engine.set_volume(&DeviceId::from(id), volume);
}

#[tauri::command]
pub fn set_output_muted(state: State<'_, AppState>, id: String, muted: bool) {
    state.engine.set_muted(&DeviceId::from(id), muted);
}

#[tauri::command]
pub fn set_output_balance(state: State<'_, AppState>, id: String, balance: f32) {
    state.engine.set_balance(&DeviceId::from(id), balance);
}

#[tauri::command]
pub fn set_output_eq(
    state: State<'_, AppState>,
    id: String,
    low_db: f32,
    mid_db: f32,
    high_db: f32,
) {
    state
        .engine
        .set_eq(&DeviceId::from(id), low_db, mid_db, high_db);
}

#[tauri::command]
pub fn play_test_signal(
    state: State<'_, AppState>,
    device_name: String,
    kind: String,
    bpm: Option<u16>,
) -> CmdResult<()> {
    let kind = match kind.as_str() {
        "click" => TestKind::Click,
        "sine" => TestKind::Sine1kHz,
        "metronome" => TestKind::Metronome {
            bpm: bpm.unwrap_or(120).clamp(40, 240),
        },
        other => {
            return Err(CommandError::bad_request(format!(
                "unknown test kind: {other}"
            )))
        }
    };
    state
        .engine
        .play_test_signal(&device_name, kind)
        .map_err(|e| CommandError::from_engine_string(e.to_string()))
}

#[tauri::command]
pub fn stop_test_signal(state: State<'_, AppState>, device_name: String) {
    state.engine.stop_test_signal(&device_name);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
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
) -> CmdResult<()> {
    state
        .sync
        .set_device_latency(&DeviceId::from(id), Duration::from_millis(latency_ms))
        .map_err(|e| CommandError::device_not_found(e.to_string()))
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
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
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
//
// `DevicePreset` и `SessionProfile` ходят и в ↔ frontend, и в ↔ session.json.
// Один общий тип (`#[serde(rename_all = "camelCase")]` в `zound-output`)
// убирает дублирование DTO, которое было раньше.

#[tauri::command]
pub fn load_session_profile(state: State<'_, AppState>) -> Option<SessionProfile> {
    let path = state.session_path.read().clone()?;
    match SessionProfile::load_from(&path) {
        Ok(Some(p)) => Some(p),
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
    devices: Vec<DevicePreset>,
    master_gain: f32,
    master_muted: bool,
) -> CmdResult<()> {
    let path = state
        .session_path
        .read()
        .clone()
        .ok_or_else(|| CommandError::backend("session path not initialized"))?;
    // Алиасы фронт не присылает: они живут собственным путём через
    // `set_device_alias` / `list_device_aliases`. Сохраняем то, что лежит
    // в файле, чтобы save_session_profile не стирал их.
    let device_aliases = SessionProfile::load_from(&path)
        .ok()
        .flatten()
        .map(|p| p.device_aliases)
        .unwrap_or_default();
    let profile = SessionProfile {
        version: zound_output::profile::PROFILE_VERSION,
        devices,
        master_gain,
        master_muted,
        device_aliases,
    };
    profile.save_to(&path).map_err(CommandError::backend)
}

// ---------- device aliases ---------- //
//
// Алиас — это пользовательское имя устройства «только в приложении». Системное
// имя WASAPI/CoreAudio/PipeWire не трогается; алиас живёт в session.json в
// `device_aliases: { endpoint_id → имя }`. Алгоритм матча устройства при
// restore-сессии по-прежнему идёт по `endpoint_id` (см. profile.rs) — алиас
// просто рендерится поверх системного имени.

const DEVICE_ALIAS_MAX_LEN: usize = 80;

#[tauri::command]
pub fn list_device_aliases(state: State<'_, AppState>) -> HashMap<String, String> {
    let path = match state.session_path.read().clone() {
        Some(p) => p,
        None => return HashMap::new(),
    };
    match SessionProfile::load_from(&path) {
        Ok(Some(p)) => p.device_aliases.into_iter().collect(),
        Ok(None) => HashMap::new(),
        Err(e) => {
            tracing::warn!(?e, "device aliases load failed");
            HashMap::new()
        }
    }
}

#[tauri::command]
pub fn set_device_alias(
    state: State<'_, AppState>,
    endpoint_id: String,
    alias: Option<String>,
) -> CmdResult<()> {
    if endpoint_id.trim().is_empty() {
        return Err(CommandError::bad_request("empty endpoint_id"));
    }
    let path = state
        .session_path
        .read()
        .clone()
        .ok_or_else(|| CommandError::backend("session path not initialized"))?;

    let mut profile = SessionProfile::load_from(&path)
        .map_err(CommandError::backend)?
        .unwrap_or_default();

    let trimmed = alias.as_deref().map(str::trim).unwrap_or("");
    if trimmed.is_empty() {
        profile.device_aliases.remove(&endpoint_id);
    } else {
        if trimmed.chars().count() > DEVICE_ALIAS_MAX_LEN {
            return Err(CommandError::bad_request(format!(
                "alias too long (max {DEVICE_ALIAS_MAX_LEN} chars)"
            )));
        }
        profile
            .device_aliases
            .insert(endpoint_id, trimmed.to_string());
    }
    profile.save_to(&path).map_err(CommandError::backend)
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
