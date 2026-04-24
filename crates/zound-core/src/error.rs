use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("audio backend: {0}")]
    Backend(String),

    #[error("device not found: {0}")]
    DeviceNotFound(String),

    #[error("unsupported sample format: {0:?}")]
    UnsupportedFormat(crate::SampleFormat),

    #[error("buffer overrun")]
    Overrun,

    #[error("buffer underrun")]
    Underrun,

    #[error("{0}")]
    Other(String),
}
