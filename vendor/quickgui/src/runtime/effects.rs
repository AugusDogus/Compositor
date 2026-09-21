use super::*;

impl Runtime {
    pub(super) fn dispatch(
        &mut self,
        event_loop: &ActiveEventLoop,
        event: Event,
        force_redraw: bool,
    ) -> bool {
        let mut cx = self.event_context();
        let Some(window) = &mut self.window else {
            return false;
        };
        window.view.event(&event, &mut cx);
        self.apply_event_context(event_loop, cx, force_redraw, true)
    }

    /// Deliver one constrain hook and return the narrowing the view requested, if any.
    ///
    /// The boolean reports whether the runtime may keep processing this native event.
    pub(super) fn dispatch_constraint(
        &mut self,
        event_loop: &ActiveEventLoop,
        event: Event,
    ) -> (bool, Option<Size>, Option<Point>) {
        let mut cx = self.event_context();
        let Some(window) = &mut self.window else {
            return (false, None, None);
        };
        window.view.event(&event, &mut cx);
        let size = cx.constrained_size;
        let position = cx.constrained_position;
        let alive = self.apply_event_context(event_loop, cx, false, true);
        (alive, size, position)
    }

    /// Deliver one window lifecycle event to a possibly inactive retained window.
    pub(super) fn dispatch_to_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        handle: WindowHandle,
        event: Event,
    ) {
        if self.current_handle() == Some(handle) {
            self.dispatch(event_loop, event, false);
            return;
        }
        let Some(window_id) = self.window_handles.get(&handle).copied() else {
            return;
        };
        if self.activate_window(window_id) {
            self.dispatch(event_loop, event, false);
            self.deactivate_window();
        }
    }

    /// Drain the bounded queue of lifecycle events produced while windows were deactivated.
    pub(super) fn process_pending_window_events(&mut self, event_loop: &ActiveEventLoop) {
        if self.pending_window_events.is_empty() {
            return;
        }
        for (handle, event) in std::mem::take(&mut self.pending_window_events) {
            if self.fatal_error.is_some() || self.exit_requested {
                return;
            }
            self.dispatch_to_window(event_loop, handle, event);
        }
    }

    /// Recompute minimize/maximize/fullscreen from the operating system and deliver the changes.
    ///
    /// Winit does not expose AppKit's `windowDidMiniaturize:`/`windowDidEnterFullScreen:`
    /// callbacks, so QuickGUI derives them at the native events that can accompany those
    /// transitions. The read is event-driven; nothing polls while the window is idle.
    pub(super) fn refresh_window_lifecycle(&mut self, event_loop: &ActiveEventLoop) -> bool {
        let Some(state) = self.window.as_ref() else {
            return false;
        };
        #[cfg(target_os = "macos")]
        let minimized = is_window_miniaturized(&state.window)
            .unwrap_or_else(|_| state.window.is_minimized().unwrap_or(state.minimized));
        #[cfg(not(target_os = "macos"))]
        let minimized = state.window.is_minimized().unwrap_or(state.minimized);
        let fullscreen = runtime_window_is_fullscreen(state);
        let maximized = runtime_window_is_maximized(state, &self.config);

        let state = self.window.as_mut().expect("window presence checked above");
        let minimized_changed = state.minimized != minimized;
        let fullscreen_changed = state.fullscreen != fullscreen;
        let maximized_changed = state.maximized != maximized;
        state.minimized = minimized;
        state.fullscreen = fullscreen;
        state.maximized = maximized;
        if !(minimized_changed || fullscreen_changed || maximized_changed) {
            return true;
        }
        let observe = state.listeners.observes_window_state;
        if minimized_changed && !self.dispatch(event_loop, Event::Minimized(minimized), observe) {
            return false;
        }
        if maximized_changed && !self.dispatch(event_loop, Event::Maximized(maximized), observe) {
            return false;
        }
        if fullscreen_changed
            && !self.dispatch(event_loop, Event::FullscreenChanged(fullscreen), observe)
        {
            return false;
        }
        true
    }

    /// Deliver `Event::WillResize` and apply the resulting inner-size narrowing.
    ///
    /// At most one corrective native resize is issued per proposal, and the corrective size is
    /// remembered so the resize it produces cannot start another round.
    pub(super) fn apply_resize_constraint(
        &mut self,
        event_loop: &ActiveEventLoop,
        proposed: Size,
    ) -> bool {
        let Some(state) = self.window.as_mut() else {
            return false;
        };
        if state.resize_correction == Some(proposed) {
            state.resize_correction = None;
            return true;
        }
        let aspect_ratio = self.config.aspect_ratio;
        let (alive, constrained, _) = self.dispatch_constraint(
            event_loop,
            Event::WillResize {
                proposed_size: proposed,
            },
        );
        if !alive {
            return false;
        }
        let mut target = constrained.unwrap_or(proposed);
        if let Some(ratio) = aspect_ratio {
            target = clamp_size_to_aspect_ratio(target, ratio);
        }
        let Some(state) = self.window.as_mut() else {
            return false;
        };
        if target != proposed {
            state.resize_correction = Some(target);
            let _ = state.window.request_inner_size(LogicalSize::new(
                f64::from(target.width),
                f64::from(target.height),
            ));
        }
        true
    }

    /// Deliver `Event::WillMove` and apply the resulting position override.
    pub(super) fn apply_move_constraint(
        &mut self,
        event_loop: &ActiveEventLoop,
        proposed: Point,
    ) -> bool {
        let Some(state) = self.window.as_mut() else {
            return false;
        };
        if state.move_correction == Some(proposed) {
            state.move_correction = None;
            return true;
        }
        let (alive, _, constrained) = self.dispatch_constraint(
            event_loop,
            Event::WillMove {
                proposed_position: proposed,
            },
        );
        if !alive {
            return false;
        }
        let Some(target) = constrained.filter(|target| *target != proposed) else {
            return true;
        };
        let Some(state) = self.window.as_mut() else {
            return false;
        };
        state.move_correction = Some(target);
        state.window.set_outer_position(LogicalPosition::new(
            f64::from(target.x),
            f64::from(target.y),
        ));
        true
    }

    pub(super) fn apply_event_context(
        &mut self,
        event_loop: &ActiveEventLoop,
        mut cx: EventContext,
        force_redraw: bool,
        announce_focus: bool,
    ) -> bool {
        let relaunch_requested = cx.relaunch.is_some();
        if let Some(request) = cx.relaunch.take() {
            self.relaunch_request = Some(request);
        }
        if let Some(code) = cx.exit_code.take() {
            self.exit_code = Some(code);
        }
        if cx.exit && !self.quit_phase_active {
            self.pending_quit = Some(if relaunch_requested {
                QuitReason::Relaunch
            } else {
                QuitReason::Explicit
            });
            return false;
        }
        let entity_notifications = std::mem::take(&mut cx.entity_notifications);
        let notify_all_entities = cx.notify_all_entities;
        let global_notifications = std::mem::take(&mut cx.global_notifications);
        let notify_all_globals = cx.notify_all_globals;
        if !enqueue_global_notifications(
            &mut self.pending_global_notifications,
            &mut self.pending_global_notification_types,
            &mut self.pending_all_globals,
            &global_notifications,
            notify_all_globals,
        ) {
            self.fail(
                event_loop,
                AppError::View(format!(
                    "one effect cycle cannot retain more than {MAX_PENDING_GLOBAL_NOTIFICATIONS} pending global notifications"
                )),
            );
            return false;
        }
        if !enqueue_entity_events(&mut self.pending_entity_events, &mut cx.entity_events) {
            self.fail(
                event_loop,
                AppError::View(format!(
                    "one effect cycle cannot retain more than {MAX_PENDING_ENTITY_EVENTS} pending entity events"
                )),
            );
            return false;
        }
        if self.targeted_actions.len() + cx.targeted_actions.len()
            > crate::MAX_PENDING_TARGETED_ACTIONS
        {
            self.fail(
                event_loop,
                AppError::View(format!(
                    "one effect cycle cannot retain more than {} cross-window actions",
                    crate::MAX_PENDING_TARGETED_ACTIONS
                )),
            );
            return false;
        }
        self.targeted_actions.extend(cx.targeted_actions.drain(..));
        for request in &mut cx.open_windows {
            let Some(anchor) = request.popover_anchor_element else {
                continue;
            };
            let Some(bounds) = self
                .window
                .as_ref()
                .and_then(|state| state.ui.element_bounds(anchor))
            else {
                self.fail(
                    event_loop,
                    AppError::View(format!(
                        "system popover anchor {anchor:?} is not mounted in its parent window"
                    )),
                );
                return false;
            };
            let Some(popover) = request.options.popover.as_mut() else {
                self.fail(
                    event_loop,
                    AppError::Window(WindowCommandError::InvalidPopoverConfiguration.to_string()),
                );
                return false;
            };
            popover.anchor_rect = bounds;
        }
        self.pending_windows.extend(cx.open_windows.drain(..));
        if cx.close_current_window
            && let Some(handle) = self.current_handle()
        {
            self.close_requests.push(handle);
        }
        self.close_requests.append(&mut cx.close_windows);
        self.focus_requests.append(&mut cx.focus_windows);
        self.invalidate_requests.append(&mut cx.invalidate_windows);
        if self.window_commands.len() + cx.window_commands.len() > MAX_PENDING_WINDOW_COMMANDS {
            self.fail(
                event_loop,
                AppError::View(format!(
                    "one effect cycle cannot retain more than {MAX_PENDING_WINDOW_COMMANDS} pending window commands"
                )),
            );
            return false;
        }
        self.window_commands.append(&mut cx.window_commands);
        enqueue_platform_requests(&mut self.platform_requests, &mut cx.platform_requests);
        let actions = std::mem::take(&mut cx.actions);
        let form_submissions = std::mem::take(&mut cx.form_submissions);
        let menus = cx.menus.take();
        let window_menus = cx.window_menus.take();
        let native_popup_menus = std::mem::take(&mut cx.native_popup_menus);
        let mut focus_changed = false;
        if let Some(state) = &mut self.window {
            let previous_focus = state.ui.focused();
            let mut deferred_focus = false;
            // Focus that stays on the same element can still change how it paints: a listener
            // focusing the keyboard-focused control from a click hides its ring, and only a
            // repaint shows that, since the focused element itself did not change.
            let mut focus_repaint = false;
            let selection_changed =
                cx.clear_text_selection && state.ui.clear_static_text_selection();
            if let Some(request) = cx.focus {
                match request {
                    Some(id) => {
                        if state.ui.is_focusable(id) {
                            state.pending_focus = None;
                            focus_repaint = state.ui.focus(id);
                        } else {
                            // The caller may be opening a view that declares this focus handle in
                            // the rebuild requested by the same event. Keep the request for exactly
                            // that rebuild instead of adding a timer or requiring a second event,
                            // together with the input making it, which has ended by then.
                            state.pending_focus = Some(state.ui.pending_focus(id));
                            state.view_dirty = true;
                            deferred_focus = true;
                        }
                    }
                    None => {
                        state.pending_focus = None;
                        focus_repaint = state.ui.blur();
                    }
                }
            }
            focus_changed = previous_focus != state.ui.focused();
            if focus_changed {
                self.pending_input = None;
            }
            #[cfg(target_os = "macos")]
            if focus_changed
                && state.ui.focused().is_some()
                && let Some(host) = &state.native_host
            {
                host.focus_framework();
            }
            if cx.invalidate {
                state.view_dirty = true;
            }
            if (force_redraw
                || cx.invalidate
                || focus_changed
                || focus_repaint
                || deferred_focus
                || selection_changed)
                && state.scheduler.invalidate()
            {
                state.window.request_redraw();
            }
        }
        if focus_changed && announce_focus {
            let focused = self.window.as_ref().and_then(|state| state.ui.focused());
            let mut focus_cx = self.event_context();
            if let Some(window) = &mut self.window {
                window
                    .view
                    .event(&Event::FocusChanged(focused), &mut focus_cx);
            }
            if !self.apply_event_context(event_loop, focus_cx, false, false) {
                return false;
            }
        }
        #[cfg(target_os = "macos")]
        if focus_changed {
            self.sync_native_menu_state();
        }
        if let Some(menus) = menus
            && !self.replace_menus(event_loop, menus)
        {
            return false;
        }
        if let Some(window_menus) = window_menus
            && !self.replace_current_window_menus(event_loop, window_menus)
        {
            return false;
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        for popup in native_popup_menus {
            if !self.show_current_native_popup_menu(event_loop, popup, None) {
                return false;
            }
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        debug_assert!(native_popup_menus.is_empty());
        for action in actions {
            if self.invoke_action(event_loop, &action).is_none() {
                return false;
            }
        }
        for form in form_submissions {
            if !self.invoke_form_submission(event_loop, form, None) {
                return false;
            }
        }
        self.invalidate_entity_observers(&entity_notifications, notify_all_entities);
        self.invalidate_global_observers(&global_notifications, notify_all_globals);
        true
    }

    pub(super) fn finalize_process_services(&mut self) {
        if self.process_services_finalized {
            return;
        }
        self.process_services_finalized = true;
        global_shortcut::clear_global_shortcut_handler_proxy();
        tray::clear_tray_handler_proxy();
        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        self.global_shortcuts.clear();
        self.tray_icons.clear();
        #[cfg(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd",
            target_os = "dragonfly",
            target_os = "openbsd",
            target_os = "netbsd"
        ))]
        {
            self.single_instance.take();
        }
        #[cfg(target_os = "macos")]
        {
            self.menu_host.take();
            self.mac_application_host.take();
            for (_, dialog) in self.active_platform_dialogs.drain() {
                dialog.native.cancel();
            }
        }
        #[cfg(target_os = "windows")]
        {
            self.windows_menu_host.take();
            self._windows_power_monitor.take();
        }
        #[cfg(target_os = "linux")]
        {
            self._linux_power_monitor.take();
        }
        #[cfg(any(
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd",
            target_os = "dragonfly",
            target_os = "openbsd",
            target_os = "netbsd"
        ))]
        self.active_platform_dialogs.clear();
        self.foreground_tasks.shutdown();
        self.background_tasks.shutdown();
        self.image_workers.shutdown();
    }

    /// Returns `None` after exit, otherwise whether a handler consumed the action.
    pub(super) fn invoke_action(
        &mut self,
        event_loop: &ActiveEventLoop,
        action: &AnyAction,
    ) -> Option<bool> {
        let Some(window) = &mut self.window else {
            return self.invoke_application_action(event_loop, action);
        };
        let path = window.ui.focus_path();
        let mut dispatch = std::mem::take(&mut window.action_dispatch_scratch);
        window
            .ui
            .collect_action_dispatch(&path, action.type_id(), &mut dispatch);

        for binding in dispatch.iter().copied() {
            let listener = self
                .window
                .as_ref()
                .and_then(|window| window.listeners.action_listener(binding.key));
            let Some(listener) = listener else {
                continue;
            };
            let mut cx = self.event_context();
            if let Some(window) = &mut self.window {
                listener(window.view.as_any_mut(), action.as_any(), &mut cx);
            }
            let propagate = cx.propagate_action;
            let stopped = cx.stop_event_propagation;
            if !self.apply_event_context(event_loop, cx, false, true) {
                return None;
            }
            let consumed = match binding.phase {
                crate::DispatchPhase::Capture => stopped,
                crate::DispatchPhase::Bubble => !propagate,
            };
            if consumed {
                dispatch.clear();
                if let Some(window) = &mut self.window {
                    window.action_dispatch_scratch = dispatch;
                }
                return Some(true);
            }
        }

        dispatch.clear();
        if let Some(window) = &mut self.window {
            window.action_dispatch_scratch = dispatch;
        }
        self.invoke_application_action(event_loop, action)
    }

    fn invoke_application_action(
        &mut self,
        event_loop: &ActiveEventLoop,
        action: &AnyAction,
    ) -> Option<bool> {
        if !self
            .application_callbacks
            .actions
            .contains_key(&action.type_id())
        {
            return Some(false);
        }
        let mut cx = self.event_context();
        let Some(consumed) = self.application_callbacks.dispatch_action(action, &mut cx) else {
            return Some(false);
        };
        self.apply_event_context(event_loop, cx, false, true)
            .then_some(consumed)
    }

    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
    pub(super) fn action_available(&self, action: &AnyAction) -> bool {
        self.application_callbacks
            .actions
            .contains_key(&action.type_id())
            || self.window.as_ref().is_some_and(|window| {
                let path = window.ui.focus_path();
                window.ui.action_available(&path, action.type_id())
            })
    }

    #[cfg(target_os = "macos")]
    pub(super) fn invoke_dock_menu_action(
        &mut self,
        event_loop: &ActiveEventLoop,
        action_id: usize,
    ) {
        let item = self
            .dock_menu_actions
            .get(action_id)
            .filter(|item| !item.disabled)
            .map(|item| (item.action.clone(), item.os_action));
        let Some((action, os_action)) = item else {
            return;
        };
        let target = self
            .active_window
            .or_else(|| self.focus_history.last().copied());
        if let Some(target) = target
            && !self.activate_window(target)
        {
            return;
        }
        let handled = action
            .as_ref()
            .is_some_and(|action| self.invoke_action(event_loop, action).unwrap_or(true));
        if !handled && let Some(os_action) = os_action {
            self.invoke_os_action(event_loop, os_action);
        }
        if target.is_some() {
            self.deactivate_window();
        }
        self.process_window_commands(event_loop);
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(super) fn active_menu_declaration(&self) -> &[Menu] {
        let Some(active) = self.active_window else {
            return &self.menus;
        };
        if self.current_window.is_some_and(|(id, _)| id == active) {
            return self.config.window_menus.as_deref().unwrap_or(&self.menus);
        }
        self.windows
            .get(&active)
            .and_then(|entry| entry.config.window_menus.as_deref())
            .unwrap_or(&self.menus)
    }

    #[cfg(target_os = "macos")]
    pub(super) fn install_active_mac_menu(&mut self, event_loop: &ActiveEventLoop) -> bool {
        let menus = self.active_menu_declaration().to_vec();
        let host = match MacMenuHost::new(&menus, self.event_proxy.clone()) {
            Ok(host) => host,
            Err(error) => {
                self.fail(event_loop, AppError::Platform(error));
                return false;
            }
        };
        self.menu_actions = collect_menu_actions(&menus);
        self.menu_host = Some(host);
        self.sync_active_native_menu_state();
        true
    }

    pub(super) fn replace_menus(&mut self, event_loop: &ActiveEventLoop, menus: Vec<Menu>) -> bool {
        if let Err(error) = validate_menus(&menus) {
            self.fail(event_loop, AppError::Platform(error.to_string()));
            return false;
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let _ = event_loop;
        #[cfg(target_os = "windows")]
        let next_host = if menus.is_empty() {
            None
        } else {
            match windows_menu::WindowsMenuHost::new(&menus, self.event_proxy.clone()) {
                Ok(host) => Some(host),
                Err(error) => {
                    self.fail(event_loop, AppError::Platform(error));
                    return false;
                }
            }
        };

        self.menus = menus;
        #[cfg(target_os = "macos")]
        {
            if !self.install_active_mac_menu(event_loop) {
                return false;
            }
        }
        #[cfg(target_os = "windows")]
        {
            // Dropping the old host detaches its menu before the replacement attaches.
            self.windows_menu_host.take();
            if let Some(next_host) = &next_host {
                for entry in self.windows.values() {
                    if entry.config.window_menus.is_some() {
                        continue;
                    }
                    if let Err(error) = next_host.attach(&entry.state.window) {
                        self.fail(event_loop, AppError::Platform(error));
                        return false;
                    }
                }
                if self.config.window_menus.is_none()
                    && let Some(window) = &self.window
                    && let Err(error) = next_host.attach(&window.window)
                {
                    self.fail(event_loop, AppError::Platform(error));
                    return false;
                }
            }
            self.windows_menu_host = next_host;
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let _ = menus;
        true
    }

    pub(super) fn replace_current_window_menus(
        &mut self,
        event_loop: &ActiveEventLoop,
        menus: Option<Vec<Menu>>,
    ) -> bool {
        if let Some(menus) = menus.as_deref()
            && let Err(error) = validate_menus(menus)
        {
            self.fail(event_loop, AppError::Platform(error.to_string()));
            return false;
        }
        let Some((_window_id, _)) = self.current_window else {
            return false;
        };

        #[cfg(target_os = "windows")]
        let next_host = match menus.as_deref() {
            Some([]) | None => None,
            Some(menus) => {
                match windows_menu::WindowsMenuHost::new(menus, self.event_proxy.clone()) {
                    Ok(host) => Some(host),
                    Err(error) => {
                        self.fail(event_loop, AppError::Platform(error));
                        return false;
                    }
                }
            }
        };

        #[cfg(target_os = "windows")]
        {
            let Some(state) = self.window.as_mut() else {
                return false;
            };
            if self.config.window_menus.is_none()
                && let Some(host) = &self.windows_menu_host
                && let Err(error) = host.detach(&state.window)
            {
                self.fail(event_loop, AppError::Platform(error));
                return false;
            }
            state.window_menu_host.take();
            if menus.is_some() {
                if let Some(host) = &next_host
                    && let Err(error) = host.attach(&state.window)
                {
                    self.fail(event_loop, AppError::Platform(error));
                    return false;
                }
                state.window_menu_host = next_host;
            } else if let Some(host) = &self.windows_menu_host
                && let Err(error) = host.attach(&state.window)
            {
                self.fail(event_loop, AppError::Platform(error));
                return false;
            }
        }

        self.config.window_menus = menus;

        #[cfg(target_os = "macos")]
        if self.active_window == Some(_window_id) && !self.install_active_mac_menu(event_loop) {
            return false;
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let _ = (event_loop, _window_id);
        true
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(super) fn show_current_native_popup_menu(
        &mut self,
        event_loop: &ActiveEventLoop,
        request: crate::event::NativePopupMenuRequest,
        responder: Option<crate::platform::PlatformResponder<()>>,
    ) -> bool {
        if self.pending_native_popup_menus.len() == MAX_PENDING_NATIVE_POPUP_MENUS {
            if let Some(responder) = &responder {
                responder.complete(Err(PlatformError::PendingQueueFull));
            }
            self.fail(
                event_loop,
                AppError::View(format!(
                    "the application cannot retain more than {MAX_PENDING_NATIVE_POPUP_MENUS} selected native popup menus"
                )),
            );
            return false;
        }
        let Some(handle) = self.current_handle() else {
            if let Some(responder) = &responder {
                responder.complete(Err(PlatformError::Unavailable));
            }
            return false;
        };
        let actions = collect_menu_actions(std::slice::from_ref(&request.menu));
        static NEXT_POPUP_MENU_ID: AtomicU64 = AtomicU64::new(1);
        let popup_id = NEXT_POPUP_MENU_ID.fetch_add(1, Ordering::Relaxed).max(1);

        #[cfg(target_os = "macos")]
        let states = {
            let contexts = self
                .window
                .as_ref()
                .map(|window| window.ui.key_context_stack())
                .unwrap_or_default();
            actions
                .iter()
                .map(|item| MacMenuItemState {
                    disabled: item.disabled,
                    action_available: item
                        .action
                        .as_ref()
                        .is_some_and(|action| self.action_available(action))
                        || item
                            .os_action
                            .is_some_and(|action| self.os_action_available(action)),
                    checked: item.checked,
                    hidden: item.hidden,
                    shortcut: item.shortcut.clone().or_else(|| {
                        item.action.as_ref().and_then(|action| {
                            self.keymap.shortcut_for_action_value(action, &contexts)
                        })
                    }),
                })
                .collect::<Vec<_>>()
        };
        #[cfg(target_os = "macos")]
        let native_focus_active = self
            .window
            .as_ref()
            .and_then(|window| window.native_host.as_ref())
            .is_some_and(MacNativeHost::native_focus_active);

        self.pending_native_popup_menus.insert(
            popup_id,
            PendingNativePopupMenu {
                window: handle,
                actions,
                responder,
            },
        );

        #[cfg(target_os = "windows")]
        tray::install_native_menu_handlers(self.event_proxy.clone());
        let result = {
            let Some(window) = self.window.as_ref() else {
                complete_native_popup_menu(
                    self.pending_native_popup_menus.remove(&popup_id),
                    Err(PlatformError::Unavailable),
                );
                return false;
            };
            #[cfg(target_os = "macos")]
            {
                crate::macos_menu::show_popup_menu(
                    &request.menu,
                    popup_id,
                    &window.window,
                    request.position,
                    &states,
                    native_focus_active,
                    self.event_proxy.clone(),
                )
            }
            #[cfg(target_os = "windows")]
            {
                windows_menu::show_popup_menu(
                    &request.menu,
                    popup_id,
                    &window.window,
                    request.position,
                )
            }
        };
        let shown = match result {
            Ok(shown) => shown,
            Err(error) => {
                complete_native_popup_menu(
                    self.pending_native_popup_menus.remove(&popup_id),
                    Err(PlatformError::Unavailable),
                );
                self.fail(event_loop, AppError::Platform(error));
                return false;
            }
        };
        if !shown {
            complete_native_popup_menu(self.pending_native_popup_menus.remove(&popup_id), Ok(()));
            return true;
        }
        if self
            .event_proxy
            .send_event(RuntimeEvent::NativePopupMenuClosed(popup_id))
            .is_err()
        {
            complete_native_popup_menu(self.pending_native_popup_menus.remove(&popup_id), Ok(()));
        }
        true
    }

    /// Show a native popup menu owned by one window from outside any effect cycle.
    ///
    /// The window is activated exactly as a targeted action is, so the popup resolves against the
    /// same `EventContext`-scoped state an in-view `show_native_popup_menu` call would see.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub(super) fn show_external_native_popup_menu(
        &mut self,
        event_loop: &ActiveEventLoop,
        request: crate::runtime::ExternalPopupMenuRequest,
    ) -> bool {
        let crate::runtime::ExternalPopupMenuRequest {
            window,
            menu,
            position,
            responder,
        } = request;
        let Some(window_id) = self.window_handles.get(&window).copied() else {
            responder.complete(Err(PlatformError::Unavailable));
            return true;
        };
        if !self.activate_window(window_id) {
            responder.complete(Err(PlatformError::Unavailable));
            return true;
        }
        let shown = self.show_current_native_popup_menu(
            event_loop,
            crate::event::NativePopupMenuRequest { menu, position },
            Some(responder),
        );
        self.deactivate_window();
        shown
    }

    /// Replace or clear one window's native menu declaration from outside any effect cycle.
    pub(super) fn replace_external_window_menus(
        &mut self,
        event_loop: &ActiveEventLoop,
        request: crate::runtime::ExternalWindowMenus,
    ) -> bool {
        let Some(window_id) = self.window_handles.get(&request.window).copied() else {
            return true;
        };
        if !self.activate_window(window_id) {
            return true;
        }
        let replaced = self.replace_current_window_menus(event_loop, request.menus);
        self.deactivate_window();
        replaced
    }

    pub(super) fn os_action_available(&self, action: OsAction) -> bool {
        let window = self.window.as_ref();
        let input_focused = window.is_some_and(|window| window.ui.focused_text_input().is_some());
        match action {
            OsAction::Cut => {
                input_focused
                    && window
                        .expect("input focus requires a current window")
                        .ui
                        .selected_input_text()
                        .is_some_and(|selection| !selection.is_empty())
            }
            OsAction::Copy => window.is_some_and(|window| {
                window
                    .ui
                    .selected_text()
                    .is_some_and(|selection| !selection.is_empty())
            }),
            OsAction::Paste => input_focused,
            OsAction::SelectAll => {
                input_focused || window.is_some_and(|window| window.ui.has_selectable_text())
            }
            OsAction::Undo => {
                input_focused && window.is_some_and(|window| window.ui.input_can_undo())
            }
            OsAction::Redo => {
                input_focused && window.is_some_and(|window| window.ui.input_can_redo())
            }
            OsAction::About => cfg!(any(target_os = "macos", target_os = "windows")),
            OsAction::ShowHelp => cfg!(target_os = "macos"),
            OsAction::PasteAndMatchStyle => input_focused,
            OsAction::Delete => {
                input_focused
                    && window
                        .expect("input focus requires a current window")
                        .ui
                        .selected_input_text()
                        .is_some_and(|selection| !selection.is_empty())
            }
            OsAction::StartSpeaking | OsAction::StopSpeaking => {
                cfg!(target_os = "macos") && window.is_some()
            }
            OsAction::SelectNextTab
            | OsAction::SelectPreviousTab
            | OsAction::MergeAllWindows
            | OsAction::MoveTabToNewWindow
            | OsAction::ToggleTabBar
            | OsAction::ToggleTabOverview => cfg!(target_os = "macos") && window.is_some(),
            OsAction::HideApplication
            | OsAction::ShowAllApplications
            | OsAction::Quit
            | OsAction::BringAllToFront => !self.window_handles.is_empty(),
            OsAction::HideOtherApplications => cfg!(target_os = "macos"),
            OsAction::CloseWindow => window.is_some() && self.config.is_closable,
            OsAction::MinimizeWindow => window.is_some() && self.config.is_minimizable,
            OsAction::ZoomWindow => {
                window.is_some() && self.config.is_resizable && self.config.is_maximizable
            }
            OsAction::ToggleFullscreen => window.is_some(),
        }
    }

    pub(super) fn invoke_os_action(
        &mut self,
        event_loop: &ActiveEventLoop,
        action: OsAction,
    ) -> bool {
        match action {
            OsAction::Copy => {
                let selected = self
                    .window
                    .as_ref()
                    .and_then(|window| window.ui.selected_text());
                if let Some(selected) = selected {
                    let _ = self.write_clipboard_text(selected.as_ref());
                    return true;
                }
                false
            }
            OsAction::Cut => {
                let selected = self
                    .window
                    .as_ref()
                    .and_then(|window| window.ui.selected_input_text());
                let Some(selected) = selected else {
                    return false;
                };
                let copied = self.write_clipboard_text(selected.as_ref());
                if copied {
                    let result = self
                        .window
                        .as_mut()
                        .map(|window| window.ui.input_backspace())
                        .unwrap_or_default();
                    self.apply_input_result(event_loop, result, true);
                }
                true
            }
            OsAction::Paste => {
                if self
                    .window
                    .as_ref()
                    .and_then(|window| window.ui.focused_text_input())
                    .is_none()
                {
                    return false;
                }
                let pasted = self.read_clipboard_text();
                if let Some(value) = pasted {
                    let result = self
                        .window
                        .as_mut()
                        .map(|window| window.ui.input_replace(&value))
                        .unwrap_or_default();
                    self.apply_input_result(event_loop, result, true);
                }
                true
            }
            OsAction::SelectAll => {
                if !self.os_action_available(OsAction::SelectAll) {
                    return false;
                }
                let result = self
                    .window
                    .as_mut()
                    .map_or_else(InputResult::default, |window| {
                        if window.ui.focused_text_input().is_some() {
                            window.ui.input_select_all()
                        } else {
                            InputResult {
                                repaint: window.ui.select_all_static_text(),
                                change: None,
                            }
                        }
                    });
                self.apply_input_result(event_loop, result, false);
                true
            }
            OsAction::Undo => {
                if self
                    .window
                    .as_ref()
                    .and_then(|window| window.ui.focused_text_input())
                    .is_none()
                {
                    return false;
                }
                let result = self
                    .window
                    .as_mut()
                    .map(|window| window.ui.input_undo())
                    .unwrap_or_default();
                self.apply_input_result(event_loop, result, true)
            }
            OsAction::Redo => {
                if self
                    .window
                    .as_ref()
                    .and_then(|window| window.ui.focused_text_input())
                    .is_none()
                {
                    return false;
                }
                let result = self
                    .window
                    .as_mut()
                    .map(|window| window.ui.input_redo())
                    .unwrap_or_default();
                self.apply_input_result(event_loop, result, true)
            }
            OsAction::About => {
                if !DesktopIntegrationSupport::current().native_about_panel
                    || self.platform_requests.len() == crate::MAX_PENDING_PLATFORM_REQUESTS
                {
                    return false;
                }
                let Ok(request) = PlatformRequest::show_about_panel(AboutPanelOptions::default())
                else {
                    return false;
                };
                self.platform_requests.push_back(request);
                true
            }
            OsAction::HideOtherApplications | OsAction::ShowHelp => false,
            OsAction::HideApplication | OsAction::ShowAllApplications => {
                let visible = action == OsAction::ShowAllApplications;
                let mut handles = self.window_handles.keys().copied().collect::<Vec<_>>();
                handles.sort_unstable();
                self.window_commands.extend(
                    handles
                        .into_iter()
                        .map(|handle| WindowCommand::SetVisible(handle, visible)),
                );
                true
            }
            OsAction::Quit => {
                self.pending_quit = Some(QuitReason::Explicit);
                true
            }
            OsAction::CloseWindow => {
                if !self.config.is_closable {
                    return false;
                }
                let Some(handle) = self.current_handle() else {
                    return false;
                };
                self.close_requests.push(handle);
                true
            }
            OsAction::MinimizeWindow => {
                if !self.config.is_minimizable {
                    return false;
                }
                let Some(handle) = self.current_handle() else {
                    return false;
                };
                self.window_commands.push(WindowCommand::Minimize(handle));
                true
            }
            OsAction::ZoomWindow => {
                if !self.config.is_resizable || !self.config.is_maximizable {
                    return false;
                }
                let Some(handle) = self.current_handle() else {
                    return false;
                };
                self.window_commands.push(WindowCommand::Zoom(handle));
                true
            }
            OsAction::ToggleFullscreen => {
                let Some(handle) = self.current_handle() else {
                    return false;
                };
                self.window_commands
                    .push(WindowCommand::ToggleFullscreen(handle));
                true
            }
            OsAction::BringAllToFront => {
                let mut handles = self.window_handles.keys().copied().collect::<Vec<_>>();
                handles.sort_unstable();
                self.focus_requests.extend(handles);
                true
            }
            // QuickGUI's retained inputs already insert unstyled plain text.
            OsAction::PasteAndMatchStyle => self.invoke_os_action(event_loop, OsAction::Paste),
            OsAction::Delete => {
                if self
                    .window
                    .as_ref()
                    .and_then(|window| window.ui.selected_input_text())
                    .is_none_or(|selection| selection.is_empty())
                {
                    return false;
                }
                let result = self
                    .window
                    .as_mut()
                    .map(|window| window.ui.input_backspace())
                    .unwrap_or_default();
                self.apply_input_result(event_loop, result, true);
                true
            }
            // Speech synthesis has no retained fallback; AppKit's responder chain owns it.
            OsAction::StartSpeaking | OsAction::StopSpeaking => false,
            OsAction::SelectNextTab
            | OsAction::SelectPreviousTab
            | OsAction::MergeAllWindows
            | OsAction::MoveTabToNewWindow
            | OsAction::ToggleTabBar
            | OsAction::ToggleTabOverview => {
                let Some(handle) = self.current_handle() else {
                    return false;
                };
                self.window_commands.push(match action {
                    OsAction::SelectNextTab => WindowCommand::SelectNextTab(handle),
                    OsAction::SelectPreviousTab => WindowCommand::SelectPreviousTab(handle),
                    OsAction::MergeAllWindows => WindowCommand::MergeAllWindows(handle),
                    OsAction::MoveTabToNewWindow => WindowCommand::MoveTabToNewWindow(handle),
                    OsAction::ToggleTabBar => WindowCommand::ToggleTabBar(handle),
                    _ => WindowCommand::ToggleTabOverview(handle),
                });
                true
            }
        }
    }

    #[cfg(target_os = "macos")]
    pub(super) fn sync_native_menu_state(&self) {
        let Some(host) = &self.menu_host else {
            return;
        };
        let contexts = self
            .window
            .as_ref()
            .map(|window| window.ui.key_context_stack())
            .unwrap_or_default();
        let states = self
            .menu_actions
            .iter()
            .map(|item| MacMenuItemState {
                disabled: item.disabled,
                action_available: item
                    .action
                    .as_ref()
                    .is_some_and(|action| self.action_available(action))
                    || item
                        .os_action
                        .is_some_and(|action| self.os_action_available(action)),
                checked: item.checked,
                hidden: item.hidden,
                shortcut: item.shortcut.clone().or_else(|| {
                    item.action
                        .as_ref()
                        .and_then(|action| self.keymap.shortcut_for_action_value(action, &contexts))
                }),
            })
            .collect::<Vec<_>>();
        let native_focus_active = self
            .window
            .as_ref()
            .and_then(|window| window.native_host.as_ref())
            .is_some_and(MacNativeHost::native_focus_active);
        let keymap_claims = |key: &str| {
            let matched = self.keymap.bindings_for_input(
                &[Keystroke::new(
                    Key::Character(key.to_owned()),
                    Modifiers::SUPER,
                )],
                &contexts,
            );
            matched.pending
                || matched
                    .bindings
                    .iter()
                    .any(|binding| self.action_available(binding.action()))
        };
        host.update(
            &states,
            native_focus_active,
            keymap_claims("w"),
            keymap_claims("m"),
        );
    }

    #[cfg(target_os = "macos")]
    pub(super) fn sync_active_native_menu_state(&mut self) {
        if self.window.is_some() {
            self.sync_native_menu_state();
            return;
        }
        let Some(window_id) = self.active_window else {
            self.sync_native_menu_state();
            return;
        };
        if self.activate_window(window_id) {
            self.sync_native_menu_state();
            self.deactivate_window();
        }
    }
}

/// Complete one retained popup menu's external responder, if the request declared one.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn complete_native_popup_menu(
    popup: Option<PendingNativePopupMenu>,
    result: Result<(), PlatformError>,
) {
    if let Some(responder) = popup.and_then(|popup| popup.responder) {
        responder.complete(result);
    }
}

/// Append one externally declared request to a bounded deferred queue.
///
/// Deferred `AppRunner` menu work is retained until the runtime reaches a window-scoped effect
/// cycle, so the queue carries the same public bound as the retained popup-menu table.
pub(super) fn queue_deferred_menu_request<T>(
    queue: &mut VecDeque<T>,
    request: T,
) -> Result<(), PlatformError> {
    if queue.len() >= MAX_PENDING_NATIVE_POPUP_MENUS {
        return Err(PlatformError::PendingQueueFull);
    }
    queue.push_back(request);
    Ok(())
}

#[cfg(test)]
mod deferred_menu_tests {
    use super::*;

    #[test]
    fn deferred_menu_requests_stop_at_the_public_popup_menu_bound() {
        let mut queue = VecDeque::new();
        for index in 0..MAX_PENDING_NATIVE_POPUP_MENUS {
            queue_deferred_menu_request(&mut queue, index)
                .expect("the queue accepts requests below its bound");
        }
        assert_eq!(queue.len(), MAX_PENDING_NATIVE_POPUP_MENUS);
        assert!(matches!(
            queue_deferred_menu_request(&mut queue, MAX_PENDING_NATIVE_POPUP_MENUS),
            Err(PlatformError::PendingQueueFull)
        ));
        assert_eq!(queue.len(), MAX_PENDING_NATIVE_POPUP_MENUS);
        queue.pop_front();
        queue_deferred_menu_request(&mut queue, MAX_PENDING_NATIVE_POPUP_MENUS)
            .expect("draining one entry frees exactly one slot");
    }
}
