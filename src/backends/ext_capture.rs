use crate::backends::CaptureBackend;
use crate::buffers::shm::ShmPool;
use crate::buffers::{CaptureBuffer, PixelFormat};
use crate::error::{Result, WlsnipError};
use crate::utils::geometry::Region;

use std::os::fd::AsFd;
use std::sync::{Arc, Mutex};

use wayland_client::protocol::{wl_buffer, wl_output, wl_registry, wl_shm, wl_shm_pool};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, WEnum};
use wayland_protocols::ext::image_capture_source::v1::client::{
    ext_image_capture_source_v1, ext_output_image_capture_source_manager_v1,
};
use wayland_protocols::ext::image_copy_capture::v1::client::{
    ext_image_copy_capture_frame_v1, ext_image_copy_capture_manager_v1,
    ext_image_copy_capture_session_v1,
};

// ── Wayland state ──────────────────────────────────────────────────────────

/// Global state used during Wayland registry enumeration.
struct ExtState {
    /// Bound ext-image-copy-capture manager
    capture_manager: Option<ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1>,
    /// Bound ext-output-image-capture-source manager
    source_manager: Option<ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1>,
    /// Bound wl_shm
    shm: Option<wl_shm::WlShm>,
    /// Discovered outputs with their names
    outputs: Vec<OutputInfo>,
}

#[derive(Debug, Clone)]
struct OutputInfo {
    name: String,
    wl_output: wl_output::WlOutput,
}

// ── Session constraint state ────────────────────────────────────────────────

/// Constraints advertised by the compositor for a capture session.
#[derive(Default)]
struct SessionConstraints {
    /// Agreed buffer width (from buffer_size event)
    width: Option<u32>,
    /// Agreed buffer height (from buffer_size event)
    height: Option<u32>,
    /// Preferred SHM format (first shm_format event wins)
    shm_format: Option<WEnum<wl_shm::Format>>,
    /// Set to true after `done` event — constraints are final
    done: bool,
    /// Set to true after `stopped` event — session is dead
    stopped: bool,
}

// ── Frame copy state ────────────────────────────────────────────────────────

/// Tracks a single frame copy operation.
#[derive(Default)]
struct FrameCopyState {
    ready: bool,
    failed: bool,
}

// ── Dispatch impls ─────────────────────────────────────────────────────────

impl Dispatch<wl_registry::WlRegistry, ()> for ExtState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { name, interface, version } = event {
            match interface.as_str() {
                "ext_image_copy_capture_manager_v1" => {
                    let mgr = registry
                        .bind::<ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1, _, _>(
                            name,
                            version.min(1),
                            qh,
                            (),
                        );
                    state.capture_manager = Some(mgr);
                }
                "ext_output_image_capture_source_manager_v1" => {
                    let mgr = registry
                        .bind::<ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1, _, _>(
                            name,
                            version.min(1),
                            qh,
                            (),
                        );
                    state.source_manager = Some(mgr);
                }
                "wl_shm" => {
                    let shm =
                        registry.bind::<wl_shm::WlShm, _, _>(name, version.min(1), qh, ());
                    state.shm = Some(shm);
                }
                "wl_output" => {
                    let output = registry
                        .bind::<wl_output::WlOutput, _, _>(name, version.min(4), qh, ());
                    state.outputs.push(OutputInfo {
                        name: String::new(),
                        wl_output: output,
                    });
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for ExtState {
    fn event(
        state: &mut Self,
        output: &wl_output::WlOutput,
        event: wl_output::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let Some(info) = state.outputs.iter_mut().find(|o| o.wl_output == *output) {
            if let wl_output::Event::Name { name } = event {
                info.name = name;
            }
        }
    }
}

impl Dispatch<wl_shm::WlShm, ()> for ExtState {
    fn event(
        _state: &mut Self,
        _shm: &wl_shm::WlShm,
        _event: wl_shm::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for ExtState {
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

impl Dispatch<wl_buffer::WlBuffer, ()> for ExtState {
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

impl Dispatch<ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1, ()> for ExtState {
    fn event(
        _state: &mut Self,
        _manager: &ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1,
        _event: ext_image_copy_capture_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1, ()>
    for ExtState
{
    fn event(
        _state: &mut Self,
        _mgr: &ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1,
        _event: ext_output_image_capture_source_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ext_image_capture_source_v1::ExtImageCaptureSourceV1, ()> for ExtState {
    fn event(
        _state: &mut Self,
        _src: &ext_image_capture_source_v1::ExtImageCaptureSourceV1,
        _event: ext_image_capture_source_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ext_image_copy_capture_session_v1::ExtImageCopyCaptureSessionV1, Arc<Mutex<SessionConstraints>>>
    for ExtState
{
    fn event(
        _state: &mut Self,
        _session: &ext_image_copy_capture_session_v1::ExtImageCopyCaptureSessionV1,
        event: ext_image_copy_capture_session_v1::Event,
        data: &Arc<Mutex<SessionConstraints>>,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let mut sc = data.lock().unwrap();
        match event {
            ext_image_copy_capture_session_v1::Event::BufferSize { width, height } => {
                sc.width = Some(width);
                sc.height = Some(height);
            }
            ext_image_copy_capture_session_v1::Event::ShmFormat { format } => {
                // Accept the first advertised SHM format
                if sc.shm_format.is_none() {
                    sc.shm_format = Some(format);
                }
            }
            ext_image_copy_capture_session_v1::Event::Done => {
                sc.done = true;
            }
            ext_image_copy_capture_session_v1::Event::Stopped => {
                sc.stopped = true;
            }
            _ => {}
        }
    }
}

impl Dispatch<ext_image_copy_capture_frame_v1::ExtImageCopyCaptureFrameV1, Arc<Mutex<FrameCopyState>>>
    for ExtState
{
    fn event(
        _state: &mut Self,
        _frame: &ext_image_copy_capture_frame_v1::ExtImageCopyCaptureFrameV1,
        event: ext_image_copy_capture_frame_v1::Event,
        data: &Arc<Mutex<FrameCopyState>>,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let mut fs = data.lock().unwrap();
        match event {
            ext_image_copy_capture_frame_v1::Event::Ready => {
                fs.ready = true;
            }
            ext_image_copy_capture_frame_v1::Event::Failed { .. } => {
                fs.failed = true;
            }
            _ => {}
        }
    }
}

// ── Backend struct ─────────────────────────────────────────────────────────

/// Backend using the ext-image-copy-capture-v1 Wayland protocol.
///
/// This is the modern standardized capture protocol, supported by COSMIC,
/// newer versions of wlroots, and other compositors that have adopted the
/// ext-* staging protocol family.
pub struct ExtCaptureBackend {
    state: ExtState,
    queue: EventQueue<ExtState>,
}

impl ExtCaptureBackend {
    /// Probe the Wayland display for ext-image-copy-capture-v1 support.
    ///
    /// Returns `Some(Self)` only if the compositor advertises both
    /// `ext_image_copy_capture_manager_v1` and
    /// `ext_output_image_capture_source_manager_v1`.
    pub fn probe() -> Option<Self> {
        let conn = Connection::connect_to_env().ok()?;
        let display = conn.display();

        let mut state = ExtState {
            capture_manager: None,
            source_manager: None,
            shm: None,
            outputs: Vec::new(),
        };

        let mut queue = conn.new_event_queue();
        let qh = queue.handle();

        display.get_registry(&qh, ());
        // Two roundtrips: first discovers globals, second populates output names
        queue.roundtrip(&mut state).ok()?;
        queue.roundtrip(&mut state).ok()?;

        if state.capture_manager.is_some()
            && state.source_manager.is_some()
            && state.shm.is_some()
        {
            Some(Self { state, queue })
        } else {
            None
        }
    }
}

// ── Helper: WEnum<wl_shm::Format> → PixelFormat ──────────────────────────

fn wenum_to_pixel_format(fmt: &WEnum<wl_shm::Format>) -> PixelFormat {
    match fmt {
        WEnum::Value(wl_shm::Format::Argb8888) => PixelFormat::Argb8888,
        WEnum::Value(wl_shm::Format::Xrgb8888) => PixelFormat::Xrgb8888,
        WEnum::Value(wl_shm::Format::Abgr8888) => PixelFormat::Abgr8888,
        WEnum::Value(wl_shm::Format::Xbgr8888) => PixelFormat::Xbgr8888,
        _ => PixelFormat::Xrgb8888, // safe fallback
    }
}

fn wenum_to_wl_format(fmt: &WEnum<wl_shm::Format>) -> wl_shm::Format {
    match fmt {
        WEnum::Value(f) => *f,
        _ => wl_shm::Format::Xrgb8888,
    }
}

// ── CaptureBackend impl ────────────────────────────────────────────────────

impl CaptureBackend for ExtCaptureBackend {
    fn name(&self) -> &'static str {
        "ext-image-copy-capture-v1"
    }

    fn capture_output(
        &mut self,
        output_name: Option<&str>,
        region: Option<&Region>,
        include_cursor: bool,
    ) -> Result<CaptureBuffer> {
        let qh = self.queue.handle();

        // ── 1. Find the target wl_output ─────────────────────────────────
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

        let capture_mgr = self
            .state
            .capture_manager
            .as_ref()
            .ok_or_else(|| WlsnipError::Capture("ext capture manager not bound".to_string()))?;

        let source_mgr = self
            .state
            .source_manager
            .as_ref()
            .ok_or_else(|| WlsnipError::Capture("ext source manager not bound".to_string()))?;

        // ── 2. Create an image capture source for the output ─────────────
        let source = source_mgr.create_source(&output.wl_output, &qh, ());

        // ── 3. Create a capture session (with cursor option if requested) ─
        let options = if include_cursor {
            ext_image_copy_capture_manager_v1::Options::PaintCursors
        } else {
            ext_image_copy_capture_manager_v1::Options::empty()
        };

        let constraints = Arc::new(Mutex::new(SessionConstraints::default()));
        let session = capture_mgr.create_session(&source, options, &qh, constraints.clone());

        // ── 4. Wait for buffer constraints (done event) ──────────────────
        loop {
            self.queue
                .roundtrip(&mut self.state)
                .map_err(|e| WlsnipError::Protocol(format!("roundtrip failed: {e}")))?;

            let sc = constraints.lock().unwrap();
            if sc.stopped {
                return Err(WlsnipError::Capture(
                    "ext capture session stopped unexpectedly".to_string(),
                ));
            }
            if sc.done {
                break;
            }
        }

        let (width, height, shm_format_enum) = {
            let sc = constraints.lock().unwrap();
            let w = sc.width.ok_or_else(|| {
                WlsnipError::Capture("compositor did not send buffer_size".to_string())
            })?;
            let h = sc.height.ok_or_else(|| {
                WlsnipError::Capture("compositor did not send buffer_size".to_string())
            })?;
            let fmt = sc.shm_format.clone().ok_or_else(|| {
                WlsnipError::Capture("compositor did not advertise any SHM format".to_string())
            })?;
            (w, h, fmt)
        };

        // ── 5. Allocate SHM buffer ────────────────────────────────────────
        let stride = width * 4; // all our supported formats are 4 bpp
        let pool_size = (stride * height) as usize;
        let shm_pool = ShmPool::new(pool_size)?;

        let wl_shm = self
            .state
            .shm
            .as_ref()
            .ok_or_else(|| WlsnipError::Capture("wl_shm not bound".to_string()))?;

        let wl_pool =
            wl_shm.create_pool(shm_pool.fd().as_fd(), pool_size as i32, &qh, ());
        let wl_buffer = wl_pool.create_buffer(
            0,
            width as i32,
            height as i32,
            stride as i32,
            wenum_to_wl_format(&shm_format_enum),
            &qh,
            (),
        );

        // ── 6. Create a capture frame, attach buffer, optionally set damage, then capture ──
        let frame_state = Arc::new(Mutex::new(FrameCopyState::default()));
        let frame = session.create_frame(&qh, frame_state.clone());

        frame.attach_buffer(&wl_buffer);

        // Set full-frame damage (required before capture)
        frame.damage_buffer(0, 0, width as i32, height as i32);

        frame.capture();

        // ── 7. Wait for ready or failed ──────────────────────────────────
        loop {
            self.queue
                .roundtrip(&mut self.state)
                .map_err(|e| WlsnipError::Protocol(format!("roundtrip failed: {e}")))?;

            let fs = frame_state.lock().unwrap();
            if fs.ready {
                break;
            }
            if fs.failed {
                return Err(WlsnipError::Capture(
                    "ext capture frame copy failed".to_string(),
                ));
            }
        }

        // ── 8. Copy pixels out of SHM into CaptureBuffer ─────────────────
        let pixel_format = wenum_to_pixel_format(&shm_format_enum);
        let mut capture_buffer = shm_pool.to_capture_buffer(width, height, stride, pixel_format);

        // ── 9. Apply region crop if requested ────────────────────────────
        if let Some(region) = region {
            capture_buffer = crate::utils::geometry::crop(&capture_buffer, region)?;
        }

        // ── 10. Cleanup ───────────────────────────────────────────────────
        wl_buffer.destroy();
        wl_pool.destroy();
        // frame and session are destroyed by the protocol (frame must be destroyed after ready/failed)
        frame.destroy();
        session.destroy();
        source.destroy();

        Ok(capture_buffer)
    }
}
