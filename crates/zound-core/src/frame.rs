use std::time::Duration;

/// Блок interleaved f32-сэмплов, привязанный к моменту захвата.
///
/// `samples.len() = frames * channels`. Frames — число мультисэмплов
/// (например, для stereo 48 kHz за 10 мс это 480 frames = 960 сэмплов).
#[derive(Debug, Clone)]
pub struct AudioFrame {
    pub samples: Vec<f32>,
    pub channels: u16,
    pub sample_rate: u32,
    /// Монотонный timestamp захвата (смещение от произвольной точки старта).
    pub capture_time: Duration,
}

impl AudioFrame {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels as usize
    }

    pub fn duration(&self) -> Duration {
        let frames = self.frames() as u64;
        Duration::from_secs_f64(frames as f64 / self.sample_rate as f64)
    }
}
