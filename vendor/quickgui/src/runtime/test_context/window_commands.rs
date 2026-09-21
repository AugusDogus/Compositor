use super::*;

impl TestAppContext {
    pub(super) fn apply_window_command(
        &mut self,
        command: WindowCommand,
    ) -> Result<(), TestAppError> {
        let window = command.handle();
        #[cfg(feature = "inspector")]
        let animation_epoch = self.animation_epoch;
        let updates_display = matches!(
            &command,
            WindowCommand::SetBounds(_, _)
                | WindowCommand::Move(_, _)
                | WindowCommand::Resize(_, _)
                | WindowCommand::Restore(_)
        );
        let displays = self.displays.clone();
        let Some(state) = self.windows.get_mut(&window) else {
            return Ok(());
        };
        let previous = state.state;
        let mut window_level_changed = None;
        match command {
            WindowCommand::SetTitle(_, title) => state.config.title = title,
            WindowCommand::SetRepresentedFile(_, represented_file) => {
                state.config.represented_file = represented_file;
                state.state.represented_file = state.config.represented_file.is_some();
            }
            WindowCommand::SetDocumentEdited(_, edited) => {
                state.config.document_edited = edited;
                state.state.document_edited = edited;
            }
            WindowCommand::ShowCharacterPalette(_) => {}
            WindowCommand::LookUpSelection(_) => {
                // The installed provider decides whether a definition can be shown headlessly.
                let _ = state.ui.input_look_up_selection();
            }
            WindowCommand::SetTabbingIdentifier(_, identifier) => {
                state.config.tabbing_identifier = identifier;
                state.state.native_tabbing = state.config.tabbing_identifier.is_some();
                if !state.state.native_tabbing {
                    state.state.native_tabs = WindowTabState::default();
                }
            }
            WindowCommand::SelectNextTab(_) => {
                if let Some(selected) = state.state.native_tabs.selected_index {
                    state.state.native_tabs.selected_index =
                        Some(selected.saturating_add(1) % state.state.native_tabs.count.max(1));
                }
            }
            WindowCommand::SelectPreviousTab(_) => {
                if let Some(selected) = state.state.native_tabs.selected_index {
                    state.state.native_tabs.selected_index = Some(if selected == 0 {
                        state.state.native_tabs.count.saturating_sub(1)
                    } else {
                        selected - 1
                    });
                }
            }
            WindowCommand::SelectTab(_, index) => {
                if index < state.state.native_tabs.count {
                    state.state.native_tabs.selected_index = Some(index);
                }
            }
            WindowCommand::MergeAllWindows(_) => {}
            WindowCommand::MoveTabToNewWindow(_) => {
                state.state.native_tabs = WindowTabState::default();
            }
            WindowCommand::ToggleTabBar(_) => {
                state.state.native_tabs.tab_bar_visible = !state.state.native_tabs.tab_bar_visible;
            }
            WindowCommand::ToggleTabOverview(_) => {
                state.state.native_tabs.overview_visible =
                    !state.state.native_tabs.overview_visible;
            }
            WindowCommand::SetBounds(_, bounds) => set_test_window_bounds(state, bounds),
            // Interactive geometry comes back through native move/resize events.
            WindowCommand::BeginMove(_) | WindowCommand::BeginResize(_, _) => {}
            WindowCommand::Move(_, position) => {
                let bounds = state.state.bounds.bounds();
                set_test_window_bounds(
                    state,
                    WindowBounds::Windowed(Rect::new(
                        position.x,
                        position.y,
                        bounds.width,
                        bounds.height,
                    )),
                );
            }
            WindowCommand::Resize(_, size) => {
                let bounds = state.state.bounds.bounds();
                set_test_window_bounds(
                    state,
                    WindowBounds::Windowed(Rect::new(bounds.x, bounds.y, size.width, size.height)),
                );
            }
            WindowCommand::Minimize(_) => {
                if state.state.minimizable {
                    state.state.minimized = true;
                }
            }
            WindowCommand::Restore(_) => {
                state.state.minimized = false;
                let bounds = state.state.bounds.bounds();
                set_test_window_bounds(state, WindowBounds::Windowed(bounds));
            }
            WindowCommand::Zoom(_) => {
                if state.state.maximized || state.state.resizable && state.state.maximizable {
                    let bounds = state.state.bounds.bounds();
                    set_test_window_bounds(
                        state,
                        if state.state.maximized {
                            WindowBounds::Windowed(bounds)
                        } else {
                            WindowBounds::Maximized(bounds)
                        },
                    );
                }
            }
            WindowCommand::ToggleFullscreen(_) => {
                let bounds = state.state.bounds.bounds();
                set_test_window_bounds(
                    state,
                    if state.state.fullscreen {
                        WindowBounds::Windowed(bounds)
                    } else {
                        WindowBounds::Fullscreen(bounds)
                    },
                );
            }
            WindowCommand::SetFullscreen(_, fullscreen) => {
                let bounds = state.state.bounds.bounds();
                set_test_window_bounds(
                    state,
                    if fullscreen {
                        WindowBounds::Fullscreen(bounds)
                    } else {
                        WindowBounds::Windowed(bounds)
                    },
                );
            }
            WindowCommand::SetVisible(_, visible) => state.state.visible = visible,
            WindowCommand::SetMovable(_, movable) => state.state.movable = movable,
            WindowCommand::SetResizable(_, resizable) => state.state.resizable = resizable,
            WindowCommand::SetMinimumSize(_, minimum) => {
                let compatible = match (minimum, state.config.maximum_size) {
                    (Some(minimum), Some(maximum)) => {
                        minimum.width <= maximum.width && minimum.height <= maximum.height
                    }
                    _ => true,
                };
                if compatible && state.config.minimum_size != minimum {
                    state.config.minimum_size = minimum;
                    state.state.minimum_size = minimum;
                }
            }
            WindowCommand::SetMaximumSize(_, maximum) => {
                let compatible = match (state.config.minimum_size, maximum) {
                    (Some(minimum), Some(maximum)) => {
                        minimum.width <= maximum.width && minimum.height <= maximum.height
                    }
                    _ => true,
                };
                if compatible && state.config.maximum_size != maximum {
                    state.config.maximum_size = maximum;
                    state.state.maximum_size = maximum;
                }
            }
            WindowCommand::SetMinimizable(_, minimizable) => {
                state.config.is_minimizable = minimizable;
                state.state.minimizable = minimizable;
            }
            WindowCommand::SetMaximizable(_, maximizable) => {
                state.config.is_maximizable = maximizable;
                state.state.maximizable = maximizable;
            }
            WindowCommand::SetClosable(_, closable) => {
                state.config.is_closable = closable;
                state.state.closable = closable;
            }
            WindowCommand::SetDecorated(_, decorated) => {
                state.config.decorated = decorated;
                state.state.decorated = decorated;
            }
            WindowCommand::SetShadow(_, shadow) => {
                state.config.shadow = shadow;
                state.state.shadow = shadow;
            }
            WindowCommand::SetContentProtected(_, protected) => {
                state.config.content_protected = protected;
                state.state.content_protected = protected;
            }
            WindowCommand::SetWindowLevel(_, level) => {
                let previous = state.state.window_level;
                state.config.window_level = level;
                state.state.window_level = effective_window_level(&state.config);
                if previous != state.state.window_level {
                    window_level_changed = Some(state.state.window_level);
                }
            }
            WindowCommand::MoveToTop(_) | WindowCommand::MoveAbove(_, _) => {}
            WindowCommand::SetIgnoreMouseEvents(_, ignore, forward) => {
                state.config.ignore_mouse_events = ignore;
                state.config.forward_mouse_events = forward;
                state.state.ignore_mouse_events = ignore;
                state.state.forward_mouse_events = forward;
            }
            WindowCommand::SetWindowEnabled(_, enabled) => {
                state.config.window_enabled = enabled;
                state.state.window_enabled = enabled;
            }
            WindowCommand::SetAspectRatio(_, ratio) => {
                state.config.aspect_ratio = ratio;
                state.state.aspect_ratio = ratio;
                if let Some(ratio) = ratio {
                    let bounds = state.state.bounds.bounds();
                    let clamped =
                        clamp_size_to_aspect_ratio(Size::new(bounds.width, bounds.height), ratio);
                    if clamped != Size::new(bounds.width, bounds.height) {
                        set_test_window_bounds(
                            state,
                            WindowBounds::Windowed(Rect::new(
                                bounds.x,
                                bounds.y,
                                clamped.width,
                                clamped.height,
                            )),
                        );
                    }
                }
            }
            WindowCommand::SetWindowButtonVisibility(_, visible) => {
                state.config.window_buttons_visible = visible;
                state.state.window_buttons_visible = visible;
            }
            WindowCommand::SetFocusable(_, focusable) => {
                state.config.focusable = focusable;
                state.state.focusable = focusable;
                if !focusable {
                    state.config.focus = false;
                    state.state.focused = false;
                }
            }
            WindowCommand::SetSkipTaskbar(_, skip) => {
                state.config.skip_taskbar = skip;
                state.state.skip_taskbar = skip;
            }
            WindowCommand::SetVisibleOnAllWorkspaces(_, visible) => {
                state.config.visible_on_all_workspaces = visible;
                state.state.visible_on_all_workspaces =
                    effective_visible_on_all_workspaces(&state.config);
            }
            WindowCommand::SetOpacity(_, opacity) => {
                state.config.opacity = opacity;
                state.state.opacity = opacity;
            }
            WindowCommand::SetIcon(_, icon) => {
                state.state.has_icon = icon.is_some();
                state.config.icon = icon;
            }
            WindowCommand::SetTaskbarProgress(_, progress_state, progress) => {
                state.config.taskbar_progress_state = progress_state;
                state.config.taskbar_progress = progress;
                state.state.taskbar_progress_state = progress_state;
                state.state.taskbar_progress = progress;
            }
            WindowCommand::SetTaskbarOverlayIcon(_, icon, description) => {
                state.state.has_taskbar_overlay_icon = icon.is_some();
                state.config.taskbar_overlay_icon = icon;
                state.config.taskbar_overlay_description = description;
            }
            WindowCommand::SetCursorVisible(_, visible) => {
                state.config.cursor_visible = visible;
                state.state.cursor_visible = visible;
            }
            WindowCommand::SetCursorOverride(_, image) => {
                state.state.cursor_override = image.as_ref().map(crate::CursorOverride::id);
            }
            WindowCommand::SetCursorGrab(_, mode) => {
                state.config.cursor_grab = mode;
                state.state.cursor_grab = mode;
            }
            WindowCommand::SetCursorHitTest(_, hit_test) => {
                state.config.cursor_hit_test = hit_test;
                state.state.cursor_hit_test = hit_test;
            }
            WindowCommand::SetCursorPosition(_, position) => {
                state.config.cursor_position = Some(position);
                state.state.cursor_position = Some(position);
            }
            WindowCommand::SetAppearance(_, preference) => {
                state.config.preferred_appearance = preference;
                state.state.appearance = preference.unwrap_or(state.system_appearance);
            }
            WindowCommand::SetBackgroundAppearance(_, appearance) => {
                if state.config.window_background != appearance {
                    state.config.window_background = appearance;
                    state.state.background_appearance = appearance;
                    state.dirty = true;
                }
            }
            WindowCommand::SetMacOsVibrancy(_, vibrancy) => {
                if state.config.macos_vibrancy != vibrancy {
                    state.config.macos_vibrancy = vibrancy;
                    state.state.macos_vibrancy = vibrancy;
                    state.dirty = true;
                }
            }
            WindowCommand::SetMacOsVisualEffectState(_, effect_state) => {
                if state.config.macos_visual_effect_state != effect_state {
                    state.config.macos_visual_effect_state = effect_state;
                    state.state.macos_visual_effect_state = effect_state;
                    state.dirty = true;
                }
            }
            #[cfg(feature = "inspector")]
            WindowCommand::SetInspector(_, open) => {
                if state.inspector.is_some() != open {
                    state.config.inspector = open;
                    state.state.inspector_active = open;
                    state.inspector = open.then(|| InspectorState::new(animation_epoch));
                    state.retained_geometry_ready = false;
                }
            }
            #[cfg(feature = "inspector")]
            WindowCommand::ToggleInspector(_) => {
                let open = state.inspector.is_none();
                state.config.inspector = open;
                state.state.inspector_active = open;
                state.inspector = open.then(|| InspectorState::new(animation_epoch));
                state.retained_geometry_ready = false;
            }
            WindowCommand::RequestAttention(_) => {}
        }
        if updates_display {
            state.state.display_id =
                crate::display::display_for_rect(&displays, state.state.bounds.bounds());
        }
        if state.state != previous && state.listeners.observes_window_state {
            state.dirty = true;
        }
        if let Some(level) = window_level_changed {
            self.queue_dispatch(TestDispatch::Event(
                window,
                Event::WindowLevelChanged(level),
            ))?;
        }
        Ok(())
    }
}
