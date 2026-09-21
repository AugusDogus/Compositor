use super::*;

impl Runtime {
    pub(super) fn handle_about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.flush_display_events(event_loop);
        let now = Instant::now();
        self.foreground_tasks.wake_due_timers(now);
        let window_ids = self.windows.keys().copied().collect::<Vec<_>>();
        let mut deadline = self.foreground_tasks.next_timer_deadline();
        for window_id in window_ids {
            if !self.activate_window(window_id) {
                continue;
            }
            if !self.flush_native_file_hover(event_loop) {
                self.deactivate_window();
                self.process_window_commands(event_loop);
                return;
            }
            if self
                .pending_input
                .as_ref()
                .is_some_and(|pending| pending.deadline <= now)
            {
                self.flush_pending_input(event_loop);
            }

            let (
                image_deadline,
                animation_deadline,
                scrollbar_deadline,
                tooltip_deadline,
                spell_check_deadline,
                view_deadline,
                accessibility_deadline,
            ) = self
                .window
                .as_mut()
                .map(|state| {
                    let mut redraw = false;
                    if state.view_deadline.is_some_and(|deadline| deadline <= now) {
                        state.view_deadline = None;
                        state.view_dirty = true;
                        redraw = true;
                    }
                    if state.image_assets.announce_due_loading(now) {
                        state.view_dirty = true;
                        redraw = true;
                    }
                    if state.ui.advance_animations(now) {
                        redraw = true;
                    }
                    if state.ui.declarative_animation_due(now) {
                        state.view_dirty = true;
                        redraw = true;
                    }
                    if state.ui.advance_scrollbars(now) {
                        redraw = true;
                    }
                    if state.ui.advance_caret_blink(now) {
                        redraw = true;
                    }
                    if state.ui.advance_tooltips(now) {
                        redraw = true;
                    }
                    let spell_check = state.ui.advance_spell_check(now);
                    if spell_check.repaint {
                        redraw = true;
                    }
                    if state.accessibility_updates.advance(now) {
                        redraw = true;
                    }
                    if redraw && state.scheduler.invalidate() {
                        state.window.request_redraw();
                    }
                    (
                        state.image_assets.next_loading_deadline(),
                        state.ui.next_animation_deadline(),
                        // The caret toggle shares the scrollbar's one-shot deadline slot.
                        [
                            state.ui.next_scrollbar_deadline(),
                            state.ui.next_caret_blink_deadline(now),
                        ]
                        .into_iter()
                        .flatten()
                        .min(),
                        state.ui.next_tooltip_deadline(),
                        spell_check.next_deadline,
                        state.view_deadline,
                        state.accessibility_updates.deadline(),
                    )
                })
                .unwrap_or((None, None, None, None, None, None, None));
            let pending_deadline = self.pending_input.as_ref().map(|pending| pending.deadline);
            let window_deadline = [
                pending_deadline,
                image_deadline,
                animation_deadline,
                scrollbar_deadline,
                tooltip_deadline,
                spell_check_deadline,
                view_deadline,
                accessibility_deadline,
            ]
            .into_iter()
            .flatten()
            .min();
            if let Some(window_deadline) = window_deadline {
                deadline = Some(
                    deadline.map_or(window_deadline, |value: Instant| value.min(window_deadline)),
                );
            }
            self.deactivate_window();
            self.process_window_commands(event_loop);
            if self.fatal_error.is_some() {
                return;
            }
        }
        if let Some(deadline) = deadline {
            event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }
}
