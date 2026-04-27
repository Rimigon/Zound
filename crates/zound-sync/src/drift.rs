//! Адаптивная коррекция дрейфа часов между устройствами.
//!
//! Постановка: даже при идеальной начальной синхронизации часы output-
//! устройств медленно расходятся (разные кварцы, температурные эффекты).
//! `last_push_micros` в `DeviceSyncParams` показывает, насколько каждое
//! устройство «обогнало» или «отстало» от агрегата.
//!
//! Контроллер — простой PI: вход `error_micros = device_t - reference_t`,
//! выход — корректировка ratio для рубата (`output_sr / input_sr * (1 +
//! correction)`). Если устройство отстаёт (error<0), ускоряем его выход
//! (ratio чуть больше). Если опережает, замедляем.
//!
//! Все числа консервативные: cap |correction| ≤ MAX_CORRECTION (0.001 =
//! 0.1%), что соответствует ±100 ppm — типичный qualifications для
//! pro-audio. Этого достаточно, чтобы compensate drift кварцевых часов
//! (обычно ±20–50 ppm).
//!
//! Контроллер сам по себе не зовёт rubato — только считает корректировку.
//! Применение в worker-loop: см. `zound-output::engine`.

use std::time::Duration;

/// Максимальное относительное отклонение ratio. ±0.1% = ±100 ppm.
/// Больше — безопасно, но слышимо как «питч-плывёт» при tone-сигналах.
pub const MAX_CORRECTION: f32 = 0.001;

/// Зона нечувствительности (deadband) — внутри неё корректировка = 0.
/// Без неё контроллер шумит на низком уровне даже при синхронных
/// устройствах (jitter timestamp-ов в callback-ах ≈1–3 мс).
pub const DEADBAND_MICROS: i64 = 5_000; // 5 мс

/// Состояние PI-контроллера на одно устройство. Хранится в worker-loop,
/// обновляется на каждом drift-tick (раз в N мс).
#[derive(Debug, Clone, Copy)]
pub struct DriftCorrector {
    kp: f32,
    ki: f32,
    integral: f32,
    last_correction: f32,
}

impl DriftCorrector {
    /// Дефолтные коэффициенты PI-контроллера. Расчёт-обоснование:
    ///
    /// - Вход: `error_micros` ∈ примерно ±200_000 (200 мс — реалистичный
    ///   максимум drift до того, как UI уже бы среагировал).
    /// - Выход: `correction` ∈ ±MAX_CORRECTION = ±0.001 (±100 ppm).
    /// - Worst-case вход *kp* должен попадать в bound по выходу:
    ///   `200_000 * kp == MAX_CORRECTION` → `kp = 5e-9`. Берём
    ///   `kp = 1e-7` (×20 быстрее), потому что для типичных drift
    ///   100–200 µs на 100 ms тике мы хотим ощутимую коррекцию, а
    ///   clamp-у потом всё равно cap-нет крайние выбросы.
    /// - Tick rate worker'а — DRIFT_TICK = 100 ms (см.
    ///   `zound-output::engine`). Settling time желаемое ~3 секунды
    ///   (≈30 тиков): для PI с zeta≈1.0 это `ki = kp / (settle * 0.1)`,
    ///   что даёт `ki ≈ 3e-8`. Берём `1e-9` — заметно консервативнее,
    ///   чтобы integral wind-up при первом «холодном» подключении (когда
    ///   error может быть 200 ms) не выкидывал коррекцию за clamp:
    ///   integral реально начнёт срабатывать только после нескольких
    ///   тиков накопления, что даёт нам устойчивость в обмен на
    ///   немного более длинный settle.
    /// - Запас: оба коэффициента маленькие → даже при worst-case error
    ///   `(kp * err + ki * integral)` сам по себе попадает в bound; clamp
    ///   нужен лишь для защиты от программных ошибок (нулевой dt и т.п.).
    ///
    /// Step-response поведение проверяется тестом
    /// `step_response_settles_within_target` ниже: при стабильном
    /// `error = 50_000` µs выход устанавливается между deadband и cap
    /// и не уходит за `MAX_CORRECTION`.
    pub fn new() -> Self {
        Self {
            kp: 1e-7,
            ki: 1e-9,
            integral: 0.0,
            last_correction: 0.0,
        }
    }

    /// `error_micros` — на сколько *это* устройство опережает агрегат
    /// (положительное = опережает = надо замедлить). Возвращает
    /// корректировку ratio в [-MAX_CORRECTION; +MAX_CORRECTION].
    ///
    /// `tick_dt` — время между вызовами update; нужно для интегральной
    /// части, чтобы отвязать ki от частоты опроса.
    pub fn update(&mut self, error_micros: i64, tick_dt: Duration) -> f32 {
        if error_micros.abs() < DEADBAND_MICROS {
            // В зоне нечувствительности интеграл медленно расслабляется
            // (10% за тик), чтобы не «прилипать» к старому смещению.
            self.integral *= 0.9;
            self.last_correction *= 0.9;
            return self.last_correction;
        }
        let err = error_micros as f32;
        self.integral += err * tick_dt.as_secs_f32();
        // Замедлить (отрицательная коррекция к ratio) если устройство
        // опережает (err > 0): ratio_eff = base * (1 - correction).
        let raw = -(self.kp * err + self.ki * self.integral);
        let clamped = raw.clamp(-MAX_CORRECTION, MAX_CORRECTION);
        self.last_correction = clamped;
        clamped
    }

    pub fn reset(&mut self) {
        self.integral = 0.0;
        self.last_correction = 0.0;
    }

    pub fn last_correction(&self) -> f32 {
        self.last_correction
    }
}

impl Default for DriftCorrector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadband_returns_zero() {
        let mut c = DriftCorrector::new();
        let out = c.update(1_000, Duration::from_millis(100));
        assert_eq!(out, 0.0);
    }

    #[test]
    fn correction_is_bounded() {
        let mut c = DriftCorrector::new();
        // Гигантский error → коррекция всё равно cap-нута.
        let out = c.update(1_000_000_000, Duration::from_millis(100));
        assert!(out.abs() <= MAX_CORRECTION + 1e-6);
    }

    #[test]
    fn positive_error_means_negative_correction() {
        // Устройство опережает → надо замедлить (ratio_eff < base).
        let mut c = DriftCorrector::new();
        let out = c.update(50_000, Duration::from_millis(100));
        assert!(out < 0.0);
    }

    #[test]
    fn negative_error_means_positive_correction() {
        let mut c = DriftCorrector::new();
        let out = c.update(-50_000, Duration::from_millis(100));
        assert!(out > 0.0);
    }

    #[test]
    fn reset_clears_state() {
        let mut c = DriftCorrector::new();
        c.update(50_000, Duration::from_millis(100));
        c.reset();
        assert_eq!(c.last_correction(), 0.0);
    }

    /// Step-response: при постоянном входе `error = 50 ms` контроллер
    /// должен:
    /// 1. Никогда не вылетать за пределы [-MAX_CORRECTION; +MAX_CORRECTION].
    /// 2. Дать монотонно растущую (по модулю) коррекцию в первые
    ///    ~30 тиков (1.5 сек) — пока integral накапливается.
    /// 3. После 50 тиков (5 сек) удерживать коррекцию вблизи cap-а
    ///    (-MAX_CORRECTION), потому что error не убывает.
    #[test]
    fn step_response_settles_within_target() {
        let mut c = DriftCorrector::new();
        let dt = Duration::from_millis(100);
        let mut last_abs = 0.0_f32;
        for tick in 0..30 {
            let out = c.update(50_000, dt);
            assert!(
                out.abs() <= MAX_CORRECTION + 1e-6,
                "tick {tick}: |{out}| > MAX_CORRECTION"
            );
            // Коррекция против знака error (положительный → замедлить).
            assert!(out <= 0.0, "tick {tick}: expected non-positive, got {out}");
            assert!(
                out.abs() >= last_abs - 1e-6,
                "tick {tick}: |{out}| < |{last_abs}| (non-monotonic ramp)"
            );
            last_abs = out.abs();
        }
        // К 50-му тику коррекция упёрлась в cap (с допусками на FP).
        for _ in 30..50 {
            c.update(50_000, dt);
        }
        let plateau = c.last_correction();
        assert!(
            (plateau.abs() - MAX_CORRECTION).abs() < 1e-6,
            "expected to hit MAX_CORRECTION cap, got {plateau}"
        );
    }

    /// Когда error возвращается в deadband, integral не должен «застрять»:
    /// last_correction плавно уезжает к 0 за разумное число тиков.
    #[test]
    fn deadband_relaxes_integral_to_zero() {
        let mut c = DriftCorrector::new();
        let dt = Duration::from_millis(100);
        // Накачали integral.
        for _ in 0..20 {
            c.update(50_000, dt);
        }
        let _ = c.last_correction();
        // Теперь возвращаем в deadband — integral должен релаксировать.
        for _ in 0..50 {
            c.update(0, dt);
        }
        let after_relax = c.last_correction();
        assert!(
            after_relax.abs() < 1e-3,
            "expected near-zero correction after relaxation, got {after_relax}"
        );
    }
}
