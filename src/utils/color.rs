use crate::buffers::{CaptureBuffer, PixelFormat};
use rayon::prelude::*;

/// Convert pixel data in-place from the source format to RGBA8888.
///
/// Most Wayland compositors use ARGB8888 or XRGB8888 (little-endian),
/// which in memory layout is actually BGRA or BGRX byte order.
/// Image encoders expect RGBA, so we swizzle the channels.
pub fn convert_to_rgba(buffer: &mut CaptureBuffer) {
    let format = buffer.format;
    let width = buffer.width as usize;
    let stride = buffer.stride as usize;

    match format {
        PixelFormat::Argb8888 | PixelFormat::Xrgb8888 => {
            // Memory layout (little-endian): B G R A → need R G B A
            buffer.data.par_chunks_mut(stride).for_each(|row| {
                for col in 0..width {
                    let offset = col * 4;
                    if offset + 3 < row.len() {
                        // Swap B and R channels
                        row.swap(offset, offset + 2);
                        // For XRGB, force alpha to 255 (fully opaque)
                        if format == PixelFormat::Xrgb8888 {
                            row[offset + 3] = 0xFF;
                        }
                    }
                }
            });
        }
        PixelFormat::Abgr8888 | PixelFormat::Xbgr8888 => {
            // Memory layout (little-endian): R G B A → already RGBA order
            // Just fix alpha for XBGR
            if format == PixelFormat::Xbgr8888 {
                buffer.data.par_chunks_mut(stride).for_each(|row| {
                    for col in 0..width {
                        let offset = col * 4;
                        if offset + 3 < row.len() {
                            row[offset + 3] = 0xFF;
                        }
                    }
                });
            }
        }
    }
    // After conversion, mark as pseudo-RGBA (we reuse Abgr8888 to signal "done")
    // In practice, callers should treat converted buffers as raw RGBA.
}

/// Map a Wayland `wl_shm::Format` value to our `PixelFormat`.
///
/// Returns `None` for unsupported formats.
#[allow(dead_code)]
pub fn wl_format_to_pixel_format(format: u32) -> Option<PixelFormat> {
    // Wayland wl_shm format enum values (from wayland.xml)
    match format {
        0 => Some(PixelFormat::Argb8888),  // WL_SHM_FORMAT_ARGB8888
        1 => Some(PixelFormat::Xrgb8888),  // WL_SHM_FORMAT_XRGB8888
        _ => None, // We only handle the two most common formats for now
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_argb_to_rgba() {
        // ARGB8888 in little-endian memory: B=0x11, G=0x22, R=0x33, A=0xFF
        let mut buffer = CaptureBuffer {
            width: 1,
            height: 1,
            stride: 4,
            format: PixelFormat::Argb8888,
            data: vec![0x11, 0x22, 0x33, 0xFF],
        };
        convert_to_rgba(&mut buffer);
        // After: R=0x33, G=0x22, B=0x11, A=0xFF
        assert_eq!(buffer.data, vec![0x33, 0x22, 0x11, 0xFF]);
    }

    #[test]
    fn test_xrgb_to_rgba_sets_alpha() {
        let mut buffer = CaptureBuffer {
            width: 1,
            height: 1,
            stride: 4,
            format: PixelFormat::Xrgb8888,
            data: vec![0x11, 0x22, 0x33, 0x00], // alpha is 0 (unused)
        };
        convert_to_rgba(&mut buffer);
        // Alpha should be forced to 0xFF
        assert_eq!(buffer.data, vec![0x33, 0x22, 0x11, 0xFF]);
    }
}
