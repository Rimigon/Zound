//! Унифицированное копирование разных форматов cpal-сэмплов в f32.
//!
//! Держим небольшой enum вместо generics, чтобы compile-time шаблоны не
//! раздували код и чтобы capture/output использовали одну и ту же логику.

pub enum CopySource<'a> {
    F32(&'a [f32]),
    I16(&'a [i16]),
    U16(&'a [u16]),
    I32(&'a [i32]),
}

impl<'a> From<&'a [f32]> for CopySource<'a> {
    fn from(s: &'a [f32]) -> Self {
        Self::F32(s)
    }
}
impl<'a> From<&'a [i16]> for CopySource<'a> {
    fn from(s: &'a [i16]) -> Self {
        Self::I16(s)
    }
}
impl<'a> From<&'a [u16]> for CopySource<'a> {
    fn from(s: &'a [u16]) -> Self {
        Self::U16(s)
    }
}
impl<'a> From<&'a [i32]> for CopySource<'a> {
    fn from(s: &'a [i32]) -> Self {
        Self::I32(s)
    }
}

impl CopySource<'_> {
    pub fn len(&self) -> usize {
        match self {
            Self::F32(s) => s.len(),
            Self::I16(s) => s.len(),
            Self::U16(s) => s.len(),
            Self::I32(s) => s.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Скопировать часть source (начиная с offset, длиной dst.len()) в dst как f32.
#[inline]
pub fn copy_as_f32(src: &CopySource<'_>, offset: usize, dst: &mut [f32]) {
    let n = dst.len();
    match src {
        CopySource::F32(s) => dst.copy_from_slice(&s[offset..offset + n]),
        CopySource::I16(s) => {
            const SCALE: f32 = 1.0 / i16::MAX as f32;
            for (d, &v) in dst.iter_mut().zip(&s[offset..offset + n]) {
                *d = v as f32 * SCALE;
            }
        }
        CopySource::U16(s) => {
            // u16 center = 32768
            const SCALE: f32 = 1.0 / 32768.0;
            for (d, &v) in dst.iter_mut().zip(&s[offset..offset + n]) {
                *d = (v as i32 - 32768) as f32 * SCALE;
            }
        }
        CopySource::I32(s) => {
            const SCALE: f32 = 1.0 / (i32::MAX as f32);
            for (d, &v) in dst.iter_mut().zip(&s[offset..offset + n]) {
                *d = v as f32 * SCALE;
            }
        }
    }
}

/// Конвертация f32 → произвольный формат на выходе.
pub trait FromF32: Copy + cpal::SizedSample {
    fn from_f32_clamped(v: f32) -> Self;
}

impl FromF32 for f32 {
    #[inline]
    fn from_f32_clamped(v: f32) -> Self {
        v.clamp(-1.0, 1.0)
    }
}

impl FromF32 for i16 {
    #[inline]
    fn from_f32_clamped(v: f32) -> Self {
        (v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
    }
}

impl FromF32 for u16 {
    #[inline]
    fn from_f32_clamped(v: f32) -> Self {
        let centered = v.clamp(-1.0, 1.0) * 32767.0 + 32768.0;
        centered as u16
    }
}

impl FromF32 for i32 {
    #[inline]
    fn from_f32_clamped(v: f32) -> Self {
        (v.clamp(-1.0, 1.0) * i32::MAX as f32) as i32
    }
}
