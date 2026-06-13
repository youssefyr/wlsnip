pub mod png;
pub mod jpeg;
pub mod webp;

use crate::buffers::CaptureBuffer;
use crate::error::Result;
use std::io::Write;

/// Supported output image formats.
#[derive(Debug, Clone, Copy)]
pub enum OutputFormat {
    Png,
    Jpeg { quality: u8 },
    Webp { quality: f32 },
}

/// Encode a capture buffer into the specified format, writing to `dest`.
pub fn encode(buffer: &CaptureBuffer, format: OutputFormat, dest: &mut dyn Write) -> Result<()> {
    match format {
        OutputFormat::Png => png::encode(buffer, dest),
        OutputFormat::Jpeg { quality } => jpeg::encode(buffer, quality, dest),
        OutputFormat::Webp { quality } => webp::encode(buffer, quality, dest),
    }
}
