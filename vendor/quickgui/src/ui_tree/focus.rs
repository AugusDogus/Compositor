use super::*;

impl UiTree {
    pub fn focused(&self) -> Option<ElementId> {
        self.focused
    }

    pub(crate) fn is_focusable(&self, id: ElementId) -> bool {
        self.focusable_ids.contains(&id)
    }

    pub(super) fn nearest_focusable_ancestor(&self, id: ElementId) -> Option<ElementId> {
        let mut current = Some(id);
        for _ in 0..MAX_FOCUSED_EVENT_PATH {
            let id = current?;
            if self.focusable_ids.contains(&id) {
                return Some(id);
            }
            current = self.parents.get(&id).copied();
        }
        None
    }

    /// Return the retained root-to-focus element path used for scoped command dispatch.
    pub fn focus_path(&self) -> Vec<ElementId> {
        let Some(root) = self.root.as_ref().map(|root| root.runtime_id) else {
            return Vec::new();
        };
        let Some(mut current) = self.focused else {
            return vec![root];
        };
        let mut path = Vec::with_capacity(8);
        path.push(current);
        while let Some(parent) = self.parents.get(&current).copied() {
            if path.len() == MAX_FOCUSED_EVENT_PATH {
                return vec![root];
            }
            path.push(parent);
            current = parent;
        }
        path.reverse();
        if path.first().copied() != Some(root) {
            vec![root]
        } else {
            path
        }
    }

    /// Collect root-to-focus capture and focus-to-root bubble key listeners.
    pub(crate) fn collect_key_dispatch(
        &self,
        path: &[ElementId],
        kind: KeyListenerKind,
        output: &mut Vec<KeyListenerBinding>,
    ) {
        output.clear();
        for id in path.iter().copied() {
            self.extend_key_dispatch_for(id, kind, DispatchPhase::Capture, output);
        }
        for id in path.iter().rev().copied() {
            self.extend_key_dispatch_for(id, kind, DispatchPhase::Bubble, output);
        }
    }

    pub(super) fn extend_key_dispatch_for(
        &self,
        id: ElementId,
        kind: KeyListenerKind,
        phase: DispatchPhase,
        output: &mut Vec<KeyListenerBinding>,
    ) {
        let Some(range) = self.key_listener_ranges.get(&id).cloned() else {
            return;
        };
        output.extend(
            self.key_listener_bindings[range]
                .iter()
                .copied()
                .filter(|binding| binding.kind == kind && binding.phase == phase),
        );
    }

    /// Collect typed action capture listeners before bubble listeners for the focused path.
    pub(crate) fn collect_action_dispatch(
        &self,
        path: &[ElementId],
        action_type: TypeId,
        output: &mut Vec<ActionListenerBinding>,
    ) {
        output.clear();
        for id in path.iter().copied() {
            self.extend_action_dispatch_for(id, action_type, DispatchPhase::Capture, output);
        }
        for id in path.iter().rev().copied() {
            self.extend_action_dispatch_for(id, action_type, DispatchPhase::Bubble, output);
        }
    }

    pub(super) fn extend_action_dispatch_for(
        &self,
        id: ElementId,
        action_type: TypeId,
        phase: DispatchPhase,
        output: &mut Vec<ActionListenerBinding>,
    ) {
        let Some(range) = self.action_listener_ranges.get(&id).cloned() else {
            return;
        };
        output.extend(
            self.action_listener_bindings[range]
                .iter()
                .copied()
                .filter(|binding| binding.action_type == action_type && binding.phase == phase),
        );
    }

    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
    pub(crate) fn action_available(&self, path: &[ElementId], action_type: TypeId) -> bool {
        path.iter().copied().any(|id| {
            self.action_listener_ranges.get(&id).is_some_and(|range| {
                self.action_listener_bindings[range.clone()]
                    .iter()
                    .any(|binding| binding.action_type == action_type)
            })
        })
    }

    pub fn key_context_stack(&self) -> Vec<KeyContext> {
        self.focus_path()
            .into_iter()
            .filter_map(|id| self.key_contexts.get(&id).cloned())
            .collect()
    }

    /// Whether the focused element paints its focus styles, like CSS `:focus-visible`.
    pub fn focus_visible(&self) -> bool {
        self.focus_visible
    }

    /// The element whose focus styles paint this frame.
    ///
    /// Focus styles follow focus visibility, except on a text input or text area: a native text
    /// field always shows its focus ring whichever device focused it, so an editor is exempt from
    /// the pointer rule. Carets, cursors, and accessibility keep following the real focus.
    pub(super) fn styled_focus(&self) -> Option<ElementId> {
        self.focused
            .filter(|id| self.focus_visible || self.text_inputs.contains_key(id))
    }

    /// Record the device whose event dispatch starts now.
    ///
    /// Every focus change until the matching [`Self::end_input_dispatch`] resolves its visibility
    /// from this device, so no listener has to carry the answer itself. A nested begin keeps the
    /// outer device: Enter activating a button dispatches the click inside the key event, and the
    /// focus that click lands is still keyboard-driven.
    pub(crate) fn begin_input_dispatch(&mut self, modality: InputModality) -> InputDispatchScope {
        let outermost = self.input_modality.is_none();
        if outermost {
            self.input_modality = Some(modality);
        }
        InputDispatchScope { outermost }
    }

    /// Close the scope opened by [`Self::begin_input_dispatch`].
    pub(crate) fn end_input_dispatch(&mut self, scope: InputDispatchScope) {
        if scope.outermost {
            self.input_modality = None;
        }
    }

    /// Capture a focus request for the rebuild it waits on, with the device making it now.
    pub(crate) fn pending_focus(&self, element: ElementId) -> PendingFocus {
        PendingFocus {
            element,
            modality: self.input_modality,
        }
    }

    /// Paint the focused element's focus styles without moving focus.
    ///
    /// Tab and the arrows are how a keyboard user finds focus, so a press that moves nothing — the
    /// only focusable control, a roving group at its edge, a key the application handles itself —
    /// still shows where focus already is. Returns whether the paint changes.
    pub(crate) fn reveal_focus(&mut self) -> bool {
        let changed = self.focused.is_some() && !self.focus_visible;
        self.focus_visible = true;
        changed
    }

    /// How a focus change made by `modality` paints, given the visibility it replaces.
    fn focus_visibility_for(&self, modality: Option<InputModality>) -> bool {
        match modality {
            Some(InputModality::Pointer) => false,
            Some(InputModality::Keyboard) => true,
            None => self.focus_visible,
        }
    }

    /// Move focus as the input being dispatched would, or programmatically outside any input.
    pub fn focus(&mut self, id: ElementId) -> bool {
        self.focus_as(id, self.input_modality)
    }

    /// Move focus as if `modality` were delivering the event that requested it.
    ///
    /// This applies a deferred [`PendingFocus`] after the rebuild it waited for, once the dispatch
    /// that made the request has already ended.
    pub(crate) fn focus_as(&mut self, id: ElementId, modality: Option<InputModality>) -> bool {
        let visible = self.focus_visibility_for(modality);
        self.set_focus(id, true, visible)
    }

    /// A press focuses without a ring: the pointer already shows the user what they pressed.
    pub(super) fn focus_from_pointer(&mut self, id: ElementId) -> bool {
        self.set_focus(id, false, false)
    }

    /// Apply the production press default to a named element without hit-test geometry.
    ///
    /// A real press focuses the innermost interactive element under the pointer when a pointer may
    /// focus it. A semantic test names its target instead of a point, so the same policy walks the
    /// retained parent chain up from that target. Returns whether the paint changes.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn press_target(&mut self, id: ElementId) -> bool {
        let target = {
            let Some(root) = &self.root else {
                return false;
            };
            let mut current = Some(id);
            let mut target = None;
            for _ in 0..MAX_FOCUSED_EVENT_PATH {
                let Some(candidate) = current else {
                    break;
                };
                let Some(element) = find_element(root, candidate) else {
                    break;
                };
                let enabled = !element.accessibility.disabled;
                if enabled && element.focusable && element.focus_on_pointer {
                    target = Some(candidate);
                    break;
                }
                if (enabled && (element.clickable || element.pointer_listener))
                    || element.blocks_pointer
                {
                    break;
                }
                current = self.parents.get(&candidate).copied();
            }
            target
        };
        target.is_some_and(|target| self.focus_from_pointer(target))
    }

    pub(super) fn set_focus(&mut self, id: ElementId, show_tooltip: bool, visible: bool) -> bool {
        if !self.focusable_ids.contains(&id) {
            return false;
        }
        let mut changed = self.focused != Some(id) || self.focus_visible != visible;
        self.focused = Some(id);
        self.focus_visible = visible;
        if self.text_inputs.contains_key(&id) {
            changed |= self.clear_static_text_selection();
        }
        // Tab may revisit the same control after keyboard dispatch revealed its focus ring.
        if show_tooltip {
            changed |= self.reconcile_tooltip(Instant::now());
        }
        changed
    }

    pub fn blur(&mut self) -> bool {
        // A press on empty space or a key that drops focus decides the visibility the next focus
        // inherits, so a later programmatic focus follows the device the user last used.
        self.focus_visible = self.focus_visibility_for(self.input_modality);
        let changed = self.focused.take().is_some();
        if changed {
            changed | self.reconcile_tooltip(Instant::now())
        } else {
            false
        }
    }

    /// Move focus to the next or previous Tab stop.
    ///
    /// Tab traversal is keyboard input by definition, so the focus it lands always shows its ring,
    /// including when the only Tab stop keeps focus and merely gains the ring.
    pub fn focus_next(&mut self, reverse: bool) -> bool {
        if self.focus_order.is_empty() {
            return false;
        }
        let current_index = self
            .focused
            .and_then(|focused| self.focus_order.iter().position(|id| *id == focused))
            .or_else(|| {
                let root = self.root.as_ref()?;
                let tab_stop = tab_stop_for_focused_tab(root, self.focused?)?;
                self.focus_order.iter().position(|id| *id == tab_stop)
            });
        let next_index = match current_index {
            Some(index) if reverse => index.checked_sub(1).unwrap_or(self.focus_order.len() - 1),
            Some(index) => (index + 1) % self.focus_order.len(),
            None if reverse => self.focus_order.len() - 1,
            None => 0,
        };
        let next = self.focus_order[next_index];
        self.set_focus(next, true, true)
    }

    pub fn activate_focused(&self) -> Option<ElementId> {
        self.focused.filter(|focused| {
            self.clickable_ids.contains(focused) && !self.text_inputs.contains_key(focused)
        })
    }

    /// Find the previous or next enabled radio in the focused radio group, wrapping at its ends.
    ///
    /// The tree is scanned only for an explicit keyboard arrow action. No group registry,
    /// observer, allocation, or idle work is retained between events.
    pub(crate) fn adjacent_radio(&self, reverse: bool) -> Option<ElementId> {
        let focused = self.focused?;
        let root = self.root.as_ref()?;
        let group = radio_group_for_focused(root, focused)?;
        let mut neighbors = RadioNeighbors::default();
        collect_radio_neighbors(group, group.runtime_id, focused, &mut neighbors);
        if !neighbors.found_focus {
            return None;
        }
        if reverse {
            neighbors.previous.or(neighbors.last)
        } else {
            neighbors.next.or(neighbors.first)
        }
    }

    /// Find the previous or next enabled tab in the focused tab list.
    ///
    /// The mounted tree is scanned only for the explicit arrow-key event. No component registry,
    /// observer, allocation, or idle source is retained.
    pub(crate) fn adjacent_tab(
        &self,
        vertical_axis: bool,
        reverse: bool,
    ) -> Option<TabNavigationTarget> {
        let focused = self.focused?;
        let root = self.root.as_ref()?;
        let list = tab_list_for_focused(root, focused)?;
        let behavior = list.tab_list_behavior?;
        if behavior.vertical != vertical_axis {
            return None;
        }
        let mut neighbors = TabNeighbors::default();
        collect_tab_neighbors(list, list.runtime_id, focused, &mut neighbors);
        if !neighbors.found_focus {
            return None;
        }
        let id = if reverse {
            neighbors
                .previous
                .or_else(|| behavior.loop_focus.then_some(neighbors.last).flatten())
        } else {
            neighbors
                .next
                .or_else(|| behavior.loop_focus.then_some(neighbors.first).flatten())
        }?;
        Some(TabNavigationTarget {
            id,
            activate: behavior.activate_on_focus,
        })
    }

    /// Find the first or last enabled tab in the focused tab list for Home/End.
    pub(crate) fn edge_tab(&self, last: bool) -> Option<TabNavigationTarget> {
        let focused = self.focused?;
        let root = self.root.as_ref()?;
        let list = tab_list_for_focused(root, focused)?;
        let behavior = list.tab_list_behavior?;
        let mut neighbors = TabNeighbors::default();
        collect_tab_neighbors(list, list.runtime_id, focused, &mut neighbors);
        if !neighbors.found_focus {
            return None;
        }
        Some(TabNavigationTarget {
            id: if last {
                neighbors.last?
            } else {
                neighbors.first?
            },
            activate: behavior.activate_on_focus,
        })
    }

    pub fn accessibility_element(&self, id: AccessibilityNodeId) -> Option<ElementId> {
        if id == ACCESSIBILITY_ROOT_ID {
            return None;
        }
        if let Some((input, _)) = self
            .accessibility_text_ids
            .iter()
            .find(|(_, text_id)| **text_id == id)
        {
            return self
                .root
                .as_ref()
                .is_some_and(|root| accessibility_tree_contains(root, *input))
                .then_some(*input);
        }
        let id = ElementId::new(id.0);
        self.root
            .as_ref()
            .is_some_and(|root| accessibility_tree_contains(root, id))
            .then_some(id)
    }

    pub fn accessibility_update(&self, window_title: &str) -> TreeUpdate {
        let mut accessible_ids = HashSet::with_capacity(self.visible_ids.len());
        if let Some(element) = &self.root {
            collect_accessible_ids(element, &self.element_bounds, &mut accessible_ids);
        }
        let mut nodes = Vec::with_capacity(
            accessible_ids.len() + usize::from(self.validation_announcement.is_some()),
        );
        let mut root = AccessibilityNode::new(Role::Window);
        root.set_bounds(accessibility_rect(Rect::from_size(self.viewport)));
        root.set_transform(Affine::scale(self.scale_factor as f64));
        root.set_label(window_title);
        let mut root_children = Vec::with_capacity(2);
        if let Some(element) = &self.root
            && accessible_ids.contains(&element.runtime_id)
        {
            root_children.push(accessibility_id(element.runtime_id));
            let context = AccessibilityBuildContext {
                element_bounds: &self.element_bounds,
                scroll_offsets: &self.scroll_offsets,
                text_inputs: &self.text_inputs,
                selectable_texts: &self.selectable_texts,
                selectable_text_indices: &self.selectable_text_indices,
                static_text_selection: self.static_text_selection,
                accessibility_text_ids: &self.accessibility_text_ids,
                accessible_ids: &accessible_ids,
            };
            build_accessibility_nodes(
                element,
                &context,
                &mut nodes,
                Vector::ZERO,
                AccessibilityBuildMode::Full,
            );
        }
        if let Some(announcement) = &self.validation_announcement {
            root_children.push(announcement.node);
            let mut alert = AccessibilityNode::new(Role::Alert);
            alert.set_value(announcement.message.to_string());
            alert.set_live(Live::Assertive);
            alert.set_live_atomic();
            nodes.push((announcement.node, alert));
        }
        root.set_children(root_children);
        nodes.insert(0, (ACCESSIBILITY_ROOT_ID, root));
        let focus = self
            .focused
            .filter(|id| accessible_ids.contains(id))
            .map(accessibility_id)
            .unwrap_or(ACCESSIBILITY_ROOT_ID);
        *self.accessibility_snapshot.borrow_mut() = Some(AccessibilitySnapshot {
            window_title: window_title.to_owned(),
            accessible_ids,
        });
        TreeUpdate {
            nodes,
            tree: Some(Tree::new(ACCESSIBILITY_ROOT_ID)),
            tree_id: TreeId::ROOT,
            focus,
        }
    }

    /// Update only scrolling containers after a retained scroll. Full accessibility trees encode
    /// descendants in stable, unscrolled coordinates and put the live translation on the scroll
    /// container, so AccessKit does not need to diff every text node while the viewport moves.
    pub fn accessibility_scroll_update(&self) -> TreeUpdate {
        let mut accessible_ids = HashSet::with_capacity(self.visible_ids.len());
        if let Some(element) = &self.root {
            collect_accessible_ids(element, &self.element_bounds, &mut accessible_ids);
        }
        // Nested clips can gain or lose painted descendants during a retained scroll.
        // A container-only update would then reference unpublished children, or retain
        // children the client has pruned. Rebuild only when this membership changes.
        let full_update_title = {
            let snapshot = self.accessibility_snapshot.borrow();
            match snapshot.as_ref() {
                Some(previous) if previous.accessible_ids == accessible_ids => None,
                Some(previous) => Some(previous.window_title.clone()),
                None => Some(String::new()),
            }
        };
        if let Some(title) = full_update_title {
            return self.accessibility_update(&title);
        }
        let mut nodes = Vec::with_capacity(self.scroll_offsets.len());
        if let Some(element) = &self.root
            && accessible_ids.contains(&element.runtime_id)
        {
            let context = AccessibilityBuildContext {
                element_bounds: &self.element_bounds,
                scroll_offsets: &self.scroll_offsets,
                text_inputs: &self.text_inputs,
                selectable_texts: &self.selectable_texts,
                selectable_text_indices: &self.selectable_text_indices,
                static_text_selection: self.static_text_selection,
                accessibility_text_ids: &self.accessibility_text_ids,
                accessible_ids: &accessible_ids,
            };
            build_accessibility_nodes(
                element,
                &context,
                &mut nodes,
                Vector::ZERO,
                AccessibilityBuildMode::ScrollContainers,
            );
        }
        TreeUpdate {
            nodes,
            tree: None,
            tree_id: TreeId::ROOT,
            focus: self
                .focused
                .filter(|id| accessible_ids.contains(id))
                .map(accessibility_id)
                .unwrap_or(ACCESSIBILITY_ROOT_ID),
        }
    }

    pub(super) fn rebuild_focus_index(&mut self) {
        self.focusable_ids.clear();
        self.clickable_ids.clear();
        self.focus_order.clear();
        let Some(root) = &self.root else {
            self.active_focus_trap = None;
            return;
        };
        let active_focus_trap = topmost_focus_trap(root);
        self.active_focus_trap = active_focus_trap;
        let mut candidates = Vec::new();
        let mut collection = FocusCollection {
            focusable_ids: &mut self.focusable_ids,
            clickable_ids: &mut self.clickable_ids,
            candidates: &mut candidates,
            active_trap: active_focus_trap,
        };
        collect_focus_candidates(root, &mut collection, false, None, false, None, false);
        candidates.sort_by_key(|candidate| {
            let group = if candidate.tab_index > 0 { 0 } else { 1 };
            let tab_index = if candidate.tab_index > 0 {
                candidate.tab_index
            } else {
                0
            };
            (group, tab_index, candidate.order)
        });
        self.focus_order
            .extend(candidates.into_iter().map(|candidate| candidate.id));
        if let Some(trap) = active_focus_trap
            && self
                .focused
                .is_none_or(|focused| !self.focusable_ids.contains(&focused))
        {
            let auto_focus = self
                .root
                .as_ref()
                .and_then(|root| find_auto_focus_in(root, &self.focusable_ids));
            self.focused = auto_focus.or_else(|| {
                self.focus_order
                    .first()
                    .copied()
                    .or_else(|| self.focusable_ids.contains(&trap).then_some(trap))
            });
        }
    }

    pub(super) fn rebuild_dispatch_index(&mut self) {
        self.parents.clear();
        self.key_contexts.clear();
        self.activation_targets.clear();
        self.invalid_ids.clear();
        self.form_ids.clear();
        self.form_submitter_ids.clear();
        self.context_menu_ids.clear();
        self.mouse_listener_bindings.clear();
        self.mouse_listener_ranges.clear();
        self.mouse_listener_elements.clear();
        self.key_listener_bindings.clear();
        self.key_listener_ranges.clear();
        self.action_listener_bindings.clear();
        self.action_listener_ranges.clear();
        self.scroll_wheel_ids.clear();
        self.touch_ids.clear();
        self.mouse_pressure_ids.clear();
        self.pinch_ids.clear();
        self.rotation_ids.clear();
        self.smart_magnify_ids.clear();
        let Some(root) = &self.root else {
            return;
        };
        collect_dispatch_metadata(
            root,
            None,
            &mut DispatchMetadata {
                parents: &mut self.parents,
                key_contexts: &mut self.key_contexts,
                activation_targets: &mut self.activation_targets,
                invalid_ids: &mut self.invalid_ids,
                form_ids: &mut self.form_ids,
                form_submitter_ids: &mut self.form_submitter_ids,
                context_menu_ids: &mut self.context_menu_ids,
                mouse_listener_bindings: &mut self.mouse_listener_bindings,
                mouse_listener_ranges: &mut self.mouse_listener_ranges,
                mouse_listener_elements: &mut self.mouse_listener_elements,
                key_listener_bindings: &mut self.key_listener_bindings,
                key_listener_ranges: &mut self.key_listener_ranges,
                action_listener_bindings: &mut self.action_listener_bindings,
                action_listener_ranges: &mut self.action_listener_ranges,
                scroll_wheel_ids: &mut self.scroll_wheel_ids,
                touch_ids: &mut self.touch_ids,
                mouse_pressure_ids: &mut self.mouse_pressure_ids,
                pinch_ids: &mut self.pinch_ids,
                rotation_ids: &mut self.rotation_ids,
                smart_magnify_ids: &mut self.smart_magnify_ids,
            },
        );
        if self
            .validation_announcement
            .as_ref()
            .is_some_and(|announcement| !self.form_ids.contains(&announcement.form))
        {
            self.validation_announcement = None;
        }
    }

    pub(super) fn rebuild_drop_predicates(&mut self) {
        self.drop_predicates.clear();
        let Some(root) = &self.root else {
            return;
        };
        collect_drop_predicates(root, &mut self.drop_predicates);
    }
}
