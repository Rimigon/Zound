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
//
// CoreAudio сам по себе loopback не даёт (нет аналога WASAPI
// AUDCLNT_STREAMFLAGS_LOOPBACK), но ScreenCaptureKit — Apple-вский API для
// скринкастов — отдаёт системное аудио как отдельный output. Без kext-а и
// без виртуального драйвера. При первом запуске система попросит разрешение
// на запись экрана (даже если мы пишем только аудио).
//
// Важная тонкость: SCK нельзя попросить «только аудио». Мы всё равно
// запускаем видео-поток с минимальным разрешением (2×2), а frame-ы молча
// выбрасываем в no-op handler. Аудио получаем через отдельный handler
// с `SCStreamOutputType::Audio`.

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use parking_lot::Mutex;
    use ringbuf::traits::{Producer, Split};
    use ringbuf::{HeapProd, HeapRb};
    use screencapturekit::cm::AudioBufferList;
    use screencapturekit::prelude::*;
    use std::sync::Arc;

    /// Drop-guard для SCStream: вызывает stop_capture перед тем, как
    /// отпустить стрим. SCStream сам по себе в Drop останавливается, но
    /// явный stop позволяет залогировать ошибку при сбое.
    struct MacHandle {
        stream: Option<SCStream>,
    }

    impl CaptureHandle for MacHandle {}

    impl Drop for MacHandle {
        fn drop(&mut self) {
            if let Some(stream) = self.stream.take() {
                if let Err(e) = stream.stop_capture() {
                    tracing::warn!(?e, "SCStream stop_capture failed");
                }
            }
        }
    }

    /// Handler на `SCStreamOutputType::Audio` — извлекает PCM-байты из
    /// CMSampleBuffer и пушит в ringbuf. `SCStreamOutputTrait` требует
    /// только `Send + Sync` от реализации.
    struct AudioHandler {
        producer: Arc<Mutex<HeapProd<f32>>>,
    }

    impl SCStreamOutputTrait for AudioHandler {
        fn did_output_sample_buffer(
            &self,
            sample: CMSampleBuffer,
            output_type: SCStreamOutputType,
        ) {
            if !matches!(output_type, SCStreamOutputType::Audio) {
                return;
            }
            let Some(list) = sample.audio_buffer_list() else {
                return;
            };
            for buffer in list.iter() {
                let bytes = buffer.data();
                // SCK отдаёт Float32 в native-endian. Копируем чанками,
                // чтобы не лочить producer на весь буфер зараз.
                const CHUNK: usize = 1024;
                let mut tmp = [0.0_f32; CHUNK];
                let mut i = 0;
                while i + 4 <= bytes.len() {
                    let end = (i + CHUNK * 4).min(bytes.len());
                    let slice = &bytes[i..end];
                    let n = slice.len() / 4;
                    for (k, chunk) in slice.chunks_exact(4).enumerate() {
                        tmp[k] =
                            f32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                    }
                    let mut prod = self.producer.lock();
                    let pushed = prod.push_slice(&tmp[..n]);
                    if pushed < n {
                        static OVERRUN: std::sync::atomic::AtomicU32 =
                            std::sync::atomic::AtomicU32::new(0);
                        let c = OVERRUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if c % 100 == 0 {
                            tracing::warn!(
                                dropped = n - pushed,
                                total = c + 1,
                                "sc capture overrun"
                            );
                        }
                    }
                    i = end;
                }
            }
        }
    }

    /// Пустой handler на `Screen`-поток. SCK не умеет «только аудио»,
    /// кадры приходят — мы их молча дропаем.
    struct VideoNoOp;
    impl SCStreamOutputTrait for VideoNoOp {
        fn did_output_sample_buffer(
            &self,
            _sample: CMSampleBuffer,
            _output_type: SCStreamOutputType,
        ) {
        }
    }

    pub fn open(opts: CaptureOpts) -> Result<Capture> {
        // 1. Anchor-дисплей для ContentFilter. SCK требует хотя бы один.
        let content = SCShareableContent::get()
            .map_err(|e| Error::Backend(format!("SCShareableContent::get: {e:?}")))?;
        let display = content
            .displays()
            .into_iter()
            .next()
            .ok_or_else(|| Error::Backend("no displays available for SCK".into()))?;

        // 2. Фильтр: весь дисплей, без исключений окон.
        let filter = SCContentFilter::create()
            .with_display(&display)
            .with_excluding_windows(&[])
            .build();

        // 3. Конфиг: минимальное видео (2×2) + stereo 48k audio,
        // исключаем собственный процесс (защита от feedback-loop на уровне SDK).
        let channels: u16 = 2;
        let sample_rate: u32 = 48_000;
        let config = SCStreamConfiguration::new()
            .with_width(2)
            .with_height(2)
            .with_captures_audio(true)
            .with_sample_rate(sample_rate as i32)
            .with_channel_count(channels as i32)
            .with_excludes_current_process_audio(true);

        // 4. Ringbuf + handlers.
        let rb = HeapRb::<f32>::new(opts.buffer_samples);
        let (producer, consumer) = rb.split();
        let producer = Arc::new(Mutex::new(producer));

        let mut stream = SCStream::new(&filter, &config);
        stream.add_output_handler(VideoNoOp, SCStreamOutputType::Screen);
        stream.add_output_handler(
            AudioHandler {
                producer: producer.clone(),
            },
            SCStreamOutputType::Audio,
        );

        stream
            .start_capture()
            .map_err(|e| Error::Backend(format!("SCStream start_capture: {e:?}")))?;

        tracing::info!(
            %sample_rate, %channels,
            "ScreenCaptureKit loopback capture started"
        );

        // Для блокера feedback-loop нужно реальное имя текущего дефолтного
        // output (SCK тапит системный звук независимо от устройства; если
        // юзер добавит этот же дефолт как Zound-output — задвоится).
        // Если CoreAudio дефолт не возвращает — оставляем константу
        // (блокер тогда просто не сработает, как было).
        let source_name = crate::macos_devices::default_output_device_name()
            .unwrap_or_else(|| "system audio (ScreenCaptureKit)".into());

        Ok(Capture {
            session: CaptureSession {
                _handle: Box::new(MacHandle {
                    stream: Some(stream),
                }),
                channels,
                sample_rate,
                source_name,
            },
            consumer,
        })
    }

    // Подавляем warning, что AudioBufferList не используется напрямую —
    // мы трогаем его через `.iter()` возвращённый из `audio_buffer_list()`.
    #[allow(dead_code)]
    type _AudioBufferListAlias = AudioBufferList;
}
