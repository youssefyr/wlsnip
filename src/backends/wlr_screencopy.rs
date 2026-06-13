use crate::backends::CaptureBackend;
use crate::buffers::shm::ShmPool;
use crate::buffers::{CaptureBuffer, PixelFormat};
use crate::error::{Result, WlsnipError};
use crate::utils::geometry::Region;

use std::os::fd::AsFd;
use std::sync::{Arc, Mutex};

use wayland_client::protocol::{wl_buffer, wl_output, wl_registry, wl_shm, wl_shm_pool};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, WEnum};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1, zwlr_screencopy_manager_v1,
};

/// State for tracking Wayland registry globals and output info.
struct WaylandState {
    /// Bound screencopy manager (if available)
    screencopy_manager: Option<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1>,
    /// Bound wl_shm global
    shm: Option<wl_shm::WlShm>,
    /// Discovered outputs with their names
    outputs: Vec<OutputInfo>,
}

#[derive(Debug, Clone)]
struct OutputInfo {
    name: String,
    x: i32,
    y: i32,
    wl_output: wl_output::WlOutput,
}

/// State passed to the screencopy frame callback.
struct FrameState {
    /// Buffer format requested by the compositor (raw WEnum)
    format: Option<WEnum<wl_shm::Format>>,
    width: Option<u32>,
    height: Option<u32>,
    stride: Option<u32>,
    /// Whether the frame is ready (pixels have been copied)
    ready: bool,
    /// Whether the frame capture failed
    failed: bool,
}

impl FrameState {
    fn new() -> Self {
        Self {
            format: None,
            width: None,
            height: None,
            stride: None,
            ready: false,
            failed: false,
        }
    }
}

/// Backend using the wlr-screencopy-unstable-v1 protocol.
///
/// Widely supported on wlroots-based compositors: Sway, Hyprland, River, etc.
pub struct WlrScreencopyBackend {
    state: WaylandState,
    queue: EventQueue<WaylandState>,
}

// -- Dispatch implementations for Wayland events --

impl Dispatch<wl_registry::WlRegistry, ()> for WaylandState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "zwlr_screencopy_manager_v1" => {
                    let manager = registry
                        .bind::<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1, _, _>(
                            name,
                            version.min(3),
                            qh,
                            (),
                        );
                    state.screencopy_manager = Some(manager);
                }
                "wl_shm" => {
                    let shm =
                        registry.bind::<wl_shm::WlShm, _, _>(name, version.min(1), qh, ());
                    state.shm = Some(shm);
                }
                "wl_output" => {
                    let output =
                        registry.bind::<wl_output::WlOutput, _, _>(name, version.min(4), qh, ());
                    state.outputs.push(OutputInfo {
                        name: String::new(),
                        x: 0,
                        y: 0,
                        wl_output: output,
                    });
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for WaylandState {
    fn event(
        state: &mut Self,
        output: &wl_output::WlOutput,
        event: wl_output::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let Some(info) = state.outputs.iter_mut().find(|o| o.wl_output == *output) {
            match event {
                wl_output::Event::Name { name } => {
                    info.name = name;
                }
                wl_output::Event::Geometry { x, y, .. } => {
                    info.x = x;
                    info.y = y;
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_shm::WlShm, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _shm: &wl_shm::WlShm,
        _event: wl_shm::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // We don't need to handle wl_shm format advertisements
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _pool: &wl_shm_pool::WlShmPool,
        _event: wl_shm_pool::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_buffer::WlBuffer, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _buffer: &wl_buffer::WlBuffer,
        _event: wl_buffer::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _manager: &zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
        _event: zwlr_screencopy_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1, Arc<Mutex<FrameState>>>
    for WaylandState
{
    fn event(
        _state: &mut Self,
        _frame: &zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event,
        data: &Arc<Mutex<FrameState>>,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let mut fs = data.lock().unwrap();
        match event {
            zwlr_screencopy_frame_v1::Event::Buffer {
                format,
                width,
                height,
                stride,
            } => {
                fs.format = Some(format);
                fs.width = Some(width);
                fs.height = Some(height);
                fs.stride = Some(stride);
            }
            zwlr_screencopy_frame_v1::Event::Ready { .. } => {
                fs.ready = true;
            }
            zwlr_screencopy_frame_v1::Event::Failed => {
                fs.failed = true;
            }
            _ => {}
        }
    }
}

impl WlrScreencopyBackend {
    /// Probe the Wayland display for wlr-screencopy support.
    ///
    /// Returns `Some(Self)` if the compositor advertises `zwlr_screencopy_manager_v1`.
    pub fn probe() -> Option<Self> {
        let conn = Connection::connect_to_env().ok()?;
        let display = conn.display();

        let mut state = WaylandState {
            screencopy_manager: None,
            shm: None,
            outputs: Vec::new(),
        };

        let mut queue = conn.new_event_queue();
        let qh = queue.handle();

        // Get the registry and do initial roundtrips to discover globals + output names
        display.get_registry(&qh, ());
        queue.roundtrip(&mut state).ok()?;
        queue.roundtrip(&mut state).ok()?;

        if state.screencopy_manager.is_some() && state.shm.is_some() {
            Some(Self { state, queue })
        } else {
            None
        }
    }
}

/// Convert a WEnum<wl_shm::Format> to our PixelFormat.
fn wenum_to_pixel_format(format: &WEnum<wl_shm::Format>) -> PixelFormat {
    match format {
        WEnum::Value(wl_shm::Format::Argb8888) => PixelFormat::Argb8888,
        WEnum::Value(wl_shm::Format::Xrgb8888) => PixelFormat::Xrgb8888,
        WEnum::Value(wl_shm::Format::Abgr8888) => PixelFormat::Abgr8888,
        WEnum::Value(wl_shm::Format::Xbgr8888) => PixelFormat::Xbgr8888,
        _ => PixelFormat::Xrgb8888, // safe fallback
    }
}

/// Convert a WEnum<wl_shm::Format> to the wl_shm::Format for create_buffer.
/// Falls back to Xrgb8888 for unknown formats.
fn wenum_to_wl_format(format: &WEnum<wl_shm::Format>) -> wl_shm::Format {
    match format {
        WEnum::Value(f) => *f,
        _ => wl_shm::Format::Xrgb8888,
    }
}

impl CaptureBackend for WlrScreencopyBackend {
    fn name(&self) -> &'static str {
        "wlr-screencopy-unstable-v1"
    }

    fn capture_output(
        &mut self,
        output_name: Option<&str>,
        region: Option<&Region>,
        include_cursor: bool,
    ) -> Result<CaptureBuffer> {
        let qh = self.queue.handle();

        // Find the target output
        let output = if let Some(name) = output_name {
            self.state
                .outputs
                .iter()
                .find(|o| o.name == name)
                .ok_or_else(|| WlsnipError::OutputNotFound(name.to_string()))?
                .clone()
        } else {
            self.state
                .outputs
                .first()
                .ok_or_else(|| WlsnipError::Capture("no outputs found".to_string()))?
                .clone()
        };

        let manager = self
            .state
            .screencopy_manager
            .as_ref()
            .ok_or_else(|| WlsnipError::Capture("screencopy manager not bound".to_string()))?;

        let frame_state = Arc::new(Mutex::new(FrameState::new()));
        let overlay_cursor = if include_cursor { 1 } else { 0 };

        // Request capture frame (with or without region)
        let frame = if let Some(region) = region {
            manager.capture_output_region(
                overlay_cursor,
                &output.wl_output,
                region.x,
                region.y,
                region.width as i32,
                region.height as i32,
                &qh,
                frame_state.clone(),
            )
        } else {
            manager.capture_output(overlay_cursor, &output.wl_output, &qh, frame_state.clone())
        };

        // First roundtrip: get buffer constraints from compositor
        self.queue
            .roundtrip(&mut self.state)
            .map_err(|e| WlsnipError::Protocol(format!("roundtrip failed: {e}")))?;

        let (wl_format_enum, width, height, stride) = {
            let fs = frame_state.lock().unwrap();
            if fs.failed {
                return Err(WlsnipError::Capture(
                    "compositor rejected frame capture".to_string(),
                ));
            }
            let format = fs.format.clone().ok_or_else(|| {
                WlsnipError::Capture("compositor did not send buffer format".to_string())
            })?;
            let width = fs.width.ok_or_else(|| {
                WlsnipError::Capture("compositor did not send buffer width".to_string())
            })?;
            let height = fs.height.ok_or_else(|| {
                WlsnipError::Capture("compositor did not send buffer height".to_string())
            })?;
            let stride = fs.stride.ok_or_else(|| {
                WlsnipError::Capture("compositor did not send buffer stride".to_string())
            })?;
            (format, width, height, stride)
        };

        // Allocate SHM pool and wl_buffer
        let pool_size = (stride * height) as usize;
        let shm_pool = ShmPool::new(pool_size)?;

        let wl_shm = self
            .state
            .shm
            .as_ref()
            .ok_or_else(|| WlsnipError::Capture("wl_shm not bound".to_string()))?;

        let wl_pool = wl_shm.create_pool(shm_pool.fd().as_fd(), pool_size as i32, &qh, ());

        let wl_buffer = wl_pool.create_buffer(
            0,
            width as i32,
            height as i32,
            stride as i32,
            wenum_to_wl_format(&wl_format_enum),
            &qh,
            (),
        );

        // Request the compositor to copy pixels into our buffer
        frame.copy(&wl_buffer);

        // Roundtrip until ready or failed
        loop {
            self.queue
                .roundtrip(&mut self.state)
                .map_err(|e| WlsnipError::Protocol(format!("roundtrip failed: {e}")))?;

            let fs = frame_state.lock().unwrap();
            if fs.ready {
                break;
            }
            if fs.failed {
                return Err(WlsnipError::Capture("frame copy failed".to_string()));
            }
        }

        // Extract pixels from SHM into an owned CaptureBuffer
        let pixel_format = wenum_to_pixel_format(&wl_format_enum);
        let capture_buffer = shm_pool.to_capture_buffer(width, height, stride, pixel_format);

        // Cleanup Wayland objects
        wl_buffer.destroy();
        wl_pool.destroy();
        frame.destroy();

        Ok(capture_buffer)
    }

    fn capture_all_outputs(&mut self, include_cursor: bool) -> Result<CaptureBuffer> {
        let mut buffers = Vec::new();
        
        let names: Vec<String> = self.state.outputs.iter().map(|o| o.name.clone()).collect();
        
        // Capture each output individually
        for name in names {
            if let Ok(buf) = self.capture_output(Some(&name), None, include_cursor) {
                // Find the output info to get x and y
                if let Some(info) = self.state.outputs.iter().find(|o| o.name == name) {
                    buffers.push((info.clone(), buf));
                }
            }
        }

        if buffers.is_empty() {
            return Err(WlsnipError::Capture("Failed to capture any outputs".to_string()));
        }

        // Calculate bounding box for the stitched image
        let mut min_x = i32::MAX;
        let mut min_y = i32::MAX;
        let mut max_x = i32::MIN;
        let mut max_y = i32::MIN;

        for (info, buf) in &buffers {
            let px = info.x;
            let py = info.y;
            if px < min_x { min_x = px; }
            if py < min_y { min_y = py; }
            // Note: outputs' width/height might be logical, while buf is physical.
            // For robust stitching, we rely on buf's physical dimensions.
            if px + (buf.width as i32) > max_x { max_x = px + (buf.width as i32); }
            if py + (buf.height as i32) > max_y { max_y = py + (buf.height as i32); }
        }

        let total_width = (max_x - min_x).max(0) as u32;
        let total_height = (max_y - min_y).max(0) as u32;
        
        // Assume all buffers have the same format and BPP
        let first_buf = &buffers[0].1;
        let format = first_buf.format;
        let bpp = format.bytes_per_pixel();
        let total_stride = total_width * bpp;
        
        let mut stitched_data = vec![0u8; (total_stride * total_height) as usize];

        // Paste each buffer into the combined image
        for (info, buf) in &buffers {
            let offset_x = (info.x - min_x).max(0) as u32;
            let offset_y = (info.y - min_y).max(0) as u32;

            for row in 0..buf.height {
                let dst_y = offset_y + row;
                if dst_y >= total_height { break; }
                
                let src_offset = (row * buf.stride) as usize;
                let dst_offset = (dst_y * total_stride + offset_x * bpp) as usize;
                let copy_len = (buf.width * bpp) as usize;
                
                // Ensure we don't write out of bounds
                let copy_len = copy_len.min((total_width - offset_x) as usize * bpp as usize);
                if dst_offset + copy_len <= stitched_data.len() && src_offset + copy_len <= buf.data.len() {
                    stitched_data[dst_offset..dst_offset + copy_len]
                        .copy_from_slice(&buf.data[src_offset..src_offset + copy_len]);
                }
            }
        }

        Ok(CaptureBuffer {
            width: total_width,
            height: total_height,
            stride: total_stride,
            format,
            data: stitched_data,
        })
    }
}
