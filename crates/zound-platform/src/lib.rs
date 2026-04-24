//! Платформенная абстракция над аудио-API.
//!
//! Основной трейт — [`AudioBackend`]. На MVP единственная реализация —
//! [`CpalBackend`] поверх крейта `cpal`. Нативные backend-ы
//! (WASAPI / CoreAudio / PipeWire напрямую) добавятся там, где `cpal` не
//! хватает (per-app capture, системный loopback с управлением).

pub mod backend;
pub mod capture;
pub mod cpal_backend;
pub mod output;
pub mod sample_convert;

pub use backend::AudioBackend;
pub use capture::{open_default_loopback, Capture, CaptureOpts, CaptureSession};
pub use cpal_backend::CpalBackend;
pub use output::{open_output_by_name, AtomicVolume, OutputOpts, OutputSink};
