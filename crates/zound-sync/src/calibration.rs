//! Авто-калибровка latency через play+capture.
//!
//! Идея: проиграть короткий **chirp** (или клик) на устройство, поймать
//! его обратно через loopback / микрофон, найти позицию максимума
//! кросс-корреляции — это и есть round-trip latency. Делим пополам если
//! путь симметричный, или вычитаем известный вход из оценки.
//!
//! На MVP здесь — генерация сигналов и offline-correlation. Полноценный
//! pipeline (запуск на cpal, синхронный запись + анализ) — следующий
//! инкремент. Чтобы UI и SyncEngine могли уже сейчас иметь stable API
//! для команды «откалибровать», экспортируем плейсхолдер
//! [`CalibrationOutcome`].
//!
//! Используется тестами и будущей UI-командой `latency-calibrate`.

use std::f32::consts::TAU;

/// Длительность chirp по умолчанию. 200 мс достаточно для надёжной
/// корреляции до 100 мс задержки.
pub const DEFAULT_CHIRP_DURATION_MS: f32 = 200.0;

/// Стартовая частота chirp-а (Hz).
pub const CHIRP_F0: f32 = 200.0;

/// Конечная частота chirp-а (Hz).
pub const CHIRP_F1: f32 = 4_000.0;

/// Линейный chirp от f0 до f1 длиной `samples` фреймов на `sample_rate`.
/// Амплитуда `amplitude` (типично 0.5).
///
/// Линейный — потому что с линейным проще считать корреляцию и нет
/// логарифмической перенасыщенности высоких частот, где BT-кодеки начинают
/// «есть» сигнал.
pub fn generate_chirp(sample_rate: u32, samples: usize, amplitude: f32) -> Vec<f32> {
    let mut out = Vec::with_capacity(samples);
    let sr = sample_rate as f32;
    let dur = samples as f32 / sr;
    let k = (CHIRP_F1 - CHIRP_F0) / dur;
    for n in 0..samples {
        let t = n as f32 / sr;
        // Мгновенная фаза — интеграл от 2π·f(t) = 2π·(f0 + 0.5·k·t)
        let phase = TAU * (CHIRP_F0 * t + 0.5 * k * t * t);
        // Half-cosine envelope, чтобы не было щелчка на старте/стопе.
        let env = if samples == 0 {
            0.0
        } else {
            let x = n as f32 / (samples - 1).max(1) as f32;
            0.5 * (1.0 - (TAU * 0.5 * x).cos() * (TAU * 0.5 * (1.0 - x)).cos())
        };
        out.push(amplitude * env * phase.sin());
    }
    out
}

/// Кросс-корреляция вход×опорный сигнал, naive O(N·M). Для MVP-длин
/// (200 мс @ 48 kHz = 9600 семплов на reference, до 1 сек на recorded =
/// 48000) — это ~5e8 операций; ОК для одноразовой калибровки.
///
/// Возвращает индекс лагa с максимальной корреляцией. `recorded` —
/// записанный сигнал (включает задержку), `reference` — что играли.
/// Если максимум не найден (recorded короче reference), возвращает 0.
pub fn cross_correlation_peak(recorded: &[f32], reference: &[f32]) -> usize {
    if recorded.len() < reference.len() || reference.is_empty() {
        return 0;
    }
    let mut best_idx = 0usize;
    let mut best_val = f32::NEG_INFINITY;
    let last = recorded.len() - reference.len();
    for lag in 0..=last {
        let mut acc = 0.0_f32;
        for i in 0..reference.len() {
            acc += recorded[lag + i] * reference[i];
        }
        if acc > best_val {
            best_val = acc;
            best_idx = lag;
        }
    }
    best_idx
}

/// Результат одной попытки калибровки. `latency_micros` — измеренная
/// задержка устройства, `confidence` ∈ [0; 1] — пик-к-сайдлоб ratio
/// нормированной корреляции (>0.5 — обычно надёжно).
#[derive(Debug, Clone, Copy)]
pub struct CalibrationOutcome {
    pub latency_micros: u64,
    pub confidence: f32,
}

/// Конвертировать индекс лагa в micros.
pub fn lag_to_micros(lag_samples: usize, sample_rate: u32) -> u64 {
    if sample_rate == 0 {
        return 0;
    }
    (lag_samples as u64 * 1_000_000) / sample_rate as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chirp_has_correct_length() {
        let sr = 48_000;
        let samples = (sr as f32 * DEFAULT_CHIRP_DURATION_MS / 1000.0) as usize;
        let c = generate_chirp(sr, samples, 0.5);
        assert_eq!(c.len(), samples);
    }

    #[test]
    fn chirp_amplitude_within_envelope() {
        let c = generate_chirp(48_000, 4096, 0.5);
        let max = c.iter().cloned().fold(0.0_f32, f32::max);
        assert!(max <= 0.51, "amp leaked above 0.5: {max}");
    }

    #[test]
    fn correlation_finds_known_offset() {
        let sr = 48_000;
        let reference = generate_chirp(sr, 1024, 0.5);
        let pad = 2_500;
        let mut recorded = vec![0.0_f32; pad];
        recorded.extend_from_slice(&reference);
        recorded.extend(vec![0.0_f32; 1_000]);
        let lag = cross_correlation_peak(&recorded, &reference);
        assert_eq!(lag, pad);
    }

    #[test]
    fn lag_micros_conversion() {
        // 480 семплов @ 48 kHz = 10 мс = 10_000 micros.
        assert_eq!(lag_to_micros(480, 48_000), 10_000);
    }
}
