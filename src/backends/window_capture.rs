use crate::backends::CaptureBackend;
use crate::buffers::shm::ShmPool;
use crate::buffers::{CaptureBuffer, PixelFormat};
use crate::error::{Result, WlsnipError};
use crate::utils::geometry::Region;

use std::os::fd::AsFd;
use std::sync::{Arc, Mutex};

use wayland_client::protocol::{wl_buffer, wl_registry, wl_shm, wl_shm_pool};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, WEnum};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1, ext_foreign_toplevel_list_v1,
};
use wayland_protocols::ext::image_capture_source::v1::client::{
    ext_foreign_toplevel_image_capture_source_manager_v1, ext_image_capture_source_v1,
};
use wayland_protocols::ext::image_copy_capture::v1::client::{
    ext_image_copy_capture_frame_v1, ext_image_copy_capture_manager_v1,
    ext_image_copy_capture_session_v1,
};

// ── Wayland state ──────────────────────────────────────────────────────────

struct WindowCaptureState {
    capture_manager: Option<ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1>,
    source_manager: Option<ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1>,
    toplevel_list: Option<ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1>,
    shm: Option<wl_shm::WlShm>,
    toplevels: Vec<ToplevelInfo>,
}

#[derive(Debug, Clone)]
struct ToplevelInfo {
    handle: ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
    title: String,
    app_id: String,
}

// ── Session constraint state ────────────────────────────────────────────────

#[derive(Default)]
struct SessionConstraints {
    width: Option<u32>,
    height: Option<u32>,
    shm_format: Option<WEnum<wl_shm::Format>>,
    done: bool,
    stopped: bool,
}

// ── Frame copy state ────────────────────────────────────────────────────────

#[derive(Default)]
struct FrameCopyState {
    ready: bool,
    failed: bool,
}

// ── Dispatch impls ─────────────────────────────────────────────────────────

impl Dispatch<wl_registry::WlRegistry, ()> for WindowCaptureState {
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
                "ext_foreign_toplevel_image_capture_source_manager_v1" => {
                    let mgr = registry
                        .bind::<ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1, _, _>(
                            name,
                            version.min(1),
                            qh,
                            (),
                        );
                    state.source_manager = Some(mgr);
                }
                "ext_foreign_toplevel_list_v1" => {
                    let list = registry
                        .bind::<ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1, _, _>(
                            name,
                            version.min(1),
                            qh,
                            (),
                        );
                    state.toplevel_list = Some(list);
                }
                "wl_shm" => {
                    let shm = registry.bind::<wl_shm::WlShm, _, _>(name, version.min(1), qh, ());
                    state.shm = Some(shm);
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1, ()> for WindowCaptureState {
    fn event(
        state: &mut Self,
        _list: &ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1,
        event: ext_foreign_toplevel_list_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } = event {
            state.toplevels.push(ToplevelInfo {
                handle: toplevel,
                title: String::new(),
                app_id: String::new(),
            });
        }
    }
}

impl Dispatch<ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1, ()> for WindowCaptureState {
    fn event(
        state: &mut Self,
        handle: &ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
        event: ext_foreign_toplevel_handle_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let Some(info) = state.toplevels.iter_mut().find(|t| t.handle == *handle) {
            match event {
                ext_foreign_toplevel_handle_v1::Event::Title { title } => {
                    info.title = title;
                }
                ext_foreign_toplevel_handle_v1::Event::AppId { app_id } => {
                    info.app_id = app_id;
                }
                ext_foreign_toplevel_handle_v1::Event::Closed => {
                    // We could remove it, but for a short-lived capture tool it's fine to leave
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_shm::WlShm, ()> for WindowCaptureState {
    fn event(
        _state: &mut Self,
        _shm: &wl_shm::WlShm,
        _event: wl_shm::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for WindowCaptureState {
    fn event(
        _state: &mut Self,
        _pool: &wl_shm_pool::WlShmPool,
        _event: wl_shm_pool::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<wl_buffer::WlBuffer, ()> for WindowCaptureState {
    fn event(
        _state: &mut Self,
        _buffer: &wl_buffer::WlBuffer,
        _event: wl_buffer::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1, ()> for WindowCaptureState {
    fn event(
        _state: &mut Self,
        _manager: &ext_image_copy_capture_manager_v1::ExtImageCopyCaptureManagerV1,
        _event: ext_image_copy_capture_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1, ()>
    for WindowCaptureState
{
    fn event(
        _state: &mut Self,
        _mgr: &ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1,
        _event: ext_foreign_toplevel_image_capture_source_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<ext_image_capture_source_v1::ExtImageCaptureSourceV1, ()> for WindowCaptureState {
    fn event(
        _state: &mut Self,
        _src: &ext_image_capture_source_v1::ExtImageCaptureSourceV1,
        _event: ext_image_capture_source_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {}
}

impl Dispatch<ext_image_copy_capture_session_v1::ExtImageCopyCaptureSessionV1, Arc<Mutex<SessionConstraints>>>
    for WindowCaptureState
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
    for WindowCaptureState
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

pub struct WindowCaptureBackend {
    state: WindowCaptureState,
    queue: EventQueue<WindowCaptureState>,
    query: Option<String>,
}

impl WindowCaptureBackend {
    pub fn probe() -> Option<Self> {
        let conn = Connection::connect_to_env().ok()?;
        let display = conn.display();

        let mut state = WindowCaptureState {
            capture_manager: None,
            source_manager: None,
            toplevel_list: None,
            shm: None,
            toplevels: Vec::new(),
        };

        let mut queue = conn.new_event_queue();
        let qh = queue.handle();

        display.get_registry(&qh, ());
        
        // Two roundtrips: first discovers globals, second populates toplevels
        queue.roundtrip(&mut state).ok()?;
        queue.roundtrip(&mut state).ok()?;

        if state.capture_manager.is_some()
            && state.source_manager.is_some()
            && state.toplevel_list.is_some()
            && state.shm.is_some()
        {
            Some(Self { state, queue, query: None })
        } else {
            None
        }
    }

    /// Set the window query manually since the CaptureBackend trait doesn't currently 
    /// pass the query natively. (We will just set it after probing).
    pub fn set_query(&mut self, query: Option<String>) {
        self.query = query;
    }
}

fn wenum_to_pixel_format(fmt: &WEnum<wl_shm::Format>) -> PixelFormat {
    match fmt {
        WEnum::Value(wl_shm::Format::Argb8888) => PixelFormat::Argb8888,
        WEnum::Value(wl_shm::Format::Xrgb8888) => PixelFormat::Xrgb8888,
        WEnum::Value(wl_shm::Format::Abgr8888) => PixelFormat::Abgr8888,
        WEnum::Value(wl_shm::Format::Xbgr8888) => PixelFormat::Xbgr8888,
        _ => PixelFormat::Xrgb8888,
    }
}

fn wenum_to_wl_format(fmt: &WEnum<wl_shm::Format>) -> wl_shm::Format {
    match fmt {
        WEnum::Value(f) => *f,
        _ => wl_shm::Format::Xrgb8888,
    }
}

impl CaptureBackend for WindowCaptureBackend {
    fn name(&self) -> &'static str {
        "ext-window-capture"
    }

    fn capture_output(
        &mut self,
        _output_name: Option<&str>,
        region: Option<&Region>,
        include_cursor: bool,
    ) -> Result<CaptureBuffer> {
        let qh = self.queue.handle();

        if self.state.toplevels.is_empty() {
            return Err(WlsnipError::Capture("no windows found".to_string()));
        }

        let target_toplevel = if let Some(query) = &self.query {
            let query_lower = query.to_lowercase();
            self.state
                .toplevels
                .iter()
                .find(|t| t.title.to_lowercase().contains(&query_lower) || t.app_id.to_lowercase().contains(&query_lower))
                .ok_or_else(|| WlsnipError::Capture(format!("no window matching '{}' found", query)))?
                .clone()
        } else {
            // For now, if no query, capture the first one. Alternatively, we could fail.
            // Active window detection requires ext-foreign-toplevel-state, which is more complex.
            self.state
                .toplevels
                .first()
                .unwrap()
                .clone()
        };

        let capture_mgr = self.state.capture_manager.as_ref().unwrap();
        let source_mgr = self.state.source_manager.as_ref().unwrap();

        let source = source_mgr.create_source(&target_toplevel.handle, &qh, ());

        let options = if include_cursor {
            ext_image_copy_capture_manager_v1::Options::PaintCursors
        } else {
            ext_image_copy_capture_manager_v1::Options::empty()
        };

        let constraints = Arc::new(Mutex::new(SessionConstraints::default()));
        let session = capture_mgr.create_session(&source, options, &qh, constraints.clone());

        loop {
            self.queue
                .roundtrip(&mut self.state)
                .map_err(|e| WlsnipError::Protocol(format!("roundtrip failed: {e}")))?;

            let sc = constraints.lock().unwrap();
            if sc.stopped {
                return Err(WlsnipError::Capture("window capture session stopped".to_string()));
            }
            if sc.done {
                break;
            }
        }

        let (width, height, shm_format_enum) = {
            let sc = constraints.lock().unwrap();
            let w = sc.width.ok_or_else(|| WlsnipError::Capture("no buffer_size".to_string()))?;
            let h = sc.height.ok_or_else(|| WlsnipError::Capture("no buffer_size".to_string()))?;
            let fmt = sc.shm_format.clone().ok_or_else(|| WlsnipError::Capture("no shm format".to_string()))?;
            (w, h, fmt)
        };

        let stride = width * 4;
        let pool_size = (stride * height) as usize;
        let shm_pool = ShmPool::new(pool_size)?;
        let wl_shm = self.state.shm.as_ref().unwrap();

        let wl_pool = wl_shm.create_pool(shm_pool.fd().as_fd(), pool_size as i32, &qh, ());
        let wl_buffer = wl_pool.create_buffer(
            0,
            width as i32,
            height as i32,
            stride as i32,
            wenum_to_wl_format(&shm_format_enum),
            &qh,
            (),
        );

        let frame_state = Arc::new(Mutex::new(FrameCopyState::default()));
        let frame = session.create_frame(&qh, frame_state.clone());

        frame.attach_buffer(&wl_buffer);
        frame.damage_buffer(0, 0, width as i32, height as i32);
        frame.capture();

        loop {
            self.queue
                .roundtrip(&mut self.state)
                .map_err(|e| WlsnipError::Protocol(format!("roundtrip failed: {e}")))?;

            let fs = frame_state.lock().unwrap();
            if fs.ready {
                break;
            }
            if fs.failed {
                return Err(WlsnipError::Capture("window capture frame failed".to_string()));
            }
        }

        let pixel_format = wenum_to_pixel_format(&shm_format_enum);
        let mut capture_buffer = shm_pool.to_capture_buffer(width, height, stride, pixel_format);

        if let Some(region) = region {
            capture_buffer = crate::utils::geometry::crop(&capture_buffer, region)?;
        }

        wl_buffer.destroy();
        wl_pool.destroy();
        frame.destroy();
        session.destroy();
        source.destroy();

        Ok(capture_buffer)
    }
}
