use super::*;

impl TestAppContext {
    /// Deliver a deterministic native minimize or restore.
    ///
    /// Nothing is dispatched when the window is already in the requested state, which mirrors the
    /// runtime's equality suppression.
    pub fn simulate_minimize(
        &mut self,
        window: WindowHandle,
        minimized: bool,
    ) -> Result<(), TestAppError> {
        let changed = {
            let state = self.window_mut(window)?;
            if state.state.minimized == minimized {
                false
            } else {
                state.state.minimized = minimized;
                if state.listeners.observes_window_state {
                    state.dirty = true;
                }
                true
            }
        };
        if changed {
            self.queue_dispatch(TestDispatch::Event(window, Event::Minimized(minimized)))?;
        }
        self.run_until_idle()
    }

    /// Deliver a deterministic native maximize or unmaximize, including its restore geometry.
    pub fn simulate_maximize(
        &mut self,
        window: WindowHandle,
        maximized: bool,
    ) -> Result<(), TestAppError> {
        let changed = {
            let state = self.window_mut(window)?;
            if state.state.maximized == maximized {
                false
            } else {
                let bounds = state.state.bounds.bounds();
                set_test_window_bounds(
                    state,
                    if maximized {
                        WindowBounds::Maximized(bounds)
                    } else {
                        WindowBounds::Windowed(bounds)
                    },
                );
                true
            }
        };
        if changed {
            self.queue_dispatch(TestDispatch::Event(window, Event::Maximized(maximized)))?;
        }
        self.run_until_idle()
    }

    /// Deliver a deterministic native fullscreen transition.
    pub fn simulate_fullscreen_change(
        &mut self,
        window: WindowHandle,
        fullscreen: bool,
    ) -> Result<(), TestAppError> {
        let changed = {
            let state = self.window_mut(window)?;
            if state.state.fullscreen == fullscreen {
                false
            } else {
                let bounds = state.state.bounds.bounds();
                set_test_window_bounds(
                    state,
                    if fullscreen {
                        WindowBounds::Fullscreen(bounds)
                    } else {
                        WindowBounds::Windowed(bounds)
                    },
                );
                true
            }
        };
        if changed {
            self.queue_dispatch(TestDispatch::Event(
                window,
                Event::FullscreenChanged(fullscreen),
            ))?;
        }
        self.run_until_idle()
    }

    /// Deliver a deterministic compositor occlusion change.
    pub fn simulate_occlusion_change(
        &mut self,
        window: WindowHandle,
        occluded: bool,
    ) -> Result<(), TestAppError> {
        let changed = {
            let state = self.window_mut(window)?;
            if state.state.occluded == occluded {
                false
            } else {
                state.state.occluded = occluded;
                if state.listeners.observes_window_state {
                    state.dirty = true;
                }
                true
            }
        };
        if changed {
            self.queue_dispatch(TestDispatch::Event(
                window,
                Event::OcclusionChanged(occluded),
            ))?;
        }
        self.run_until_idle()
    }

    /// Deliver the one-time ready-to-show event for a window.
    ///
    /// Repeated calls are ignored exactly as the runtime ignores every frame after the first.
    pub fn simulate_first_presented(&mut self, window: WindowHandle) -> Result<(), TestAppError> {
        let first = {
            let state = self.window_mut(window)?;
            if state.first_presented {
                false
            } else {
                state.first_presented = true;
                true
            }
        };
        if first {
            self.queue_dispatch(TestDispatch::Event(window, Event::FirstPresented))?;
        }
        self.run_until_idle()
    }

    /// Deliver a window-manager resize through the constrain hook.
    ///
    /// The view sees [`Event::WillResize`], may narrow the size with
    /// [`EventContext::constrain_resize`](crate::EventContext::constrain_resize), and then sees
    /// [`Event::Resized`] with the size that was actually applied. Any retained aspect ratio is
    /// applied after the view's own narrowing, matching the runtime order.
    pub fn simulate_window_resize(
        &mut self,
        window: WindowHandle,
        proposed: Size,
    ) -> Result<Size, TestAppError> {
        self.window(window)?;
        let mut cx = self.event_context(Some(window));
        self.window_mut(window)?.view.event(
            &Event::WillResize {
                proposed_size: proposed,
            },
            &mut cx,
        );
        let constrained = cx.constrained_size;
        self.apply_context(Some(window), cx)?;
        let aspect_ratio = self.window(window)?.config.aspect_ratio;
        let mut applied = constrained.unwrap_or(proposed);
        if let Some(ratio) = aspect_ratio {
            applied = clamp_size_to_aspect_ratio(applied, ratio);
        }
        let scale_factor = {
            let state = self.window_mut(window)?;
            let bounds = state.state.bounds.bounds();
            set_test_window_bounds(
                state,
                WindowBounds::Windowed(Rect::new(
                    bounds.x,
                    bounds.y,
                    applied.width,
                    applied.height,
                )),
            );
            state.state.scale_factor
        };
        self.queue_dispatch(TestDispatch::Event(
            window,
            Event::Resized {
                logical_size: applied,
                scale_factor,
            },
        ))?;
        self.run_until_idle()?;
        Ok(applied)
    }

    /// Deliver a window-manager move through the constrain hook.
    ///
    /// The view sees [`Event::WillMove`], may replace the position with
    /// [`EventContext::constrain_move`](crate::EventContext::constrain_move), and then sees
    /// [`Event::Moved`] with the position that was actually applied.
    pub fn simulate_window_move(
        &mut self,
        window: WindowHandle,
        proposed: Point,
    ) -> Result<Point, TestAppError> {
        self.window(window)?;
        let mut cx = self.event_context(Some(window));
        self.window_mut(window)?.view.event(
            &Event::WillMove {
                proposed_position: proposed,
            },
            &mut cx,
        );
        let constrained = cx.constrained_position;
        self.apply_context(Some(window), cx)?;
        let applied = constrained.unwrap_or(proposed);
        let scale_factor = {
            let state = self.window_mut(window)?;
            let bounds = state.state.bounds.bounds();
            set_test_window_bounds(
                state,
                WindowBounds::Windowed(Rect::new(
                    applied.x,
                    applied.y,
                    bounds.width,
                    bounds.height,
                )),
            );
            state.state.scale_factor
        };
        self.queue_dispatch(TestDispatch::Event(
            window,
            Event::Moved {
                logical_position: applied,
                scale_factor,
            },
        ))?;
        self.run_until_idle()?;
        Ok(applied)
    }

    /// Capture persistable geometry for one deterministic window.
    pub fn window_restore_state(
        &self,
        window: WindowHandle,
    ) -> Result<WindowRestoreState, TestAppError> {
        Ok(self.window(window)?.state.restore_state(&self.displays))
    }
}
