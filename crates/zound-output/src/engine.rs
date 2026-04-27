//! AudioEngine — связывает loopback-захват с несколькими output-устройствами.
//!
//! cpal::Stream на WASAPI не `Send`, поэтому мы используем **actor-паттерн**:
//! выделенный audio-thread владеет всеми стримами (capture + output sinks)
//! и внутренней pipeline-логикой. Внешний API — [`AudioEngine`] — просто
//! отправляет команды в этот поток через канал. За счёт этого сам
//! `AudioEngine` получается `Send + Sync` и может храниться в Tauri-state.
//!
//! Поток данных внутри audio-thread-а:
//!
//! ```text
//!   [loopback capture] --consumer--> [tick loop]
//!                                          |
//!                  ┌───────────────────────┼───────────────────────┐
//!                  ▼                       ▼                       ▼
//!            per-output resample     (compensation —             (volume —
//!             (если SR разный)       prefill silence             атомарно в cpal
//!                                    при open_output)             callback-е)
//!                  ▼                       ▼                       ▼
//!              output ringbuf          output ringbuf          output ringbuf
//!                  ▼                       ▼                       ▼
//!              cpal callback           cpal callback           cpal callback
//!                  ▼                       ▼                       ▼
//!                 🎧 A                    🔊 B                    📶 C
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parking_lot::{Mutex, RwLock};
use ringbuf::traits::{Consumer, Observer, Producer};
use rubato::{FastFixedIn, PolynomialDegree, Resampler};
use zound_core::{DeviceId, Error, Result, ThreeBandEq};
use zound_platform::{Capture, CaptureOpts, OutputOpts, OutputSink, TestStream};
use zound_sync::{DriftCorrector, SyncEngine};

use crate::guard::{is_capture_source, ERR_FEEDBACK_LOOP};
use crate::OutputManager;

/// Размер чанка, который worker обрабатывает за один проход (в frames
/// на канал). 480 @ 48 kHz = 10 мс — типичная гранулярность callback-ов.
const WORKER_CHUNK_FRAMES: usize = 480;

/// Период тика drift-коррекции. Реже не нужен (резонанс с jitter
/// callback-ов), чаще — даёт всплески интегральной части.
const DRIFT_TICK: Duration = Duration::from_millis(100);

/// Затухание peak-метра за один worker-tick (10 мс при заполненной
/// capture-очереди). Линейный множитель: 0.92 ≈ -0.7 dB / tick → -70 dB
/// за 1 секунду без сигнала. Эмулирует «hold + release» аналоговых
/// VU-метров.
const PEAK_DECAY_PER_TICK: f32 = 0.92;

/// Команды, которые внешний API шлёт audio-thread-у.
enum EngineCmd {
    Start(SyncSender<Result<(), String>>),
    StopCapture(SyncSender<()>),
    AddOutput {
        device_name: String,
        reply: SyncSender<Result<DeviceId, String>>,
    },
    RemoveOutput {
        id: DeviceId,
        reply: SyncSender<()>,
    },
    SetVolume {
        id: DeviceId,
        volume: f32,
        reply: SyncSender<Result<(), String>>,
    },
    SetMuted {
        id: DeviceId,
        muted: bool,
        reply: SyncSender<Result<(), String>>,
    },
    SetBalance {
        id: DeviceId,
        balance: f32,
        reply: SyncSender<Result<(), String>>,
    },
    SetEq {
        id: DeviceId,
        low_db: f32,
        mid_db: f32,
        high_db: f32,
        reply: SyncSender<Result<(), String>>,
    },
    SetMasterGain {
        gain: f32,
        reply: SyncSender<()>,
    },
    SetMasterMuted {
        muted: bool,
        reply: SyncSender<()>,
    },
    PlayTestSignal {
        device_name: String,
        kind: zound_platform::TestKind,
        reply: SyncSender<Result<(), String>>,
    },
    StopTestSignal {
        device_name: String,
        reply: SyncSender<()>,
    },
    Shutdown,
}

/// Публичный handle. Реальная работа — внутри audio-thread-а.
pub struct AudioEngine {
    cmd_tx: Sender<EngineCmd>,
    active_ids: Arc<RwLock<Vec<DeviceId>>>,
    /// Имя устройства, с которого сейчас снимается loopback (или None,
    /// если capture остановлен). Shared c audio-thread.
    loopback_source: Arc<RwLock<Option<String>>>,
    running: Arc<RwLock<bool>>,
    /// Per-device peak — последний абсолютный максимум выхода (после
    /// volume / mute / balance / master). Шарится с UI без локов.
    /// Значения f32 ∈ [0; ~2.0] закодированы через `to_bits` в u32 atomic.
    peak_meters: Arc<RwLock<HashMap<DeviceId, Arc<AtomicU32>>>>,
    _sync: Arc<SyncEngine>,
    outputs: Arc<OutputManager>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl AudioEngine {
    pub fn new(sync: Arc<SyncEngine>, outputs: Arc<OutputManager>) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let active_ids = Arc::new(RwLock::new(Vec::<DeviceId>::new()));
        let loopback_source = Arc::new(RwLock::new(None::<String>));
        let running = Arc::new(RwLock::new(false));
        let peak_meters: Arc<RwLock<HashMap<DeviceId, Arc<AtomicU32>>>> =
            Arc::new(RwLock::new(HashMap::new()));

        let t_sync = sync.clone();
        let t_outputs = outputs.clone();
        let t_active = active_ids.clone();
        let t_loop = loopback_source.clone();
        let t_running = running.clone();
        let t_peaks = peak_meters.clone();
        let handle = thread::Builder::new()
            .name("zound-audio".into())
            .spawn(move || {
                audio_thread(
                    cmd_rx, t_sync, t_outputs, t_active, t_loop, t_running, t_peaks,
                )
            })
            .expect("spawn audio thread");

        Self {
            cmd_tx,
            active_ids,
            loopback_source,
            running,
            peak_meters,
            _sync: sync,
            outputs,
            thread: Mutex::new(Some(handle)),
        }
    }

    /// Линейный peak (0..1+) для устройства за последний tick. None если
    /// устройство не активно или ещё не обновляло метр.
    pub fn peak_for(&self, id: &DeviceId) -> Option<f32> {
        let meters = self.peak_meters.read();
        meters
            .get(id)
            .map(|a| f32::from_bits(a.load(Ordering::Relaxed)))
    }

    /// Снимок peak-ов всех активных устройств.
    pub fn peaks_snapshot(&self) -> Vec<(DeviceId, f32)> {
        let meters = self.peak_meters.read();
        meters
            .iter()
            .map(|(id, a)| (id.clone(), f32::from_bits(a.load(Ordering::Relaxed))))
            .collect()
    }

    /// Установить master gain ∈ [0; 1]. Применяется всем активным
    /// устройствам на следующем чанке.
    pub fn set_master_gain(&self, gain: f32) {
        let (tx, rx) = mpsc::sync_channel(1);
        if self
            .cmd_tx
            .send(EngineCmd::SetMasterGain { gain, reply: tx })
            .is_ok()
        {
            let _ = rx.recv();
        }
    }

    pub fn set_master_muted(&self, muted: bool) {
        let (tx, rx) = mpsc::sync_channel(1);
        if self
            .cmd_tx
            .send(EngineCmd::SetMasterMuted { muted, reply: tx })
            .is_ok()
        {
            let _ = rx.recv();
        }
    }

    /// Чтение master-gain прямо из `OutputManager` (минуя audio-thread).
    pub fn master_gain(&self) -> f32 {
        self.outputs.master().gain()
    }

    pub fn master_muted(&self) -> bool {
        self.outputs.master().muted()
    }

    /// Открыть loopback-захват. Идемпотентно.
    pub fn start(&self) -> Result<()> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.cmd_tx
            .send(EngineCmd::Start(tx))
            .map_err(|_| Error::Other("engine thread gone".into()))?;
        rx.recv()
            .map_err(|_| Error::Other("engine thread dropped reply".into()))?
            .map_err(Error::Other)
    }

    /// Остановить capture и все output-потоки. Audio-thread остаётся жив
    /// и может быть снова запущен через `start`.
    pub fn stop(&self) {
        let (tx, rx) = mpsc::sync_channel(1);
        if self.cmd_tx.send(EngineCmd::StopCapture(tx)).is_ok() {
            let _ = rx.recv();
        }
    }

    /// Имя устройства-источника loopback. `None`, если capture не запущен.
    pub fn loopback_source(&self) -> Option<String> {
        self.loopback_source.read().clone()
    }

    /// Запущен ли capture прямо сейчас.
    pub fn is_running(&self) -> bool {
        *self.running.read()
    }

    /// Добавить output по имени устройства.
    pub fn add_output(&self, device_name: &str) -> Result<DeviceId> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.cmd_tx
            .send(EngineCmd::AddOutput {
                device_name: device_name.to_string(),
                reply: tx,
            })
            .map_err(|_| Error::Other("engine thread gone".into()))?;
        rx.recv()
            .map_err(|_| Error::Other("engine thread dropped reply".into()))?
            .map_err(Error::Other)
    }

    pub fn remove_output(&self, id: &DeviceId) {
        let (tx, rx) = mpsc::sync_channel(1);
        let _ = self.cmd_tx.send(EngineCmd::RemoveOutput {
            id: id.clone(),
            reply: tx,
        });
        let _ = rx.recv();
    }

    pub fn set_volume(&self, id: &DeviceId, volume: f32) -> Result<()> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.cmd_tx
            .send(EngineCmd::SetVolume {
                id: id.clone(),
                volume,
                reply: tx,
            })
            .map_err(|_| Error::Other("engine thread gone".into()))?;
        rx.recv()
            .map_err(|_| Error::Other("engine thread dropped reply".into()))?
            .map_err(Error::Other)
    }

    pub fn set_muted(&self, id: &DeviceId, muted: bool) -> Result<()> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.cmd_tx
            .send(EngineCmd::SetMuted {
                id: id.clone(),
                muted,
                reply: tx,
            })
            .map_err(|_| Error::Other("engine thread gone".into()))?;
        rx.recv()
            .map_err(|_| Error::Other("engine thread dropped reply".into()))?
            .map_err(Error::Other)
    }

    /// 3-полосный EQ: low / mid / high gain в dB. ±12 dB clamp в worker.
    /// Все три = 0 → EQ переходит в bypass.
    pub fn set_eq(&self, id: &DeviceId, low_db: f32, mid_db: f32, high_db: f32) -> Result<()> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.cmd_tx
            .send(EngineCmd::SetEq {
                id: id.clone(),
                low_db,
                mid_db,
                high_db,
                reply: tx,
            })
            .map_err(|_| Error::Other("engine thread gone".into()))?;
        rx.recv()
            .map_err(|_| Error::Other("engine thread dropped reply".into()))?
            .map_err(Error::Other)
    }

    /// Balance L/R, диапазон [-1.0; +1.0]. Применяется только для stereo-
    /// устройств; для mono/4ch+ просто игнорируется (apply skip).
    pub fn set_balance(&self, id: &DeviceId, balance: f32) -> Result<()> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.cmd_tx
            .send(EngineCmd::SetBalance {
                id: id.clone(),
                balance: balance.clamp(-1.0, 1.0),
                reply: tx,
            })
            .map_err(|_| Error::Other("engine thread gone".into()))?;
        rx.recv()
            .map_err(|_| Error::Other("engine thread dropped reply".into()))?
            .map_err(Error::Other)
    }

    pub fn play_test_signal(
        &self,
        device_name: &str,
        kind: zound_platform::TestKind,
    ) -> Result<()> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.cmd_tx
            .send(EngineCmd::PlayTestSignal {
                device_name: device_name.to_string(),
                kind,
                reply: tx,
            })
            .map_err(|_| Error::Other("engine thread gone".into()))?;
        rx.recv()
            .map_err(|_| Error::Other("engine thread dropped reply".into()))?
            .map_err(Error::Other)
    }

    pub fn stop_test_signal(&self, device_name: &str) {
        let (tx, rx) = mpsc::sync_channel(1);
        let _ = self.cmd_tx.send(EngineCmd::StopTestSignal {
            device_name: device_name.to_string(),
            reply: tx,
        });
        let _ = rx.recv();
    }

    pub fn active_outputs(&self) -> Vec<DeviceId> {
        self.active_ids.read().clone()
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(EngineCmd::Shutdown);
        if let Some(h) = self.thread.lock().take() {
            let _ = h.join();
        }
    }
}

// `ERR_FEEDBACK_LOOP` теперь живёт в `crate::guard`. Реэкспорт сделан
// через `crate::lib`.

// ---------- внутри audio-thread ---------- //

struct WorkerOutput {
    sink: OutputSink,
    resampler: Option<ResampleCtx>,
    /// Pre-allocated buffer for the final interleaved chunk we push into
    /// `sink.producer`. Capacity = `max(WORKER_CHUNK_FRAMES,
    /// max_output_frames) * out_channels` — фиксируется при
    /// `handle_add_output`. Без этого аллокировали Vec на каждый чанк в
    /// worker-loop (jitter-риск).
    interleaved_out: Vec<f32>,
    /// Balance L/R, f32 в диапазоне [-1.0; +1.0] через AtomicU32 bits.
    /// Применяется в worker-thread перед push в ringbuf, только если
    /// `out_channels == 2` (stereo). Для не-stereo игнорируется.
    balance: Arc<AtomicU32>,
    /// Timestamp последнего успешного push в ringbuf, micros UNIX epoch.
    /// Шарится с `SyncEngine` для агрегации drift между устройствами.
    last_push_micros: Arc<AtomicU64>,
    /// Peak-метр (атомарно шарится с UI). f32 bits в u32.
    peak_bits: Arc<AtomicU32>,
    /// PI-контроллер дрейфа. Хранит интегральную часть и последнюю
    /// корректировку. Применяется в `push_to_output` к выходному
    /// буферу через линейный stretch (не через rubato — rubato
    /// `FastFixedIn` не умеет менять ratio в рантайме без re-init).
    drift: DriftCorrector,
    /// Сколько interleaved-семплов компенсации мы УЖЕ применили к этому
    /// устройству (положительные = добавили тишины, отрицательные =
    /// съели). При каждом tick worker сравнивает с
    /// `compensation_delay`-таргетом из SyncEngine: разницу
    /// добавляет/выбрасывает чанками ≤ `WORKER_CHUNK_FRAMES`, чтобы
    /// большие сдвиги latency не создавали скачки на одном кадре.
    applied_comp_samples: i64,
    /// `(out_sr, out_channels)` — для перевода `compensation_delay` в
    /// семплы выходного формата.
    out_sr: u32,
    out_channels: u16,
    /// 3-полосный EQ. В bypass ничего не делает (один if на чанк).
    eq: ThreeBandEq,
}

struct ResampleCtx {
    resampler: FastFixedIn<f32>,
    input_planar: Vec<Vec<f32>>,
    output_planar: Vec<Vec<f32>>,
    input_frames: usize,
    max_output_frames: usize,
}

#[allow(clippy::too_many_arguments)]
fn audio_thread(
    cmd_rx: Receiver<EngineCmd>,
    sync: Arc<SyncEngine>,
    outputs: Arc<OutputManager>,
    active_ids: Arc<RwLock<Vec<DeviceId>>>,
    loopback_source: Arc<RwLock<Option<String>>>,
    running: Arc<RwLock<bool>>,
    peak_meters: Arc<RwLock<HashMap<DeviceId, Arc<AtomicU32>>>>,
) {
    let mut capture: Option<Capture> = None;
    let mut workers: Vec<WorkerOutput> = Vec::new();
    let mut test_streams: Vec<TestStream> = Vec::new();
    let mut interleaved: Vec<f32> = Vec::new();
    let mut planar: Vec<Vec<f32>> = Vec::new();

    let tick_sleep = Duration::from_millis(2);
    let mut last_drift_tick = Instant::now();

    loop {
        // 1. Обработать все ожидающие команды.
        loop {
            match cmd_rx.try_recv() {
                Ok(EngineCmd::Start(reply)) => {
                    let res = handle_start(&mut capture, &mut interleaved, &mut planar);
                    if res.is_ok() {
                        if let Some(cap) = capture.as_ref() {
                            *loopback_source.write() = Some(cap.session.source_name.clone());
                            *running.write() = true;
                        }
                    }
                    let _ = reply.send(res);
                }
                Ok(EngineCmd::StopCapture(reply)) => {
                    // Снять все output-потоки и capture. audio-thread остаётся жив.
                    for w in workers.drain(..) {
                        sync.remove_device(&w.sink.device_id);
                        outputs.remove(&w.sink.device_id);
                        peak_meters.write().remove(&w.sink.device_id);
                        // sink Drop остановит cpal::Stream.
                        drop(w);
                    }
                    active_ids.write().clear();
                    capture = None;
                    *loopback_source.write() = None;
                    *running.write() = false;
                    tracing::info!("capture stopped");
                    let _ = reply.send(());
                }
                Ok(EngineCmd::AddOutput { device_name, reply }) => {
                    let src = capture.as_ref().map(|c| c.session.source_name.as_str());
                    let res = if is_capture_source(src, &device_name) {
                        Err(ERR_FEEDBACK_LOOP.to_string())
                    } else {
                        handle_add_output(
                            &capture,
                            &mut workers,
                            &sync,
                            &outputs,
                            &active_ids,
                            &peak_meters,
                            &device_name,
                        )
                    };
                    let _ = reply.send(res);
                }
                Ok(EngineCmd::RemoveOutput { id, reply }) => {
                    workers.retain(|w| w.sink.device_id != id);
                    sync.remove_device(&id);
                    outputs.remove(&id);
                    peak_meters.write().remove(&id);
                    active_ids.write().retain(|x| x != &id);
                    let _ = reply.send(());
                }
                Ok(EngineCmd::SetVolume { id, volume, reply }) => {
                    let res = match workers.iter().find(|w| w.sink.device_id == id) {
                        Some(w) => {
                            w.sink.volume.set(volume);
                            Ok(())
                        }
                        None => Err(format!("device not found: {id}")),
                    };
                    let _ = reply.send(res);
                }
                Ok(EngineCmd::SetMuted { id, muted, reply }) => {
                    let res = match workers.iter().find(|w| w.sink.device_id == id) {
                        Some(w) => {
                            w.sink.muted.store(muted, Ordering::Relaxed);
                            outputs.set_muted(&id, muted).ok();
                            Ok(())
                        }
                        None => Err(format!("device not found: {id}")),
                    };
                    let _ = reply.send(res);
                }
                Ok(EngineCmd::SetBalance { id, balance, reply }) => {
                    let res = match workers.iter().find(|w| w.sink.device_id == id) {
                        Some(w) => {
                            w.balance.store(balance.to_bits(), Ordering::Relaxed);
                            Ok(())
                        }
                        None => Err(format!("device not found: {id}")),
                    };
                    let _ = reply.send(res);
                }
                Ok(EngineCmd::SetEq {
                    id,
                    low_db,
                    mid_db,
                    high_db,
                    reply,
                }) => {
                    let res = match workers.iter_mut().find(|w| w.sink.device_id == id) {
                        Some(w) => {
                            w.eq.set_low_gain(low_db);
                            w.eq.set_mid_gain(mid_db);
                            w.eq.set_high_gain(high_db);
                            Ok(())
                        }
                        None => Err(format!("device not found: {id}")),
                    };
                    let _ = reply.send(res);
                }
                Ok(EngineCmd::SetMasterGain { gain, reply }) => {
                    outputs.master().set_gain(gain);
                    let _ = reply.send(());
                }
                Ok(EngineCmd::SetMasterMuted { muted, reply }) => {
                    outputs.master().set_muted(muted);
                    let _ = reply.send(());
                }
                Ok(EngineCmd::PlayTestSignal {
                    device_name,
                    kind,
                    reply,
                }) => {
                    // Защита от feedback: тест на source = loopback вернётся
                    // в наш capture и зациклится.
                    let src = capture.as_ref().map(|c| c.session.source_name.as_str());
                    let res = if is_capture_source(src, &device_name) {
                        Err(ERR_FEEDBACK_LOOP.to_string())
                    } else if test_streams.iter().any(|t| t.device_name == device_name) {
                        Err(format!("test already playing on {device_name}"))
                    } else {
                        match zound_platform::start_test_stream(&device_name, kind) {
                            Ok(t) => {
                                test_streams.push(t);
                                Ok(())
                            }
                            Err(e) => Err(e.to_string()),
                        }
                    };
                    let _ = reply.send(res);
                }
                Ok(EngineCmd::StopTestSignal { device_name, reply }) => {
                    test_streams.retain(|t| t.device_name != device_name);
                    let _ = reply.send(());
                }
                Ok(EngineCmd::Shutdown) => {
                    tracing::info!("audio-thread shutdown");
                    return;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    tracing::info!("audio-thread: command channel closed");
                    return;
                }
            }
        }

        // 2. Pipeline-тик.
        if let Some(cap) = capture.as_mut() {
            let chunk_samples = interleaved.len();
            let available = cap.consumer.occupied_len();
            if chunk_samples > 0 && available >= chunk_samples {
                let _ = cap.consumer.pop_slice(&mut interleaved);
                deinterleave(&interleaved, cap.session.channels as usize, &mut planar);
                let master_gain = outputs.master().effective_gain();
                for w in workers.iter_mut() {
                    push_to_output(
                        w,
                        &interleaved,
                        &planar,
                        cap.session.channels as usize,
                        master_gain,
                        &sync,
                    );
                }
            } else {
                thread::sleep(tick_sleep);
            }
        } else {
            thread::sleep(tick_sleep);
        }

        // 3. Drift-tick (раз в DRIFT_TICK).
        if last_drift_tick.elapsed() >= DRIFT_TICK {
            let dt = last_drift_tick.elapsed();
            last_drift_tick = Instant::now();
            let errors = sync.per_device_errors();
            if !errors.is_empty() {
                for w in workers.iter_mut() {
                    if let Some((_, err)) = errors.iter().find(|(id, _)| id == &w.sink.device_id) {
                        let _ = w.drift.update(*err, dt);
                    }
                }
            }
        }

        // 4. Сборка отыгравших test-streams (Click/Sine self-terminate
        // через stop_flag; Metronome — только по UI-команде Stop).
        test_streams.retain(|t| !t.is_done());
    }
}

fn handle_start(
    capture: &mut Option<Capture>,
    interleaved: &mut Vec<f32>,
    planar: &mut Vec<Vec<f32>>,
) -> Result<(), String> {
    if capture.is_some() {
        return Ok(());
    }
    let cap =
        zound_platform::open_default_loopback(CaptureOpts::default()).map_err(|e| e.to_string())?;
    let chunk_samples = WORKER_CHUNK_FRAMES * cap.session.channels as usize;
    *interleaved = vec![0.0_f32; chunk_samples];
    *planar = vec![vec![0.0_f32; WORKER_CHUNK_FRAMES]; cap.session.channels as usize];
    tracing::info!(
        sr = cap.session.sample_rate,
        ch = cap.session.channels,
        "loopback capture started"
    );
    *capture = Some(cap);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn handle_add_output(
    capture: &Option<Capture>,
    workers: &mut Vec<WorkerOutput>,
    sync: &Arc<SyncEngine>,
    outputs: &Arc<OutputManager>,
    active_ids: &Arc<RwLock<Vec<DeviceId>>>,
    peak_meters: &Arc<RwLock<HashMap<DeviceId, Arc<AtomicU32>>>>,
    device_name: &str,
) -> Result<DeviceId, String> {
    let cap = capture
        .as_ref()
        .ok_or_else(|| "engine not started".to_string())?;
    let cap_rate = cap.session.sample_rate;

    // Intrinsic latency — неизвестна, дефолт 20 мс (будет поправлено
    // пользователем через UI-слайдер).
    let intrinsic = Duration::from_millis(20);
    let sink = zound_platform::open_output_by_name(
        device_name,
        OutputOpts {
            buffer_samples: 96_000,
            // Компенсацию считаем после того, как узнаем реальный SR.
            prefill_silence_samples: 0,
        },
    )
    .map_err(|e| e.to_string())?;

    let resampler = build_resampler_ctx(cap_rate, sink.sample_rate, sink.channels)
        .map_err(|e| e.to_string())?;
    let device_id = sink.device_id.clone();

    let last_push_micros = sync.add_device(device_id.clone(), intrinsic, sink.sample_rate);
    outputs.add(device_id.clone());
    active_ids.write().push(device_id.clone());

    let max_out_frames = resampler
        .as_ref()
        .map(|r| r.max_output_frames)
        .unwrap_or(WORKER_CHUNK_FRAMES);
    // Capacity с запасом 50%: на этот буфер приходится не только
    // ресемплированный чанк, но и временные расширения от drift-stretch
    // и compensation-delta (вставка тишины). Без запаса они будут
    // переаллоцировать Vec на каждом тике — это аллок в audio-потоке.
    let interleaved_out = Vec::with_capacity(max_out_frames * sink.channels as usize * 3 / 2 + 16);
    let balance = Arc::new(AtomicU32::new(0.0_f32.to_bits()));
    let peak_bits = Arc::new(AtomicU32::new(0.0_f32.to_bits()));
    peak_meters
        .write()
        .insert(device_id.clone(), peak_bits.clone());

    let out_sr = sink.sample_rate;
    let out_channels = sink.channels;
    let eq = ThreeBandEq::new(out_sr as f32);
    workers.push(WorkerOutput {
        sink,
        resampler,
        interleaved_out,
        balance,
        last_push_micros,
        peak_bits,
        drift: DriftCorrector::new(),
        applied_comp_samples: 0,
        out_sr,
        out_channels,
        eq,
    });
    tracing::info!(%device_id, "output added");
    Ok(device_id)
}

fn build_resampler_ctx(in_sr: u32, out_sr: u32, channels: u16) -> Result<Option<ResampleCtx>> {
    if in_sr == out_sr {
        return Ok(None);
    }
    let ratio = out_sr as f64 / in_sr as f64;
    let input_frames = WORKER_CHUNK_FRAMES;
    let resampler = FastFixedIn::<f32>::new(
        ratio,
        1.1,
        PolynomialDegree::Septic,
        input_frames,
        channels as usize,
    )
    .map_err(|e| Error::Other(format!("rubato init: {e}")))?;

    let max_output_frames = resampler.output_frames_max();
    let input_planar = vec![vec![0.0_f32; input_frames]; channels as usize];
    let output_planar = vec![vec![0.0_f32; max_output_frames]; channels as usize];

    Ok(Some(ResampleCtx {
        resampler,
        input_planar,
        output_planar,
        input_frames,
        max_output_frames,
    }))
}

fn deinterleave(src: &[f32], channels: usize, dst: &mut [Vec<f32>]) {
    let frames = src.len() / channels;
    for f in 0..frames {
        for ch in 0..channels {
            dst[ch][f] = src[f * channels + ch];
        }
    }
}

fn push_to_output(
    w: &mut WorkerOutput,
    interleaved_src: &[f32],
    planar_src: &[Vec<f32>],
    cap_channels: usize,
    master_gain: f32,
    sync: &Arc<SyncEngine>,
) {
    let out_channels = w.sink.channels as usize;

    match w.resampler.as_mut() {
        None => {
            adapt_channels_into(
                interleaved_src,
                cap_channels,
                out_channels,
                &mut w.interleaved_out,
            );
        }
        Some(ctx) => {
            // Деинтерлив под каналы устройства (если разнятся — дублируем).
            for ch in 0..ctx.input_planar.len() {
                let src_ch = if ch < cap_channels {
                    ch
                } else {
                    cap_channels - 1
                };
                ctx.input_planar[ch][..ctx.input_frames]
                    .copy_from_slice(&planar_src[src_ch][..ctx.input_frames]);
            }
            let (_in, out_frames) = match ctx.resampler.process_into_buffer(
                &ctx.input_planar,
                &mut ctx.output_planar,
                None,
            ) {
                Ok(pair) => pair,
                Err(e) => {
                    tracing::warn!(?e, "resample failed");
                    return;
                }
            };
            debug_assert!(out_frames <= ctx.max_output_frames);
            w.interleaved_out.clear();
            for f in 0..out_frames {
                for ch in 0..out_channels {
                    let src_ch = ch.min(ctx.output_planar.len() - 1);
                    w.interleaved_out.push(ctx.output_planar[src_ch][f]);
                }
            }
        }
    }

    // EQ — между ресемплингом и balance/master, чтобы фильтрация шла на
    // выходных частотах устройства. В bypass — no-op.
    w.eq.process_interleaved(&mut w.interleaved_out, out_channels);

    // Применить balance — только для stereo. Один atomic load на чанк.
    apply_stereo_balance(&mut w.interleaved_out, out_channels, &w.balance);

    // Master gain — один f32 множитель на чанк. mute уже отражён в
    // `effective_gain` (=> 0.0 при mute).
    if master_gain != 1.0 {
        for s in w.interleaved_out.iter_mut() {
            *s *= master_gain;
        }
    }

    // Drift-correction — линейный stretch буфера через простой decimate
    // /дублирование. Реальная коррекция ±0.1% — не слышимая. Применяем
    // только если correction != 0.
    let correction = w.drift.last_correction();
    if correction.abs() > f32::EPSILON {
        apply_drift_stretch(&mut w.interleaved_out, out_channels, correction);
    }

    // Compensation reconcile — выравнивание per-device latency к target
    // через delay-line. Compensation_delay из SyncEngine = на сколько
    // нам нужно отстать от реального capture-момента, чтобы все
    // устройства попали в один wall-clock момент. Худые устройства
    // (BT) с большой intrinsic_latency получают compensation ≈ 0;
    // быстрые (wired) — compensation = max - intrinsic.
    //
    // Применение: сравниваем applied_comp_samples с target_samples,
    // дельту распределяем по чанкам ≤ chunk_len, чтобы не давать
    // dropouts при больших скачках слайдера.
    apply_compensation_delta(w, sync);

    // Peak — после всех gain-ов. Update before push to keep meter
    // в синхроне с тем, что реально пошло на устройство.
    update_peak(&w.interleaved_out, &w.peak_bits);

    let pushed = w.sink.producer.push_slice(&w.interleaved_out);
    if pushed < w.interleaved_out.len() {
        tracing::debug!(
            dropped = w.interleaved_out.len() - pushed,
            device = %w.sink.device_id,
            "output ringbuf full"
        );
    }
    if pushed > 0 {
        let now_micros = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_micros() as u64)
            .unwrap_or(0);
        w.last_push_micros.store(now_micros, Ordering::Relaxed);
    }
}

/// Constant-power pan law: theta = (balance + 1) * π / 4. При balance=0 →
/// 1/√2 на каждом канале, perceptual loudness не проседает.
/// Применяется in-place; для не-stereo skip.
fn apply_stereo_balance(buf: &mut [f32], channels: usize, balance: &AtomicU32) {
    if channels != 2 {
        return;
    }
    let b = f32::from_bits(balance.load(Ordering::Relaxed)).clamp(-1.0, 1.0);
    if b == 0.0 {
        // unity-эквивалент: сохраняем оригинальную loudness.
        // 1/√2 ≈ 0.7071 — не нужно трогать (centred = unity).
        // Но constant-power даёт 0.7071 для центра, а пользователь ожидает
        // unity. Поэтому при balance=0 — пропускаем (no-op).
        return;
    }
    let theta = (b + 1.0) * std::f32::consts::FRAC_PI_4;
    let l_gain = theta.cos() * std::f32::consts::SQRT_2;
    let r_gain = theta.sin() * std::f32::consts::SQRT_2;
    let frames = buf.len() / 2;
    for f in 0..frames {
        buf[f * 2] *= l_gain;
        buf[f * 2 + 1] *= r_gain;
    }
}

/// Линейный stretch буфера по фреймам: при positive correction добавляем
/// фреймы (растягиваем = замедляем = выход «уезжает» в будущее), при
/// negative — выкидываем. Делаем in-place через сдвиг хвоста.
///
/// Это самая дешёвая компенсация дрейфа: коэффициент маленький
/// (≤±0.1%), на 480-фреймовом чанке это <=1 фрейм за чанк, абсолютно
/// inaudible. Корректное решение — менять rubato ratio, но
/// `FastFixedIn` не поддерживает hot-update; делать это через
/// `SincFixedIn::set_resample_ratio` — отдельная задача.
fn apply_drift_stretch(buf: &mut Vec<f32>, channels: usize, correction: f32) {
    if channels == 0 || buf.is_empty() {
        return;
    }
    let frames = buf.len() / channels;
    // delta_frames > 0 → нужно вставить (замедлить); < 0 → выбросить.
    let delta = (frames as f32 * correction).round() as i32;
    if delta == 0 {
        return;
    }
    if delta > 0 {
        // Дублируем последний фрейм delta раз. Cap чтобы не вылезти.
        let n = (delta as usize).min(frames);
        let last_start = (frames - 1) * channels;
        for _ in 0..n {
            for ch in 0..channels {
                buf.push(buf[last_start + ch]);
            }
        }
    } else {
        let n = ((-delta) as usize).min(frames.saturating_sub(1));
        let new_len = (frames - n) * channels;
        buf.truncate(new_len);
    }
}

/// Привести `applied_comp_samples` к таргету `compensation_delay × out_sr ×
/// out_channels` за несколько worker-tick'ов.
///
/// - `delta > 0` (целевая компенсация выросла, нам надо «опоздать»
///   сильнее) → вставляем тишину в начало текущего чанка.
/// - `delta < 0` (компенсация уменьшилась) → выбрасываем нужное число
///   семплов из начала чанка (они «съедаются» — устройство догоняет).
///
/// Размер шага cap-нут половиной `interleaved_out.len()`: при больших
/// сдвигах latency (>10 мс единомоментно) изменение раскатывается на
/// несколько worker-тиков, чтобы не было полной тишины или скачка.
fn apply_compensation_delta(w: &mut WorkerOutput, sync: &Arc<SyncEngine>) {
    let params = match sync.device_params(&w.sink.device_id) {
        Some(p) => p,
        None => return,
    };
    let target_seconds = params.compensation_delay.as_secs_f64();
    let target_samples = (target_seconds * w.out_sr as f64 * w.out_channels as f64).round() as i64;
    // Округляем до полного frame, чтобы не сдвигать каналы относительно
    // друг друга при чётном out_channels.
    let ch = w.out_channels.max(1) as i64;
    let target_samples = target_samples - target_samples.rem_euclid(ch);

    let delta = target_samples - w.applied_comp_samples;
    if delta == 0 {
        return;
    }
    let buf_len = w.interleaved_out.len() as i64;
    if buf_len == 0 {
        return;
    }
    // Cap — не больше половины чанка, чтобы оставить и аудио-данные,
    // и место для тишины.
    let max_step = (buf_len / 2 / ch * ch).max(ch);
    let step = delta.clamp(-max_step, max_step);

    if step > 0 {
        // Вставить step семплов тишины в начало.
        let n = step as usize;
        // Сдвиг хвоста: расширяем Vec и копируем.
        let old_len = w.interleaved_out.len();
        w.interleaved_out.resize(old_len + n, 0.0);
        w.interleaved_out.copy_within(0..old_len, n);
        for s in &mut w.interleaved_out[..n] {
            *s = 0.0;
        }
    } else {
        // Съесть |step| семплов с начала.
        let n = (-step) as usize;
        if n >= w.interleaved_out.len() {
            w.interleaved_out.clear();
        } else {
            w.interleaved_out.drain(..n);
        }
    }
    w.applied_comp_samples += step;
}

/// Обновить peak-метр на основе текущего чанка. Линейный peak ∈ [0; ~2],
/// после которого экспоненциальное затухание (см. PEAK_DECAY_PER_TICK).
fn update_peak(buf: &[f32], peak_bits: &AtomicU32) {
    let mut max = 0.0_f32;
    for &s in buf {
        let a = s.abs();
        if a > max {
            max = a;
        }
    }
    let prev = f32::from_bits(peak_bits.load(Ordering::Relaxed));
    let decayed = prev * PEAK_DECAY_PER_TICK;
    let next = if max > decayed { max } else { decayed };
    peak_bits.store(next.to_bits(), Ordering::Relaxed);
}

/// Адаптирует каналы (mono↔stereo, downmix к меньшему N) и записывает
/// результат в `out`, переиспользуя его capacity. `out` очищается перед
/// заполнением — без переаллокаций, если capacity достаточен.
fn adapt_channels_into(src: &[f32], src_ch: usize, dst_ch: usize, out: &mut Vec<f32>) {
    out.clear();
    if src_ch == dst_ch {
        out.extend_from_slice(src);
        return;
    }
    let frames = src.len() / src_ch;
    for f in 0..frames {
        let base = f * src_ch;
        match (src_ch, dst_ch) {
            (2, 1) => out.push((src[base] + src[base + 1]) * 0.5),
            (1, 2) => {
                let v = src[base];
                out.push(v);
                out.push(v);
            }
            _ => {
                for ch in 0..dst_ch {
                    out.push(src[base + ch.min(src_ch - 1)]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atomic_f32(v: f32) -> AtomicU32 {
        AtomicU32::new(v.to_bits())
    }

    #[test]
    fn channel_adapt_stereo_to_mono_averages() {
        let src = vec![1.0, -1.0, 0.5, 0.5];
        let mut out = Vec::new();
        adapt_channels_into(&src, 2, 1, &mut out);
        assert_eq!(out, vec![0.0, 0.5]);
    }

    #[test]
    fn channel_adapt_mono_to_stereo_duplicates() {
        let src = vec![0.3, -0.4];
        let mut out = Vec::new();
        adapt_channels_into(&src, 1, 2, &mut out);
        assert_eq!(out, vec![0.3, 0.3, -0.4, -0.4]);
    }

    #[test]
    fn channel_adapt_same_count_passthrough() {
        let src = vec![0.1, 0.2, 0.3, 0.4];
        let mut out = Vec::new();
        adapt_channels_into(&src, 2, 2, &mut out);
        assert_eq!(out, src);
    }

    #[test]
    fn balance_centre_is_no_op() {
        let mut buf = vec![0.5, 0.5, -0.25, -0.25];
        let bal = atomic_f32(0.0);
        apply_stereo_balance(&mut buf, 2, &bal);
        assert_eq!(buf, vec![0.5, 0.5, -0.25, -0.25]);
    }

    #[test]
    fn balance_full_left_zeros_right() {
        let mut buf = vec![1.0, 1.0];
        let bal = atomic_f32(-1.0);
        apply_stereo_balance(&mut buf, 2, &bal);
        // L should be √2·1 ≈ 1.414, R should be 0.
        assert!((buf[0] - std::f32::consts::SQRT_2).abs() < 1e-5);
        assert!(buf[1].abs() < 1e-5);
    }

    #[test]
    fn balance_full_right_zeros_left() {
        let mut buf = vec![1.0, 1.0];
        let bal = atomic_f32(1.0);
        apply_stereo_balance(&mut buf, 2, &bal);
        assert!(buf[0].abs() < 1e-5);
        assert!((buf[1] - std::f32::consts::SQRT_2).abs() < 1e-5);
    }

    #[test]
    fn balance_skips_non_stereo() {
        let mut buf = vec![0.5_f32; 8];
        let bal = atomic_f32(-1.0);
        apply_stereo_balance(&mut buf, 4, &bal);
        // mono/4-ch — no-op.
        assert!(buf.iter().all(|s| (*s - 0.5).abs() < 1e-6));
    }

    #[test]
    fn drift_stretch_zero_correction_is_noop() {
        let mut buf = vec![0.1, 0.2, 0.3, 0.4];
        let original = buf.clone();
        apply_drift_stretch(&mut buf, 2, 0.0);
        assert_eq!(buf, original);
    }

    #[test]
    fn drift_stretch_positive_inserts_frames() {
        // 100 frames * 2 channels, correction=0.05 → 5 extra frames.
        let mut buf = vec![1.0; 200];
        apply_drift_stretch(&mut buf, 2, 0.05);
        assert_eq!(buf.len(), 210);
    }

    #[test]
    fn drift_stretch_negative_drops_frames() {
        let mut buf = vec![1.0; 200];
        apply_drift_stretch(&mut buf, 2, -0.05);
        assert_eq!(buf.len(), 190);
    }

    #[test]
    fn peak_decays_after_silence() {
        let peak = AtomicU32::new(0.0_f32.to_bits());
        update_peak(&[0.8, -0.8], &peak);
        assert!((f32::from_bits(peak.load(Ordering::Relaxed)) - 0.8).abs() < 1e-6);
        // Тишина — должно затухнуть до prev * decay.
        update_peak(&[0.0; 100], &peak);
        let v = f32::from_bits(peak.load(Ordering::Relaxed));
        assert!(v < 0.8 && v > 0.0);
    }

    #[test]
    fn peak_holds_max() {
        let peak = AtomicU32::new(0.0_f32.to_bits());
        update_peak(&[0.3], &peak);
        update_peak(&[0.9, -0.5], &peak);
        let v = f32::from_bits(peak.load(Ordering::Relaxed));
        assert!((v - 0.9).abs() < 1e-6);
    }
}
