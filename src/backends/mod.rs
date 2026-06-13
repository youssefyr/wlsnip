pub mod wlr_screencopy;
pub mod ext_capture;
pub mod xdg_portal;
pub mod window_capture;

use crate::buffers::CaptureBuffer;
use crate::error::Result;
use crate::utils::geometry::Region;

/// Trait implemented by each capture backend.
pub trait CaptureBackend {
    /// Human-readable name of this backend.
    #[allow(dead_code)]
    fn name(&self) -> &'static str;

    /// Capture a full output, optionally cropped to a region.
    fn capture_output(
        &mut self,
        output_name: Option<&str>,
        region: Option<&Region>,
        include_cursor: bool,
    ) -> Result<CaptureBuffer>;

    /// Capture all outputs and stitch them together (multi-output/workspace).
    fn capture_all_outputs(&mut self, include_cursor: bool) -> Result<CaptureBuffer> {
        // Default fallback: just capture the first output
        self.capture_output(None, None, include_cursor)
    }
}
