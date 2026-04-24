//! Захват системного аудио.
//!
//! На Windows через WASAPI cpal умеет loopback: если вызвать
//! `build_input_stream` на *output*-устройстве, мы получим поток с того, что
//! играет система. На macOS/Linux этот приём не работает — нужны отдельные
//! backend-ы (ScreenCaptureKit / PipeWire monitor). Для MVP — только Windows.

use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use parking_lot::Mutex;
use ringbuf::traits::{Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};
use zound_core::{Error, Result};

use crate::sample_convert::{copy_as_f32, CopySource};

/// Активная сессия захвата. Держит cpal-стрим живым — пока structure
/// существует, callback вызывается. cpal::Stream на WASAPI не `Send`,
/// поэтому сессия должна оставаться на том же потоке, что её создал.
pub struct CaptureSession {
    _stream: Stream,
    pub channels: u16,
    pub sample_rate: u32,
    /// Имя устройства, с которого снимается loopback. Нужно, чтобы
    /// заблокировать добавление его же в output-набор (иначе feedback).
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

/// Открыть loopback-захват дефолтного output-устройства системы.
/// Возвращает сессию (владеет стримом) и consumer для чтения сэмплов.
pub fn open_default_loopback(opts: CaptureOpts) -> Result<Capture> {
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

    tracing::info!(%sample_rate, %channels, ?format, source = %source_name, "loopback capture started");

    Ok(Capture {
        session: CaptureSession {
            _stream: stream,
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

/// Realtime-callback. Никаких аллокаций; lock parking_lot::Mutex —
/// в SPSC-сценарии деграддируется до spin, что приемлемо для нашего случая.
/// Альтернатива — atomic swap producer-а, но усложняет код.
fn write_samples(producer: Arc<Mutex<HeapProd<f32>>>, src: CopySource<'_>) {
    // Небольшой буфер на стеке, чтобы не аллоцировать.
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
            // Overrun — consumer не успевает. Старые данные уже улетели;
            // логируем редко (без аллокаций), реальное решение — выше по стеку.
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
