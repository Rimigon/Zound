//! Захват системного аудио.
//!
//! Публичный API — [`open_default_loopback`] — возвращает одинаковый
//! [`Capture`] независимо от платформы. Реализация под капотом:
//!
//! - **Windows**: WASAPI loopback через cpal (`build_input_stream` на
//!   output-устройстве — cpal неявно ставит `AUDCLNT_STREAMFLAGS_LOOPBACK`).
//! - **Linux**: PulseAudio / PipeWire через libpulse-simple; читаем с
//!   `<default_sink>.monitor` — виртуальный source, который даёт то, что
//!   играет sink. PipeWire предоставляет Pulse-совместимый API, тот же код
//!   работает и на чистом Pulse, и на PipeWire-системах.
//! - **macOS**: ScreenCaptureKit (macOS 13+) — единственный способ забрать
//!   системное аудио без kext-а / виртуального драйвера.

use ringbuf::HeapCons;
use zound_core::{Error, Result};

/// Активная сессия захвата. Держит платформенный handle живым — пока
/// structure существует, сэмплы пишутся в ringbuf. Drop останавливает
/// захват.
pub struct CaptureSession {
    _handle: Box<dyn CaptureHandle>,
    pub channels: u16,
    pub sample_rate: u32,
    /// Имя устройства / источника, с которого снимается loopback. Нужно,
    /// чтобы заблокировать добавление его же в output-набор (иначе feedback).
    pub source_name: String,
}

/// Пара «сессия + consumer». Consumer (Send) можно отдать на другой
/// поток для чтения сэмплов; сессия остаётся на исходном.
pub struct Capture {
    pub session: CaptureSession,
    pub consumer: HeapCons<f32>,
}

/// Параметры захвата.
#[derive(Debug, Clone)]
pub struct CaptureOpts {
    /// Ёмкость ringbuf-а (в interleaved-сэмплах). Запас под джиттер.
    pub buffer_samples: usize,
}

impl Default for CaptureOpts {
    fn default() -> Self {
        // ~250 мс stereo 48k = 24000 сэмплов. Берём в 4 раза больше.
        Self {
            buffer_samples: 96_000,
        }
    }
}

/// Type-erased владелец платформенных ресурсов (cpal::Stream / thread /
/// SCStream). Trait пустой: всю работу делает `Drop` конкретного типа.
/// `Send` не требуется — `Capture` всегда живёт на одном audio-thread-е.
trait CaptureHandle {}

/// Открыть loopback-захват дефолтного output-устройства системы.
/// Возвращает сессию (владеет потоком) и consumer для чтения сэмплов.
pub fn open_default_loopback(opts: CaptureOpts) -> Result<Capture> {
    #[cfg(target_os = "windows")]
    {
        windows::open(opts)
    }
    #[cfg(target_os = "linux")]
    {
        linux::open(opts)
    }
    #[cfg(target_os = "macos")]
    {
        macos::open(opts)
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        let _ = opts;
        Err(Error::Backend(
            "loopback capture is not implemented on this platform".into(),
        ))
    }
}

// ---------------------------------------------------------------------------
// Windows — WASAPI loopback через cpal
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use crate::sample_convert::{copy_as_f32, CopySource};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{SampleFormat, Stream};
    use parking_lot::Mutex;
    use ringbuf::traits::{Producer, Split};
    use ringbuf::{HeapProd, HeapRb};
    use std::sync::Arc;

    impl CaptureHandle for Stream {}

    pub fn open(opts: CaptureOpts) -> Result<Capture> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| Error::Backend("no default output device for loopback".into()))?;

        let source_name = device.name().unwrap_or_else(|_| "default".to_string());

        let supported = device
            .default_output_config()
            .map_err(|e| Error::Backend(format!("default_output_config: {e}")))?;
        let sample_rate = supported.sample_rate().0;
        let channels = supported.channels();
        let format = supported.sample_format();

        let config = cpal::StreamConfig {
            channels,
            sample_rate: cpal::SampleRate(sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        let rb = HeapRb::<f32>::new(opts.buffer_samples);
        let (producer, consumer) = rb.split();
        let producer = Arc::new(Mutex::new(producer));

        let err_cb = |e| tracing::error!(?e, "capture stream error");
        let stream = build_input_stream(&device, &config, format, producer, err_cb)?;
        stream
            .play()
            .map_err(|e| Error::Backend(format!("stream.play: {e}")))?;

        tracing::info!(
            %sample_rate, %channels, ?format, source = %source_name,
            "WASAPI loopback capture started"
        );

        Ok(Capture {
            session: CaptureSession {
                _handle: Box::new(stream),
                channels,
                sample_rate,
                source_name,
            },
            consumer,
        })
    }

    fn build_input_stream<E>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        format: SampleFormat,
        producer: Arc<Mutex<HeapProd<f32>>>,
        err_cb: E,
    ) -> Result<Stream>
    where
        E: FnMut(cpal::StreamError) + Send + 'static,
    {
        macro_rules! build {
            ($ty:ty) => {{
                let prod = producer.clone();
                device
                    .build_input_stream(
                        config,
                        move |data: &[$ty], _info: &cpal::InputCallbackInfo| {
                            write_samples(prod.clone(), CopySource::from(data));
                        },
                        err_cb,
                        None,
                    )
                    .map_err(|e| Error::Backend(format!("build_input_stream: {e}")))
            }};
        }
        match format {
            SampleFormat::F32 => build!(f32),
            SampleFormat::I16 => build!(i16),
            SampleFormat::U16 => build!(u16),
            SampleFormat::I32 => build!(i32),
            other => Err(Error::Backend(format!(
                "unsupported capture format: {other}"
            ))),
        }
    }

    fn write_samples(producer: Arc<Mutex<HeapProd<f32>>>, src: CopySource<'_>) {
        const CHUNK: usize = 1024;
        let mut tmp = [0.0_f32; CHUNK];
        let mut remaining = src.len();
        let mut offset = 0;
        while remaining > 0 {
            let n = remaining.min(CHUNK);
            copy_as_f32(&src, offset, &mut tmp[..n]);
            let mut prod = producer.lock();
            let pushed = prod.push_slice(&tmp[..n]);
            if pushed < n {
                static OVERRUN_COUNT: std::sync::atomic::AtomicU32 =
                    std::sync::atomic::AtomicU32::new(0);
                let c = OVERRUN_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if c % 100 == 0 {
                    tracing::warn!(
                        dropped = n - pushed,
                        total_overruns = c + 1,
                        "capture overrun"
                    );
                }
            }
            offset += n;
            remaining -= n;
        }
    }
}

// ---------------------------------------------------------------------------
// Linux — PulseAudio / PipeWire monitor source
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use ringbuf::traits::{Producer, Split};
    use ringbuf::HeapRb;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread::{self, JoinHandle};

    use libpulse_binding::context::introspect::ServerInfo;
    use libpulse_binding::context::{Context, FlagSet as ContextFlags, State as ContextState};
    use libpulse_binding::mainloop::standard::{IterateResult, Mainloop};
    use libpulse_binding::proplist::{properties, Proplist};
    use libpulse_binding::sample::{Format as SampleFmt, Spec};
    use libpulse_binding::stream::Direction;
    use libpulse_simple_binding::Simple;

    const APP_NAME: &str = "Zound";
    const STREAM_NAME: &str = "system capture";

    /// Handle для Linux backend: выставляет `stop` и ждёт thread.
    struct LinuxHandle {
        stop: Arc<AtomicBool>,
        join: Option<JoinHandle<()>>,
    }

    impl CaptureHandle for LinuxHandle {}

    impl Drop for LinuxHandle {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(j) = self.join.take() {
                let _ = j.join();
            }
        }
    }

    pub fn open(opts: CaptureOpts) -> Result<Capture> {
        // 1. Узнаём имя default sink-а, чтобы сформировать <sink>.monitor.
        let default_sink = discover_default_sink()
            .map_err(|e| Error::Backend(format!("pulse discover default sink: {e}")))?;
        let monitor_source = format!("{default_sink}.monitor");

        // 2. Открываем blocking record-stream на monitor.
        let channels: u16 = 2;
        let sample_rate: u32 = 48_000;
        let spec = Spec {
            format: SampleFmt::FLOAT32NE,
            channels: channels as u8,
            rate: sample_rate,
        };
        if !spec.is_valid() {
            return Err(Error::Backend("invalid pulse sample spec".into()));
        }

        let simple = Simple::new(
            None,                    // сервер по умолчанию
            APP_NAME,                // application name
            Direction::Record,       //
            Some(&monitor_source),   //
            STREAM_NAME,             //
            &spec,                   //
            None,                    // default channel map
            None,                    // default buffer attrs
        )
        .map_err(|e| Error::Backend(format!("pulse Simple::new({monitor_source}): {e}")))?;

        // 3. Ringbuf + поток-читатель.
        let rb = HeapRb::<f32>::new(opts.buffer_samples);
        let (mut producer, consumer) = rb.split();

        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = stop.clone();

        // Читаем ~10 мс за проход: 480 frames * 2 ch * 4 bytes = 3840 bytes.
        const FRAMES_PER_READ: usize = 480;
        let samples_per_read = FRAMES_PER_READ * channels as usize;
        let bytes_per_read = samples_per_read * std::mem::size_of::<f32>();

        let join = thread::Builder::new()
            .name("zound-pulse-capture".into())
            .spawn(move || {
                let mut byte_buf = vec![0u8; bytes_per_read];
                let mut float_buf = vec![0.0_f32; samples_per_read];
                while !stop_for_thread.load(Ordering::Relaxed) {
                    if let Err(e) = simple.read(&mut byte_buf) {
                        tracing::error!(?e, "pulse read");
                        break;
                    }
                    // FLOAT32NE = native-endian f32, трактуем побайтово.
                    for (i, chunk) in byte_buf.chunks_exact(4).enumerate() {
                        float_buf[i] = f32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                    }
                    let pushed = producer.push_slice(&float_buf);
                    if pushed < float_buf.len() {
                        static OVERRUN: std::sync::atomic::AtomicU32 =
                            std::sync::atomic::AtomicU32::new(0);
                        let c = OVERRUN.fetch_add(1, Ordering::Relaxed);
                        if c % 100 == 0 {
                            tracing::warn!(
                                dropped = float_buf.len() - pushed,
                                total = c + 1,
                                "pulse capture overrun"
                            );
                        }
                    }
                }
                tracing::info!("pulse capture thread exiting");
            })
            .map_err(|e| Error::Backend(format!("spawn pulse thread: {e}")))?;

        tracing::info!(
            %sample_rate, %channels, source = %monitor_source,
            "PulseAudio/PipeWire loopback capture started"
        );

        Ok(Capture {
            session: CaptureSession {
                _handle: Box::new(LinuxHandle {
                    stop,
                    join: Some(join),
                }),
                channels,
                sample_rate,
                source_name: monitor_source,
            },
            consumer,
        })
    }

    /// Запрашиваем у Pulse имя default sink-а через async context API.
    /// Блокирующе итерируем mainloop до получения ServerInfo.
    fn discover_default_sink() -> std::result::Result<String, String> {
        let mut proplist = Proplist::new().ok_or_else(|| "proplist alloc".to_string())?;
        proplist
            .set_str(properties::APPLICATION_NAME, APP_NAME)
            .map_err(|_| "proplist set".to_string())?;

        let mut mainloop = Mainloop::new().ok_or_else(|| "mainloop alloc".to_string())?;
        let mut context = Context::new_with_proplist(&mainloop, "zound-discovery", &proplist)
            .ok_or_else(|| "context alloc".to_string())?;

        context
            .connect(None, ContextFlags::NOFLAGS, None)
            .map_err(|e| format!("context.connect: {e:?}"))?;

        // Ждём READY.
        loop {
            match mainloop.iterate(true) {
                IterateResult::Err(e) => return Err(format!("mainloop iterate: {e:?}")),
                IterateResult::Quit(_) => return Err("mainloop quit".into()),
                IterateResult::Success(_) => {}
            }
            match context.get_state() {
                ContextState::Ready => break,
                ContextState::Failed | ContextState::Terminated => {
                    return Err("context failed".into())
                }
                _ => {}
            }
        }

        // Запрашиваем ServerInfo.
        let result = std::rc::Rc::new(std::cell::RefCell::new(None::<Option<String>>));
        let result_cb = result.clone();
        let op = context
            .introspect()
            .get_server_info(move |info: &ServerInfo| {
                let name = info
                    .default_sink_name
                    .as_ref()
                    .map(|s| s.to_string());
                *result_cb.borrow_mut() = Some(name);
            });

        // Крутим до завершения операции.
        while op.get_state() == libpulse_binding::operation::State::Running {
            match mainloop.iterate(true) {
                IterateResult::Err(e) => return Err(format!("mainloop iterate: {e:?}")),
                IterateResult::Quit(_) => return Err("mainloop quit".into()),
                IterateResult::Success(_) => {}
            }
        }

        let name = result
            .borrow_mut()
            .take()
            .flatten()
            .ok_or_else(|| "no default sink in ServerInfo".to_string())?;

        context.disconnect();
        Ok(name)
    }
}

// ---------------------------------------------------------------------------
// macOS — ScreenCaptureKit (macOS 13+)
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use ringbuf::traits::{Producer, Split};
    use ringbuf::{HeapProd, HeapRb};
    use std::sync::Arc;

    use parking_lot::Mutex;
    use screencapturekit::{
        cm_sample_buffer::CMSampleBuffer,
        sc_content_filter::{InitParams, SCContentFilter},
        sc_error_handler::StreamErrorHandler,
        sc_output_handler::{SCStreamOutputType, StreamOutput},
        sc_shareable_content::SCShareableContent,
        sc_stream::SCStream,
        sc_stream_configuration::SCStreamConfiguration,
    };

    struct MacHandle {
        _stream: SCStream,
    }

    impl CaptureHandle for MacHandle {}

    struct AudioSink {
        producer: Arc<Mutex<HeapProd<f32>>>,
        channels: u16,
    }

    struct ErrorLogger;

    impl StreamErrorHandler for ErrorLogger {
        fn on_error(&self) {
            tracing::error!("ScreenCaptureKit stream error");
        }
    }

    impl StreamOutput for AudioSink {
        fn did_output_sample_buffer(&self, sample: CMSampleBuffer, of_type: SCStreamOutputType) {
            if !matches!(of_type, SCStreamOutputType::Audio) {
                return;
            }
            // Извлекаем PCM-данные. CMSampleBuffer даёт AudioBufferList-подобный
            // слайс байт; ScreenCaptureKit на macOS отдаёт Float32 non-interleaved
            // или interleaved в зависимости от конфига — мы просим interleaved.
            let Ok(audio_buffers) = sample.sys_ref.get_av_audio_buffer_list() else {
                return;
            };
            for buf in audio_buffers.iter() {
                let bytes = buf.data();
                // FLOAT32 → интерпретируем напрямую.
                let samples: &[f32] = unsafe {
                    std::slice::from_raw_parts(
                        bytes.as_ptr() as *const f32,
                        bytes.len() / std::mem::size_of::<f32>(),
                    )
                };
                let mut prod = self.producer.lock();
                let pushed = prod.push_slice(samples);
                if pushed < samples.len() {
                    static OVERRUN: std::sync::atomic::AtomicU32 =
                        std::sync::atomic::AtomicU32::new(0);
                    let c = OVERRUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if c % 100 == 0 {
                        tracing::warn!(
                            dropped = samples.len() - pushed,
                            total = c + 1,
                            channels = self.channels,
                            "sc capture overrun"
                        );
                    }
                }
            }
        }
    }

    pub fn open(opts: CaptureOpts) -> Result<Capture> {
        // Нужен дисплей как «anchor» для ContentFilter — аудио сам по себе
        // захватить нельзя, но SCStream допускает «без видео-output-а».
        let content = SCShareableContent::current();
        let display = content
            .displays
            .into_iter()
            .next()
            .ok_or_else(|| Error::Backend("no displays available for ScreenCaptureKit".into()))?;

        let channels: u16 = 2;
        let sample_rate: u32 = 48_000;

        let config = SCStreamConfiguration {
            width: 2,
            height: 2,
            captures_audio: true,
            sample_rate: sample_rate as i32,
            channel_count: channels as i32,
            excludes_current_process_audio: true,
            ..Default::default()
        };

        let filter = SCContentFilter::new(InitParams::Display(display));
        let mut stream = SCStream::new(filter, config, ErrorLogger {});

        let rb = HeapRb::<f32>::new(opts.buffer_samples);
        let (producer, consumer) = rb.split();
        let producer = Arc::new(Mutex::new(producer));

        stream.add_output(
            AudioSink {
                producer: producer.clone(),
                channels,
            },
            SCStreamOutputType::Audio,
        );

        stream
            .start_capture()
            .map_err(|e| Error::Backend(format!("sc start_capture: {e:?}")))?;

        tracing::info!(
            %sample_rate, %channels,
            "ScreenCaptureKit loopback capture started"
        );

        Ok(Capture {
            session: CaptureSession {
                _handle: Box::new(MacHandle { _stream: stream }),
                channels,
                sample_rate,
                source_name: "system audio (ScreenCaptureKit)".into(),
            },
            consumer,
        })
    }
}
