//! Общие типы и примитивы аудио-обработки для Zound.
//!
//! Внутренний формат: f32, interleaved, native endian. Конвертация
//! происходит на границах — в `zound-platform` (capture/output).

pub mod device;
pub mod error;
pub mod frame;
pub mod sample;

pub use device::{DeviceId, DeviceInfo, DeviceKind};
pub use error::Error;
pub use frame::AudioFrame;
pub use sample::SampleFormat;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Внутренний sample rate по умолчанию. 48 kHz — стандарт для видео/игр,
/// достаточен для музыки. Все потоки ресемплятся к нему.
pub const DEFAULT_SAMPLE_RATE: u32 = 48_000;

/// Каналов по умолчанию (stereo).
pub const DEFAULT_CHANNELS: u16 = 2;
