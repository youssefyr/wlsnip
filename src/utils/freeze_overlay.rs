use crate::buffers::shm::ShmPool;
use crate::buffers::CaptureBuffer;
use crate::error::{Result, WlsnipError};
use crate::utils::geometry::Region;

use std::os::fd::AsFd;
use wayland_client::protocol::{wl_buffer, wl_compositor, wl_output, wl_registry, wl_shm, wl_shm_pool, wl_surface};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1, zwlr_layer_surface_v1,
};
use wayland_client::protocol::{wl_seat, wl_pointer, wl_keyboard};
use font8x8::UnicodeFonts;

// ── Wayland State ─────────────────────────────────────────────────────────

struct FreezeState {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    layer_shell: Option<zwlr_layer_shell_v1::ZwlrLayerShellV1>,
    outputs: Vec<wl_output::WlOutput>,
    
    seat: Option<wl_seat::WlSeat>,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    
    // Configured layers
    layers: Vec<LayerData>,

    // Selection state
    pointer_focus: Option<usize>, // index in layers
    selection_start: Option<(f64, f64)>, // x, y on current surface
    selection_current: Option<(f64, f64)>, // x, y on current surface
    is_dragging: bool,
    result: Option<Region>,
    canceled: bool,
}

struct LayerData {
    surface: wl_surface::WlSurface,
    layer_surface: zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
    configured: bool,
    width: u32,
    height: u32,
}

impl Dispatch<wl_registry::WlRegistry, ()> for FreezeState {
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
                    let comp = registry.bind::<wl_compositor::WlCompositor, _, _>(name, version.min(4), qh, ());
                    state.compositor = Some(comp);
                }
                "wl_shm" => {
                    let shm = registry.bind::<wl_shm::WlShm, _, _>(name, version.min(1), qh, ());
                    state.shm = Some(shm);
                }
                "zwlr_layer_shell_v1" => {
                    let shell = registry.bind::<zwlr_layer_shell_v1::ZwlrLayerShellV1, _, _>(name, version.min(1), qh, ());
                    state.layer_shell = Some(shell);
                }
                "wl_output" => {
                    let output = registry.bind::<wl_output::WlOutput, _, _>(name, version.min(1), qh, ());
                    state.outputs.push(output);
                }
                "wl_seat" => {
                    let seat = registry.bind::<wl_seat::WlSeat, _, _>(name, version.min(1), qh, ());
                    state.seat = Some(seat);
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for FreezeState {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
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

impl Dispatch<wl_keyboard::WlKeyboard, ()> for FreezeState {
    fn event(
        state: &mut Self,
        _keyboard: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_keyboard::Event::Key { key, state: key_state, .. } = event {
            if key_state == wayland_client::WEnum::Value(wayland_client::protocol::wl_keyboard::KeyState::Pressed) {
                if key == 1 { // KEY_ESC
                    state.canceled = true;
                }
            }
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for FreezeState {
    fn event(
        state: &mut Self,
        _pointer: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter { surface, surface_x, surface_y, .. } => {
                state.pointer_focus = state.layers.iter().position(|l| l.surface == surface);
                if !state.is_dragging {
                    state.selection_current = Some((surface_x, surface_y));
                }
            }
            wl_pointer::Event::Leave { .. } => {
                if !state.is_dragging {
                    state.pointer_focus = None;
                    state.selection_current = None;
                }
            }
            wl_pointer::Event::Motion { surface_x, surface_y, .. } => {
                if state.pointer_focus.is_some() {
                    state.selection_current = Some((surface_x, surface_y));
                }
            }
            wl_pointer::Event::Button { button, state: btn_state, .. } => {
                // button 272 is left click, 273 is right click
                if btn_state == wayland_client::WEnum::Value(wayland_client::protocol::wl_pointer::ButtonState::Pressed) {
                    if button == 272 {
                        state.is_dragging = true;
                        state.selection_start = state.selection_current;
                    } else if button == 273 {
                        state.canceled = true;
                    }
                } else if btn_state == wayland_client::WEnum::Value(wayland_client::protocol::wl_pointer::ButtonState::Released) {
                    if button == 272 {
                        state.is_dragging = false;
                        if let (Some(start), Some(end), Some(_)) = (state.selection_start, state.selection_current, state.pointer_focus) {
                            let x = start.0.min(end.0) as i32;
                            let y = start.1.min(end.1) as i32;
                            let width = (start.0 - end.0).abs() as u32;
                            let height = (start.1 - end.1).abs() as u32;
                            
                            // For simplicity, we just use the surface-local coordinates. 
                            // A robust multi-monitor selector would add wl_output global offsets.
                            state.result = Some(Region { x, y, width, height });
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_compositor::WlCompositor, ()> for FreezeState {
    fn event(_: &mut Self, _: &wl_compositor::WlCompositor, _: wl_compositor::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<wl_shm::WlShm, ()> for FreezeState {
    fn event(_: &mut Self, _: &wl_shm::WlShm, _: wl_shm::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<zwlr_layer_shell_v1::ZwlrLayerShellV1, ()> for FreezeState {
    fn event(_: &mut Self, _: &zwlr_layer_shell_v1::ZwlrLayerShellV1, _: zwlr_layer_shell_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<wl_output::WlOutput, ()> for FreezeState {
    fn event(_: &mut Self, _: &wl_output::WlOutput, _: wl_output::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<wl_surface::WlSurface, ()> for FreezeState {
    fn event(_: &mut Self, _: &wl_surface::WlSurface, _: wl_surface::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for FreezeState {
    fn event(_: &mut Self, _: &wl_shm_pool::WlShmPool, _: wl_shm_pool::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<wl_buffer::WlBuffer, ()> for FreezeState {
    fn event(_: &mut Self, _: &wl_buffer::WlBuffer, _: wl_buffer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, usize> for FreezeState {
    fn event(
        state: &mut Self,
        layer_surface: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        data: &usize,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let zwlr_layer_surface_v1::Event::Configure { serial, width, height } = event {
            layer_surface.ack_configure(serial);
            
            if let Some(layer_data) = state.layers.get_mut(*data) {
                layer_data.configured = true;
                layer_data.width = width;
                layer_data.height = height;
            }
        }
    }
}

// ── Execution ─────────────────────────────────────────────────────────────

/// Runs the freeze overlay. Displays the `CaptureBuffer` on all outputs, spawns `slurp`, and returns the cropped buffer.
pub fn run_with_freeze(full_buffer: &CaptureBuffer, selection_color: Option<&str>, use_native_selector: bool) -> Result<Option<Region>> {
    let conn = Connection::connect_to_env().map_err(|e| WlsnipError::Capture(format!("Wayland connect failed: {e}")))?;
    let display = conn.display();

    let mut state = FreezeState {
        compositor: None,
        shm: None,
        layer_shell: None,
        outputs: Vec::new(),
        seat: None,
        pointer: None,
        keyboard: None,
        layers: Vec::new(),
        pointer_focus: None,
        selection_start: None,
        selection_current: None,
        is_dragging: false,
        result: None,
        canceled: false,
    };

    let mut queue = conn.new_event_queue();
    let qh = queue.handle();

    display.get_registry(&qh, ());
    queue.roundtrip(&mut state).map_err(|e| WlsnipError::Protocol(format!("roundtrip 1: {e}")))?;
    queue.roundtrip(&mut state).map_err(|e| WlsnipError::Protocol(format!("roundtrip 2: {e}")))?;

    let compositor = state.compositor.as_ref().ok_or_else(|| WlsnipError::Capture("No wl_compositor".into()))?;
    let layer_shell = state.layer_shell.as_ref().ok_or_else(|| WlsnipError::Capture("No zwlr_layer_shell_v1".into()))?;
    let shm = state.shm.as_ref().ok_or_else(|| WlsnipError::Capture("No wl_shm".into()))?;

    // Create a single shared SHM buffer holding our image.
    // For simplicity, we just blit `full_buffer` to a wl_buffer.
    // Ensure format matches. Our `CaptureBuffer` is usually XRGB8888 or ABGR8888.
    let format = match full_buffer.format {
        crate::buffers::PixelFormat::Argb8888 => wl_shm::Format::Argb8888,
        crate::buffers::PixelFormat::Xrgb8888 => wl_shm::Format::Xrgb8888,
        crate::buffers::PixelFormat::Abgr8888 => wl_shm::Format::Abgr8888,
        crate::buffers::PixelFormat::Xbgr8888 => wl_shm::Format::Xbgr8888,
    };

    let pool_size = full_buffer.data.len();
    let mut shm_pool = ShmPool::new(pool_size * 2)?;
    shm_pool.as_mut_slice()[0..pool_size].copy_from_slice(&full_buffer.data);

    let wl_pool = shm.create_pool(shm_pool.fd().as_fd(), (pool_size * 2) as i32, &qh, ());
    let wl_buffer_0 = wl_pool.create_buffer(
        0,
        full_buffer.width as i32,
        full_buffer.height as i32,
        full_buffer.stride as i32,
        format,
        &qh,
        (),
    );
    let wl_buffer_1 = wl_pool.create_buffer(
        pool_size as i32,
        full_buffer.width as i32,
        full_buffer.height as i32,
        full_buffer.stride as i32,
        format,
        &qh,
        (),
    );
    let buffers = [&wl_buffer_0, &wl_buffer_1];
    let mut front_buffer = 0;

    // Create a layer surface for each output
    let num_outputs = state.outputs.len();
    for i in 0..num_outputs {
        let output = &state.outputs[i];
        let surface = compositor.create_surface(&qh, ());
        
        // Use Layer::Top so Slurp (which usually uses Overlay) is on top
        let layer_surface = layer_shell.get_layer_surface(
            &surface,
            Some(output),
            zwlr_layer_shell_v1::Layer::Top,
            "wlsnip-freeze".to_string(),
            &qh,
            i,
        );

        // Span full screen
        layer_surface.set_size(0, 0);
        layer_surface.set_anchor(
            zwlr_layer_surface_v1::Anchor::Top
                | zwlr_layer_surface_v1::Anchor::Bottom
                | zwlr_layer_surface_v1::Anchor::Left
                | zwlr_layer_surface_v1::Anchor::Right,
        );
        layer_surface.set_exclusive_zone(-1);

        surface.commit();

        state.layers.push(LayerData {
            surface,
            layer_surface,
            configured: false,
            width: 0,
            height: 0,
        });
    }

    // Wait for all surfaces to be configured
    while state.layers.iter().any(|l| !l.configured) {
        queue.blocking_dispatch(&mut state).map_err(|e| WlsnipError::Protocol(format!("dispatch: {e}")))?;
    }

    // Attach buffers and commit
    for layer in &state.layers {
        layer.surface.attach(Some(buffers[front_buffer]), 0, 0);
        layer.surface.damage_buffer(0, 0, i32::MAX, i32::MAX);
        layer.surface.commit();
    }
    
    conn.flush().map_err(|e| WlsnipError::Protocol(format!("flush: {e}")))?;

    if !use_native_selector {
        // Spawn slurp in a background thread so we can keep dispatching Wayland events
        // (compositors may require the client to remain responsive or might not show the buffer otherwise).
        let (tx, rx) = std::sync::mpsc::channel();
        let selection_color_owned = selection_color.map(String::from);
        std::thread::spawn(move || {
            let res = crate::utils::geometry::select_region_with_slurp(selection_color_owned.as_deref());
            let _ = tx.send(res);
        });

        // Run the Wayland event loop until slurp finishes
        let slurp_res = loop {
            if let Ok(res) = rx.try_recv() {
                break res;
            }
            let _ = queue.dispatch_pending(&mut state);
            std::thread::sleep(std::time::Duration::from_millis(50));
        };

        // Tear down
        for layer in state.layers.drain(..) {
            layer.layer_surface.destroy();
            layer.surface.destroy();
        }
        wl_buffer_0.destroy();
        wl_buffer_1.destroy();
        wl_pool.destroy();
        if let Some(pointer) = state.pointer.take() {
            use wayland_client::Proxy;
            if pointer.version() >= 3 {
                pointer.release();
            }
        }
        if let Some(keyboard) = state.keyboard.take() {
            use wayland_client::Proxy;
            if keyboard.version() >= 3 {
                keyboard.release();
            }
        }
        let _ = queue.roundtrip(&mut state);

        return slurp_res.map(|r| Some(r)).or_else(|e| {
            if e.to_string().contains("cancelled") {
                Ok(None)
            } else {
                Err(e)
            }
        });
    }

    let mut last_drawn_rect = None;

    // Event loop for native region selection
    let result = loop {
        queue.blocking_dispatch(&mut state).map_err(|e| WlsnipError::Protocol(format!("dispatch: {e}")))?;

        if state.canceled {
            break None;
        }

        if let Some(res) = &state.result {
            break Some(res.clone());
        }

        // Simple visual feedback: update the focused surface if dragging
        if state.is_dragging {
            if let (Some(start), Some(end), Some(focus)) = (state.selection_start, state.selection_current, state.pointer_focus) {
                if let Some(layer) = state.layers.get(focus) {
                    let scale_x = if layer.width > 0 { full_buffer.width as f64 / layer.width as f64 } else { 1.0 };
                    let scale_y = if layer.height > 0 { full_buffer.height as f64 / layer.height as f64 } else { 1.0 };

                    let min_x = (start.0.min(end.0) * scale_x) as usize;
                    let min_y = (start.1.min(end.1) * scale_y) as usize;
                    let max_x = (start.0.max(end.0) * scale_x) as usize;
                    let max_y = (start.1.max(end.1) * scale_y) as usize;

                    let current_rect = (min_x, min_y, max_x, max_y);
                    if last_drawn_rect != Some(current_rect) {
                        last_drawn_rect = Some(current_rect);

                        let width = full_buffer.width as usize;
                        let height = full_buffer.height as usize;
                        let stride_u32 = (full_buffer.stride / 4) as usize;

                        let back_buffer = 1 - front_buffer;
                        let pool_slice = shm_pool.as_mut_slice();
                        
                        let offset_start = back_buffer * pool_size;
                        let offset_end = offset_start + pool_size;
                        let back_slice = &mut pool_slice[offset_start..offset_end];
                        
                        back_slice.copy_from_slice(&full_buffer.data);

                        let (prefix, pool_u32, suffix) = unsafe { back_slice.align_to_mut::<u32>() };
                        if prefix.is_empty() && suffix.is_empty() {
                            let min_x_c = min_x.clamp(0, width);
                            let max_x_c = max_x.clamp(0, width);
                            let min_y_c = min_y.clamp(0, height);
                            let max_y_c = max_y.clamp(0, height);

                            // Dim top and bottom
                            for y in 0..min_y_c {
                                let row = y * stride_u32;
                                for x in 0..width { pool_u32[row + x] = (pool_u32[row + x] >> 1) & 0x7F7F7F7F; }
                            }
                            for y in max_y_c..height {
                                let row = y * stride_u32;
                                for x in 0..width { pool_u32[row + x] = (pool_u32[row + x] >> 1) & 0x7F7F7F7F; }
                            }
                            // Dim left and right
                            for y in min_y_c..max_y_c {
                                let row = y * stride_u32;
                                for x in 0..min_x_c { pool_u32[row + x] = (pool_u32[row + x] >> 1) & 0x7F7F7F7F; }
                                for x in max_x_c..width { pool_u32[row + x] = (pool_u32[row + x] >> 1) & 0x7F7F7F7F; }
                            }

                            // Draw border (e.g. Red)
                            let border_color = 0x00FF0000; // Works well enough as visual indicator
                            for y in min_y_c..max_y_c {
                                let row = y * stride_u32;
                                if min_x_c > 0 { pool_u32[row + min_x_c - 1] = border_color; }
                                if max_x_c < width { pool_u32[row + max_x_c] = border_color; }
                            }
                            for x in min_x_c..max_x_c {
                                if min_y_c > 0 { pool_u32[(min_y_c - 1) * stride_u32 + x] = border_color; }
                                if max_y_c < height { pool_u32[max_y_c * stride_u32 + x] = border_color; }
                            }

                            // Draw dimensions text using font8x8
                            let sel_w = max_x_c.saturating_sub(min_x_c);
                            let sel_h = max_y_c.saturating_sub(min_y_c);
                            if sel_w > 0 && sel_h > 0 {
                                let text = format!("{}x{}", sel_w, sel_h);
                                let scale = 2; // Scale text by 2x
                                let padding = 4;
                                let mut text_x = max_x_c + padding;
                                let mut text_y = max_y_c + padding;
                                
                                // Ensure text doesn't go off screen
                                let text_width = text.len() * 8 * scale;
                                let text_height = 8 * scale;
                                if text_x + text_width > width {
                                    text_x = min_x_c.saturating_sub(text_width + padding).max(0);
                                }
                                if text_y + text_height > height {
                                    text_y = min_y_c.saturating_sub(text_height + padding).max(0);
                                }

                                for ch in text.chars() {
                                    if let Some(glyph) = font8x8::BASIC_FONTS.get(ch) {
                                        for r in 0..8 {
                                            for c in 0..8 {
                                                if (glyph[r] & (1 << c)) != 0 {
                                                    for sr in 0..scale {
                                                        for sc in 0..scale {
                                                            let py = text_y + (r * scale) + sr;
                                                            let px = text_x + (c * scale) + sc;
                                                            if py < height && px < width {
                                                                pool_u32[py * stride_u32 + px] = 0x00FFFFFF; // White text
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    text_x += 8 * scale;
                                }
                            }
                        }

                        layer.surface.attach(Some(buffers[back_buffer]), 0, 0);
                        layer.surface.damage_buffer(0, 0, i32::MAX, i32::MAX);
                        layer.surface.commit();
                        let _ = conn.flush();
                        
                        front_buffer = back_buffer;
                    }
                }
            }
        }
    };

    // Tear down
    for layer in state.layers.drain(..) {
        layer.layer_surface.destroy();
        layer.surface.destroy();
    }
    wl_buffer_0.destroy();
    wl_buffer_1.destroy();
    wl_pool.destroy();
    if let Some(pointer) = state.pointer.take() {
        use wayland_client::Proxy;
        if pointer.version() >= 3 {
            pointer.release();
        }
    }
    if let Some(keyboard) = state.keyboard.take() {
        use wayland_client::Proxy;
        if keyboard.version() >= 3 {
            keyboard.release();
        }
    }
    
    // We do one final roundtrip to let the compositor clean up
    let _ = queue.roundtrip(&mut state);

    Ok(result)
}
