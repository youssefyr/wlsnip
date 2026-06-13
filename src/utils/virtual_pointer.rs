use std::time::Duration;
use wayland_client::{Connection, Dispatch, QueueHandle, protocol::{wl_registry, wl_seat}};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1, zwlr_virtual_pointer_v1,
};

struct VirtualPointerState {
    manager: Option<zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1>,
    seat: Option<wl_seat::WlSeat>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for VirtualPointerState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { name, interface, version } = event {
            match interface.as_str() {
                "zwlr_virtual_pointer_manager_v1" => {
                    let manager = registry.bind::<zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1, _, _>(name, version.min(2), qh, ());
                    state.manager = Some(manager);
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

impl Dispatch<wl_seat::WlSeat, ()> for VirtualPointerState {
    fn event(_: &mut Self, _: &wl_seat::WlSeat, _: wl_seat::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1, ()> for VirtualPointerState {
    fn event(_: &mut Self, _: &zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1, _: zwlr_virtual_pointer_manager_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1, ()> for VirtualPointerState {
    fn event(_: &mut Self, _: &zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1, _: zwlr_virtual_pointer_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

/// Spawns a background thread that waits `delay_ms` then mimics a human dragging the mouse
/// from (x, y) to (x+w, y+h). Uses relative coordinate ratios across an extent of 1,000,000.
pub fn simulate_drag(
    x_ratio: f64,
    y_ratio: f64,
    w_ratio: f64,
    h_ratio: f64,
    delay_ms: u64
) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(delay_ms));

        let conn = match Connection::connect_to_env() {
            Ok(c) => c,
            Err(_) => return,
        };
        let mut state = VirtualPointerState { manager: None, seat: None };
        let mut queue = conn.new_event_queue();
        let qh = queue.handle();
        
        let _ = conn.display().get_registry(&qh, ());
        let _ = queue.roundtrip(&mut state);
        
        let (manager, seat) = match (state.manager.as_ref(), state.seat.as_ref()) {
            (Some(m), Some(s)) => (m, s),
            _ => return,
        };

        // If seat has no pointer capability, wait? Just create it.
        let pointer = manager.create_virtual_pointer(Some(&seat), &qh, ());
        let _ = queue.roundtrip(&mut state);

        let extent = 1_000_000u32;
        
        let start_x = (x_ratio * extent as f64) as u32;
        let start_y = (y_ratio * extent as f64) as u32;
        let end_x = ((x_ratio + w_ratio) * extent as f64) as u32;
        let end_y = ((y_ratio + h_ratio) * extent as f64) as u32;

        // Move to start
        pointer.motion_absolute(0, start_x, start_y, extent, extent);
        pointer.frame();
        let _ = conn.flush();
        std::thread::sleep(Duration::from_millis(50));

        // Press left mouse button (BTN_LEFT is 272)
        // state 1 = pressed
        pointer.button(0, 272, wayland_client::protocol::wl_pointer::ButtonState::Pressed);
        pointer.frame();
        let _ = conn.flush();
        std::thread::sleep(Duration::from_millis(10));

        // Drag to end in small increments
        let steps = 10;
        for i in 1..=steps {
            let cx = start_x + (end_x.saturating_sub(start_x)) * i / steps;
            let cy = start_y + (end_y.saturating_sub(start_y)) * i / steps;
            pointer.motion_absolute(0, cx, cy, extent, extent);
            pointer.frame();
            let _ = conn.flush();
            std::thread::sleep(Duration::from_millis(10));
        }

        std::thread::sleep(Duration::from_millis(10));

        // Release (state 0 = released)
        pointer.button(0, 272, wayland_client::protocol::wl_pointer::ButtonState::Released);
        pointer.frame();
        let _ = conn.flush();
        
        pointer.destroy();
        let _ = queue.roundtrip(&mut state);
    });
}
