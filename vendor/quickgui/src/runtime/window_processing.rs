use super::*;

impl Runtime {
    pub(super) fn process_queued_window_commands(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(not(target_os = "macos"))]
        let _ = event_loop;
        #[cfg(target_os = "macos")]
        let mut refresh_native_tabs = false;
        // A command for a window whose platform creation is still queued waits for that window;
        // one for a window that no longer exists, or never will, is dropped.
        let mut waiting = Vec::new();
        for command in std::mem::take(&mut self.window_commands) {
            let handle = command.handle();
            let Some(window_id) = self.window_handles.get(&handle).copied() else {
                if self.window_is_pending(handle) && waiting.len() < MAX_PENDING_WINDOW_COMMANDS {
                    waiting.push(command);
                }
                continue;
            };
            #[cfg(target_os = "macos")]
            let sibling_window = match &command {
                WindowCommand::MoveAbove(_, other) => self
                    .window_handles
                    .get(other)
                    .copied()
                    .and_then(|id| self.windows.get(&id))
                    .map(|entry| entry.state.window.clone()),
                _ => None,
            };
            #[cfg(target_os = "macos")]
            let parent_window = self.windows.get(&window_id).and_then(|entry| {
                entry
                    .state
                    .parent
                    .and_then(|parent| self.window_handles.get(&parent).copied())
                    .and_then(|parent_id| self.windows.get(&parent_id))
                    .map(|parent| parent.state.window.clone())
            });
            let Some(entry) = self.windows.get_mut(&window_id) else {
                continue;
            };
            let state = &mut entry.state;
            let mut state_changed = false;
            let mut force_redraw = false;
            let mut window_events: Vec<Event> = Vec::new();
            #[cfg(target_os = "macos")]
            let mut tabbing_ownership_delta = 0_i8;
            #[cfg(feature = "inspector")]
            let mut inspector_redraw = false;

            match command {
                WindowCommand::SetTitle(_, title) => {
                    if entry.config.title != title {
                        entry.config.title = title;
                        state.window.set_title(&entry.config.title);
                        // AppKit relays out even a hidden titlebar when its title changes. Restore
                        // the inset in this same native turn, before a frame can show the default
                        // controls while waiting for NSWindowDidUpdateNotification.
                        #[cfg(target_os = "macos")]
                        if let Some(position) = entry.config.traffic_light_position
                            && let Err(error) = position_traffic_lights(&state.window, position)
                        {
                            tracing::warn!(%error, "could not restore traffic lights after changing the window title");
                        }
                        force_redraw = true;
                    }
                }
                WindowCommand::SetRepresentedFile(_, represented_file) => {
                    if entry.config.represented_file != represented_file {
                        #[cfg(target_os = "macos")]
                        let applied = set_window_represented_file(
                            &state.window,
                            represented_file.as_deref(),
                        )
                        .map_err(|error| {
                            tracing::warn!(%error, "could not change represented document file");
                        })
                        .is_ok();
                        #[cfg(not(target_os = "macos"))]
                        let applied = true;
                        if applied {
                            entry.config.represented_file = represented_file;
                            #[cfg(target_os = "macos")]
                            if let Some(position) = entry.config.traffic_light_position
                                && let Err(error) = position_traffic_lights(&state.window, position)
                            {
                                tracing::warn!(%error, "could not restore traffic lights after changing represented document file");
                            }
                            state_changed = true;
                        }
                    }
                }
                WindowCommand::SetDocumentEdited(_, edited) => {
                    if entry.config.document_edited != edited {
                        #[cfg(target_os = "macos")]
                        let applied = set_window_document_edited(&state.window, edited)
                            .map_err(|error| {
                                tracing::warn!(%error, "could not change native document edited state");
                            })
                            .is_ok();
                        #[cfg(not(target_os = "macos"))]
                        let applied = true;
                        if applied {
                            entry.config.document_edited = edited;
                            #[cfg(target_os = "macos")]
                            if let Some(position) = entry.config.traffic_light_position
                                && let Err(error) = position_traffic_lights(&state.window, position)
                            {
                                tracing::warn!(%error, "could not restore traffic lights after changing document edited state");
                            }
                            state_changed = true;
                        }
                    }
                }
                WindowCommand::ShowCharacterPalette(_) => {
                    #[cfg(target_os = "macos")]
                    if let Err(error) = show_character_palette(&state.window) {
                        tracing::warn!(%error, "could not present the native character palette");
                    }
                }
                WindowCommand::LookUpSelection(_) => {
                    if let Err(error) = state.ui.input_look_up_selection() {
                        tracing::debug!(%error, "no dictionary definition was presented");
                    }
                }
                WindowCommand::SetTabbingIdentifier(_, identifier) => {
                    if entry.config.tabbing_identifier != identifier {
                        #[cfg(target_os = "macos")]
                        let applied = set_window_tabbing_identifier(
                            &state.window,
                            identifier.as_deref(),
                        )
                        .map_err(|error| {
                            tracing::warn!(%error, "could not change native tabbing identifier");
                        })
                        .is_ok();
                        #[cfg(not(target_os = "macos"))]
                        let applied = true;
                        if applied {
                            #[cfg(target_os = "macos")]
                            {
                                tabbing_ownership_delta = match (
                                    entry.config.tabbing_identifier.is_some(),
                                    identifier.is_some(),
                                ) {
                                    (false, true) => 1,
                                    (true, false) => -1,
                                    _ => 0,
                                };
                                refresh_native_tabs = true;
                            }
                            entry.config.tabbing_identifier = identifier;
                            state_changed = true;
                        }
                    }
                }
                WindowCommand::SelectNextTab(_) => {
                    #[cfg(target_os = "macos")]
                    {
                        if let Err(error) =
                            perform_window_tab_action(&state.window, MacWindowTabAction::SelectNext)
                        {
                            tracing::warn!(%error, "could not select the next native window tab");
                        }
                        refresh_native_tabs = true;
                    }
                }
                WindowCommand::SelectPreviousTab(_) => {
                    #[cfg(target_os = "macos")]
                    {
                        if let Err(error) = perform_window_tab_action(
                            &state.window,
                            MacWindowTabAction::SelectPrevious,
                        ) {
                            tracing::warn!(%error, "could not select the previous native window tab");
                        }
                        refresh_native_tabs = true;
                    }
                }
                WindowCommand::SelectTab(_, index) => {
                    #[cfg(target_os = "macos")]
                    {
                        if let Err(error) = perform_window_tab_action(
                            &state.window,
                            MacWindowTabAction::Select(index),
                        ) {
                            tracing::warn!(%error, "could not select a native window tab");
                        }
                        refresh_native_tabs = true;
                    }
                    #[cfg(not(target_os = "macos"))]
                    let _ = index;
                }
                WindowCommand::MergeAllWindows(_) => {
                    #[cfg(target_os = "macos")]
                    {
                        if let Err(error) =
                            perform_window_tab_action(&state.window, MacWindowTabAction::MergeAll)
                        {
                            tracing::warn!(%error, "could not merge native windows into tabs");
                        }
                        refresh_native_tabs = true;
                    }
                }
                WindowCommand::MoveTabToNewWindow(_) => {
                    #[cfg(target_os = "macos")]
                    {
                        if let Err(error) = perform_window_tab_action(
                            &state.window,
                            MacWindowTabAction::MoveToNewWindow,
                        ) {
                            tracing::warn!(%error, "could not move native tab to a new window");
                        }
                        refresh_native_tabs = true;
                    }
                }
                WindowCommand::ToggleTabBar(_) => {
                    #[cfg(target_os = "macos")]
                    {
                        if let Err(error) =
                            perform_window_tab_action(&state.window, MacWindowTabAction::ToggleBar)
                        {
                            tracing::warn!(%error, "could not toggle the native window tab bar");
                        }
                        refresh_native_tabs = true;
                    }
                }
                WindowCommand::ToggleTabOverview(_) => {
                    #[cfg(target_os = "macos")]
                    {
                        if let Err(error) = perform_window_tab_action(
                            &state.window,
                            MacWindowTabAction::ToggleOverview,
                        ) {
                            tracing::warn!(%error, "could not toggle the native window tab overview");
                        }
                        refresh_native_tabs = true;
                    }
                }
                WindowCommand::SetBounds(_, bounds) => {
                    apply_window_bounds(state, bounds);
                    state_changed = true;
                    force_redraw = true;
                }
                WindowCommand::BeginMove(_) => {
                    if entry.config.is_movable
                        && let Err(error) = state.window.drag_window()
                    {
                        tracing::warn!(%error, "could not start native window move");
                    }
                }
                WindowCommand::BeginResize(_, edge) => {
                    if entry.config.is_resizable
                        && let Err(error) = state.window.drag_resize_window(edge)
                    {
                        tracing::warn!(%error, "could not start native window resize");
                    }
                }
                WindowCommand::Move(_, position) => {
                    let bounds = Rect::new(
                        position.x,
                        position.y,
                        state.restore_bounds.width,
                        state.restore_bounds.height,
                    );
                    apply_window_bounds(state, WindowBounds::Windowed(bounds));
                    state_changed = true;
                }
                WindowCommand::Resize(_, size) => {
                    apply_window_size(state, size);
                    state_changed = true;
                    force_redraw = true;
                }
                WindowCommand::Minimize(_) => {
                    if entry.config.is_minimizable && state.window.is_minimized() != Some(true) {
                        state.window.set_minimized(true);
                        state_changed = true;
                    }
                }
                WindowCommand::Restore(_) => {
                    state.window.set_minimized(false);
                    apply_window_bounds(state, WindowBounds::Windowed(state.restore_bounds));
                    state_changed = true;
                    force_redraw = true;
                }
                WindowCommand::Zoom(_) => {
                    state.maximized = runtime_window_is_maximized(state, &entry.config);
                    if state.maximized {
                        apply_window_bounds(state, WindowBounds::Windowed(state.restore_bounds));
                        state_changed = true;
                        force_redraw = true;
                    } else if !runtime_window_is_fullscreen(state)
                        && entry.config.is_resizable
                        && entry.config.is_maximizable
                    {
                        let bounds = Rect::new(
                            state.logical_position.x,
                            state.logical_position.y,
                            state.logical_size.width,
                            state.logical_size.height,
                        );
                        apply_window_bounds(state, WindowBounds::Maximized(bounds));
                        state_changed = true;
                        force_redraw = true;
                    }
                }
                WindowCommand::ToggleFullscreen(_) => {
                    // Winit retains the requested state while AppKit animates between spaces.
                    // Flipping that value lets a second toggle reverse an in-flight transition
                    // instead of mistaking the still-old native style mask for the target state.
                    let fullscreen = state.window.fullscreen().is_none();
                    state_changed = set_runtime_window_fullscreen(state, fullscreen);
                    force_redraw = state_changed;
                }
                WindowCommand::SetFullscreen(_, fullscreen) => {
                    state_changed = set_runtime_window_fullscreen(state, fullscreen);
                    force_redraw = state_changed;
                }
                WindowCommand::SetVisible(_, visible) => {
                    if state.visible != visible {
                        #[cfg(target_os = "macos")]
                        if !visible
                            && window_dismisses_system_popover_on_pointer_outside(&entry.config)
                        {
                            self.popover_monitor.unwatch(handle);
                        }
                        #[cfg(target_os = "macos")]
                        if state.relation_presented && !visible {
                            if let Err(error) =
                                dismiss_window_relation(&state.window, entry.config.kind)
                            {
                                tracing::warn!(%error, "could not dismiss native window relation");
                            }
                            state.relation_presented = false;
                        }
                        #[cfg(target_os = "macos")]
                        if visible && !state.relation_presented {
                            if let Some(popover) = entry.config.popover.as_ref()
                                && let Some(parent) = parent_window.as_ref()
                                && let Err(error) =
                                    position_system_popover(&state.window, parent, popover)
                            {
                                tracing::warn!(%error, "could not restore system popover placement");
                            }
                            match present_window_relation(
                                &state.window,
                                parent_window.as_ref(),
                                entry.config.kind,
                            ) {
                                Ok(presented) => state.relation_presented = presented,
                                Err(error) => {
                                    tracing::warn!(%error, "could not present native window relation");
                                }
                            }
                        }
                        #[cfg(target_os = "macos")]
                        if visible
                            && window_dismisses_system_popover_on_pointer_outside(&entry.config)
                            && let Some(popover) = entry.config.popover.as_ref()
                            && let Some(parent) = parent_window.as_ref()
                            && let Err(error) = self.popover_monitor.watch(
                                handle,
                                &state.window,
                                parent,
                                popover.anchor_rect,
                            )
                        {
                            tracing::warn!(%error, "could not restore native popover grab");
                        }
                        #[cfg(target_os = "macos")]
                        if let Err(error) = set_window_visibility(
                            &state.window,
                            visible,
                            entry.config.focus && entry.config.focusable,
                        ) {
                            tracing::warn!(%error, "could not change native window visibility");
                        }
                        #[cfg(target_os = "macos")]
                        if visible && window_presentation_activates_application(&entry.config) {
                            state.window.focus_window();
                        }
                        #[cfg(not(target_os = "macos"))]
                        state.window.set_visible(visible);
                        state.visible = visible;
                        if visible {
                            #[cfg(target_os = "windows")]
                            {
                                // Explorer creates the taskbar button asynchronously after the
                                // HWND becomes visible. The first redraw retries both retained
                                // taskbar properties at that native boundary.
                                state.taskbar_state_applied = false;
                                state.taskbar_apply_attempts = 0;
                            }
                            #[cfg(not(target_os = "macos"))]
                            if entry.config.focus && entry.config.focusable {
                                state.window.focus_window();
                            }
                            force_redraw = true;
                        }
                        state_changed = true;
                    }
                }
                WindowCommand::SetMovable(_, movable) => {
                    if entry.config.is_movable != movable {
                        entry.config.is_movable = movable;
                        #[cfg(target_os = "macos")]
                        if let Err(error) = set_window_movable(
                            &state.window,
                            implicit_native_movable(&entry.config),
                        ) {
                            tracing::warn!(%error, "could not change native window movability");
                        }
                        state_changed = true;
                    }
                }
                WindowCommand::SetResizable(_, resizable) => {
                    if entry.config.is_resizable != resizable {
                        entry.config.is_resizable = resizable;
                        state.window.set_resizable(resizable);
                        state
                            .window
                            .set_enabled_buttons(window_buttons(&entry.config));
                        state_changed = true;
                    }
                }
                WindowCommand::SetMinimumSize(_, minimum) => {
                    if entry.config.minimum_size != minimum {
                        entry.config.minimum_size = minimum;
                        state.window.set_min_inner_size(minimum.map(|minimum| {
                            LogicalSize::new(minimum.width as f64, minimum.height as f64)
                        }));
                        if let Some(minimum) = minimum {
                            state.restore_bounds.width =
                                state.restore_bounds.width.max(minimum.width);
                            state.restore_bounds.height =
                                state.restore_bounds.height.max(minimum.height);
                            let constrained = constrained_window_size(
                                state.logical_size,
                                Some(minimum),
                                entry.config.maximum_size,
                            );
                            if constrained != state.logical_size
                                && !runtime_window_is_fullscreen(state)
                                && !runtime_window_is_maximized(state, &entry.config)
                            {
                                if let Some(physical) =
                                    state.window.request_inner_size(LogicalSize::new(
                                        constrained.width as f64,
                                        constrained.height as f64,
                                    ))
                                {
                                    state.renderer.resize(physical.width, physical.height);
                                    state.logical_size =
                                        logical_window_size(physical, state.scale_factor);
                                }
                                state.layout_dirty = true;
                                state.view_dirty |= state.listeners.observes_viewport;
                                force_redraw = true;
                            }
                        }
                        state_changed = true;
                    }
                }
                WindowCommand::SetMaximumSize(_, maximum) => {
                    let compatible = match (entry.config.minimum_size, maximum) {
                        (Some(minimum), Some(maximum)) => {
                            minimum.width <= maximum.width && minimum.height <= maximum.height
                        }
                        _ => true,
                    };
                    if !compatible {
                        tracing::warn!(
                            "ignored a maximum window size smaller than the current minimum size"
                        );
                    } else if entry.config.maximum_size != maximum {
                        entry.config.maximum_size = maximum;
                        state.window.set_max_inner_size(maximum.map(|maximum| {
                            LogicalSize::new(maximum.width as f64, maximum.height as f64)
                        }));
                        if let Some(maximum) = maximum {
                            state.restore_bounds.width =
                                state.restore_bounds.width.min(maximum.width);
                            state.restore_bounds.height =
                                state.restore_bounds.height.min(maximum.height);
                            let constrained = constrained_window_size(
                                state.logical_size,
                                entry.config.minimum_size,
                                Some(maximum),
                            );
                            if constrained != state.logical_size
                                && !runtime_window_is_fullscreen(state)
                                && !runtime_window_is_maximized(state, &entry.config)
                            {
                                if let Some(physical) =
                                    state.window.request_inner_size(LogicalSize::new(
                                        constrained.width as f64,
                                        constrained.height as f64,
                                    ))
                                {
                                    state.renderer.resize(physical.width, physical.height);
                                    state.logical_size =
                                        logical_window_size(physical, state.scale_factor);
                                }
                                state.layout_dirty = true;
                                state.view_dirty |= state.listeners.observes_viewport;
                                force_redraw = true;
                            }
                        }
                        state_changed = true;
                    }
                }
                WindowCommand::SetMinimizable(_, minimizable) => {
                    if entry.config.is_minimizable != minimizable {
                        entry.config.is_minimizable = minimizable;
                        state
                            .window
                            .set_enabled_buttons(window_buttons(&entry.config));
                        state_changed = true;
                    }
                }
                WindowCommand::SetMaximizable(_, maximizable) => {
                    if entry.config.is_maximizable != maximizable {
                        entry.config.is_maximizable = maximizable;
                        state
                            .window
                            .set_enabled_buttons(window_buttons(&entry.config));
                        state_changed = true;
                    }
                }
                WindowCommand::SetClosable(_, closable) => {
                    if entry.config.is_closable != closable {
                        entry.config.is_closable = closable;
                        state
                            .window
                            .set_enabled_buttons(window_buttons(&entry.config));
                        state_changed = true;
                    }
                }
                WindowCommand::SetDecorated(_, decorated) => {
                    if entry.config.decorated != decorated {
                        entry.config.decorated = decorated;
                        state.window.set_decorations(decorated);
                        state_changed = true;
                        force_redraw = true;
                    }
                }
                WindowCommand::SetShadow(_, shadow) => {
                    if entry.config.shadow != shadow {
                        entry.config.shadow = shadow;
                        #[cfg(target_os = "macos")]
                        state.window.set_has_shadow(shadow);
                        state_changed = true;
                    }
                }
                WindowCommand::SetContentProtected(_, protected) => {
                    if entry.config.content_protected != protected {
                        entry.config.content_protected = protected;
                        state.window.set_content_protected(protected);
                        state_changed = true;
                    }
                }
                WindowCommand::SetWindowLevel(_, level) => {
                    if entry.config.window_level != level {
                        let previous = effective_window_level(&entry.config);
                        entry.config.window_level = level;
                        let effective = effective_window_level(&entry.config);
                        state.window.set_window_level(effective.to_winit());
                        #[cfg(target_os = "macos")]
                        if let Err(error) = set_window_level(&state.window, effective) {
                            tracing::warn!(%error, "could not apply the native window level");
                        }
                        if previous != effective {
                            window_events.push(Event::WindowLevelChanged(effective));
                        }
                        state_changed = true;
                    }
                }
                WindowCommand::MoveToTop(_) => {
                    #[cfg(target_os = "macos")]
                    if let Err(error) = order_window_front(&state.window) {
                        tracing::warn!(%error, "could not raise the native window");
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        // Winit exposes no portable restack request, so the closest honest
                        // approximation on these backends is a native focus request.
                        state.window.focus_window();
                    }
                }
                WindowCommand::MoveAbove(_, _other) => {
                    #[cfg(target_os = "macos")]
                    match sibling_window {
                        Some(above) => {
                            if let Err(error) = order_window_above(&state.window, &above) {
                                tracing::warn!(%error, "could not order the native window above its sibling");
                            }
                        }
                        None => tracing::warn!(
                            "ignoring a window ordering request for an unknown sibling window"
                        ),
                    }
                    #[cfg(not(target_os = "macos"))]
                    state.window.focus_window();
                }
                WindowCommand::SetIgnoreMouseEvents(_, ignore, forward) => {
                    if entry.config.ignore_mouse_events != ignore
                        || entry.config.forward_mouse_events != forward
                    {
                        #[cfg(target_os = "macos")]
                        let applied =
                            set_window_ignores_mouse_events(&state.window, ignore, forward)
                                .map_err(|error| {
                                    tracing::warn!(%error, "could not change native mouse-event pass-through");
                                })
                                .is_ok();
                        #[cfg(not(target_os = "macos"))]
                        let applied = state
                            .window
                            .set_cursor_hittest(!ignore)
                            .map_err(|error| {
                                tracing::warn!(%error, "could not change native mouse-event pass-through");
                            })
                            .is_ok();
                        if applied {
                            entry.config.ignore_mouse_events = ignore;
                            entry.config.forward_mouse_events = forward;
                            state_changed = true;
                        }
                    }
                }
                WindowCommand::SetWindowEnabled(_, enabled) => {
                    if entry.config.window_enabled != enabled {
                        #[cfg(target_os = "macos")]
                        if let Err(error) = set_window_input_enabled(&state.window, enabled) {
                            tracing::warn!(%error, "could not change native window input policy");
                        }
                        #[cfg(target_os = "windows")]
                        if let Err(error) =
                            windows_window::set_window_input_enabled(&state.window, enabled)
                        {
                            tracing::warn!(%error, "could not change native window input policy");
                        }
                        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
                        tracing::warn!(
                            "disabling native window input is not supported by this backend"
                        );
                        entry.config.window_enabled = enabled;
                        state_changed = true;
                    }
                }
                WindowCommand::SetAspectRatio(_, ratio) => {
                    if entry.config.aspect_ratio != ratio {
                        entry.config.aspect_ratio = ratio;
                        #[cfg(target_os = "macos")]
                        if let Err(error) = set_window_aspect_ratio(&state.window, ratio) {
                            tracing::warn!(%error, "could not change the native content aspect ratio");
                        }
                        if let Some(ratio) = ratio {
                            let clamped = clamp_size_to_aspect_ratio(state.logical_size, ratio);
                            if clamped != state.logical_size {
                                let _ = state.window.request_inner_size(LogicalSize::new(
                                    f64::from(clamped.width),
                                    f64::from(clamped.height),
                                ));
                            }
                        }
                        state_changed = true;
                    }
                }
                WindowCommand::SetWindowButtonVisibility(_, visible) => {
                    if entry.config.window_buttons_visible != visible {
                        #[cfg(target_os = "macos")]
                        if let Err(error) = set_window_button_visibility(&state.window, visible) {
                            tracing::warn!(%error, "could not change native window button visibility");
                        }
                        #[cfg(not(target_os = "macos"))]
                        tracing::warn!(
                            "native window buttons can only be hidden independently on macOS"
                        );
                        entry.config.window_buttons_visible = visible;
                        state_changed = true;
                    }
                }
                WindowCommand::SetFocusable(_, focusable) => {
                    if entry.config.focusable != focusable {
                        entry.config.focusable = focusable;
                        if !focusable {
                            entry.config.focus = false;
                        }
                        if let Err(error) = set_window_focusable(&state.window, focusable) {
                            tracing::warn!(%error, "could not change native window focusability");
                        }
                        state_changed = true;
                    }
                }
                WindowCommand::SetSkipTaskbar(_, skip) => {
                    if entry.config.skip_taskbar != skip {
                        entry.config.skip_taskbar = skip;
                        #[cfg(target_os = "windows")]
                        state.window.set_skip_taskbar(skip);
                        state_changed = true;
                    }
                }
                WindowCommand::SetVisibleOnAllWorkspaces(_, visible) => {
                    if entry.config.visible_on_all_workspaces != visible {
                        entry.config.visible_on_all_workspaces = visible;
                        if let Err(error) = set_window_visible_on_all_workspaces(
                            &state.window,
                            effective_visible_on_all_workspaces(&entry.config),
                        ) {
                            tracing::warn!(%error, "could not change native workspace visibility");
                        }
                        state_changed = true;
                    }
                }
                WindowCommand::SetOpacity(_, opacity) => {
                    if entry.config.opacity != opacity {
                        entry.config.opacity = opacity;
                        if let Err(error) = set_window_opacity(&state.window, opacity) {
                            tracing::warn!(%error, "could not change native window opacity");
                        }
                        state_changed = true;
                    }
                }
                WindowCommand::SetIcon(_, icon) => {
                    if entry.config.icon.as_ref().map(Image::id) != icon.as_ref().map(Image::id) {
                        state
                            .window
                            .set_window_icon(icon.as_ref().map(winit_window_icon));
                        entry.config.icon = icon;
                        state_changed = true;
                    }
                }
                WindowCommand::SetTaskbarProgress(_, progress_state, progress) => {
                    if entry.config.taskbar_progress_state != progress_state
                        || entry.config.taskbar_progress != progress
                    {
                        #[cfg(target_os = "windows")]
                        {
                            state.taskbar_state_applied &= windows_window::set_taskbar_progress(
                                &state.window,
                                progress_state,
                                progress,
                            )
                            .map_err(|error| {
                                tracing::warn!(%error, "could not change native taskbar progress");
                            })
                            .is_ok();
                            state.taskbar_apply_attempts = 0;
                        }
                        entry.config.taskbar_progress_state = progress_state;
                        entry.config.taskbar_progress = progress;
                        state_changed = true;
                        #[cfg(target_os = "windows")]
                        {
                            force_redraw = true;
                        }
                    }
                }
                WindowCommand::SetTaskbarOverlayIcon(_, icon, description) => {
                    if entry.config.taskbar_overlay_icon.as_ref().map(Image::id)
                        != icon.as_ref().map(Image::id)
                        || entry.config.taskbar_overlay_description != description
                    {
                        #[cfg(target_os = "windows")]
                        {
                            state.taskbar_state_applied &=
                                windows_window::set_taskbar_overlay_icon(
                                    &state.window,
                                    icon.as_ref(),
                                    description.as_deref(),
                                )
                                .map_err(|error| {
                                    tracing::warn!(%error, "could not change native taskbar overlay icon");
                                })
                                .is_ok();
                            state.taskbar_apply_attempts = 0;
                        }
                        entry.config.taskbar_overlay_icon = icon;
                        entry.config.taskbar_overlay_description = description;
                        state_changed = true;
                        #[cfg(target_os = "windows")]
                        {
                            force_redraw = true;
                        }
                    }
                }
                WindowCommand::SetCursorVisible(_, visible) => {
                    if entry.config.cursor_visible != visible {
                        entry.config.cursor_visible = visible;
                        state.window.set_cursor_visible(visible);
                        state_changed = true;
                    }
                }
                WindowCommand::SetCursorOverride(_, image) => {
                    let id = image.as_ref().map(crate::CursorOverride::id);
                    if state.cursor_override.as_ref().map(|(id, _)| *id) != id {
                        if let Some(image) = image {
                            let id = image.id();
                            let native = match image {
                                crate::CursorOverride::System(style) => {
                                    Ok(winit::window::Cursor::Icon(platform_cursor(style)))
                                }
                                crate::CursorOverride::Image(image) => {
                                    let alpha = cursor_alpha(&state.window);
                                    image.native_source(alpha).map(|source| {
                                        winit::window::Cursor::Custom(
                                            event_loop.create_custom_cursor(source),
                                        )
                                    })
                                }
                            };
                            match native {
                                Ok(native) => {
                                    state.window.set_cursor(native.clone());
                                    state.cursor_override = Some((id, native));
                                    state_changed = true;
                                }
                                Err(error) => {
                                    tracing::error!(%error, "validated native cursor image could not be created")
                                }
                            }
                        } else {
                            state.cursor_override = None;
                            state.window.set_cursor(state.cursor);
                            state_changed = true;
                        }
                    }
                }
                WindowCommand::SetCursorGrab(_, mode) => {
                    if entry.config.cursor_grab != mode {
                        match state.window.set_cursor_grab(mode.to_winit()) {
                            Ok(()) => {
                                entry.config.cursor_grab = mode;
                                state_changed = true;
                            }
                            Err(error) => {
                                tracing::warn!(%error, "could not change native cursor confinement");
                            }
                        }
                    }
                }
                WindowCommand::SetCursorHitTest(_, hit_test) => {
                    if entry.config.cursor_hit_test != hit_test {
                        match state.window.set_cursor_hittest(hit_test) {
                            Ok(()) => {
                                entry.config.cursor_hit_test = hit_test;
                                state_changed = true;
                            }
                            Err(error) => {
                                tracing::warn!(%error, "could not change native pointer hit testing");
                            }
                        }
                    }
                }
                WindowCommand::SetCursorPosition(_, position) => {
                    if let Err(error) = state.window.set_cursor_position(LogicalPosition::new(
                        f64::from(position.x),
                        f64::from(position.y),
                    )) {
                        tracing::warn!(%error, "could not change native cursor position");
                    } else {
                        entry.config.cursor_position = Some(position);
                        state.pointer = Some(position);
                        state_changed = true;
                    }
                }
                WindowCommand::SetAppearance(_, preference) => {
                    if entry.config.preferred_appearance != preference {
                        entry.config.preferred_appearance = preference;
                        state.window.set_theme(preference.map(to_winit_theme));
                        let appearance = preference
                            .or_else(|| state.window.theme().map(map_window_appearance))
                            .unwrap_or(state.appearance);
                        if state.appearance != appearance {
                            state.appearance = appearance;
                            state_changed = true;
                        }
                    }
                }
                WindowCommand::SetBackgroundAppearance(_, appearance) => {
                    if entry.config.window_background != appearance {
                        let previous = entry.config.window_background;
                        #[cfg(target_os = "macos")]
                        let vibrancy_active = entry.config.macos_vibrancy.is_some();
                        #[cfg(not(target_os = "macos"))]
                        let vibrancy_active = false;
                        let previous_transparent = previous.is_transparent() || vibrancy_active;
                        let next_transparent = appearance.is_transparent() || vibrancy_active;
                        let previous_blur = previous.is_blurred() && !vibrancy_active;
                        let next_blur = appearance.is_blurred() && !vibrancy_active;
                        let transparency_changed = previous_transparent != next_transparent;
                        let surface_change = if transparency_changed {
                            state.renderer.set_transparent(next_transparent)
                        } else {
                            Ok(false)
                        };
                        match surface_change {
                            Ok(_) => {
                                if transparency_changed {
                                    state.window.set_transparent(next_transparent);
                                }
                                if previous_blur != next_blur {
                                    state.window.set_blur(next_blur);
                                }
                                entry.config.window_background = appearance;
                                state_changed = true;
                                force_redraw = true;
                            }
                            Err(error) => {
                                tracing::warn!(%error, "could not change window background appearance");
                            }
                        }
                    }
                }
                WindowCommand::SetMacOsVibrancy(_, vibrancy) => {
                    if entry.config.macos_vibrancy != vibrancy {
                        #[cfg(target_os = "macos")]
                        {
                            let previous_vibrancy = entry.config.macos_vibrancy;
                            let previous_transparent =
                                entry.config.window_background.is_transparent()
                                    || previous_vibrancy.is_some();
                            let next_transparent = entry.config.window_background.is_transparent()
                                || vibrancy.is_some();
                            let previous_blur = entry.config.window_background.is_blurred()
                                && previous_vibrancy.is_none();
                            let next_blur =
                                entry.config.window_background.is_blurred() && vibrancy.is_none();
                            let transparency_changed = previous_transparent != next_transparent;
                            let surface_change = if transparency_changed {
                                state.renderer.set_transparent(next_transparent)
                            } else {
                                Ok(false)
                            };
                            match surface_change {
                                Ok(_) => {
                                    if transparency_changed {
                                        state.window.set_transparent(next_transparent);
                                    }
                                    if previous_blur != next_blur {
                                        state.window.set_blur(next_blur);
                                    }
                                    let native_result = match vibrancy {
                                        Some(vibrancy) => {
                                            if let Some(host) = &state.vibrancy_host {
                                                host.set_vibrancy(vibrancy);
                                                Ok(())
                                            } else {
                                                MacVibrancyHost::new(
                                                    &state.window,
                                                    vibrancy,
                                                    entry.config.macos_visual_effect_state,
                                                )
                                                .map(|host| state.vibrancy_host = Some(host))
                                            }
                                        }
                                        None => {
                                            state.vibrancy_host = None;
                                            Ok(())
                                        }
                                    };
                                    match native_result {
                                        Ok(()) => {
                                            entry.config.macos_vibrancy = vibrancy;
                                            state_changed = true;
                                            force_redraw = true;
                                        }
                                        Err(error) => {
                                            if transparency_changed {
                                                if let Err(rollback_error) = state
                                                    .renderer
                                                    .set_transparent(previous_transparent)
                                                {
                                                    tracing::warn!(%rollback_error, "could not roll back the macOS vibrancy surface mode");
                                                }
                                                state.window.set_transparent(previous_transparent);
                                            }
                                            if previous_blur != next_blur {
                                                state.window.set_blur(previous_blur);
                                            }
                                            tracing::warn!(%error, "could not change macOS vibrancy");
                                        }
                                    }
                                }
                                Err(error) => {
                                    tracing::warn!(%error, "could not enable the transparent surface required by macOS vibrancy");
                                }
                            }
                        }
                        #[cfg(not(target_os = "macos"))]
                        {
                            entry.config.macos_vibrancy = vibrancy;
                            state_changed = true;
                        }
                    }
                }
                WindowCommand::SetMacOsVisualEffectState(_, effect_state) => {
                    if entry.config.macos_visual_effect_state != effect_state {
                        #[cfg(target_os = "macos")]
                        if let Some(host) = &state.vibrancy_host {
                            host.set_state(effect_state);
                        }
                        entry.config.macos_visual_effect_state = effect_state;
                        state_changed = true;
                    }
                }
                #[cfg(feature = "inspector")]
                WindowCommand::SetInspector(_, open) => {
                    if state.inspector.is_some() != open {
                        entry.config.inspector = open;
                        state.inspector = open.then(|| InspectorState::new(self.animation_epoch));
                        reconcile_inspector_pointer_state(state);
                        state_changed = true;
                        inspector_redraw = true;
                    }
                }
                #[cfg(feature = "inspector")]
                WindowCommand::ToggleInspector(_) => {
                    let open = state.inspector.is_none();
                    entry.config.inspector = open;
                    state.inspector = open.then(|| InspectorState::new(self.animation_epoch));
                    reconcile_inspector_pointer_state(state);
                    state_changed = true;
                    inspector_redraw = true;
                }
                WindowCommand::RequestAttention(_) => state
                    .window
                    .request_user_attention(Some(UserAttentionType::Informational)),
            }

            #[cfg(feature = "inspector")]
            let redraw = force_redraw
                || inspector_redraw
                || state_changed && state.listeners.observes_window_state;
            #[cfg(not(feature = "inspector"))]
            let redraw = force_redraw || state_changed && state.listeners.observes_window_state;
            if redraw {
                state.view_dirty |= force_redraw || state.listeners.observes_window_state;
                if state.visible && state.scheduler.invalidate() {
                    state.window.request_redraw();
                }
            }

            if !window_events.is_empty()
                && self.pending_window_events.len() + window_events.len()
                    <= MAX_PENDING_WINDOW_COMMANDS
            {
                self.pending_window_events
                    .extend(window_events.into_iter().map(|event| (handle, event)));
            }

            #[cfg(target_os = "macos")]
            match tabbing_ownership_delta {
                1 => self.register_native_tabbing(event_loop),
                -1 => self.unregister_native_tabbing(event_loop),
                _ => {}
            }
        }
        self.window_commands.extend(waiting);
        #[cfg(target_os = "macos")]
        if refresh_native_tabs {
            self.refresh_native_tab_states();
        }
    }
}
