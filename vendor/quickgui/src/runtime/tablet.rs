//! Feed native tablet frames through the normal hit-testing/capture path.
use super::*;

impl Runtime {
    pub(super) fn handle_tablet(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        sample: winit::event::TabletEvent,
    ) {
        if !self.activate_window(window_id) {
            return;
        }
        let Some(window) = self.window.as_mut() else {
            return;
        };
        if !sample.position.x.is_finite() || !sample.position.y.is_finite() {
            self.deactivate_window();
            return;
        }
        let prior = window.tablet_buttons;
        let buttons = if sample.proximity {
            sample.buttons
        } else {
            [false; 3]
        };
        window.tablet = Some(crate::TabletInfo {
            pressure: sample
                .pressure
                .filter(|p| p.is_finite())
                .map(|p| p.clamp(0., 1.)),
            tilt: sample
                .tilt
                .filter(|t| t.iter().all(|v| v.is_finite()))
                .map(|t| t.map(|v| v.clamp(-90., 90.))),
            eraser: sample.eraser,
        });
        window.tablet_buttons = buttons;
        window.dispatching_tablet = true;
        self.deactivate_window();
        let device_id = winit::event::DeviceId::dummy();
        self.handle_window_event(
            event_loop,
            window_id,
            WindowEvent::CursorMoved {
                device_id,
                position: sample.position,
            },
        );
        for (index, button) in [
            winit::event::MouseButton::Left,
            winit::event::MouseButton::Right,
            winit::event::MouseButton::Middle,
        ]
        .into_iter()
        .enumerate()
        {
            if prior[index] != buttons[index] {
                self.handle_window_event(
                    event_loop,
                    window_id,
                    WindowEvent::MouseInput {
                        device_id,
                        button,
                        state: if buttons[index] {
                            ElementState::Pressed
                        } else {
                            ElementState::Released
                        },
                    },
                );
            }
        }
        if !sample.proximity {
            self.handle_window_event(event_loop, window_id, WindowEvent::CursorLeft { device_id });
        }
        if self.activate_window(window_id) {
            if let Some(window) = &mut self.window {
                window.dispatching_tablet = false;
                if !sample.proximity {
                    window.tablet = None;
                }
            }
            self.deactivate_window();
        }
    }
}
