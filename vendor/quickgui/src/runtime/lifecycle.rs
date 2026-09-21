use super::*;

impl Runtime {
    pub(super) fn invoke_open_urls(&mut self, event_loop: &ActiveEventLoop, urls: OpenUrls) {
        let Some(mut callback) = self.application_callbacks.open_urls.take() else {
            return;
        };
        let mut context = self.event_context();
        callback(urls, &mut context);
        self.application_callbacks.open_urls = Some(callback);
        self.apply_application_context(event_loop, context);
    }

    pub(super) fn invoke_finish_launching(&mut self, event_loop: &ActiveEventLoop) {
        let Some(callback) = self.application_callbacks.finish_launching.take() else {
            return;
        };
        let mut context = self.event_context();
        callback(&mut context);
        self.apply_application_context(event_loop, context);
    }

    #[cfg(target_os = "macos")]
    pub(super) fn invoke_reopen(
        &mut self,
        event_loop: &ActiveEventLoop,
        has_visible_windows: bool,
    ) {
        let Some(mut callback) = self.application_callbacks.reopen.take() else {
            return;
        };
        let mut context = self.event_context();
        callback(has_visible_windows, &mut context);
        self.application_callbacks.reopen = Some(callback);
        self.apply_application_context(event_loop, context);
    }

    #[cfg(target_os = "macos")]
    pub(super) fn invoke_system_wake(&mut self, event_loop: &ActiveEventLoop) {
        let Some(mut callback) = self.application_callbacks.system_wake.take() else {
            return;
        };
        let mut context = self.event_context();
        callback(&mut context);
        self.application_callbacks.system_wake = Some(callback);
        self.apply_application_context(event_loop, context);
    }

    /// Run the application-wide "did become active" hook.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(super) fn invoke_did_become_active(&mut self, event_loop: &ActiveEventLoop) {
        let Some(mut callback) = self.application_callbacks.did_become_active.take() else {
            return;
        };
        let mut context = self.event_context();
        callback(&mut context);
        self.application_callbacks.did_become_active = Some(callback);
        self.apply_application_context(event_loop, context);
    }

    /// Run the application-wide "did resign active" hook.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(super) fn invoke_did_resign_active(&mut self, event_loop: &ActiveEventLoop) {
        let Some(mut callback) = self.application_callbacks.did_resign_active.take() else {
            return;
        };
        let mut context = self.event_context();
        callback(&mut context);
        self.application_callbacks.did_resign_active = Some(callback);
        self.apply_application_context(event_loop, context);
    }

    #[cfg(target_os = "macos")]
    pub(super) fn invoke_keyboard_layout_change(&mut self, event_loop: &ActiveEventLoop) {
        let Some(mut callback) = self.application_callbacks.keyboard_layout.take() else {
            return;
        };
        let layout = self.keyboard.layout().clone();
        let mut context = self.event_context();
        callback(&layout, &mut context);
        self.application_callbacks.keyboard_layout = Some(callback);
        self.apply_application_context(event_loop, context);
    }

    pub(super) fn invoke_system_notification_response(
        &mut self,
        event_loop: &ActiveEventLoop,
        response: SystemNotificationResponse,
    ) {
        let Some(mut callback) = self
            .application_callbacks
            .system_notification_response
            .take()
        else {
            return;
        };
        let mut context = self.event_context();
        callback(response, &mut context);
        self.application_callbacks.system_notification_response = Some(callback);
        self.apply_application_context(event_loop, context);
    }

    pub(super) fn apply_application_context(
        &mut self,
        event_loop: &ActiveEventLoop,
        context: EventContext,
    ) {
        debug_assert!(self.current_window.is_none());
        debug_assert!(self.window.is_none());
        if self.apply_event_context(event_loop, context, false, false) || self.exit_requested {
            self.process_window_commands(event_loop);
        }
    }

    #[cfg(any(
        target_os = "macos",
        target_os = "windows",
        target_os = "linux",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    pub(super) fn windows_share_parent_chain(
        &self,
        first: WindowHandle,
        second: WindowHandle,
    ) -> bool {
        self.window_is_ancestor(first, second) || self.window_is_ancestor(second, first)
    }

    #[cfg(any(
        target_os = "macos",
        target_os = "windows",
        target_os = "linux",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    pub(super) fn window_is_ancestor(
        &self,
        ancestor: WindowHandle,
        mut window: WindowHandle,
    ) -> bool {
        for _ in 0..=self.windows.len() {
            if ancestor == window {
                return true;
            }
            let Some(window_id) = self.window_handles.get(&window) else {
                return false;
            };
            let Some(parent) = self
                .windows
                .get(window_id)
                .and_then(|entry| entry.state.parent)
            else {
                return false;
            };
            window = parent;
        }
        false
    }

    pub(super) fn close_requested_window_trees(
        &mut self,
        event_loop: &ActiveEventLoop,
    ) -> Vec<ClosedWindow> {
        #[cfg(not(target_os = "macos"))]
        let _ = event_loop;
        let mut stack = std::mem::take(&mut self.close_requests)
            .into_iter()
            .map(|handle| (handle, false))
            .collect::<Vec<_>>();
        let mut discovered = HashSet::new();
        let mut order = Vec::new();
        while let Some((handle, expanded)) = stack.pop() {
            if expanded {
                order.push(handle);
                continue;
            }
            if !discovered.insert(handle) {
                continue;
            }
            stack.push((handle, true));
            let mut children = self
                .windows
                .values()
                .filter_map(|entry| (entry.state.parent == Some(handle)).then_some(entry.handle))
                .collect::<Vec<_>>();
            children.sort_unstable();
            for child in children.into_iter().rev() {
                stack.push((child, false));
            }
        }

        let mut closed = Vec::with_capacity(order.len());
        for handle in order {
            let Some(window_id) = self.window_handles.remove(&handle) else {
                continue;
            };
            let (parent, restore_focus) = self
                .windows
                .get(&window_id)
                .map(|entry| (entry.state.parent, entry.state.restore_focus_on_close))
                .unwrap_or((None, None));
            self.foreground_tasks.cancel_window(handle);
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            {
                let closing = self
                    .pending_native_popup_menus
                    .extract_if(|_, popup| popup.window == handle)
                    .filter_map(|(_, popup)| popup.responder)
                    .collect::<Vec<_>>();
                for responder in closing {
                    responder.complete(Err(crate::PlatformError::Unavailable));
                }
                self.external_popup_menus.retain(|request| {
                    if request.window == handle {
                        request
                            .responder
                            .complete(Err(crate::PlatformError::Unavailable));
                        false
                    } else {
                        true
                    }
                });
                self.external_window_menus
                    .retain(|request| request.window != handle);
            }
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            self.external_window_menus
                .retain(|request| request.window != handle);
            #[cfg(target_os = "macos")]
            if let Some(dialog) = self.active_platform_dialogs.remove(&Some(handle)) {
                dialog.native.cancel();
            }
            #[cfg(any(
                target_os = "windows",
                target_os = "linux",
                target_os = "freebsd",
                target_os = "dragonfly",
                target_os = "openbsd",
                target_os = "netbsd"
            ))]
            self.active_platform_dialogs.remove(&Some(handle));
            #[cfg(target_os = "macos")]
            self.popover_monitor.unwatch(handle);
            if let Some(entry) = self.windows.remove(&window_id) {
                #[cfg(target_os = "macos")]
                let native_tabbing = entry.config.tabbing_identifier.is_some();
                #[cfg(target_os = "macos")]
                if entry.state.relation_presented
                    && let Err(error) =
                        dismiss_window_relation(&entry.state.window, entry.config.kind)
                {
                    tracing::warn!(%error, "could not dismiss closing native window relation");
                }
                #[cfg(target_os = "macos")]
                if let Err(error) = set_window_visibility(&entry.state.window, false, false) {
                    tracing::warn!(%error, "could not hide closing native window");
                }
                #[cfg(not(target_os = "macos"))]
                entry.state.window.set_visible(false);
                #[cfg(target_os = "macos")]
                if native_tabbing {
                    self.unregister_native_tabbing(event_loop);
                }
            }
            self.focus_history
                .retain(|candidate| *candidate != window_id);
            if self.active_window == Some(window_id) {
                self.active_window = self.focus_history.last().copied();
            }
            closed.push(ClosedWindow {
                handle,
                parent,
                restore_focus,
            });
        }
        #[cfg(target_os = "macos")]
        if !closed.is_empty() {
            self.refresh_native_tab_states();
        }
        closed
    }

    pub(super) fn invoke_window_closed_callbacks(
        &mut self,
        event_loop: &ActiveEventLoop,
        closed: Vec<ClosedWindow>,
    ) {
        #[cfg(target_os = "macos")]
        let menus_changed = !closed.is_empty() && !self.exit_requested;
        for closed in closed {
            if let Some(parent) = closed.parent
                && let Some(window_id) = self.window_handles.get(&parent).copied()
                && self.activate_window(window_id)
            {
                let callbacks = self
                    .window
                    .as_ref()
                    .map(|window| {
                        let mut callbacks =
                            Vec::with_capacity(window.listeners.any_child_window_closed.len() + 1);
                        if let Some(callback) = window
                            .listeners
                            .child_window_closed
                            .get(&closed.handle)
                            .cloned()
                        {
                            callbacks.push(callback);
                        }
                        callbacks.extend(window.listeners.any_child_window_closed.iter().cloned());
                        callbacks
                    })
                    .unwrap_or_default();
                if !callbacks.is_empty() || closed.restore_focus.is_some() {
                    let mut context = self.event_context();
                    context.focus = closed.restore_focus.map(Some);
                    if let Some(window) = &mut self.window {
                        for callback in callbacks {
                            callback(window.view.as_any_mut(), closed.handle, &mut context);
                        }
                    }
                    let _ = self.apply_event_context(event_loop, context, false, true);
                }
                self.deactivate_window();
                if self.fatal_error.is_some() {
                    return;
                }
            }

            if let Some(mut callback) = self.application_callbacks.window_closed.take() {
                let mut context = self.event_context();
                callback(closed.handle, &mut context);
                self.application_callbacks.window_closed = Some(callback);
                let _ = self.apply_event_context(event_loop, context, false, false);
                if self.fatal_error.is_some() {
                    return;
                }
            }
        }
        #[cfg(target_os = "macos")]
        if menus_changed {
            self.install_active_mac_menu(event_loop);
        }
    }

    pub(super) fn invoke_quit_callback(
        &mut self,
        event_loop: &ActiveEventLoop,
        request: QuitRequest,
        before: bool,
    ) -> bool {
        let callback = if before {
            self.application_callbacks.before_quit.take()
        } else {
            self.application_callbacks.will_quit.take()
        };
        let Some(mut callback) = callback else {
            return true;
        };
        let mut context = self.event_context();
        self.quit_phase_active = true;
        callback(request, &mut context);
        let prevented = context.prevent_quit;
        if before {
            self.application_callbacks.before_quit = Some(callback);
        } else {
            self.application_callbacks.will_quit = Some(callback);
        }
        let _ = self.apply_event_context(event_loop, context, false, false);
        self.quit_phase_active = false;
        !prevented && self.fatal_error.is_none()
    }

    pub(super) fn process_pending_quit(&mut self, event_loop: &ActiveEventLoop) {
        let Some(reason) = self.pending_quit.take() else {
            return;
        };
        if self.exit_requested {
            return;
        }
        let request = QuitRequest { reason };
        let accepted = self.invoke_quit_callback(event_loop, request, true)
            && self.invoke_quit_callback(event_loop, request, false);
        if !accepted {
            if reason == QuitReason::Relaunch {
                self.relaunch_request = None;
            }
            if reason == QuitReason::LastWindowClosed {
                self.last_window_quit_prevented = true;
            }
            #[cfg(target_os = "macos")]
            if reason == QuitReason::OperatingSystem && self.native_termination_pending {
                if let Some(host) = self.mac_application_host.as_ref() {
                    host.reply_to_application_should_terminate(false);
                }
                self.native_termination_pending = false;
            }
            return;
        }
        self.exit_requested = true;
        self.pending_windows.clear();
        self.targeted_actions.clear();
        self.close_requests
            .extend(self.window_handles.keys().copied());
    }

    pub(super) fn process_window_commands(&mut self, event_loop: &ActiveEventLoop) {
        debug_assert!(self.current_window.is_none());
        debug_assert!(self.window.is_none());

        for _ in 0..MAX_WINDOW_LIFECYCLE_TURNS {
            if !self.process_deferred_effects(event_loop) {
                return;
            }

            self.process_pending_quit(event_loop);
            if self.fatal_error.is_some() {
                return;
            }

            if self.exit_requested {
                self.pending_windows.clear();
                self.close_requests
                    .extend(self.window_handles.keys().copied());
            } else {
                // Window targeting must use the work area that exists at the placement boundary.
                // On macOS, adding a command-line application's Dock presence can resize a
                // left/right Dock after `resumed` without a screen-parameters notification. One
                // bounded refresh per non-popover creation batch keeps default centering current;
                // system-popover churn, the idle path, and ordinary frames perform no monitor
                // query.
                if self
                    .pending_windows
                    .iter()
                    .any(|request| request.options.kind != WindowKind::SystemPopover)
                {
                    self.refresh_displays(event_loop);
                }
                while let Some(mut request) = self.pending_windows.pop_front() {
                    if let Err(error) = self.resolve_pending_system_popover_anchor(&mut request) {
                        self.fail(event_loop, error);
                        return;
                    }
                    self.create_window(event_loop, request);
                    if self.fatal_error.is_some() {
                        return;
                    }
                    if !self.process_deferred_effects(event_loop) {
                        return;
                    }
                    if self.exit_requested {
                        self.pending_windows.clear();
                        self.close_requests
                            .extend(self.window_handles.keys().copied());
                        break;
                    }
                }
            }

            if !self.process_targeted_actions(event_loop) {
                return;
            }

            self.process_queued_window_commands(event_loop);
            self.process_pending_window_events(event_loop);

            self.process_global_shortcut_commands();
            self.process_tray_commands();

            if let Some(menus) = self.external_menus.take()
                && !self.replace_menus(event_loop, menus)
            {
                return;
            }

            while let Some(request) = self.external_window_menus.pop_front() {
                if !self.replace_external_window_menus(event_loop, request) {
                    return;
                }
            }

            #[cfg(any(target_os = "macos", target_os = "windows"))]
            while let Some(request) = self.external_popup_menus.pop_front() {
                if !self.show_external_native_popup_menu(event_loop, request) {
                    return;
                }
            }

            for handle in std::mem::take(&mut self.invalidate_requests) {
                let Some(window_id) = self.window_handles.get(&handle).copied() else {
                    continue;
                };
                let Some(entry) = self.windows.get_mut(&window_id) else {
                    continue;
                };
                entry.state.view_dirty = true;
                if entry.state.scheduler.invalidate() {
                    entry.state.window.request_redraw();
                }
            }

            for handle in std::mem::take(&mut self.focus_requests) {
                let Some(window_id) = self.window_handles.get(&handle).copied() else {
                    continue;
                };
                if let Some(entry) = self.windows.get(&window_id) {
                    if !entry.config.focusable {
                        continue;
                    }
                    #[cfg(target_os = "linux")]
                    if wayland_activation::request(
                        &mut self.wayland_activation,
                        event_loop,
                        entry.state.window.clone(),
                        self.windows.values().find_map(|source| {
                            source
                                .state
                                .window
                                .has_focus()
                                .then(|| source.state.window.clone())
                        }),
                    ) {
                        // Wayland focus changes only when the compositor accepts the request.
                        continue;
                    }
                    #[cfg(target_os = "macos")]
                    if matches!(
                        entry.config.kind,
                        WindowKind::Popover | WindowKind::SystemPopover
                    ) {
                        if let Err(error) = set_window_visibility(&entry.state.window, true, true) {
                            tracing::warn!(%error, "could not focus native popover without activation");
                        }
                    } else {
                        entry.state.window.focus_window();
                    }
                    #[cfg(not(target_os = "macos"))]
                    entry.state.window.focus_window();
                    self.note_window_focused(window_id);
                }
            }

            let closed = self.close_requested_window_trees(event_loop);
            self.invoke_window_closed_callbacks(event_loop, closed);
            if self.fatal_error.is_some() {
                return;
            }

            self.process_platform_requests();

            #[cfg(target_os = "macos")]
            self.sync_active_native_menu_state();

            if self.exit_requested {
                self.pending_windows.clear();
                if self.windows.is_empty() {
                    #[cfg(target_os = "macos")]
                    if self.native_termination_pending {
                        if let Some(host) = self.mac_application_host.as_ref() {
                            host.reply_to_application_should_terminate(true);
                        }
                        self.native_termination_pending = false;
                        return;
                    }
                    event_loop.exit();
                    return;
                }
                continue;
            }

            let lifecycle_pending = !self.pending_windows.is_empty()
                || !self.targeted_actions.is_empty()
                || !self.close_requests.is_empty()
                || !self.window_commands.is_empty()
                || !self.focus_requests.is_empty()
                || !self.invalidate_requests.is_empty();
            if lifecycle_pending {
                continue;
            }

            if self.opened_window
                && self.windows.is_empty()
                && self.quit_mode.quits_when_empty()
                && !self.last_window_quit_prevented
            {
                self.pending_quit = Some(QuitReason::LastWindowClosed);
                continue;
            }
            return;
        }

        self.fail(
            event_loop,
            AppError::View(format!(
                "window lifecycle exceeded {MAX_WINDOW_LIFECYCLE_TURNS} effect turns"
            )),
        );
    }

    pub(super) fn resolve_pending_system_popover_anchor(
        &self,
        request: &mut WindowRequest,
    ) -> Result<(), AppError> {
        let Some(anchor) = request.popover_anchor_element else {
            return Ok(());
        };
        let parent = request.parent.ok_or_else(|| {
            AppError::Window(WindowCommandError::PopoverParentRequired.to_string())
        })?;
        let bounds = self
            .window_handles
            .get(&parent)
            .and_then(|window_id| self.windows.get(window_id))
            .and_then(|entry| entry.state.ui.element_bounds(anchor))
            .ok_or_else(|| {
                AppError::Window(format!(
                    "system popover anchor {anchor:?} is not mounted in its parent window"
                ))
            })?;
        let popover = request.options.popover.as_mut().ok_or_else(|| {
            AppError::Window(WindowCommandError::InvalidPopoverConfiguration.to_string())
        })?;
        popover.anchor_rect = bounds;
        Ok(())
    }

    pub(super) fn process_targeted_actions(&mut self, event_loop: &ActiveEventLoop) -> bool {
        let mut deliveries = 0_usize;
        while let Some((handle, action)) = self.targeted_actions.pop_front() {
            if deliveries == crate::MAX_PENDING_TARGETED_ACTIONS {
                self.fail(
                    event_loop,
                    AppError::View(format!(
                        "one effect cycle exceeded {} cross-window action deliveries",
                        crate::MAX_PENDING_TARGETED_ACTIONS
                    )),
                );
                return false;
            }
            deliveries += 1;
            let Some(window_id) = self.window_handles.get(&handle).copied() else {
                continue;
            };
            if !self.activate_window(window_id) {
                continue;
            }
            let delivered = self.invoke_action(event_loop, &action).is_some();
            self.deactivate_window();
            if !delivered {
                return false;
            }
        }
        true
    }

    pub(super) fn resume_deferred_image_loads(&mut self) {
        debug_assert!(self.current_window.is_none());
        let window_ids = self.windows.keys().copied().collect::<Vec<_>>();
        for window_id in window_ids {
            if !self.activate_window(window_id) {
                continue;
            }
            if let Some(state) = &mut self.window
                && state.image_assets.resume_deferred()
            {
                state.view_dirty = true;
                if state.scheduler.invalidate() {
                    state.window.request_redraw();
                }
            }
            self.deactivate_window();
        }
    }

    pub(super) fn invalidate_entity_observers(&mut self, entities: &[EntityId], all: bool) {
        if entities.is_empty() && !all {
            return;
        }
        for state in self
            .window
            .iter_mut()
            .chain(self.windows.values_mut().map(|entry| &mut entry.state))
        {
            let root = state.listeners.observes_entity_change(entities, all);
            let scoped = state.listeners.scopes.invalidate_entities(entities);
            if root || scoped {
                state.view_dirty |= root;
                if state.scheduler.invalidate() {
                    state.window.request_redraw();
                }
            }
        }
    }

    pub(super) fn invalidate_global_observers(&mut self, global_types: &[TypeId], all: bool) {
        if global_types.is_empty() && !all {
            return;
        }
        for state in self
            .window
            .iter_mut()
            .chain(self.windows.values_mut().map(|entry| &mut entry.state))
        {
            let root = state.listeners.observes_global_change(global_types, all);
            let scoped = state.listeners.scopes.invalidate_globals(global_types);
            if root || scoped {
                state.view_dirty |= root;
                if state.scheduler.invalidate() {
                    state.window.request_redraw();
                }
            }
        }
    }
}
