//! Тест-сигналы на отдельный output-стрим.
//!
//! Открывает второй cpal::Stream на устройстве, в callback'е генерирует
//! сигнал (Click / Sine 1kHz / Metronome). Не идёт через capture loopback
//! — сам себя поймать не может. Громкость теста зашита (0.5), не зависит
//! от per-device volume.

use std::f32::consts::TAU;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use zound_core::{DeviceId, Error, Result};

use crate::output::resolve_output_config_pub as resolve_output_config;
use crate::sample_convert::FromF32;

/// Тип тест-сигнала.
#[derive(Debug, Clone, Copy)]
pub enum TestKind {
    /// Одиночный 5 мс импульс (квадратная волна), потом тишина и stop.
    Click,
    /// 5 секунд тон 1 кГц, потом stop.
    Sine1kHz,
    /// Метроном: периодические клики до Stop. BPM 40-240.
    Metronome { bpm: u16 },
}

/// Активный тест-стрим. Drop останавливает cpal::Stream.
pub struct TestStream {
    _stream: Stream,
    pub device_id: DeviceId,
    pub device_name: String,
    pub kind: TestKind,
    /// `true` → callback пишет тишину и стрим помечен как «можно дропнуть».
    /// Используется и для self-termination (Click/Sine отыграли) и для
    /// внешнего Stop (UI-команда).
    pub stop_flag: Arc<AtomicBool>,
}

impl TestStream {
    /// Стрим помечен как остановленный (сам собой или по команде).
    pub fn is_done(&self) -> bool {
        self.stop_flag.load(Ordering::Relaxed)
    }
}

/// Зафиксированная амплитуда тест-сигнала. Не зависит от per-device volume,
/// чтобы калибровка не зависела от пользовательских настроек.
const TEST_AMPLITUDE: f32 = 0.5;

/// Длина одиночного «щелчка» — короткий импульс ~5 мс @ 48 kHz.
const CLICK_DURATION_MS: f32 = 5.0;

/// Длина sine-сигнала.
const SINE_DURATION_SEC: f32 = 5.0;

/// Частота sine-тона.
const SINE_FREQ_HZ: f32 = 1000.0;

/// Открыть тест-стрим на устройстве по имени.
pub fn start_test_stream(device_name: &str, kind: TestKind) -> Result<TestStream> {
    let host = cpal::default_host();
    // На macOS используем `host.devices()` (без cpal-фильтра) — см.
    // комментарий в `output::open_output_by_name`.
    #[cfg(target_os = "macos")]
    let mut devices = host
        .devices()
        .map_err(|e| Error::Backend(format!("devices: {e}")))?;
    #[cfg(not(target_os = "macos"))]
    let mut devices = host
        .output_devices()
        .map_err(|e| Error::Backend(format!("output_devices: {e}")))?;

    let device = devices
        .find(|d| d.name().map(|n| n == device_name).unwrap_or(false))
        .ok_or_else(|| Error::DeviceNotFound(device_name.to_string()))?;

    let (sample_rate, channels, format) = resolve_output_config(&device, device_name)?;

    let config = cpal::StreamConfig {
        channels,
        sample_rate: cpal::SampleRate(sample_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    let stop_flag = Arc::new(AtomicBool::new(false));

    let stream = build_test_stream(
        &device,
        &config,
        format,
        kind,
        sample_rate,
        channels,
        stop_flag.clone(),
    )?;

    stream
        .play()
        .map_err(|e| Error::Backend(format!("test stream.play: {e}")))?;

    let resolved_name = device.name().unwrap_or_else(|_| device_name.to_string());
    tracing::info!(?kind, name = %resolved_name, sr = sample_rate, ch = channels, "test stream started");

    Ok(TestStream {
        _stream: stream,
        device_id: DeviceId::from(resolved_name.clone()),
        device_name: resolved_name,
        kind,
        stop_flag,
    })
}

/// State, владеемый callback-closure. Без alloc'ов внутри callback.
struct GenState {
    kind: TestKind,
    sample_rate: u32,
    channels: u16,
    /// Глобальный sample-counter (interleaved frames).
    samples_played: u64,
    /// Sample'ов до следующего beat'а (только Metronome).
    samples_to_next_beat: u32,
    /// Sample'ов осталось проиграть текущий «click» внутри beat'а.
    click_samples_left: u32,
    /// Phase для Sine [0; TAU).
    sine_phase: f32,
    stop_flag: Arc<AtomicBool>,
}

impl GenState {
    fn new(kind: TestKind, sample_rate: u32, channels: u16, stop_flag: Arc<AtomicBool>) -> Self {
        let click_total = (sample_rate as f32 * CLICK_DURATION_MS / 1000.0) as u32;
        let beat_interval = match kind {
            TestKind::Metronome { bpm } => {
                let bpm = bpm.clamp(40, 240) as u32;
                sample_rate * 60 / bpm
            }
            _ => 0,
        };
        let initial_click = match kind {
            TestKind::Click => click_total,
            TestKind::Metronome { .. } => click_total, // первый бит на старте
            _ => 0,
        };
        Self {
            kind,
            sample_rate,
            channels,
            samples_played: 0,
            samples_to_next_beat: beat_interval,
            click_samples_left: initial_click,
            sine_phase: 0.0,
            stop_flag,
        }
    }

    /// Сгенерировать один f32-sample (один frame, моно). Возвращает 0.0
    /// для тишины. Управляет внутренним state.
    fn next_sample(&mut self) -> f32 {
        if self.stop_flag.load(Ordering::Relaxed) {
            return 0.0;
        }
        let sr = self.sample_rate as f32;
        let click_total = (sr * CLICK_DURATION_MS / 1000.0) as u32;

        let value = match self.kind {
            TestKind::Click => {
                if self.click_samples_left > 0 {
                    self.click_samples_left -= 1;
                    // Square-wave для slabого «щелчка», но звучит как clean click.
                    if (self.samples_played / (sr as u64 / 2000).max(1)) % 2 == 0 {
                        TEST_AMPLITUDE
                    } else {
                        -TEST_AMPLITUDE
                    }
                } else {
                    // Click отыграл → self-terminate.
                    self.stop_flag.store(true, Ordering::Relaxed);
                    0.0
                }
            }
            TestKind::Sine1kHz => {
                let total = (sr * SINE_DURATION_SEC) as u64;
                if self.samples_played >= total {
                    self.stop_flag.store(true, Ordering::Relaxed);
                    return 0.0;
                }
                let s = (self.sine_phase).sin() * TEST_AMPLITUDE;
                self.sine_phase += TAU * SINE_FREQ_HZ / sr;
                if self.sine_phase >= TAU {
                    self.sine_phase -= TAU;
                }
                s
            }
            TestKind::Metronome { .. } => {
                let s = if self.click_samples_left > 0 {
                    self.click_samples_left -= 1;
                    if (self.samples_played / (sr as u64 / 2000).max(1)) % 2 == 0 {
                        TEST_AMPLITUDE
                    } else {
                        -TEST_AMPLITUDE
                    }
                } else {
                    0.0
                };
                if self.samples_to_next_beat == 0 {
                    self.click_samples_left = click_total;
                    let bpm = match self.kind {
                        TestKind::Metronome { bpm } => bpm.clamp(40, 240) as u32,
                        _ => 120,
                    };
                    self.samples_to_next_beat = self.sample_rate * 60 / bpm;
                } else {
                    self.samples_to_next_beat -= 1;
                }
                s
            }
        };
        self.samples_played += 1;
        value
    }
}

fn build_test_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    format: SampleFormat,
    kind: TestKind,
    sample_rate: u32,
    channels: u16,
    stop_flag: Arc<AtomicBool>,
) -> Result<Stream> {
    let err_cb = |e| tracing::error!(?e, "test stream error");

    macro_rules! build {
        ($ty:ty) => {{
            let mut state = GenState::new(kind, sample_rate, channels, stop_flag.clone());
            device
                .build_output_stream(
                    config,
                    move |data: &mut [$ty], _info: &cpal::OutputCallbackInfo| {
                        let ch = state.channels as usize;
                        let frames = data.len() / ch;
                        for f in 0..frames {
                            let v = state.next_sample();
                            let conv = <$ty>::from_f32_clamped(v);
                            for c in 0..ch {
                                data[f * ch + c] = conv;
                            }
                        }
                    },
                    err_cb,
                    None,
                )
                .map_err(|e| Error::Backend(format!("build_output_stream(test): {e}")))
        }};
    }
    match format {
        SampleFormat::F32 => build!(f32),
        SampleFormat::I16 => build!(i16),
        SampleFormat::U16 => build!(u16),
        SampleFormat::I32 => build!(i32),
        other => Err(Error::Backend(format!(
            "unsupported test signal format: {other}"
        ))),
    }
}
