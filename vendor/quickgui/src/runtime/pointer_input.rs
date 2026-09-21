use super::*;

impl Runtime {
    /// Open the dispatch scope for one input event on the active window, if there is one.
    ///
    /// The scope is what lets focus styles follow the input device like CSS `:focus-visible`:
    /// every focus change until [`Self::end_input_dispatch`] — a listener's `cx.focus`, the
    /// retained press default, Tab traversal, a roving arrow — resolves its visibility from the
    /// device recorded here instead of from a flag each of those paths would have to carry.
    pub(super) fn begin_input_dispatch(
        &mut self,
        modality: InputModality,
    ) -> Option<InputDispatchScope> {
        self.window
            .as_mut()
            .map(|window| window.ui.begin_input_dispatch(modality))
    }

    /// Close the scope from [`Self::begin_input_dispatch`]; a window the event closed needs none.
    pub(super) fn end_input_dispatch(&mut self, scope: Option<InputDispatchScope>) {
        if let (Some(scope), Some(window)) = (scope, self.window.as_mut()) {
            window.ui.end_input_dispatch(scope);
        }
    }

    /// Show the focused element's focus styles for a navigation key that may move nothing.
    pub(super) fn reveal_focus(&mut self) {
        if let Some(window) = &mut self.window
            && window.ui.reveal_focus()
            && window.scheduler.invalidate()
        {
            window.window.request_redraw();
        }
    }

    /// Move focus on the active window and repaint when only its focus styles changed.
    ///
    /// `announce_focus_change` rebuilds the view when the focused element changes; this covers a
    /// focus that stays put while its visibility flips, which is paint-only.
    pub(super) fn focus_element(&mut self, id: ElementId) {
        if let Some(window) = &mut self.window
            && window.ui.focus(id)
            && window.scheduler.invalidate()
        {
            window.window.request_redraw();
        }
    }

    pub(super) fn invoke_click(&mut self, event_loop: &ActiveEventLoop, id: ElementId) {
        let activation_target = self
            .window
            .as_ref()
            .and_then(|window| window.ui.activation_target(id));
        let form = self
            .window
            .as_ref()
            .and_then(|window| window.ui.form_for_submitter(id));
        let listener = self
            .window
            .as_ref()
            .and_then(|window| window.listeners.clicks.get(&id).cloned());
        let mut default_prevented = false;
        if let Some(listener) = listener {
            let mut cx = self.event_context();
            if let Some(window) = &mut self.window {
                listener(window.view.as_any_mut(), &mut cx);
            }
            default_prevented = cx.prevent_default;
            if !self.apply_event_context(event_loop, cx, false, true) {
                return;
            }
        }
        if !self.dispatch(event_loop, Event::Click(id), false) {
            return;
        }
        if let Some(form) = form {
            self.invoke_form_submission(event_loop, form, Some(id));
        }
        if default_prevented {
            return;
        }
        let Some(target) = activation_target.filter(|target| *target != id) else {
            return;
        };
        let previous_focus = self.window.as_ref().and_then(|window| window.ui.focused());
        self.focus_element(target);
        self.announce_focus_change(event_loop, previous_focus);
        let clickable = self
            .window
            .as_ref()
            .is_some_and(|window| window.ui.is_clickable(target));
        if clickable {
            self.invoke_click(event_loop, target);
        }
    }

    pub(super) fn invoke_context_menu(
        &mut self,
        event_loop: &ActiveEventLoop,
        event: ContextMenuEvent,
    ) -> bool {
        let listener = self
            .window
            .as_ref()
            .and_then(|window| window.listeners.context_menus.get(&event.target).cloned());
        if let Some(listener) = listener {
            let mut cx = self.event_context();
            if let Some(window) = &mut self.window {
                listener(window.view.as_any_mut(), &event, &mut cx);
            }
            if !self.apply_event_context(event_loop, cx, false, true) {
                return false;
            }
        }
        self.dispatch(event_loop, Event::ContextMenu(event), false);
        true
    }

    pub(super) fn invoke_pointer(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: ElementId,
        event: PointerEvent,
    ) -> bool {
        let (listener, bounds) = self.window.as_ref().map_or((None, None), |window| {
            (
                window.listeners.pointers.get(&id).cloned(),
                window.ui.element_bounds(id),
            )
        });
        let Some(listener) = listener else {
            return true;
        };
        let event = bounds.map_or(event, |bounds| event.localize(bounds));
        let mut cx = self.event_context();
        if let Some(window) = &mut self.window {
            listener(window.view.as_any_mut(), &event, &mut cx);
        }
        self.apply_event_context(event_loop, cx, false, true)
    }

    /// Dispatch one bounded desktop mouse event through outside capture, capture, and bubble.
    ///
    /// `None` means the callback closed the runtime. `Some(true)` means at least one listener
    /// prevented the retained default behavior.
    pub(super) fn invoke_mouse_event_at(
        &mut self,
        event_loop: &ActiveEventLoop,
        position: Point,
        kind: MouseListenerKind,
        button: Option<MouseButton>,
        event: MouseListenerEvent,
    ) -> Option<bool> {
        let (mut path, mut dispatch) = {
            let window = self.window.as_mut()?;
            (
                std::mem::take(&mut window.mouse_event_path_scratch),
                std::mem::take(&mut window.mouse_dispatch_scratch),
            )
        };
        let path_complete = {
            let window = self.window.as_ref()?;
            let complete = window.ui.mouse_event_path_at(position, &mut path);
            if complete {
                window
                    .ui
                    .collect_mouse_dispatch(&path, kind, button, &mut dispatch);
            }
            complete
        };
        if let Some(window) = &mut self.window {
            path.clear();
            window.mouse_event_path_scratch = path;
        }
        if !path_complete {
            tracing::warn!(
                limit = crate::MAX_MOUSE_EVENT_PATH,
                "ignored a targeted mouse event whose retained ancestor path exceeded the safety bound"
            );
            dispatch.clear();
        }

        let mut default_prevented = false;
        for key in dispatch.iter().copied() {
            let listener = self
                .window
                .as_ref()
                .and_then(|window| window.listeners.mouse_listener(key));
            let Some(listener) = listener else {
                continue;
            };
            let mut cx = self.event_context();
            if let Some(window) = &mut self.window {
                listener(window.view.as_any_mut(), &event, &mut cx);
            }
            let stop_propagation = cx.stop_event_propagation;
            default_prevented |= cx.prevent_default;
            if !self.apply_event_context(event_loop, cx, false, true) {
                return None;
            }
            if stop_propagation {
                break;
            }
        }
        if let Some(window) = &mut self.window {
            dispatch.clear();
            window.mouse_dispatch_scratch = dispatch;
        }
        Some(default_prevented)
    }

    /// Dispatch one raw key event through the retained root-to-focus capture path and reverse
    /// bubble path. `None` means a callback exited the runtime; otherwise the result reports
    /// whether any listener prevented QuickGUI's key-down default behavior.
    pub(super) fn invoke_key_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        event: KeyListenerEvent,
    ) -> Option<bool> {
        let Some(window) = &mut self.window else {
            return Some(false);
        };
        let path = window.ui.focus_path();
        let mut dispatch = std::mem::take(&mut window.key_dispatch_scratch);
        window
            .ui
            .collect_key_dispatch(&path, event.kind(), &mut dispatch);

        let mut default_prevented = false;
        for binding in dispatch.iter().copied() {
            let listener = self
                .window
                .as_ref()
                .and_then(|window| window.listeners.key_listener(binding.key));
            let Some(listener) = listener else {
                continue;
            };
            let mut cx = self.event_context();
            if let Some(window) = &mut self.window {
                listener(window.view.as_any_mut(), &event, &mut cx);
            }
            let stop_propagation = cx.stop_event_propagation;
            default_prevented |= cx.prevent_default;
            if !self.apply_event_context(event_loop, cx, false, true) {
                return None;
            }
            if stop_propagation {
                break;
            }
        }

        dispatch.clear();
        if let Some(window) = &mut self.window {
            window.key_dispatch_scratch = dispatch;
        }
        Some(default_prevented)
    }

    pub(super) fn invoke_pending_mouse_hover(&mut self, event_loop: &ActiveEventLoop) -> bool {
        let mut changes = {
            let Some(window) = &mut self.window else {
                return false;
            };
            let mut changes = std::mem::take(&mut window.mouse_hover_changes_scratch);
            window.ui.take_mouse_hover_changes(&mut changes);
            changes
        };
        for change in changes.iter().copied() {
            let listener = self
                .window
                .as_ref()
                .and_then(|window| window.listeners.mouse_listener(change.key));
            let Some(listener) = listener else {
                continue;
            };
            let mut cx = self.event_context();
            if let Some(window) = &mut self.window {
                listener(
                    window.view.as_any_mut(),
                    &MouseListenerEvent::Hover(change.hovered),
                    &mut cx,
                );
            }
            if !self.apply_event_context(event_loop, cx, false, true) {
                return false;
            }
        }
        if let Some(window) = &mut self.window {
            changes.clear();
            window.mouse_hover_changes_scratch = changes;
        }
        true
    }

    /// Dispatch one wheel event from the topmost listener through listening ancestors.
    ///
    /// `None` means event processing closed the runtime. `Some(true)` means at least one listener
    /// prevented retained default scrolling.
    pub(super) fn invoke_scroll_wheel(
        &mut self,
        event_loop: &ActiveEventLoop,
        target: ElementId,
        event: ScrollWheelEvent,
    ) -> Option<bool> {
        let mut current = Some(target);
        let mut default_prevented = false;
        while let Some(id) = current {
            let listener = self
                .window
                .as_ref()
                .and_then(|window| window.listeners.scroll_wheels.get(&id).cloned());
            if let Some(listener) = listener {
                let mut cx = self.event_context();
                if let Some(window) = &mut self.window {
                    listener(window.view.as_any_mut(), &event, &mut cx);
                }
                let stop_propagation = cx.stop_event_propagation;
                default_prevented |= cx.prevent_default;
                if !self.apply_event_context(event_loop, cx, false, true) {
                    return None;
                }
                if stop_propagation {
                    break;
                }
            }
            current = self
                .window
                .as_ref()
                .and_then(|window| window.ui.parent_scroll_wheel_listener(id));
        }
        Some(default_prevented)
    }

    pub(super) fn invoke_touch(
        &mut self,
        event_loop: &ActiveEventLoop,
        target: Option<ElementId>,
        event: TouchEvent,
    ) -> bool {
        let mut current = target;
        while let Some(id) = current {
            let listener = self
                .window
                .as_ref()
                .and_then(|window| window.listeners.touches.get(&id).cloned());
            if let Some(listener) = listener {
                let mut cx = self.event_context();
                if let Some(window) = &mut self.window {
                    listener(window.view.as_any_mut(), &event, &mut cx);
                }
                let stop_propagation = cx.stop_event_propagation;
                if !self.apply_event_context(event_loop, cx, false, true) {
                    return false;
                }
                if stop_propagation {
                    break;
                }
            }
            current = self
                .window
                .as_ref()
                .and_then(|window| window.ui.parent_touch_listener(id));
        }
        self.dispatch(event_loop, Event::Touch(event), false)
    }

    pub(super) fn invoke_mouse_pressure(
        &mut self,
        event_loop: &ActiveEventLoop,
        target: Option<ElementId>,
        event: MousePressureEvent,
    ) -> bool {
        let listener = target.and_then(|target| {
            self.window
                .as_ref()
                .and_then(|window| window.listeners.mouse_pressures.get(&target).cloned())
        });
        if let Some(listener) = listener {
            let mut cx = self.event_context();
            if let Some(window) = &mut self.window {
                listener(window.view.as_any_mut(), &event, &mut cx);
            }
            if !self.apply_event_context(event_loop, cx, false, true) {
                return false;
            }
        }
        self.dispatch(event_loop, Event::MousePressure(event), false)
    }

    pub(super) fn invoke_pinch(
        &mut self,
        event_loop: &ActiveEventLoop,
        target: Option<ElementId>,
        event: PinchEvent,
    ) -> bool {
        let listener = target.and_then(|target| {
            self.window
                .as_ref()
                .and_then(|window| window.listeners.pinches.get(&target).cloned())
        });
        if let Some(listener) = listener {
            let mut cx = self.event_context();
            if let Some(window) = &mut self.window {
                listener(window.view.as_any_mut(), &event, &mut cx);
            }
            if !self.apply_event_context(event_loop, cx, false, true) {
                return false;
            }
        }
        self.dispatch(event_loop, Event::Pinch(event), false)
    }

    pub(super) fn invoke_rotation(
        &mut self,
        event_loop: &ActiveEventLoop,
        target: Option<ElementId>,
        event: RotationEvent,
    ) -> bool {
        let listener = target.and_then(|target| {
            self.window
                .as_ref()
                .and_then(|window| window.listeners.rotations.get(&target).cloned())
        });
        if let Some(listener) = listener {
            let mut cx = self.event_context();
            if let Some(window) = &mut self.window {
                listener(window.view.as_any_mut(), &event, &mut cx);
            }
            if !self.apply_event_context(event_loop, cx, false, true) {
                return false;
            }
        }
        self.dispatch(event_loop, Event::Rotation(event), false)
    }

    pub(super) fn invoke_smart_magnify(
        &mut self,
        event_loop: &ActiveEventLoop,
        target: Option<ElementId>,
        event: SmartMagnifyEvent,
    ) -> bool {
        let listener = target.and_then(|target| {
            self.window
                .as_ref()
                .and_then(|window| window.listeners.smart_magnifies.get(&target).cloned())
        });
        if let Some(listener) = listener {
            let mut cx = self.event_context();
            if let Some(window) = &mut self.window {
                listener(window.view.as_any_mut(), &event, &mut cx);
            }
            if !self.apply_event_context(event_loop, cx, false, true) {
                return false;
            }
        }
        self.dispatch(event_loop, Event::SmartMagnify(event), false)
    }

    pub(super) fn invoke_drag_start(
        &mut self,
        event_loop: &ActiveEventLoop,
        source: ElementId,
        event: DragStartEvent,
    ) -> bool {
        let listener = self
            .window
            .as_ref()
            .and_then(|window| window.listeners.drag_sources.get(&source).cloned());
        let Some((registered_type, listener)) = listener else {
            if let Some(window) = &mut self.window {
                window.drag_candidate = None;
            }
            return true;
        };
        let mut cx = self.event_context();
        let drag = {
            let Some(window) = &mut self.window else {
                return false;
            };
            listener(window.view.as_any_mut(), &event, &mut cx)
        };
        debug_assert_eq!(drag.value_type, registered_type);
        if !self.apply_event_context(event_loop, cx, false, true) {
            return false;
        }
        let Some(window) = &mut self.window else {
            return false;
        };
        let mut preview = drag.preview;
        if let Some(preview) = &mut preview {
            window.image_assets.resolve_tree(preview);
        }
        window.drag_candidate = None;
        window.drag_session = Some(DragSession {
            source,
            position: event.position,
            value: drag.value,
            value_type: drag.value_type,
            external_payload: drag.external_payload,
        });
        let changed = window.ui.begin_drag(source);
        // Preview installation is independent of the retained application view and therefore does
        // not make each drag move rebuild `View::render`.
        let preview_changed = match window.ui.set_drag_preview(
            preview,
            source,
            event.origin,
            event.position,
            drag.cursor_offset,
            &mut window.renderer,
            Instant::now(),
        ) {
            Ok(changed) => changed,
            Err(error) => {
                self.fail(event_loop, AppError::View(error.to_string()));
                return false;
            }
        };
        if (changed || preview_changed) && window.scheduler.invalidate() {
            window.window.request_redraw();
        }
        true
    }

    pub(super) fn invoke_drop(
        &mut self,
        event_loop: &ActiveEventLoop,
        target: ElementId,
        value_type: TypeId,
        value: &dyn Any,
        event: DropEvent,
    ) -> bool {
        let listener = self
            .window
            .as_ref()
            .and_then(|window| window.listeners.drops.get(&(target, value_type)).cloned());
        let Some(listener) = listener else {
            return true;
        };
        let mut cx = self.event_context();
        if let Some(window) = &mut self.window {
            listener(window.view.as_any_mut(), value, &event, &mut cx);
        }
        self.apply_event_context(event_loop, cx, false, true)
    }

    pub(super) fn compatible_drop_target(
        &self,
        point: Point,
        value_type: TypeId,
        value: &dyn Any,
    ) -> Option<ElementId> {
        let window = self.window.as_ref()?;
        window.ui.drop_target_at(point, |id| {
            window.listeners.drops.contains_key(&(id, value_type))
                && window.ui.can_drop(id, value_type, value)
        })
    }

    pub(super) fn update_drag_target(&mut self, point: Point) -> bool {
        let active = self.window.as_ref().and_then(|window| {
            if let Some(drag) = &window.drag_session {
                return Some((drag.value_type, Arc::clone(&drag.value)));
            }
            let files = window.native_file_drag.as_ref()?.hover_value.as_ref()?;
            let files: Arc<DroppedFiles> = Arc::clone(files);
            let value: Arc<dyn Any> = files;
            Some((TypeId::of::<DroppedFiles>(), value))
        });
        let Some((value_type, value)) = active else {
            return false;
        };
        let target = self.compatible_drop_target(point, value_type, value.as_ref());
        self.window
            .as_mut()
            .is_some_and(|window| window.ui.set_drag_over(target))
    }

    pub(super) fn finish_internal_drag(
        &mut self,
        event_loop: &ActiveEventLoop,
        position: Point,
    ) -> bool {
        let target = self
            .window
            .as_ref()
            .and_then(|window| window.drag_session.as_ref())
            .and_then(|drag| {
                self.compatible_drop_target(position, drag.value_type, drag.value.as_ref())
            });
        let Some(window) = &mut self.window else {
            return false;
        };
        let Some(drag) = window.drag_session.take() else {
            return false;
        };
        #[cfg(target_os = "macos")]
        if let Some(monitor) = &window.external_drag_monitor {
            monitor.disarm();
        }
        window.drag_candidate = None;
        let repaint = window.ui.end_drag() | window.ui.clear_drag_preview();
        set_cursor_if_changed(window, CursorIcon::Default);
        if repaint && window.scheduler.invalidate() {
            window.window.request_redraw();
        }
        if let Some(target) = target {
            return self.invoke_drop(
                event_loop,
                target,
                drag.value_type,
                drag.value.as_ref(),
                DropEvent {
                    position,
                    modifiers: self.modifiers,
                    origin: DragOrigin::Internal(drag.source),
                },
            );
        }
        true
    }

    pub(super) fn cancel_internal_drag(&mut self) -> bool {
        let Some(window) = &mut self.window else {
            return false;
        };
        let had_drag = window.drag_session.take().is_some();
        let had_candidate = window.drag_candidate.take().is_some();
        #[cfg(target_os = "macos")]
        if let Some(monitor) = &window.external_drag_monitor {
            monitor.disarm();
        }
        let repaint = window.ui.end_drag() | window.ui.clear_drag_preview();
        set_cursor_if_changed(window, CursorIcon::Default);
        if repaint && window.scheduler.invalidate() {
            window.window.request_redraw();
        }
        had_drag || had_candidate
    }

    #[cfg(target_os = "macos")]
    pub(super) fn arm_external_drag_monitor(&mut self) {
        let Some(handle) = self.current_handle() else {
            return;
        };
        let proxy = self.event_proxy.clone();
        let Some(window) = &mut self.window else {
            return;
        };
        if window.external_drag_monitor.is_none() {
            match MacExternalDragMonitor::new(&window.window, handle, proxy) {
                Ok(monitor) => window.external_drag_monitor = Some(monitor),
                Err(error) => {
                    tracing::warn!(%error, "could not install the AppKit drag boundary monitor");
                }
            }
        }
        if let Some(monitor) = &window.external_drag_monitor {
            monitor.arm();
        }
    }

    #[cfg(target_os = "macos")]
    pub(super) fn promote_external_drag_at_boundary(
        &mut self,
        event_loop: &ActiveEventLoop,
        point: Point,
    ) -> Option<bool> {
        if let Some(state) = &mut self.window {
            state.pointer = Some(point);
        }
        let boundary_start = self.window.as_ref().and_then(|state| {
            let candidate = state.drag_candidate?;
            if state.drag_session.is_some() || state.pointer_capture.is_some() {
                return None;
            }
            let delta = point - candidate.origin;
            (delta.x.abs().max(delta.y.abs()) >= DRAG_THRESHOLD)
                .then_some((candidate.source, candidate.origin))
        });
        if let Some((source, origin)) = boundary_start
            && !self.invoke_drag_start(
                event_loop,
                source,
                DragStartEvent {
                    origin,
                    position: point,
                    modifiers: self.modifiers,
                },
            )
        {
            return None;
        }
        let promoted = self.promote_external_drag();
        if promoted {
            if let Some(state) = &mut self.window {
                state.pointer = None;
            }
            self.dispatch(event_loop, Event::PointerLeft, false);
        }
        Some(promoted)
    }

    #[cfg(target_os = "macos")]
    pub(super) fn promote_external_drag(&mut self) -> bool {
        let Some(handle) = self.current_handle() else {
            return false;
        };
        let typed_registry = self.native_drag_registry.clone();
        let proxy = self.event_proxy.clone();
        let Some(state) = &mut self.window else {
            return false;
        };
        let Some(drag) = &mut state.drag_session else {
            return false;
        };
        let Some(mouse_down) = state.external_drag_mouse_down.clone() else {
            if let Some(monitor) = &state.external_drag_monitor {
                monitor.disarm();
            }
            tracing::warn!(
                "could not promote the internal drag because AppKit did not expose its mouse-down event"
            );
            return false;
        };
        let payload = drag.external_payload.take();
        let typed_payload = MacTypedDragPayload::new(
            Arc::clone(&drag.value),
            drag.value_type,
            handle,
            drag.source,
        );
        let window = Arc::clone(&state.window);

        let native = match start_external_drag(
            &window,
            &mouse_down,
            payload.as_ref(),
            typed_payload,
            &typed_registry,
            handle,
            proxy,
        ) {
            Ok(native) => native,
            Err(error) => {
                if let Some(drag) = self
                    .window
                    .as_mut()
                    .and_then(|window| window.drag_session.as_mut())
                {
                    drag.external_payload = payload;
                }
                if let Some(monitor) = self
                    .window
                    .as_ref()
                    .and_then(|window| window.external_drag_monitor.as_ref())
                {
                    monitor.disarm();
                }
                tracing::warn!(%error, "could not promote the internal drag to AppKit");
                return false;
            }
        };

        let Some(state) = &mut self.window else {
            return false;
        };
        let Some(drag) = state.drag_session.take() else {
            return false;
        };
        state.drag_candidate = None;
        state.external_drag_mouse_down = None;
        if let Some(monitor) = &state.external_drag_monitor {
            monitor.disarm();
        }
        state.suppress_external_drag_release = true;
        state.outbound_external_drag = Some(OutboundExternalDrag {
            source: drag.source,
            _session: native,
        });
        let repaint = state.ui.end_drag()
            | state.ui.clear_drag_preview()
            | state.ui.pointer_left()
            | state.ui.set_drag_over(None);
        set_cursor_if_changed(state, CursorIcon::Default);
        if repaint && state.scheduler.invalidate() {
            state.window.request_redraw();
        }
        true
    }

    pub(super) fn refresh_native_file_pointer(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(window) = &mut self.window
            && let Some(point) = current_pointer_position(&window.window)
        {
            window.pointer = Some(point);
        }
    }

    #[cfg(target_os = "macos")]
    pub(super) fn compatible_native_offer(
        &self,
        point: Point,
        offer: &MacNativeDropOffer,
    ) -> Option<(ElementId, usize)> {
        let window = self.window.as_ref()?;
        window
            .ui
            .drop_offer_target_at(point, offer.iter(), |id, value_type, value| {
                window.listeners.drops.contains_key(&(id, value_type))
                    && window.ui.can_drop(id, value_type, value)
            })
    }

    #[cfg(target_os = "macos")]
    pub(super) fn hover_native_offer(&mut self, offer: MacNativeDropOffer, point: Point) -> bool {
        // A platform-owned drag supersedes any unpromoted local pointer gesture.
        if self
            .window
            .as_ref()
            .is_none_or(|window| window.native_external_drag.is_none())
        {
            self.cancel_internal_drag();
        }
        let target = self
            .compatible_native_offer(point, &offer)
            .map(|(target, _)| target);
        let Some(window) = &mut self.window else {
            return false;
        };
        window.pointer = Some(point);
        window.native_external_drag = Some(offer);
        let repaint = window.ui.begin_external_drag() | window.ui.set_drag_over(target);
        if repaint && window.scheduler.invalidate() {
            window.window.request_redraw();
        }
        true
    }

    #[cfg(target_os = "macos")]
    pub(super) fn drop_native_offer(
        &mut self,
        event_loop: &ActiveEventLoop,
        offer: MacNativeDropOffer,
        point: Point,
    ) -> bool {
        let destination = self.current_handle();
        let target = self.compatible_native_offer(point, &offer);
        let Some(window) = &mut self.window else {
            return false;
        };
        window.pointer = Some(point);
        window.native_external_drag = None;
        let repaint = window.ui.end_drag();
        if repaint && window.scheduler.invalidate() {
            window.window.request_redraw();
        }
        if let Some((target, payload_index)) = target
            && let Some(payload) = offer.payload(payload_index)
        {
            let origin = native_drop_origin(destination, payload);
            return self.invoke_drop(
                event_loop,
                target,
                payload.value_type(),
                payload.value(),
                DropEvent {
                    position: point,
                    modifiers: self.modifiers,
                    origin,
                },
            );
        }
        true
    }

    #[cfg(target_os = "macos")]
    pub(super) fn cancel_native_payload(&mut self) -> bool {
        let Some(window) = &mut self.window else {
            return false;
        };
        if window.native_external_drag.take().is_none() {
            return true;
        }
        window.pointer = None;
        let repaint = window.ui.end_drag();
        if repaint && window.scheduler.invalidate() {
            window.window.request_redraw();
        }
        true
    }

    #[cfg(target_os = "macos")]
    pub(super) fn handle_native_drop_pending(
        &mut self,
        event_loop: &ActiveEventLoop,
        pending: MacNativeDropPending,
    ) -> bool {
        match pending {
            MacNativeDropPending::Hover { offer, point } => self.hover_native_offer(offer, point),
            MacNativeDropPending::Exit => self.cancel_native_payload(),
            MacNativeDropPending::Drop { offer, point } => {
                self.drop_native_offer(event_loop, offer, point)
            }
        }
    }

    pub(super) fn hover_native_file(&mut self, path: PathBuf) -> bool {
        // A platform-owned drag supersedes a local pointer gesture.
        self.cancel_internal_drag();
        {
            let Some(window) = &mut self.window else {
                return false;
            };
            let drag = window.native_file_drag.get_or_insert_with(Default::default);
            drag.hover(path);
            let repaint = window.ui.begin_external_drag();
            if repaint && window.scheduler.invalidate() {
                window.window.request_redraw();
            }
        }
        true
    }

    pub(super) fn flush_native_file_hover(&mut self, event_loop: &ActiveEventLoop) -> bool {
        let files = self
            .window
            .as_mut()
            .and_then(|window| window.native_file_drag.as_mut())
            .and_then(NativeFileDrag::take_hovered_files);
        let Some(files) = files else {
            return true;
        };
        if let Some(window) = &mut self.window
            && let Some(drag) = &mut window.native_file_drag
        {
            drag.hover_value = Some(Arc::new(files.clone()));
        }
        if let Some(point) = self.window.as_ref().and_then(|window| window.pointer) {
            let changed = self.update_drag_target(point);
            if changed
                && let Some(window) = &mut self.window
                && window.scheduler.invalidate()
            {
                window.window.request_redraw();
            }
        }
        self.dispatch(event_loop, Event::FilesHovered(files), false)
    }

    pub(super) fn drop_native_file(&mut self, event_loop: &ActiveEventLoop, path: PathBuf) -> bool {
        if !self.flush_native_file_hover(event_loop) {
            return false;
        }
        let complete = {
            let Some(window) = &mut self.window else {
                return false;
            };
            window
                .native_file_drag
                .get_or_insert_with(Default::default)
                .drop_path(path)
        };
        if !complete {
            return true;
        }
        let position = self
            .window
            .as_ref()
            .and_then(|window| window.pointer)
            .unwrap_or(Point::ZERO);
        let files = {
            let Some(window) = &mut self.window else {
                return false;
            };
            let files = window
                .native_file_drag
                .take()
                .expect("native file drag was created above")
                .into_dropped_files();
            let repaint = window.ui.end_drag();
            if repaint && window.scheduler.invalidate() {
                window.window.request_redraw();
            }
            files
        };
        let target = self.compatible_drop_target(position, TypeId::of::<DroppedFiles>(), &files);
        if let Some(target) = target
            && !self.invoke_drop(
                event_loop,
                target,
                TypeId::of::<DroppedFiles>(),
                &files,
                DropEvent {
                    position,
                    modifiers: self.modifiers,
                    origin: DragOrigin::External,
                },
            )
        {
            return false;
        }
        self.dispatch(event_loop, Event::FilesDropped(files), false)
    }

    pub(super) fn cancel_native_file_hover(&mut self, event_loop: &ActiveEventLoop) -> bool {
        if !self.flush_native_file_hover(event_loop) {
            return false;
        }
        let Some(window) = &mut self.window else {
            return false;
        };
        if window.native_file_drag.take().is_none() {
            return true;
        }
        let repaint = window.ui.end_drag();
        if repaint && window.scheduler.invalidate() {
            window.window.request_redraw();
        }
        self.dispatch(event_loop, Event::FilesHoverCancelled, false)
    }
}
