use crate::buffers::CaptureBuffer;
use crate::error::{Result, WlsnipError};
use crate::utils::color;

use turbojpeg::{Compressor, PixelFormat, Image};
use std::io::Write;

/// Encode a capture buffer as JPEG, writing to `dest`.
pub fn encode(buffer: &CaptureBuffer, quality: u8, dest: &mut dyn Write) -> Result<()> {
    // Convert to RGBA first (TurboJPEG can compress RGBA natively)
    let mut working = CaptureBuffer {
        width: buffer.width,
        height: buffer.height,
        stride: buffer.stride,
        format: buffer.format,
        data: buffer.data.clone(),
    };
    color::convert_to_rgba(&mut working);

    let image = Image {
        pixels: &working.data[..],
        width: working.width as usize,
        pitch: working.stride as usize,
        height: working.height as usize,
        format: PixelFormat::RGBA,
    };

    let mut comp = Compressor::new()
        .map_err(|e| WlsnipError::Encode(format!("Failed to init turbojpeg: {e}")))?;
    let _ = comp.set_quality(quality as i32);

    let jpeg_data = comp.compress_to_owned(image)
        .map_err(|e| WlsnipError::Encode(format!("JPEG encoding failed: {e}")))?;

    dest.write_all(&jpeg_data)
        .map_err(|e| WlsnipError::Encode(format!("JPEG write failed: {e}")))?;

    Ok(())
}


