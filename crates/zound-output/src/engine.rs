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

use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use parking_lot::{Mutex, RwLock};
use ringbuf::traits::{Consumer, Observer, Producer};
use rubato::{FastFixedIn, PolynomialDegree, Resampler};
use zound_core::{DeviceId, Error, Result};
use zound_platform::{Capture, CaptureOpts, OutputOpts, OutputSink};
use zound_sync::SyncEngine;

use crate::OutputManager;

/// Размер чанка, который worker обрабатывает за один проход (в frames
/// на канал). 480 @ 48 kHz = 10 мс — типичная гранулярность callback-ов.
const WORKER_CHUNK_FRAMES: usize = 480;

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
    _sync: Arc<SyncEngine>,
    _outputs: Arc<OutputManager>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl AudioEngine {
    pub fn new(sync: Arc<SyncEngine>, outputs: Arc<OutputManager>) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let active_ids = Arc::new(RwLock::new(Vec::<DeviceId>::new()));
        let loopback_source = Arc::new(RwLock::new(None::<String>));
        let running = Arc::new(RwLock::new(false));

        let t_sync = sync.clone();
        let t_outputs = outputs.clone();
        let t_active = active_ids.clone();
        let t_loop = loopback_source.clone();
        let t_running = running.clone();
        let handle = thread::Builder::new()
            .name("zound-audio".into())
            .spawn(move || audio_thread(cmd_rx, t_sync, t_outputs, t_active, t_loop, t_running))
            .expect("spawn audio thread");

        Self {
            cmd_tx,
            active_ids,
            loopback_source,
            running,
            _sync: sync,
            _outputs: outputs,
            thread: Mutex::new(Some(handle)),
        }
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

/// Специальная ошибка: запрос добавить default-устройство как output —
/// приведёт к feedback loop, поэтому отклоняем на входе.
pub const ERR_FEEDBACK_LOOP: &str = "feedback-default-blocked";

// ---------- внутри audio-thread ---------- //

struct WorkerOutput {
    sink: OutputSink,
    resampler: Option<ResampleCtx>,
}

struct ResampleCtx {
    resampler: FastFixedIn<f32>,
    input_planar: Vec<Vec<f32>>,
    output_planar: Vec<Vec<f32>>,
    input_frames: usize,
    max_output_frames: usize,
}

fn audio_thread(
    cmd_rx: Receiver<EngineCmd>,
    sync: Arc<SyncEngine>,
    outputs: Arc<OutputManager>,
    active_ids: Arc<RwLock<Vec<DeviceId>>>,
    loopback_source: Arc<RwLock<Option<String>>>,
    running: Arc<RwLock<bool>>,
) {
    let mut capture: Option<Capture> = None;
    let mut workers: Vec<WorkerOutput> = Vec::new();
    let mut interleaved: Vec<f32> = Vec::new();
    let mut planar: Vec<Vec<f32>> = Vec::new();

    let tick_sleep = Duration::from_millis(2);

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
                    // Блокируем добавление source-устройства, чтобы не было feedback.
                    let is_source = capture
                        .as_ref()
                        .map(|c| c.session.source_name == device_name)
                        .unwrap_or(false);
                    let res = if is_source {
                        Err(ERR_FEEDBACK_LOOP.to_string())
                    } else {
                        handle_add_output(
                            &capture,
                            &mut workers,
                            &sync,
                            &outputs,
                            &active_ids,
                            &device_name,
                        )
                    };
                    let _ = reply.send(res);
                }
                Ok(EngineCmd::RemoveOutput { id, reply }) => {
                    workers.retain(|w| w.sink.device_id != id);
                    sync.remove_device(&id);
                    outputs.remove(&id);
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
                for w in workers.iter_mut() {
                    push_to_output(w, &interleaved, &planar, cap.session.channels as usize);
                }
            } else {
                thread::sleep(tick_sleep);
            }
        } else {
            thread::sleep(tick_sleep);
        }
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

fn handle_add_output(
    capture: &Option<Capture>,
    workers: &mut Vec<WorkerOutput>,
    sync: &Arc<SyncEngine>,
    outputs: &Arc<OutputManager>,
    active_ids: &Arc<RwLock<Vec<DeviceId>>>,
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

    sync.add_device(device_id.clone(), intrinsic, sink.sample_rate);
    outputs.add(device_id.clone());
    active_ids.write().push(device_id.clone());

    workers.push(WorkerOutput { sink, resampler });
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
) {
    let out_channels = w.sink.channels as usize;

    match w.resampler.as_mut() {
        None => {
            let adapted = adapt_channels(interleaved_src, cap_channels, out_channels);
            let pushed = w.sink.producer.push_slice(&adapted);
            if pushed < adapted.len() {
                tracing::debug!(
                    dropped = adapted.len() - pushed,
                    device = %w.sink.device_id,
                    "output ringbuf full"
                );
            }
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
            let mut out_interleaved = Vec::with_capacity(out_frames * out_channels);
            for f in 0..out_frames {
                for ch in 0..out_channels {
                    let src_ch = ch.min(ctx.output_planar.len() - 1);
                    out_interleaved.push(ctx.output_planar[src_ch][f]);
                }
            }
            let pushed = w.sink.producer.push_slice(&out_interleaved);
            if pushed < out_interleaved.len() {
                tracing::debug!(
                    dropped = out_interleaved.len() - pushed,
                    device = %w.sink.device_id,
                    "resampled output ringbuf full"
                );
            }
        }
    }
}

fn adapt_channels(src: &[f32], src_ch: usize, dst_ch: usize) -> Vec<f32> {
    if src_ch == dst_ch {
        return src.to_vec();
    }
    let frames = src.len() / src_ch;
    let mut out = Vec::with_capacity(frames * dst_ch);
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
    out
}
