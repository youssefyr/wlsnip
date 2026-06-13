use crate::buffers::CaptureBuffer;
use crate::error::{Result, WlsnipError};
use crate::utils::color;

use std::io::Write;
use webp::Encoder;

/// Encode a capture buffer as WebP, writing to `dest`.
pub fn encode(buffer: &CaptureBuffer, quality: f32, dest: &mut dyn Write) -> Result<()> {
    // We need RGBA data for the encoder
    let mut working = CaptureBuffer {
        width: buffer.width,
        height: buffer.height,
        stride: buffer.stride,
        format: buffer.format,
        data: buffer.data.clone(),
    };
    color::convert_to_rgba(&mut working);

    // If stride has padding, we need to strip it to get packed RGBA rows
    let packed = strip_stride_padding(&working);

    let encoder = Encoder::from_rgba(&packed, buffer.width, buffer.height);
    let webp_memory = encoder.encode(quality);
    
    dest.write_all(&webp_memory)
        .map_err(|e| WlsnipError::Encode(format!("WebP write failed: {e}")))?;

    Ok(())
}

/// Remove stride padding so each row is exactly `width * 4` bytes.
fn strip_stride_padding(buffer: &CaptureBuffer) -> Vec<u8> {
    let row_bytes = (buffer.width * 4) as usize;
    if buffer.stride as usize == row_bytes {
        // No padding, return data as-is
        return buffer.data.clone();
    }

    let mut packed = Vec::with_capacity(row_bytes * buffer.height as usize);
    for row in 0..buffer.height {
        let start = (row * buffer.stride) as usize;
        packed.extend_from_slice(&buffer.data[start..start + row_bytes]);
    }
    packed
}
