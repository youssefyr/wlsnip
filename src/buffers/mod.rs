pub mod shm;

/// Pixel format of a captured buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// ARGB with 8 bits per channel (Wayland's most common format)
    Argb8888,
    /// XRGB with 8 bits per channel (opaque, alpha ignored)
    Xrgb8888,
    /// XBGR with 8 bits per channel
    Xbgr8888,
    /// ABGR with 8 bits per channel
    Abgr8888,
}

impl PixelFormat {
    /// Bytes per pixel for this format (always 4 for 32-bit formats).
    #[allow(dead_code)]
    pub fn bytes_per_pixel(&self) -> u32 {
        4
    }
}

/// A captured image buffer holding raw pixel data.
#[derive(Debug, Clone)]
pub struct CaptureBuffer {
    /// Image width in pixels
    pub width: u32,
    /// Image height in pixels
    pub height: u32,
    /// Row stride in bytes (may include padding)
    pub stride: u32,
    /// Pixel format of the data
    pub format: PixelFormat,
    /// Raw pixel data, row-major, `stride * height` bytes
    pub data: Vec<u8>,
}

impl CaptureBuffer {
    /// Create a new capture buffer with pre-allocated zeroed data.
    #[allow(dead_code)]
    pub fn new(width: u32, height: u32, stride: u32, format: PixelFormat) -> Self {
        let size = (stride * height) as usize;
        Self {
            width,
            height,
            stride,
            format,
            data: vec![0u8; size],
        }
    }

    /// Total size of the pixel data in bytes.
    #[allow(dead_code)]
    pub fn data_size(&self) -> usize {
        (self.stride * self.height) as usize
    }
}
