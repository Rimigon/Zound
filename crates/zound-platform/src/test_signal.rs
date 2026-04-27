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
    /// Серия из `CLICK_REPEATS` коротких щелчков 1 кГц с `CLICK_INTERVAL_MS`
    /// между ними. Позволяет на слух прикинуть latency между устройствами.
    /// После последнего клика стрим самопрекращается.
    Click,
    /// Тон 1 кГц длительностью `SINE_DURATION_SEC` с fade-in/out, чтобы не
    /// было щелчка на старте и стопе.
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

/// Длина одного «щелчка» — короткий тональный импульс с envelope.
const CLICK_DURATION_MS: f32 = 10.0;

/// Сколько щелчков играет TestKind::Click при одном Start.
const CLICK_REPEATS: u32 = 5;

/// Интервал между щелчками в серии.
const CLICK_INTERVAL_MS: f32 = 1000.0;

/// Частота тона внутри щелчка / удара метронома.
const CLICK_TONE_HZ: f32 = 1000.0;

/// Длительность атаки/релиза raised-cosine envelope. Без неё резкий старт
/// тона даёт aliasing-«квак» вместо чистого щелчка.
const RAMP_MS: f32 = 1.0;

/// Длина sine-сигнала.
const SINE_DURATION_SEC: f32 = 5.0;

/// Частота sine-тона.
const SINE_FREQ_HZ: f32 = 1000.0;

/// Fade-in/out для sine, чтобы старт/стоп были без щелчка.
const SINE_FADE_MS: f32 = 50.0;

/// Raised-cosine envelope для одного «выстрела» длиной `total` семплов
/// с симметричной атакой/релизом длиной `ramp`. Возвращает 0..=1.
fn ramp_env(pos: u32, total: u32, ramp: u32) -> f32 {
    if total == 0 {
        return 0.0;
    }
    let ramp = ramp.min(total / 2).max(1);
    if pos < ramp {
        let x = pos as f32 / ramp as f32;
        0.5 * (1.0 - (std::f32::consts::PI * x).cos())
    } else if pos >= total - ramp {
        let x = (total - 1 - pos) as f32 / ramp as f32;
        0.5 * (1.0 - (std::f32::consts::PI * x).cos())
    } else {
        1.0
    }
}

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
    /// Длительность одного клика в семплах (общая для Click/Metronome).
    click_total: u32,
    /// Атака/релиз envelope в семплах.
    ramp_samples: u32,
    /// Тон-фаза клика [0; TAU).
    click_phase: f32,
    /// Позиция внутри текущего клика (0..click_total). Если >= click_total —
    /// клик не играет, сейчас тишина.
    click_pos: u32,
    /// Сэмплов до следующего клика (Click-серия / Metronome).
    samples_to_next_click: u32,
    /// Сколько кликов уже отыграло (только Click-серия).
    clicks_done: u32,
    /// Sine: общая длина и длина fade'а в семплах.
    sine_total: u64,
    sine_fade: u32,
    /// Sine phase [0; TAU).
    sine_phase: f32,
    stop_flag: Arc<AtomicBool>,
}

impl GenState {
    fn new(kind: TestKind, sample_rate: u32, channels: u16, stop_flag: Arc<AtomicBool>) -> Self {
        let sr = sample_rate as f32;
        let click_total = (sr * CLICK_DURATION_MS / 1000.0).max(1.0) as u32;
        let ramp_samples = (sr * RAMP_MS / 1000.0).max(1.0) as u32;

        // Для Click-серии и Metronome первый клик стартует немедленно
        // (click_pos = 0); для Sine клика нет.
        let click_pos = match kind {
            TestKind::Click | TestKind::Metronome { .. } => 0,
            TestKind::Sine1kHz => click_total, // вне клика
        };
        let samples_to_next_click = match kind {
            TestKind::Click => (sr * CLICK_INTERVAL_MS / 1000.0) as u32,
            TestKind::Metronome { bpm } => {
                let bpm = bpm.clamp(40, 240) as u32;
                sample_rate * 60 / bpm
            }
            TestKind::Sine1kHz => 0,
        };

        let sine_total = (sr * SINE_DURATION_SEC) as u64;
        let sine_fade = (sr * SINE_FADE_MS / 1000.0).max(1.0) as u32;

        Self {
            kind,
            sample_rate,
            channels,
            samples_played: 0,
            click_total,
            ramp_samples,
            click_phase: 0.0,
            click_pos,
            samples_to_next_click,
            clicks_done: 0,
            sine_total,
            sine_fade,
            sine_phase: 0.0,
            stop_flag,
        }
    }

    /// Один сэмпл одного клика-«импульса». Двигает фазу и позицию,
    /// применяет raised-cosine envelope. Возвращает 0.0 если клик закончился.
    ///
    /// Сигнал — sin(2π·f·t) с `ramp_env`. Чистый синус 1 кГц в окне
    /// raised-cosine — это band-limited импульс с энергией только вокруг
    /// 1 кГц и энвелопными боковыми лепестками: щадит твитер, не даёт
    /// «квака» и DC-смещения. Альтернативная пред-баканная WAV-таблица
    /// (через FFT-оконные click-pulses) дала бы тот же effect, но при
    /// добавила бы ~5 КБ static data без аудиторского выигрыша.
    fn click_sample(&mut self) -> f32 {
        if self.click_pos >= self.click_total {
            return 0.0;
        }
        let env = ramp_env(self.click_pos, self.click_total, self.ramp_samples);
        // Дополнительное low-pass smoothing амплитуды через cos²-окно
        // поверх ramp_env. Это режет HF-края клика на ~6 dB ниже,
        // повторяет поведение «Hann × Hann» из window-design DSP.
        let smooth = env * env;
        let s = self.click_phase.sin() * TEST_AMPLITUDE * smooth;
        self.click_phase += TAU * CLICK_TONE_HZ / self.sample_rate as f32;
        if self.click_phase >= TAU {
            self.click_phase -= TAU;
        }
        self.click_pos += 1;
        s
    }

    /// Сгенерировать один f32-sample (один frame, моно). Возвращает 0.0
    /// для тишины. Управляет внутренним state.
    fn next_sample(&mut self) -> f32 {
        if self.stop_flag.load(Ordering::Relaxed) {
            return 0.0;
        }

        let value = match self.kind {
            TestKind::Click => {
                let s = self.click_sample();
                if self.click_pos >= self.click_total {
                    // Клик завершился — отсчитываем паузу до следующего.
                    if self.samples_to_next_click > 0 {
                        self.samples_to_next_click -= 1;
                        if self.samples_to_next_click == 0 {
                            self.clicks_done += 1;
                            if self.clicks_done >= CLICK_REPEATS {
                                self.stop_flag.store(true, Ordering::Relaxed);
                            } else {
                                self.click_pos = 0;
                                self.click_phase = 0.0;
                                self.samples_to_next_click =
                                    (self.sample_rate as f32 * CLICK_INTERVAL_MS / 1000.0) as u32;
                            }
                        }
                    }
                }
                s
            }
            TestKind::Sine1kHz => {
                if self.samples_played >= self.sine_total {
                    self.stop_flag.store(true, Ordering::Relaxed);
                    return 0.0;
                }
                let pos = self.samples_played;
                let fade = self.sine_fade as u64;
                // Линейный 0..1 в зонах fade, raised-cosine на нём.
                let lin = if pos < fade {
                    pos as f32 / fade as f32
                } else if pos + fade >= self.sine_total {
                    (self.sine_total - 1 - pos) as f32 / fade as f32
                } else {
                    1.0
                };
                let env = 0.5 * (1.0 - (std::f32::consts::PI * lin).cos());
                let s = self.sine_phase.sin() * TEST_AMPLITUDE * env;
                self.sine_phase += TAU * SINE_FREQ_HZ / self.sample_rate as f32;
                if self.sine_phase >= TAU {
                    self.sine_phase -= TAU;
                }
                s
            }
            TestKind::Metronome { bpm } => {
                let s = self.click_sample();
                if self.click_pos >= self.click_total && self.samples_to_next_click > 0 {
                    self.samples_to_next_click -= 1;
                    if self.samples_to_next_click == 0 {
                        let bpm = bpm.clamp(40, 240) as u32;
                        self.click_pos = 0;
                        self.click_phase = 0.0;
                        self.samples_to_next_click = self.sample_rate * 60 / bpm;
                    }
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
