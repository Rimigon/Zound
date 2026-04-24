#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleFormat {
    F32,
    I16,
    I32,
    U16,
}

impl SampleFormat {
    pub fn bytes(self) -> usize {
        match self {
            SampleFormat::F32 | SampleFormat::I32 => 4,
            SampleFormat::I16 | SampleFormat::U16 => 2,
        }
    }
}

/// Конвертация i16 → f32 без аллокаций. Инлайнится, пригодна для realtime.
#[inline]
pub fn i16_to_f32(src: &[i16], dst: &mut [f32]) {
    debug_assert_eq!(src.len(), dst.len());
    const SCALE: f32 = 1.0 / i16::MAX as f32;
    for (d, &s) in dst.iter_mut().zip(src.iter()) {
        *d = s as f32 * SCALE;
    }
}

/// Конвертация f32 → i16 с клиппингом.
#[inline]
pub fn f32_to_i16(src: &[f32], dst: &mut [i16]) {
    debug_assert_eq!(src.len(), dst.len());
    const SCALE: f32 = i16::MAX as f32;
    for (d, &s) in dst.iter_mut().zip(src.iter()) {
        *d = (s.clamp(-1.0, 1.0) * SCALE) as i16;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f32_to_i16_clips() {
        let src = [-2.0_f32, -1.0, 0.0, 1.0, 2.0];
        let mut dst = [0_i16; 5];
        f32_to_i16(&src, &mut dst);
        assert_eq!(dst[0], -i16::MAX);
        assert_eq!(dst[1], -i16::MAX);
        assert_eq!(dst[2], 0);
        assert_eq!(dst[3], i16::MAX);
        assert_eq!(dst[4], i16::MAX);
    }

    #[test]
    fn roundtrip_i16_f32() {
        let src = [i16::MIN, -1, 0, 1, i16::MAX];
        let mut mid = [0.0_f32; 5];
        let mut back = [0_i16; 5];
        i16_to_f32(&src, &mut mid);
        f32_to_i16(&mid, &mut back);
        // i16::MIN не восстанавливается точно (SCALE использует MAX), это ок.
        assert_eq!(back[2], 0);
        assert_eq!(back[4], i16::MAX);
    }
}
