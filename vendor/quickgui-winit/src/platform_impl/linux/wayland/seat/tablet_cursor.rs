//! Tool cursors follow the same visibility and shape as the window's mouse cursor.
use crate::{platform_impl::wayland::state::WinitState, window::CursorIcon};
use sctk::reexports::client::{
    protocol::{wl_shm::Format, wl_surface::WlSurface},
    Proxy,
};
use sctk::reexports::protocols::wp::{
    cursor_shape::v1::client::wp_cursor_shape_device_v1::{Shape, WpCursorShapeDeviceV1},
    tablet::zv2::client::zwp_tablet_tool_v2::ZwpTabletToolV2,
};
use sctk::shm::slot::Buffer;

pub struct Cursor {
    pub tool: ZwpTabletToolV2,
    serial: u32,
    shape: Option<WpCursorShapeDeviceV1>,
    fallback: Option<(WlSurface, Buffer)>,
}
impl Cursor {
    pub fn new(
        state: &WinitState,
        tool: ZwpTabletToolV2,
        shape: Option<WpCursorShapeDeviceV1>,
        serial: u32,
        queue: &sctk::reexports::client::QueueHandle<WinitState>,
    ) -> Self {
        let fallback = if shape.is_none() {
            let mut pool = state.custom_cursor_pool.lock().unwrap();
            match pool.create_buffer(16, 24, 64, Format::Argb8888) {
                Ok((buffer, canvas)) => {
                    // A visible arrow remains available on compositors without
                    // cursor-shape-v1. Brush overlays still hide this cursor.
                    for y in 0..24usize {
                        for x in 0..16usize {
                            let inside = x <= y / 2 && y < 20 && (x < 4 || x + 5 > y);
                            let edge = x == 0 || x == y / 2 || y == 19;
                            let color: u32 = if !inside {
                                0
                            } else if edge {
                                0xff000000
                            } else {
                                0xffffffff
                            };
                            canvas[(y * 16 + x) * 4..(y * 16 + x + 1) * 4]
                                .copy_from_slice(&color.to_ne_bytes());
                        }
                    }
                    let surface = state.compositor_state.create_surface(queue);
                    surface.attach(Some(buffer.wl_buffer()), 0, 0);
                    surface.damage_buffer(0, 0, 16, 24);
                    surface.commit();
                    Some((surface, buffer))
                }
                Err(error) => {
                    tracing::warn!(%error, "Could not allocate tablet cursor");
                    None
                }
            }
        } else {
            None
        };
        Self {
            tool,
            serial,
            shape,
            fallback,
        }
    }
    pub fn update(&self, visible: bool, icon: CursorIcon) {
        if !visible {
            self.tool.set_cursor(self.serial, None, 0, 0);
            return;
        }
        if let Some(shape) = &self.shape {
            let icon = match icon {
                CursorIcon::Crosshair => Shape::Crosshair,
                CursorIcon::Text => Shape::Text,
                CursorIcon::VerticalText => Shape::VerticalText,
                CursorIcon::Pointer => Shape::Pointer,
                CursorIcon::Grab => Shape::Grab,
                CursorIcon::Grabbing => Shape::Grabbing,
                CursorIcon::EwResize => Shape::EwResize,
                CursorIcon::NsResize => Shape::NsResize,
                CursorIcon::NwseResize => Shape::NwseResize,
                CursorIcon::NeswResize => Shape::NeswResize,
                CursorIcon::ColResize => Shape::ColResize,
                CursorIcon::RowResize => Shape::RowResize,
                CursorIcon::Move => Shape::Move,
                CursorIcon::NotAllowed => Shape::NotAllowed,
                CursorIcon::Wait => Shape::Wait,
                CursorIcon::Progress => Shape::Progress,
                _ => Shape::Default,
            };
            shape.set_shape(self.serial, icon);
        } else if let Some((surface, _buffer)) = &self.fallback {
            self.tool.set_cursor(self.serial, Some(surface), 0, 0);
        }
    }
}
impl Drop for Cursor {
    fn drop(&mut self) {
        if let Some((surface, _)) = &self.fallback {
            if surface.is_alive() {
                surface.destroy();
            }
        }
    }
}
