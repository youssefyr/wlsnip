use crate::buffers::CaptureBuffer;
use crate::buffers::shm::ShmPool;
use crate::error::{Result, WlsnipError};

use std::os::fd::AsFd;
use wayland_client::protocol::{wl_buffer, wl_compositor, wl_output, wl_registry, wl_shm, wl_shm_pool, wl_surface, wl_seat, wl_pointer, wl_keyboard};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1, zwlr_layer_surface_v1,
};

struct PinState {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    layer_shell: Option<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
    outputs: Vec<wl_output::WlOutput>,
    seat: Option<wl_seat::WlSeat>,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    
    surfaces: Vec<PinnedSurface>,
    dismissed: bool,
}

struct PinnedSurface {
    surface: wl_surface::WlSurface,
    layer_surface: zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
    configured: bool,
}

impl Dispatch<wl_registry::WlRegistry, ()> for PinState {
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
                "wl_compositor" => {
                    state.compositor = Some(registry.bind::<wl_compositor::WlCompositor, _, _>(name, version.min(4), qh, ()));
                }
                "wl_shm" => {
                    state.shm = Some(registry.bind::<wl_shm::WlShm, _, _>(name, version.min(1), qh, ()));
                }
                "zwlr_layer_shell_v1" => {
                    state.layer_shell = Some(registry.bind::<zwlr_layer_shell_v1::ZwlrLayerShellV1, _, _>(name, version.min(1), qh, ()));
                }
                "wl_output" => {
                    state.outputs.push(registry.bind::<wl_output::WlOutput, _, _>(name, version.min(1), qh, ()));
                }
                "wl_seat" => {
                    state.seat = Some(registry.bind::<wl_seat::WlSeat, _, _>(name, version.min(1), qh, ()));
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for PinState {
    fn event(state: &mut Self, seat: &wl_seat::WlSeat, event: wl_seat::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        if let wl_seat::Event::Capabilities { capabilities } = event {
            if let wayland_client::WEnum::Value(caps) = capabilities {
                if caps.contains(wl_seat::Capability::Pointer) && state.pointer.is_none() {
                    state.pointer = Some(seat.get_pointer(qh, ()));
                }
                if caps.contains(wl_seat::Capability::Keyboard) && state.keyboard.is_none() {
                    state.keyboard = Some(seat.get_keyboard(qh, ()));
                }
            }
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for PinState {
    fn event(state: &mut Self, _: &wl_pointer::WlPointer, event: wl_pointer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_pointer::Event::Button { state: btn_state, .. } = event {
            if btn_state == wayland_client::WEnum::Value(wayland_client::protocol::wl_pointer::ButtonState::Pressed) {
                state.dismissed = true;
            }
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for PinState {
    fn event(state: &mut Self, _: &wl_keyboard::WlKeyboard, event: wl_keyboard::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_keyboard::Event::Key { key, state: key_state, .. } = event {
            if key_state == wayland_client::WEnum::Value(wayland_client::protocol::wl_keyboard::KeyState::Pressed) {
                if key == 1 { // KEY_ESC
                    state.dismissed = true;
                }
            }
        }
    }
}

impl Dispatch<wl_compositor::WlCompositor, ()> for PinState { fn event(_: &mut Self, _: &wl_compositor::WlCompositor, _: wl_compositor::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {} }
impl Dispatch<wl_shm::WlShm, ()> for PinState { fn event(_: &mut Self, _: &wl_shm::WlShm, _: wl_shm::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {} }
impl Dispatch<zwlr_layer_shell_v1::ZwlrLayerShellV1, ()> for PinState { fn event(_: &mut Self, _: &zwlr_layer_shell_v1::ZwlrLayerShellV1, _: zwlr_layer_shell_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {} }
impl Dispatch<wl_output::WlOutput, ()> for PinState { fn event(_: &mut Self, _: &wl_output::WlOutput, _: wl_output::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {} }
impl Dispatch<wl_surface::WlSurface, ()> for PinState { fn event(_: &mut Self, _: &wl_surface::WlSurface, _: wl_surface::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {} }
impl Dispatch<wl_shm_pool::WlShmPool, ()> for PinState { fn event(_: &mut Self, _: &wl_shm_pool::WlShmPool, _: wl_shm_pool::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {} }
impl Dispatch<wl_buffer::WlBuffer, ()> for PinState { fn event(_: &mut Self, _: &wl_buffer::WlBuffer, _: wl_buffer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {} }

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, usize> for PinState {
    fn event(state: &mut Self, layer_surface: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, event: zwlr_layer_surface_v1::Event, data: &usize, _: &Connection, _: &QueueHandle<Self>) {
        if let zwlr_layer_surface_v1::Event::Configure { serial, .. } = event {
            layer_surface.ack_configure(serial);
            if let Some(surf) = state.surfaces.get_mut(*data) {
                surf.configured = true;
            }
        }
    }
}

pub fn pin_buffer(buffer: &CaptureBuffer) -> Result<()> {
    let conn = Connection::connect_to_env().map_err(|e| WlsnipError::Capture(format!("Wayland connect failed: {e}")))?;
    let display = conn.display();

    let mut state = PinState {
        compositor: None,
        shm: None,
        layer_shell: None,
        outputs: Vec::new(),
        seat: None,
        pointer: None,
        keyboard: None,
        surfaces: Vec::new(),
        dismissed: false,
    };

    let mut queue = conn.new_event_queue();
    let qh = queue.handle();

    display.get_registry(&qh, ());
    queue.roundtrip(&mut state).map_err(|e| WlsnipError::Protocol(format!("roundtrip 1: {e}")))?;
    queue.roundtrip(&mut state).map_err(|e| WlsnipError::Protocol(format!("roundtrip 2: {e}")))?;

    let compositor = state.compositor.as_ref().ok_or_else(|| WlsnipError::Capture("No wl_compositor".into()))?;
    let layer_shell = state.layer_shell.as_ref().ok_or_else(|| WlsnipError::Capture("No zwlr_layer_shell_v1".into()))?;
    let shm = state.shm.as_ref().ok_or_else(|| WlsnipError::Capture("No wl_shm".into()))?;

    let format = match buffer.format {
        crate::buffers::PixelFormat::Argb8888 => wl_shm::Format::Argb8888,
        crate::buffers::PixelFormat::Xrgb8888 => wl_shm::Format::Xrgb8888,
        crate::buffers::PixelFormat::Abgr8888 => wl_shm::Format::Abgr8888,
        crate::buffers::PixelFormat::Xbgr8888 => wl_shm::Format::Xbgr8888,
    };

    let pool_size = buffer.data.len();
    let mut shm_pool = ShmPool::new(pool_size)?;
    shm_pool.as_mut_slice().copy_from_slice(&buffer.data);

    let wl_pool = shm.create_pool(shm_pool.fd().as_fd(), pool_size as i32, &qh, ());
    let wl_buffer = wl_pool.create_buffer(
        0,
        buffer.width as i32,
        buffer.height as i32,
        buffer.stride as i32,
        format,
        &qh,
        (),
    );

    // Create a layer surface (Overlay layer)
    let surface = compositor.create_surface(&qh, ());
    let layer_surface = layer_shell.get_layer_surface(
        &surface,
        None, // default output
        zwlr_layer_shell_v1::Layer::Overlay,
        "wlsnip-pin".to_string(),
        &qh,
        0,
    );

    layer_surface.set_size(buffer.width as u32, buffer.height as u32);
    // Anchor top-right so it's not in the middle of the screen
    layer_surface.set_anchor(zwlr_layer_surface_v1::Anchor::Top | zwlr_layer_surface_v1::Anchor::Right);
    layer_surface.set_margin(20, 20, 0, 0);
    // Require keyboard focus so we can intercept ESC
    layer_surface.set_keyboard_interactivity(wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::KeyboardInteractivity::Exclusive);

    surface.commit();

    state.surfaces.push(PinnedSurface {
        surface,
        layer_surface,
        configured: false,
    });

    while !state.surfaces[0].configured {
        queue.blocking_dispatch(&mut state).map_err(|e| WlsnipError::Protocol(format!("dispatch: {e}")))?;
    }

    state.surfaces[0].surface.attach(Some(&wl_buffer), 0, 0);
    state.surfaces[0].surface.damage_buffer(0, 0, i32::MAX, i32::MAX);
    state.surfaces[0].surface.commit();
    let _ = conn.flush();

    // Event loop until dismissed
    while !state.dismissed {
        queue.blocking_dispatch(&mut state).map_err(|e| WlsnipError::Protocol(format!("dispatch: {e}")))?;
    }

    // Cleanup
    for s in state.surfaces.drain(..) {
        s.layer_surface.destroy();
        s.surface.destroy();
    }
    wl_buffer.destroy();
    wl_pool.destroy();
    if let Some(pointer) = state.pointer.take() {
        use wayland_client::Proxy;
        if pointer.version() >= 3 { pointer.release(); }
    }
    if let Some(keyboard) = state.keyboard.take() {
        use wayland_client::Proxy;
        if keyboard.version() >= 3 { keyboard.release(); }
    }
    let _ = queue.roundtrip(&mut state);

    Ok(())
}
