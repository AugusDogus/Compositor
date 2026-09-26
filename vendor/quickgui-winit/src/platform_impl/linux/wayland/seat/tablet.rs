//! Native tablet-v2 frames, routed by their compositor-provided surface.
use crate::platform_impl::wayland::{make_wid, state::WinitState};
use crate::{
    dpi::{LogicalPosition, PhysicalPosition},
    event::{TabletEvent, WindowEvent},
};
use sctk::reexports::client::backend::ObjectId;
use sctk::reexports::client::protocol::wl_surface::WlSurface;
use sctk::reexports::client::{Connection, Dispatch, Proxy, QueueHandle, WEnum};
use sctk::reexports::protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::WpCursorShapeDeviceV1;
use sctk::reexports::protocols::wp::tablet::zv2::client::{
    zwp_tablet_manager_v2::ZwpTabletManagerV2,
    zwp_tablet_pad_group_v2::{self as group, ZwpTabletPadGroupV2},
    zwp_tablet_pad_ring_v2::ZwpTabletPadRingV2,
    zwp_tablet_pad_strip_v2::ZwpTabletPadStripV2,
    zwp_tablet_pad_v2::{self as pad, ZwpTabletPadV2},
    zwp_tablet_seat_v2::{self as seat, ZwpTabletSeatV2},
    zwp_tablet_tool_v2::{self as tool, ZwpTabletToolV2},
    zwp_tablet_v2::{self as tablet, ZwpTabletV2},
};
use std::sync::Mutex;

#[derive(Debug)]
pub struct ToolData(Mutex<Tool>);
#[derive(Debug)]
struct Tool {
    seat: Option<ObjectId>,
    tablet: Option<ObjectId>,
    cursor: Option<WpCursorShapeDeviceV1>,
    surface: Option<WlSurface>,
    position: LogicalPosition<f64>,
    sample: TabletEvent,
}
impl Default for ToolData {
    fn default() -> Self {
        Self(Mutex::new(Tool {
            seat: None,
            tablet: None,
            cursor: None,
            surface: None,
            position: LogicalPosition::new(0., 0.),
            sample: TabletEvent {
                position: PhysicalPosition::new(0., 0.),
                pressure: None,
                tilt: None,
                buttons: [false; 3],
                eraser: false,
                proximity: false,
            },
        }))
    }
}
impl Tool {
    fn axes(&mut self, event: &tool::Event) -> bool {
        match event {
            tool::Event::Type { tool_type } => {
                self.sample.eraser = *tool_type == WEnum::Value(tool::Type::Eraser)
            }
            tool::Event::Motion { x, y } => self.position = LogicalPosition::new(*x, *y),
            tool::Event::Pressure { pressure } => {
                self.sample.pressure = Some((*pressure).min(65535) as f32 / 65535.)
            }
            tool::Event::Tilt { tilt_x, tilt_y } => {
                self.sample.tilt = Some([
                    tilt_x.clamp(-90., 90.) as f32,
                    tilt_y.clamp(-90., 90.) as f32,
                ])
            }
            tool::Event::Down { .. } => self.sample.buttons[0] = true,
            tool::Event::Up => self.sample.buttons[0] = false,
            tool::Event::Button { button, state, .. } => {
                let index = match button {
                    0x14b => 1,
                    0x14c => 2,
                    _ => return true,
                };
                self.sample.buttons[index] = *state == WEnum::Value(tool::ButtonState::Pressed);
            }
            _ => return false,
        }
        true
    }
    fn leave(&mut self, state: &mut WinitState, proxy: &ZwpTabletToolV2) {
        if let Some(surface) = self.surface.take() {
            if let Some(window) = state.windows.get_mut().get(&make_wid(&surface)) {
                window.lock().unwrap().tablet_cursors.remove(&proxy.id());
            }
        }
    }
    fn emit(&self, state: &mut WinitState) {
        let Some(surface) = &self.surface else {
            return;
        };
        let window_id = make_wid(surface);
        let Some(window) = state.windows.get_mut().get(&window_id) else {
            return;
        };
        let scale = window.lock().unwrap().scale_factor();
        let mut frame = self.sample;
        frame.position = self.position.to_physical(scale);
        state
            .events_sink
            .push_window_event(WindowEvent::Tablet(frame), window_id);
    }
}
impl Dispatch<ZwpTabletSeatV2, ObjectId> for WinitState {
    fn event(
        state: &mut Self,
        _: &ZwpTabletSeatV2,
        event: seat::Event,
        seat_id: &ObjectId,
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        // The compositor retains tablets and tools until Removed. Pad express
        // keys remain desktop shortcuts; safely destroy children already queued.
        match event {
            seat::Event::PadAdded { id } => id.destroy(),
            seat::Event::ToolAdded { id } => {
                if let Some(data) = id.data::<ToolData>() {
                    let mut tool = data.0.lock().unwrap();
                    tool.seat = Some(seat_id.clone());
                    tool.cursor = state.tablet_cursor_manager.as_ref().map(|manager| {
                        manager.get_tablet_tool_v2(&id, queue, sctk::globals::GlobalData)
                    });
                }
                state.tablet_tools.insert(id.id(), id);
            }
            _ => {}
        }
    }
    sctk::reexports::client::event_created_child!(WinitState, ZwpTabletSeatV2, [
        seat::EVT_TOOL_ADDED_OPCODE => (ZwpTabletToolV2, ToolData::default()),
        seat::EVT_TABLET_ADDED_OPCODE => (ZwpTabletV2, ()),
        seat::EVT_PAD_ADDED_OPCODE => (ZwpTabletPadV2, ()),
    ]);
}
impl Dispatch<ZwpTabletToolV2, ToolData> for WinitState {
    fn event(
        state: &mut Self,
        proxy: &ZwpTabletToolV2,
        event: tool::Event,
        data: &ToolData,
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        let mut tool = data.0.lock().unwrap();
        if tool.axes(&event) {
            return;
        }
        match event {
            tool::Event::ProximityIn {
                surface,
                tablet,
                serial,
            } => {
                let window_id = make_wid(&surface);
                let cursor = super::tablet_cursor::Cursor::new(
                    state,
                    proxy.clone(),
                    tool.cursor.clone(),
                    serial,
                    queue,
                );
                if let Some(window) = state.windows.get_mut().get(&window_id) {
                    let mut window = window.lock().unwrap();
                    window.tablet_cursors.insert(proxy.id(), cursor);
                    window.update_tablet_cursors();
                }
                tool.tablet = Some(tablet.id());
                tool.surface = Some(surface);
                tool.sample.proximity = true;
                tool.sample.pressure = None;
                tool.sample.tilt = None;
                tool.sample.buttons = [false; 3];
            }
            tool::Event::ProximityOut => {
                tool.sample.proximity = false;
                tool.sample.buttons = [false; 3];
            }
            tool::Event::Frame { .. } => {
                tool.emit(state);
                if !tool.sample.proximity {
                    tool.leave(state, proxy);
                }
            }
            tool::Event::Removed => {
                tool.sample.proximity = false;
                tool.sample.buttons = [false; 3];
                tool.emit(state);
                tool.leave(state, proxy);
                if let Some(cursor) = tool.cursor.take() {
                    cursor.destroy();
                }
                state.tablet_tools.remove(&proxy.id());
                proxy.destroy();
            }
            _ => {}
        }
    }
}
impl Dispatch<ZwpTabletV2, ()> for WinitState {
    fn event(
        state: &mut Self,
        proxy: &ZwpTabletV2,
        event: tablet::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let tablet::Event::Removed = event {
            let tools: Vec<_> = state.tablet_tools.values().cloned().collect();
            for proxy_tool in tools {
                if let Some(data) = proxy_tool.data::<ToolData>() {
                    let mut tool = data.0.lock().unwrap();
                    if tool.tablet.as_ref() == Some(&proxy.id()) {
                        tool.sample.proximity = false;
                        tool.sample.buttons = [false; 3];
                        tool.emit(state);
                        tool.leave(state, &proxy_tool);
                    }
                }
            }
            proxy.destroy();
        }
    }
}
impl Dispatch<ZwpTabletPadV2, ()> for WinitState {
    fn event(
        _: &mut Self,
        _: &ZwpTabletPadV2,
        event: pad::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let pad::Event::Group { pad_group } = event {
            pad_group.destroy();
        }
    }
    sctk::reexports::client::event_created_child!(WinitState, ZwpTabletPadV2, [pad::EVT_GROUP_OPCODE => (ZwpTabletPadGroupV2, ())]);
}
impl Dispatch<ZwpTabletPadGroupV2, ()> for WinitState {
    fn event(
        _: &mut Self,
        _: &ZwpTabletPadGroupV2,
        event: group::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            group::Event::Ring { ring } => ring.destroy(),
            group::Event::Strip { strip } => strip.destroy(),
            _ => {}
        }
    }
    sctk::reexports::client::event_created_child!(WinitState, ZwpTabletPadGroupV2, [
        group::EVT_RING_OPCODE => (ZwpTabletPadRingV2, ()),
        group::EVT_STRIP_OPCODE => (ZwpTabletPadStripV2, ()),
    ]);
}
sctk::reexports::client::delegate_noop!(WinitState: ignore ZwpTabletManagerV2);
sctk::reexports::client::delegate_noop!(WinitState: ignore ZwpTabletPadRingV2);
sctk::reexports::client::delegate_noop!(WinitState: ignore ZwpTabletPadStripV2);

impl WinitState {
    pub(super) fn remove_tablet_seat(&mut self, seat: &ObjectId) {
        let tools: Vec<_> = self.tablet_tools.values().cloned().collect();
        for proxy in tools {
            if let Some(data) = proxy.data::<ToolData>() {
                let mut tool = data.0.lock().unwrap();
                if tool.seat.as_ref() == Some(seat) {
                    tool.sample.proximity = false;
                    tool.sample.buttons = [false; 3];
                    tool.emit(self);
                    tool.leave(self, &proxy);
                    if let Some(cursor) = tool.cursor.take() {
                        cursor.destroy();
                    }
                    self.tablet_tools.remove(&proxy.id());
                    proxy.destroy();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wayland_axes_stay_in_one_frame_and_lift_clears_only_the_tip() {
        let data = ToolData::default();
        let mut tool = data.0.lock().unwrap();
        tool.axes(&tool::Event::Type {
            tool_type: WEnum::Value(tool::Type::Eraser),
        });
        tool.axes(&tool::Event::Down { serial: 1 });
        tool.axes(&tool::Event::Pressure { pressure: 32768 });
        tool.axes(&tool::Event::Tilt {
            tilt_x: -35.,
            tilt_y: 60.,
        });
        tool.axes(&tool::Event::Motion { x: 50.5, y: 25. });
        assert!(tool.sample.buttons[0] && tool.sample.eraser);
        assert!((tool.sample.pressure.unwrap() - 0.5).abs() < 0.0001);
        assert_eq!(tool.sample.tilt, Some([-35., 60.]));
        assert_eq!(tool.position, LogicalPosition::new(50.5, 25.));
        tool.axes(&tool::Event::Up);
        assert_eq!(tool.sample.buttons, [false; 3]);
        assert_eq!(tool.sample.tilt, Some([-35., 60.]));
    }
}
