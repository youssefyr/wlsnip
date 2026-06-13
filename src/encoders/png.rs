use crate::buffers::CaptureBuffer;
use crate::error::{Result, WlsnipError};
use crate::utils::color;

use zune_png::PngEncoder;
use zune_core::options::EncoderOptions;
use zune_core::colorspace::ColorSpace;
use zune_core::bit_depth::BitDepth;
use std::io::Write;

/// Encode a capture buffer as PNG, writing to `dest`.
pub fn encode(buffer: &CaptureBuffer, dest: &mut dyn Write) -> Result<()> {
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

    let options = EncoderOptions::default()
        .set_width(buffer.width as usize)
        .set_height(buffer.height as usize)
        .set_colorspace(ColorSpace::RGBA)
        .set_depth(BitDepth::Eight);

    let mut encoder = PngEncoder::new(&packed, options);
    let mut encoded_bytes = Vec::new();
    encoder.encode(&mut encoded_bytes)
        .map_err(|e| WlsnipError::Encode(format!("PNG encoding failed: {e:?}")))?;
    
    dest.write_all(&encoded_bytes)
        .map_err(|e| WlsnipError::Encode(format!("PNG write failed: {e}")))?;

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
