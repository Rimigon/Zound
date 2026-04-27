//! Простой 3-полосный биквад-эквалайзер. Применяется per-channel в
//! audio worker до push в ringbuf.
//!
//! Полосы: low-shelf @ 100 Гц, peak @ 1 кГц, high-shelf @ 8 кГц.
//! Gain в dB, диапазон ±12 dB. При gain==0 → bypass всей цепи (один
//! сравнительный if в worker, нулевой DSP-cost).
//!
//! Формулы — стандартные RBJ Audio EQ Cookbook. Q=0.707 (Butterworth-
//! style), Q peak band = 1.0 для умеренной ширины.

/// Один биквад-фильтр (Direct Form I, transposed). Состояние per-channel,
/// поэтому для stereo нужны два независимых инстанса.
#[derive(Debug, Clone, Copy)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    pub fn identity() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    pub fn low_shelf(sample_rate: f32, freq_hz: f32, gain_db: f32, q: f32) -> Self {
        if gain_db.abs() < f32::EPSILON {
            return Self::identity();
        }
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * freq_hz / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q);
        let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;

        let b0 = a * ((a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha);
        let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0);
        let b2 = a * ((a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha);
        let a0 = (a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha;
        let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0);
        let a2 = (a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha;

        Self::from_coeffs(b0, b1, b2, a0, a1, a2)
    }

    pub fn high_shelf(sample_rate: f32, freq_hz: f32, gain_db: f32, q: f32) -> Self {
        if gain_db.abs() < f32::EPSILON {
            return Self::identity();
        }
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * freq_hz / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q);
        let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;

        let b0 = a * ((a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha);
        let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0);
        let b2 = a * ((a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha);
        let a0 = (a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha;
        let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_w0);
        let a2 = (a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha;

        Self::from_coeffs(b0, b1, b2, a0, a1, a2)
    }

    pub fn peaking(sample_rate: f32, freq_hz: f32, gain_db: f32, q: f32) -> Self {
        if gain_db.abs() < f32::EPSILON {
            return Self::identity();
        }
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * freq_hz / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q);

        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos_w0;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha / a;

        Self::from_coeffs(b0, b1, b2, a0, a1, a2)
    }

    fn from_coeffs(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Self {
        let inv = 1.0 / a0;
        Self {
            b0: b0 * inv,
            b1: b1 * inv,
            b2: b2 * inv,
            a1: a1 * inv,
            a2: a2 * inv,
            z1: 0.0,
            z2: 0.0,
        }
    }

    /// Direct Form I, transposed — численно стабильнее DF1 при f32.
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// Параметры одной полосы EQ.
#[derive(Debug, Clone, Copy)]
pub struct EqBand {
    pub freq_hz: f32,
    pub gain_db: f32,
    pub q: f32,
}

/// 3-полосный EQ для stereo-сигнала. По одному набору фильтров на канал
/// (mono использует только index 0).
#[derive(Debug, Clone)]
pub struct ThreeBandEq {
    sample_rate: f32,
    pub low: EqBand,
    pub mid: EqBand,
    pub high: EqBand,
    /// `[low_l, mid_l, high_l, low_r, mid_r, high_r]` — пара биквадов
    /// на канал × 3 полосы. Каналов больше двух → используем index 1.
    filters: [Biquad; 6],
    /// Cached: bypass-флаг. Если все три gain_db ≈ 0, не выполняем DSP.
    bypass: bool,
}

impl ThreeBandEq {
    pub fn new(sample_rate: f32) -> Self {
        let low = EqBand {
            freq_hz: 100.0,
            gain_db: 0.0,
            q: 0.707,
        };
        let mid = EqBand {
            freq_hz: 1_000.0,
            gain_db: 0.0,
            q: 1.0,
        };
        let high = EqBand {
            freq_hz: 8_000.0,
            gain_db: 0.0,
            q: 0.707,
        };
        let mut s = Self {
            sample_rate,
            low,
            mid,
            high,
            filters: [Biquad::identity(); 6],
            bypass: true,
        };
        s.recompute();
        s
    }

    pub fn set_low_gain(&mut self, gain_db: f32) {
        self.low.gain_db = gain_db.clamp(-12.0, 12.0);
        self.recompute();
    }

    pub fn set_mid_gain(&mut self, gain_db: f32) {
        self.mid.gain_db = gain_db.clamp(-12.0, 12.0);
        self.recompute();
    }

    pub fn set_high_gain(&mut self, gain_db: f32) {
        self.high.gain_db = gain_db.clamp(-12.0, 12.0);
        self.recompute();
    }

    pub fn is_bypass(&self) -> bool {
        self.bypass
    }

    fn recompute(&mut self) {
        self.bypass = self.low.gain_db.abs() < f32::EPSILON
            && self.mid.gain_db.abs() < f32::EPSILON
            && self.high.gain_db.abs() < f32::EPSILON;
        for ch in 0..2 {
            let off = ch * 3;
            self.filters[off] = Biquad::low_shelf(
                self.sample_rate,
                self.low.freq_hz,
                self.low.gain_db,
                self.low.q,
            );
            self.filters[off + 1] = Biquad::peaking(
                self.sample_rate,
                self.mid.freq_hz,
                self.mid.gain_db,
                self.mid.q,
            );
            self.filters[off + 2] = Biquad::high_shelf(
                self.sample_rate,
                self.high.freq_hz,
                self.high.gain_db,
                self.high.q,
            );
        }
    }

    /// Обработать interleaved-буфер in-place. Если EQ в bypass —
    /// функция выходит сразу (один branch на чанк).
    pub fn process_interleaved(&mut self, buf: &mut [f32], channels: usize) {
        if self.bypass || channels == 0 {
            return;
        }
        let frames = buf.len() / channels;
        for f in 0..frames {
            for ch in 0..channels {
                let chain_idx = if ch == 0 { 0 } else { 3 }; // stereo: ch0/ch1; >2 каналов → клонируем index 1
                let i = f * channels + ch;
                let x = buf[i];
                let y1 = self.filters[chain_idx].process(x);
                let y2 = self.filters[chain_idx + 1].process(y1);
                let y3 = self.filters[chain_idx + 2].process(y2);
                buf[i] = y3;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_passes_signal_unchanged() {
        let mut bq = Biquad::identity();
        for x in [0.0, 0.5, -0.7, 1.0] {
            assert!((bq.process(x) - x).abs() < 1e-6);
        }
    }

    #[test]
    fn flat_eq_is_bypass() {
        let eq = ThreeBandEq::new(48_000.0);
        assert!(eq.is_bypass());
    }

    #[test]
    fn nonzero_gain_disables_bypass() {
        let mut eq = ThreeBandEq::new(48_000.0);
        eq.set_mid_gain(3.0);
        assert!(!eq.is_bypass());
        eq.set_mid_gain(0.0);
        assert!(eq.is_bypass());
    }

    #[test]
    fn process_interleaved_is_noop_in_bypass() {
        let eq = ThreeBandEq::new(48_000.0);
        let mut eq = eq.clone();
        let mut buf = vec![0.1, 0.2, -0.1, -0.2];
        let original = buf.clone();
        eq.process_interleaved(&mut buf, 2);
        assert_eq!(buf, original);
    }

    #[test]
    fn low_shelf_boost_increases_low_freq_amplitude() {
        // DC-сигнал (постоянный) — после low-shelf +6dB должен вырасти ~×2.
        let mut eq = ThreeBandEq::new(48_000.0);
        eq.set_low_gain(6.0);
        let mut buf = vec![0.5_f32; 9_600]; // 100 ms — выйдет на устойчивое.
        eq.process_interleaved(&mut buf, 1);
        let last = buf[buf.len() - 1];
        // 6 dB ≈ ×1.995. Допуск шире, потому что shelf на 100 Гц
        // не идеально DC.
        assert!(last > 0.85 && last < 1.2, "expected ~1.0, got {last}");
    }

    #[test]
    fn gain_clamped_to_12db() {
        let mut eq = ThreeBandEq::new(48_000.0);
        eq.set_mid_gain(50.0);
        assert_eq!(eq.mid.gain_db, 12.0);
        eq.set_low_gain(-50.0);
        assert_eq!(eq.low.gain_db, -12.0);
    }
}
