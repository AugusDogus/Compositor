use super::*;

impl Runtime {
    pub(super) fn create_window(&mut self, event_loop: &ActiveEventLoop, request: WindowRequest) {
        let WindowRequest {
            handle,
            view,
            options,
            parent,
            popover_anchor_element,
            #[cfg(all(target_os = "macos", feature = "swift-ui"))]
            embedded,
        } = request;
        if let Err(error) = validate_window_options(&options) {
            self.fail(event_loop, AppError::Window(error.to_string()));
            return;
        }
        self.config = options;
        self.pending_input = None;
        self.modifiers = Modifiers::default();

        let requested_bounds = self.config.window_bounds;
        let selected_display_id = self
            .config
            .display_id
            .filter(|id| self.displays.find(*id).is_some())
            .or_else(|| self.displays.primary_id());
        let selected_display = selected_display_id
            .and_then(|id| self.displays.find(id))
            .cloned();
        let selected_monitor = selected_display_id
            .and_then(|id| crate::display::native_monitor(event_loop, id))
            .or_else(|| event_loop.primary_monitor());
        let constrained_size = constrained_window_size(
            self.config.size,
            self.config.minimum_size,
            self.config.maximum_size,
        );
        let requested_rect = requested_bounds
            .map(WindowBounds::bounds)
            .unwrap_or_else(|| {
                if self.config.display_id.is_some() {
                    selected_display.as_ref().map_or_else(
                        || Rect::from_size(constrained_size),
                        |display| display.centered_bounds(constrained_size),
                    )
                } else {
                    Rect::from_size(constrained_size)
                }
            });
        let constrained_size = constrained_window_size(
            Size::new(requested_rect.width, requested_rect.height),
            self.config.minimum_size,
            self.config.maximum_size,
        );
        let restore_rect = Rect::new(
            requested_rect.x,
            requested_rect.y,
            constrained_size.width,
            constrained_size.height,
        );
        let parent_window = parent
            .and_then(|parent| self.window_handles.get(&parent).copied())
            .and_then(|window_id| self.windows.get(&window_id))
            .map(|entry| entry.state.window.clone());
        if self.config.kind == WindowKind::SystemPopover && parent_window.is_none() {
            self.fail(
                event_loop,
                AppError::Window(WindowCommandError::PopoverParentRequired.to_string()),
            );
            return;
        }

        #[cfg(target_os = "macos")]
        if self.config.tabbing_identifier.is_some() {
            // Enable AppKit's process-wide automatic tabbing only while at least one QuickGUI
            // window explicitly opts in. Every other window is configured as Disallowed below.
            self.register_native_tabbing(event_loop);
        }

        let mut attributes = Window::default_attributes()
            .with_title(self.config.title.clone())
            .with_visible(false)
            .with_resizable(self.config.is_resizable)
            .with_decorations(self.config.decorated)
            .with_transparent(self.config.uses_transparent_surface())
            .with_blur(self.config.uses_legacy_background_blur())
            .with_content_protected(self.config.content_protected)
            .with_theme(self.config.preferred_appearance.map(to_winit_theme))
            .with_enabled_buttons(window_buttons(&self.config))
            .with_window_level(effective_window_level(&self.config).to_winit())
            .with_window_icon(self.config.icon.as_ref().map(winit_window_icon))
            .with_inner_size(LogicalSize::new(
                restore_rect.width as f64,
                restore_rect.height as f64,
            ));
        #[cfg(target_os = "linux")]
        if let Some(info) = &self.app_info {
            // Winit shares this name between the Wayland app ID and X11 WM_CLASS.
            // Set it before mapping so desktop launchers and window rules can match.
            use winit::platform::wayland::WindowAttributesExtWayland;
            attributes = attributes.with_name(info.identifier(), info.name());
        }
        #[cfg(target_arch = "wasm32")]
        {
            use winit::platform::web::WindowAttributesExtWebSys;
            attributes = attributes
                .with_canvas(self.web_canvas.take())
                .with_append(true);
        }
        #[cfg(target_os = "windows")]
        {
            attributes = attributes.with_skip_taskbar(self.config.skip_taskbar);
        }
        if (self.config.window_bounds.is_some() || self.config.display_id.is_some())
            && self.config.kind != WindowKind::SystemPopover
        {
            attributes = attributes.with_position(LogicalPosition::new(
                restore_rect.x as f64,
                restore_rect.y as f64,
            ));
        }
        match requested_bounds {
            // Maximization is applied after the hidden concrete window is placed. AppKit can
            // otherwise constrain the initializer frame to the main screen before maximizing.
            Some(WindowBounds::Maximized(_)) => {}
            Some(WindowBounds::Fullscreen(_)) => {
                attributes = attributes
                    .with_fullscreen(Some(Fullscreen::Borderless(selected_monitor.clone())))
            }
            Some(WindowBounds::Windowed(_)) | None => {}
        }
        #[cfg(target_os = "macos")]
        match self.config.title_bar_style {
            TitleBarStyle::Default => {}
            TitleBarStyle::HiddenInset => {
                attributes = attributes
                    .with_titlebar_transparent(true)
                    .with_title_hidden(true)
                    .with_fullsize_content_view(true);
            }
            TitleBarStyle::Hidden => {
                attributes = attributes
                    .with_titlebar_transparent(true)
                    .with_title_hidden(true)
                    .with_titlebar_hidden(true)
                    .with_titlebar_buttons_hidden(true)
                    .with_fullsize_content_view(true);
            }
        }
        #[cfg(target_os = "macos")]
        if let Some(identifier) = self.config.tabbing_identifier.as_deref() {
            attributes = attributes.with_tabbing_identifier(identifier);
        }
        #[cfg(target_os = "macos")]
        if matches!(
            self.config.kind,
            WindowKind::Popover | WindowKind::SystemPopover
        ) {
            attributes = attributes.with_panel(true);
        }
        #[cfg(target_os = "macos")]
        if self.config.kind == WindowKind::SystemPopover {
            attributes = attributes
                .with_titlebar_transparent(true)
                .with_title_hidden(true)
                .with_titlebar_hidden(true)
                .with_titlebar_buttons_hidden(true)
                .with_fullsize_content_view(true);
        }
        #[cfg(not(target_os = "macos"))]
        if let (Some(popover), Some(parent)) =
            (self.config.popover.as_ref(), parent_window.as_ref())
        {
            let local = crate::popover::unconstrained_popover_rect(
                popover.anchor_rect,
                Size::new(restore_rect.width, restore_rect.height),
                popover,
            );
            if let Ok(parent_position) = parent.inner_position() {
                let scale = sane_scale_factor(parent.scale_factor());
                attributes = attributes.with_position(PhysicalPosition::new(
                    parent_position.x + (local.x * scale).round() as i32,
                    parent_position.y + (local.y * scale).round() as i32,
                ));
            }
            if let Ok(parent_handle) = parent.window_handle() {
                #[cfg(target_os = "linux")]
                let x11 = matches!(
                    parent_handle.as_raw(),
                    winit::raw_window_handle::RawWindowHandle::Xlib(_)
                        | winit::raw_window_handle::RawWindowHandle::Xcb(_)
                );
                #[cfg(not(target_os = "linux"))]
                let x11 = false;
                if !x11 {
                    // SAFETY: `parent_window` retains the referenced native window through creation,
                    // and RuntimeWindow retains the parent handle for the complete child lifetime.
                    attributes =
                        unsafe { attributes.with_parent_window(Some(parent_handle.as_raw())) };
                }
                #[cfg(target_os = "linux")]
                if x11 {
                    // X11 parent_window embeds a child in the owner's client area. A native
                    // popup needs its own focus and screen coordinates, outside that hierarchy.
                    use winit::platform::x11::{WindowAttributesExtX11, WindowType};
                    attributes = attributes.with_x11_window_type(vec![WindowType::PopupMenu]);
                }
            }
            attributes = attributes.with_decorations(false);
        }
        if let Some(minimum) = self.config.minimum_size {
            attributes = attributes.with_min_inner_size(LogicalSize::new(
                minimum.width as f64,
                minimum.height as f64,
            ));
        }
        if let Some(maximum) = self.config.maximum_size {
            attributes = attributes.with_max_inner_size(LogicalSize::new(
                maximum.width as f64,
                maximum.height as f64,
            ));
        }
        #[cfg(target_os = "macos")]
        {
            attributes = attributes.with_has_shadow(self.config.shadow);
        }
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                self.fail(event_loop, AppError::Window(error.to_string()));
                return;
            }
        };
        // AppKit may constrain an initializer-created window to the main screen before Winit has a
        // concrete `NSScreen` for a windowed request. Re-apply explicit global placement while the
        // window is still hidden so a selected secondary display is authoritative on first frame.
        if (self.config.window_bounds.is_some() || self.config.display_id.is_some())
            && self.config.kind != WindowKind::SystemPopover
            && !matches!(requested_bounds, Some(WindowBounds::Fullscreen(_)))
        {
            window.set_outer_position(LogicalPosition::new(
                restore_rect.x as f64,
                restore_rect.y as f64,
            ));
        }
        if matches!(requested_bounds, Some(WindowBounds::Maximized(_))) {
            window.set_maximized(true);
        }
        let window_id = window.id();
        window.set_ime_allowed(false);
        window.set_cursor_visible(self.config.cursor_visible);
        if let Err(error) = window.set_cursor_grab(self.config.cursor_grab.to_winit()) {
            tracing::warn!(%error, "could not apply initial cursor confinement");
        }
        if let Err(error) = window.set_cursor_hittest(self.config.cursor_hit_test) {
            tracing::warn!(%error, "could not apply initial native pointer hit testing");
        }
        if let Some(position) = self.config.cursor_position
            && let Err(error) = window.set_cursor_position(LogicalPosition::new(
                f64::from(position.x),
                f64::from(position.y),
            ))
        {
            tracing::warn!(%error, "could not apply initial native cursor position");
        }
        let appearance = self
            .config
            .preferred_appearance
            .or_else(|| window.theme().map(map_window_appearance))
            .or_else(|| event_loop.system_theme().map(map_window_appearance))
            .unwrap_or_default();
        #[cfg(target_os = "macos")]
        if let Err(error) = configure_window_kind(
            &window,
            self.config.kind,
            self.config.focus,
            self.config
                .popover
                .as_ref()
                .is_none_or(|popover| popover.accepts_key_focus),
        ) {
            self.fail(event_loop, AppError::Platform(error));
            return;
        }
        if let Err(error) = set_window_focusable(&window, self.config.focusable) {
            tracing::warn!(%error, "could not apply native window focusability");
        }
        if let Err(error) = set_window_opacity(&window, self.config.opacity) {
            tracing::warn!(%error, "could not apply native window opacity");
        }
        if let Err(error) = set_window_visible_on_all_workspaces(
            &window,
            effective_visible_on_all_workspaces(&self.config),
        ) {
            tracing::warn!(%error, "could not apply native workspace visibility");
        }
        if self.config.window_level.is_some() {
            window.set_window_level(effective_window_level(&self.config).to_winit());
        }
        #[cfg(target_os = "macos")]
        if (self.config.represented_file.is_some()
            || self.config.document_edited
            || self.config.tabbing_identifier.is_some())
            && let Err(error) = configure_document_window(
                &window,
                self.config.represented_file.as_deref(),
                self.config.document_edited,
                self.config.tabbing_identifier.as_deref(),
            )
        {
            self.fail(event_loop, AppError::Platform(error));
            return;
        }
        #[cfg(target_os = "macos")]
        if self.config.represented_file.is_none()
            && !self.config.document_edited
            && self.config.tabbing_identifier.is_none()
            && let Err(error) = set_window_tabbing_identifier(&window, None)
        {
            self.fail(event_loop, AppError::Platform(error));
            return;
        }
        #[cfg(target_os = "macos")]
        if let Some(popover) = self.config.popover.as_ref()
            && let Err(error) = position_system_popover(
                &window,
                parent_window
                    .as_ref()
                    .expect("system popover parent checked above"),
                popover,
            )
        {
            self.fail(event_loop, AppError::Platform(error));
            return;
        }
        #[cfg(target_os = "macos")]
        if self.menu_host.is_none() {
            let menus = self.config.window_menus.as_deref().unwrap_or(&self.menus);
            self.menu_actions = collect_menu_actions(menus);
            self.menu_host = match MacMenuHost::new(menus, self.event_proxy.clone()) {
                Ok(host) => Some(host),
                Err(error) => {
                    self.fail(event_loop, AppError::Platform(error));
                    return;
                }
            };
        }
        #[cfg(target_os = "windows")]
        let window_menu_host = {
            if self.windows_menu_host.is_none() && !self.menus.is_empty() {
                self.windows_menu_host =
                    match windows_menu::WindowsMenuHost::new(&self.menus, self.event_proxy.clone())
                    {
                        Ok(host) => Some(host),
                        Err(error) => {
                            self.fail(event_loop, AppError::Platform(error));
                            return;
                        }
                    };
            }
            if let Some(menus) = self.config.window_menus.as_deref() {
                let host = if menus.is_empty() {
                    None
                } else {
                    match windows_menu::WindowsMenuHost::new(menus, self.event_proxy.clone()) {
                        Ok(host) => Some(host),
                        Err(error) => {
                            self.fail(event_loop, AppError::Platform(error));
                            return;
                        }
                    }
                };
                if let Some(host) = &host
                    && let Err(error) = host.attach(&window)
                {
                    self.fail(event_loop, AppError::Platform(error));
                    return;
                }
                host
            } else {
                if let Some(host) = &self.windows_menu_host
                    && let Err(error) = host.attach(&window)
                {
                    self.fail(event_loop, AppError::Platform(error));
                    return;
                }
                None
            }
        };
        let accessibility = AccessibilityAdapter::with_event_loop_proxy(
            event_loop,
            &window,
            self.event_proxy.clone(),
        );
        let profile = self.config.performance_profile;
        let shared_gpu = self.gpu_contexts.get(&profile).cloned();
        let renderer = match pollster::block_on(GpuRenderer::new(
            window.clone(),
            event_loop,
            profile,
            self.config.uses_transparent_surface(),
            self.font_system.clone(),
            shared_gpu.as_ref(),
        )) {
            Ok(renderer) => renderer,
            Err(error) => {
                self.fail(
                    event_loop,
                    AppError::GraphicsInitialization(error.to_string()),
                );
                return;
            }
        };
        self.gpu_contexts
            .entry(profile)
            .or_insert_with(|| renderer.context());
        #[cfg(target_os = "macos")]
        if let Err(error) = configure_gpu_window_resize(&window) {
            self.fail(event_loop, AppError::Platform(error));
            return;
        }
        #[cfg(target_os = "macos")]
        if let Err(error) = set_window_movable(&window, implicit_native_movable(&self.config)) {
            self.fail(event_loop, AppError::Platform(error));
            return;
        }
        #[cfg(target_os = "macos")]
        let vibrancy_host = match self.config.macos_vibrancy {
            Some(vibrancy) => {
                match MacVibrancyHost::new(&window, vibrancy, self.config.macos_visual_effect_state)
                {
                    Ok(host) => Some(host),
                    Err(error) => {
                        self.fail(event_loop, AppError::Platform(error));
                        return;
                    }
                }
            }
            None => None,
        };
        #[cfg(target_os = "macos")]
        let traffic_light_host = match self.config.traffic_light_position {
            Some(position) => match MacTrafficLightHost::new(&window, position) {
                Ok(host) => Some(host),
                Err(error) => {
                    self.fail(event_loop, AppError::Platform(error));
                    return;
                }
            },
            None => None,
        };
        #[cfg(target_os = "macos")]
        let native_drop_host = match MacNativeDropHost::new(
            &window,
            handle,
            self.event_proxy.clone(),
            self.native_drag_registry.clone(),
        ) {
            Ok(host) => host,
            Err(error) => {
                self.fail(event_loop, AppError::Platform(error));
                return;
            }
        };
        #[cfg(target_os = "macos")]
        let first_frame_guard = match MacFirstFrameGuard::new(&window, self.config.background) {
            Ok(guard) => Some(guard),
            Err(error) => {
                self.fail(event_loop, AppError::Platform(error));
                return;
            }
        };
        let scale_factor = sane_scale_factor(window.scale_factor());
        let physical = window.inner_size();
        let logical_size = logical_window_size(physical, scale_factor);
        let logical_position = logical_window_position(&window, scale_factor)
            .unwrap_or_else(|| Point::new(restore_rect.x, restore_rect.y));
        let display_id = crate::display::display_for_rect(
            &self.displays,
            Rect::new(
                logical_position.x,
                logical_position.y,
                logical_size.width,
                logical_size.height,
            ),
        )
        .or_else(|| {
            window
                .current_monitor()
                .map(|monitor| crate::display::native_display_id(&monitor))
                .filter(|id| self.displays.find(*id).is_some())
        });
        let restore_bounds = requested_bounds.map_or_else(
            || {
                Rect::new(
                    logical_position.x,
                    logical_position.y,
                    logical_size.width,
                    logical_size.height,
                )
            },
            WindowBounds::bounds,
        );
        let maximized = matches!(requested_bounds, Some(WindowBounds::Maximized(_)));
        #[cfg(target_os = "macos")]
        let native_tabs = if self.config.tabbing_identifier.is_some() {
            window_tab_state(&window).unwrap_or_else(|error| {
                tracing::warn!(%error, "could not read initial native window tab state");
                WindowTabState::default()
            })
        } else {
            WindowTabState::default()
        };
        #[cfg(not(target_os = "macos"))]
        let native_tabs = WindowTabState::default();
        let mut scheduler = FrameScheduler::default();
        scheduler.invalidate();
        let reduce_motion = self.config.reduce_motion
            || self
                .system_preferences
                .reduce_motion()
                .is_some_and(|enabled| enabled);
        let mut ui = UiTree::new_at(self.animation_epoch);
        ui.set_reduce_motion(reduce_motion);
        ui.set_animations_enabled(!reduce_motion, Instant::now());
        // A never-key popover leaves focus in its owner window. Restoring its anchor on close
        // would undo legitimate owner-side focus movement such as normal Tab traversal.
        let restore_focus_on_close = popover_anchor_element.filter(|_| {
            self.config
                .popover
                .as_ref()
                .is_some_and(|popover| popover.accepts_key_focus)
        });
        self.current_window = Some((window_id, handle));
        self.window_handles.insert(handle, window_id);
        if self.active_window.is_none()
            && self.config.show
            && self.config.focus
            && self.config.focusable
        {
            self.note_window_focused(window_id);
        }
        self.window = Some(RuntimeWindow {
            parent,
            restore_focus_on_close,
            #[cfg(all(target_os = "macos", feature = "swift-ui"))]
            embedded,
            view,
            renderer,
            image_assets: ImageAssetCache::new(handle, self.image_workers.clone()),
            #[cfg(target_os = "macos")]
            native_host: None,
            #[cfg(target_os = "macos")]
            vibrancy_host,
            #[cfg(target_os = "macos")]
            _traffic_light_host: traffic_light_host,
            #[cfg(target_os = "macos")]
            native_drop_host,
            #[cfg(target_os = "macos")]
            first_frame_guard,
            #[cfg(target_os = "windows")]
            window_menu_host,
            #[cfg(target_os = "windows")]
            taskbar_state_applied: false,
            #[cfg(target_os = "windows")]
            taskbar_apply_attempts: 0,
            ui,
            #[cfg(feature = "inspector")]
            inspector: self
                .config
                .inspector
                .then(|| InspectorState::new(self.animation_epoch)),
            scheduler,
            scene: Scene::new(),
            metrics: MetricsTracker::default(),
            scale_factor,
            logical_size,
            logical_position,
            display_id,
            appearance,
            native_tabs,
            restore_bounds,
            maximized,
            pointer: None,
            pointer_capture: None,
            pressed_mouse_buttons: PressedMouseButtons::default(),
            mouse_clicks: MouseClickTracker::default(),
            mouse_event_path_scratch: Vec::with_capacity(16),
            mouse_dispatch_scratch: Vec::with_capacity(16),
            mouse_hover_changes_scratch: Vec::with_capacity(8),
            key_dispatch_scratch: Vec::with_capacity(16),
            action_dispatch_scratch: Vec::with_capacity(16),
            touch_captures: HashMap::with_capacity(8),
            drag_candidate: None,
            drag_session: None,
            native_file_drag: None,
            #[cfg(target_os = "macos")]
            native_external_drag: None,
            #[cfg(target_os = "macos")]
            external_drag_mouse_down: None,
            #[cfg(target_os = "macos")]
            external_drag_monitor: None,
            #[cfg(target_os = "macos")]
            outbound_external_drag: None,
            #[cfg(target_os = "macos")]
            suppress_external_drag_release: false,
            cursor: CursorIcon::Default,
            cursor_override: None,
            ime_target: None,
            pending_focus: None,
            occluded: false,
            minimized: false,
            fullscreen: matches!(requested_bounds, Some(WindowBounds::Fullscreen(_))),
            first_presented: false,
            resize_correction: None,
            pressure_stage: 0,
            move_correction: None,
            focused: false,
            visible: false,
            relation_presented: false,
            reduce_motion,
            view_dirty: true,
            layout_dirty: true,
            view_deadline: None,
            accessibility_updates: AccessibilityUpdateSchedule::default(),
            listeners: ListenerRegistry::default(),
            accessibility,
            window,
        });
        self.dispatch(
            event_loop,
            Event::Resized {
                logical_size,
                scale_factor,
            },
            false,
        );

        #[cfg(target_os = "macos")]
        {
            // Populate layout, text, scene, and native composition while the window is hidden.
            // This attached preparation pass stops at the expected surface-occlusion boundary.
            self.redraw(event_loop);
            if self.fatal_error.is_some() {
                self.deactivate_window();
                return;
            }
            let detached = match self
                .window
                .as_ref()
                .and_then(|state| state.first_frame_guard.as_ref())
                .map(|guard| guard.detach_content_for_first_present())
                .transpose()
            {
                Ok(detached) => detached,
                Err(error) => {
                    self.fail(event_loop, AppError::Platform(error));
                    self.deactivate_window();
                    return;
                }
            };

            // With the content detached, WGPU can acquire the actual CAMetalLayer drawable even
            // though the NSWindow remains hidden. The renderer completes that real surface frame
            // and removes the shield before RAII reattaches the unchanged content view.
            self.redraw(event_loop);
            drop(detached);
            if self.fatal_error.is_some() {
                self.deactivate_window();
                return;
            }
            if self
                .window
                .as_ref()
                .is_some_and(|state| state.first_frame_guard.is_some())
            {
                self.fail(
                    event_loop,
                    AppError::Render(
                        "the hidden Metal surface did not present its first frame".to_owned(),
                    ),
                );
                self.deactivate_window();
                return;
            }

            #[cfg(feature = "swift-ui")]
            if let Some(embedded) = self
                .window
                .as_ref()
                .and_then(|state| state.embedded.clone())
            {
                let native_view = match self
                    .window
                    .as_ref()
                    .map(|state| crate::macos::appkit_view(&state.window))
                    .transpose()
                {
                    Ok(Some(view)) => crate::MacNativeView::new(&view),
                    Ok(None) => {
                        self.fail(
                            event_loop,
                            AppError::Platform(
                                "an embedded QuickGUI surface lost its native window".to_owned(),
                            ),
                        );
                        self.deactivate_window();
                        return;
                    }
                    Err(error) => {
                        self.fail(event_loop, AppError::Platform(error));
                        self.deactivate_window();
                        return;
                    }
                };
                if let Some(state) = self.window.as_mut() {
                    state.window.set_embedded_view(true);
                    state.visible = true;
                    if state.scheduler.invalidate() {
                        // Reparenting the Winit view moves its CAMetalLayer out of the hidden
                        // backing window. The prepared frame is not guaranteed to survive that
                        // AppKit hierarchy change, so schedule a real surface present in the
                        // native host instead of only leaving the retained frame marked dirty.
                        state.window.request_redraw();
                    }
                }
                embedded.install(native_view);
                if !self.invalidate_requests.contains(&embedded.owner) {
                    self.invalidate_requests.push(embedded.owner);
                }
            }

            if self.config.kind != WindowKind::SystemPopover {
                // GPU initialization gives AppKit and the Dock a complete launch turn while this
                // window remains hidden. Re-read the bounded snapshot now so every ordinary
                // window uses the work area that exists at its actual presentation boundary.
                // Explicit global bounds remain authoritative; only `.display(id)` automatic
                // centering is reconciled, before a single pixel can become visible.
                self.refresh_displays(event_loop);
                if requested_bounds.is_none()
                    && self.config.display_id.is_some()
                    && let Some(target) = self
                        .config
                        .display_id
                        .filter(|id| self.displays.find(*id).is_some())
                        .or_else(|| self.displays.primary_id())
                        .and_then(|id| self.displays.find(id))
                        .cloned()
                    && let Some(state) = self.window.as_mut()
                {
                    let centered = target.centered_bounds(state.logical_size);
                    state.window.set_outer_position(LogicalPosition::new(
                        centered.x as f64,
                        centered.y as f64,
                    ));
                    state.logical_position = Point::new(centered.x, centered.y);
                    state.restore_bounds.x = centered.x;
                    state.restore_bounds.y = centered.y;
                    state.display_id = Some(target.id());
                }
            }
        }

        if self.config.show {
            #[cfg(target_os = "macos")]
            if let Some(popover) = self.config.popover.as_ref()
                && let Some(state) = self.window.as_ref()
                && let Err(error) = position_system_popover(
                    &state.window,
                    parent_window
                        .as_ref()
                        .expect("system popover parent checked above"),
                    popover,
                )
            {
                self.fail(event_loop, AppError::Platform(error));
                self.deactivate_window();
                return;
            }
            #[cfg(target_os = "macos")]
            let relation_presented = match self.window.as_ref() {
                Some(state) => match present_window_relation(
                    &state.window,
                    parent_window.as_ref(),
                    self.config.kind,
                ) {
                    Ok(presented) => presented,
                    Err(error) => {
                        self.fail(event_loop, AppError::Platform(error));
                        self.deactivate_window();
                        return;
                    }
                },
                None => false,
            };
            #[cfg(not(target_os = "macos"))]
            let relation_presented = false;
            #[cfg(target_os = "macos")]
            if window_dismisses_system_popover_on_pointer_outside(&self.config)
                && let Some(popover) = self.config.popover.as_ref()
                && let Some(parent) = parent_window.as_ref()
                && let Some(state) = self.window.as_ref()
                && let Err(error) =
                    self.popover_monitor
                        .watch(handle, &state.window, parent, popover.anchor_rect)
            {
                self.fail(event_loop, AppError::Platform(error));
                self.deactivate_window();
                return;
            }
            #[cfg(target_os = "macos")]
            if let Some(state) = self.window.as_ref()
                && let Err(error) = set_window_visibility(
                    &state.window,
                    true,
                    self.config.focus && self.config.focusable,
                )
            {
                self.popover_monitor.unwatch(handle);
                self.fail(event_loop, AppError::Platform(error));
                self.deactivate_window();
                return;
            }
            #[cfg(target_os = "macos")]
            if window_presentation_activates_application(&self.config)
                && let Some(state) = self.window.as_ref()
            {
                // A windowless AppRunner completes AppKit launch before an embedding runtime
                // queues its first window. Winit's launch-time activation pass therefore cannot
                // promote that window, so use its ordinary focus path after native presentation.
                state.window.focus_window();
            }
            if let Some(state) = &mut self.window {
                state.relation_presented = relation_presented;
                state.visible = true;
                #[cfg(not(target_os = "macos"))]
                state.window.set_visible(true);
                #[cfg(not(target_os = "macos"))]
                if self.config.focus && self.config.focusable {
                    state.window.focus_window();
                }
                state.scheduler.invalidate();
                state.window.request_redraw();
            }
        }
        self.opened_window = true;
        self.last_window_quit_prevented = false;
        self.deactivate_window();
    }

    pub(super) fn process_foreground_tasks(&mut self, event_loop: &ActiveEventLoop) {
        debug_assert!(self.current_window.is_none());
        debug_assert!(self.window.is_none());

        let mut batch = self.foreground_tasks.take_ready_batch();
        while let Some(ScheduledForegroundTask {
            task,
            window: owner,
            runnable,
        }) = batch.pop_front()
        {
            if !self.foreground_tasks.owns(task, owner) {
                drop(runnable);
                continue;
            }
            if let Some(owner) = owner {
                let Some(window_id) = self.window_handles.get(&owner).copied() else {
                    self.foreground_tasks.cancel_task(task);
                    drop(runnable);
                    continue;
                };
                if !self.activate_window(window_id) {
                    self.foreground_tasks.cancel_task(task);
                    drop(runnable);
                    continue;
                }
            }

            let poll = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| runnable.run()));
            if poll.is_err() {
                tracing::error!(?task, ?owner, "foreground task panicked and was cancelled");
                self.foreground_tasks.cancel_task(task);
            }

            let mut continue_running = true;
            let mut updates = self.foreground_tasks.take_updates(task);
            debug_assert!(owner.is_some() || updates.is_empty());
            while let Some(update) = updates.pop_front() {
                let mut cx = self.event_context();
                if let Some(window) = &mut self.window {
                    update(window.view.as_any_mut(), &mut cx);
                }
                if !self.apply_event_context(event_loop, cx, false, true) {
                    continue_running = false;
                    break;
                }
            }

            self.deactivate_window();
            self.process_window_commands(event_loop);
            if !continue_running || self.fatal_error.is_some() {
                return;
            }
        }
    }
}
