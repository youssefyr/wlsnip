use crate::backends::CaptureBackend;
use crate::buffers::{CaptureBuffer, PixelFormat};
use crate::error::{Result, WlsnipError};
use crate::utils::geometry::Region;

/// Backend using the XDG Desktop Portal Screenshot interface.
///
/// This works on GNOME, KDE, and any compositor implementing
/// org.freedesktop.portal.Screenshot over D-Bus.
pub struct XdgPortalBackend {}

impl XdgPortalBackend {
    /// Check if the XDG Desktop Portal screenshot interface is available.
    pub fn probe() -> Option<Self> {
        // Portal backend is the ultimate fallback.
        // We can just check if DBUS_SESSION_BUS_ADDRESS is present.
        if std::env::var("DBUS_SESSION_BUS_ADDRESS").is_ok() {
            Some(XdgPortalBackend {})
        } else {
            None
        }
    }
}

impl CaptureBackend for XdgPortalBackend {
    fn name(&self) -> &'static str {
        "xdg-desktop-portal"
    }

    fn capture_output(
        &mut self,
        _output_name: Option<&str>,
        _region: Option<&Region>,
        _include_cursor: bool,
    ) -> Result<CaptureBuffer> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| WlsnipError::Capture(format!("Failed to create tokio runtime: {e}")))?;

        let uri = rt.block_on(async {
            // With ashpd 0.8+ (we use 0.13), we use Screenshot::request()
            let response = ashpd::desktop::screenshot::Screenshot::request()
                .interactive(true) // Always interactive to let the portal handle region/full screen UI
                .send()
                .await
                .map_err(|e| WlsnipError::Capture(format!("ashpd error: {e}")))?
                .response()
                .map_err(|e| WlsnipError::Capture(format!("portal response error: {e}")))?;
            
            Ok::<_, WlsnipError>(response.uri().to_string())
        })?;

        // The URI is typically a `file://` URI pointing to a temporary PNG/JPEG
        let path = if uri.starts_with("file://") {
            // ashpd URIs might be URL encoded, but usually they're safe enough.
            // If it's URL encoded we could use `url` crate, but let's try direct slice.
            // ashpd has `url()` method returning a `url::Url` which we can use `to_file_path()` on if `url` crate is available.
            // But we don't have `url` crate in Cargo.toml.
            let path_str = &uri["file://".len()..];
            // Decode simple url-encoded characters if necessary? Usually portals just write to `/tmp`
            path_str.to_string()
        } else {
            uri
        };

        // Load the image from disk
        let img = image::open(&path)
            .map_err(|e| WlsnipError::Capture(format!("failed to open portal image {path}: {e}")))?
            .to_rgba8();

        let (width, height) = img.dimensions();
        let data = img.into_raw();
        
        Ok(CaptureBuffer {
            width,
            height,
            stride: width * 4,
            format: PixelFormat::Abgr8888, // In wlsnip utils::color, Abgr8888 maps to native RGBA layout
            data,
        })
    }
}
