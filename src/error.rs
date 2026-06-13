use thiserror::Error;

/// Unified error type for all wlsnip operations.
#[derive(Debug, Error)]
pub enum WlsnipError {
    #[error("failed to connect to Wayland display: {0}")]
    #[allow(dead_code)]
    WaylandConnect(String),

    #[error("no supported capture backend available")]
    NoBackendAvailable,

    #[error("wayland protocol error: {0}")]
    Protocol(String),

    #[error("buffer allocation failed: {0}")]
    BufferAlloc(String),

    #[error("capture failed: {0}")]
    Capture(String),

    #[error("encoding failed: {0}")]
    Encode(String),

    #[error("XDG portal error: {0}")]
    #[allow(dead_code)]
    Portal(String),

    #[error("region selection failed: {0}")]
    RegionSelection(String),

    #[error("output not found: {0}")]
    OutputNotFound(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid argument: {0}")]
    #[allow(dead_code)]
    InvalidArg(String),
}

pub type Result<T> = std::result::Result<T, WlsnipError>;
