use super::*;

#[derive(Clone, Copy)]
pub(super) struct FocusCandidate {
    pub(super) id: ElementId,
    pub(super) tab_index: i16,
    pub(super) order: usize,
}

pub(super) struct FocusCollection<'a> {
    pub(super) focusable_ids: &'a mut HashSet<ElementId>,
    pub(super) clickable_ids: &'a mut HashSet<ElementId>,
    pub(super) candidates: &'a mut Vec<FocusCandidate>,
    pub(super) active_trap: Option<ElementId>,
}

#[derive(Default)]
pub(super) struct FormCollection {
    pub(super) fields: Vec<FormField>,
    pub(super) fields_truncated: bool,
    pub(super) issues: Vec<ValidationIssue>,
    pub(super) issues_truncated: bool,
    pub(super) first_focusable_invalid: Option<ElementId>,
}

pub(super) fn find_element(element: &Element, id: ElementId) -> Option<&Element> {
    if element.runtime_id == id {
        return Some(element);
    }
    element
        .children
        .iter()
        .find_map(|child| find_element(child, id))
}

pub(super) fn collect_form_controls(
    element: &Element,
    text_inputs: &HashMap<ElementId, TextInputState>,
    focusable_ids: &HashSet<ElementId>,
    collection: &mut FormCollection,
) {
    if element.is_display_none() || element.is_visibility_hidden() {
        return;
    }
    // Forms do not inherit controls from nested forms. This also gives malformed declarative
    // nesting deterministic browser-like ownership without retaining a second membership index.
    if element.form {
        return;
    }
    if !element.accessibility.disabled {
        if let Some(input) = text_inputs.get(&element.runtime_id) {
            if collection.fields.len() < MAX_FORM_FIELDS {
                collection.fields.push(FormField::new(
                    element.runtime_id,
                    input.committed_shared_text(),
                ));
            } else {
                collection.fields_truncated = true;
            }
        }
        if element.accessibility.invalid {
            if collection.first_focusable_invalid.is_none()
                && focusable_ids.contains(&element.runtime_id)
            {
                collection.first_focusable_invalid = Some(element.runtime_id);
            }
            if collection.issues.len() < MAX_VALIDATION_ISSUES {
                collection.issues.push(ValidationIssue::new(
                    element.runtime_id,
                    element.accessibility.validation_message.clone(),
                    element.accessibility.validation_message_truncated,
                ));
            } else {
                collection.issues_truncated = true;
            }
        }
    }
    for child in &element.children {
        collect_form_controls(child, text_inputs, focusable_ids, collection);
    }
}

pub(super) fn validation_announcement_message(report: &ValidationReport) -> Arc<str> {
    let first = report
        .first()
        .and_then(ValidationIssue::message)
        .unwrap_or("This field is invalid.");
    let mut message = if report.issues().len() == 1 && !report.is_truncated() {
        first.to_owned()
    } else {
        let qualifier = if report.is_truncated() {
            "at least "
        } else {
            ""
        };
        format!(
            "{qualifier}{} fields need attention. {first}",
            report.issues().len()
        )
    };
    if message.len() > MAX_VALIDATION_MESSAGE_BYTES {
        let mut end = MAX_VALIDATION_MESSAGE_BYTES;
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
    }
    Arc::from(message)
}

pub(super) fn topmost_focus_trap(root: &Element) -> Option<ElementId> {
    fn visit(
        element: &Element,
        parent_layer: PaintLayerKey,
        source: &mut usize,
        topmost: &mut Option<(PaintOrder, ElementId)>,
    ) {
        if element.is_display_none() || element.is_visibility_hidden() {
            return;
        }
        let plane = element.plane.unwrap_or(parent_layer.plane);
        let z_index = if plane == parent_layer.plane {
            parent_layer
                .z_index
                .saturating_add(element.z_index.unwrap_or(0))
        } else {
            element.z_index.unwrap_or(0)
        };
        let layer = PaintLayerKey {
            plane,
            z_index,
            group: parent_layer.group,
        };
        let order = PaintOrder {
            layer,
            source: *source,
        };
        *source = source.saturating_add(1);
        if element.focus_trap
            && topmost
                .as_ref()
                .is_none_or(|(previous, _)| order > *previous)
        {
            *topmost = Some((order, element.runtime_id));
        }
        for child in &element.children {
            visit(child, layer, source, topmost);
        }
    }

    let mut source = 0;
    let mut topmost = None;
    visit(root, PaintLayerKey::default(), &mut source, &mut topmost);
    topmost.map(|(_, id)| id)
}

pub(super) fn collect_focus_candidates(
    element: &Element,
    collection: &mut FocusCollection<'_>,
    inside_radio_group: bool,
    radio_tab_stop: Option<ElementId>,
    inside_tab_list: bool,
    tab_stop: Option<ElementId>,
    inside_active_focus_trap: bool,
) {
    if element.is_display_none() || element.is_visibility_hidden() {
        return;
    }
    let inside_active_focus_trap = inside_active_focus_trap
        || collection.active_trap.is_none()
        || collection.active_trap == Some(element.runtime_id);
    if inside_active_focus_trap && element.is_keyboard_focusable() {
        collection.focusable_ids.insert(element.runtime_id);
        let radio_is_tab_stop = element.accessibility.role != AccessibilityRole::RadioButton
            || !inside_radio_group
            || radio_tab_stop == Some(element.runtime_id);
        let tab_is_tab_stop = element.accessibility.role != AccessibilityRole::Tab
            || !inside_tab_list
            || tab_stop == Some(element.runtime_id);
        if element.tab_index >= 0 && radio_is_tab_stop && tab_is_tab_stop {
            collection.candidates.push(FocusCandidate {
                id: element.runtime_id,
                tab_index: element.tab_index,
                order: collection.candidates.len(),
            });
        }
    }
    if element.clickable && !element.accessibility.disabled {
        collection.clickable_ids.insert(element.runtime_id);
    }
    let (inside_radio_group, radio_tab_stop) =
        if element.accessibility.role == AccessibilityRole::RadioGroup {
            (true, radio_group_tab_stop(element))
        } else {
            (inside_radio_group, radio_tab_stop)
        };
    let (inside_tab_list, tab_stop) = if element.accessibility.role == AccessibilityRole::TabList {
        (true, tab_list_tab_stop(element))
    } else {
        (inside_tab_list, tab_stop)
    };
    for child in &element.children {
        collect_focus_candidates(
            child,
            collection,
            inside_radio_group,
            radio_tab_stop,
            inside_tab_list,
            tab_stop,
            inside_active_focus_trap,
        );
    }
}

pub(super) fn tab_list_tab_stop(list: &Element) -> Option<ElementId> {
    fn visit(
        element: &Element,
        list_id: ElementId,
        first: &mut Option<ElementId>,
        selected: &mut Option<ElementId>,
    ) {
        if element.is_display_none()
            || element.is_visibility_hidden()
            || (element.runtime_id != list_id
                && element.accessibility.role == AccessibilityRole::TabList)
        {
            return;
        }
        if element.accessibility.role == AccessibilityRole::Tab
            && element.focusable
            && !element.accessibility.disabled
            && element.tab_index >= 0
        {
            first.get_or_insert(element.runtime_id);
            if selected.is_none() && element.accessibility.selected {
                *selected = Some(element.runtime_id);
            }
        }
        for child in &element.children {
            visit(child, list_id, first, selected);
        }
    }

    let mut first = None;
    let mut selected = None;
    visit(list, list.runtime_id, &mut first, &mut selected);
    selected.or(first)
}

pub(super) fn tab_list_for_focused(element: &Element, focused: ElementId) -> Option<&Element> {
    fn visit(element: &Element, focused: ElementId) -> (bool, Option<&Element>) {
        if element.is_display_none() || element.is_visibility_hidden() {
            return (false, None);
        }
        let mut contains = element.runtime_id == focused;
        for child in &element.children {
            let (child_contains, child_list) = visit(child, focused);
            if let Some(list) = child_list {
                return (true, Some(list));
            }
            contains |= child_contains;
        }
        if contains && element.accessibility.role == AccessibilityRole::TabList {
            (true, Some(element))
        } else {
            (contains, None)
        }
    }

    visit(element, focused).1
}

pub(super) fn tab_stop_for_focused_tab(root: &Element, focused: ElementId) -> Option<ElementId> {
    let focused_element = find_element(root, focused)?;
    if focused_element.accessibility.role != AccessibilityRole::Tab {
        return None;
    }
    tab_list_tab_stop(tab_list_for_focused(root, focused)?)
}

#[derive(Default)]
pub(super) struct TabNeighbors {
    pub(super) first: Option<ElementId>,
    pub(super) previous: Option<ElementId>,
    pub(super) next: Option<ElementId>,
    pub(super) last: Option<ElementId>,
    pub(super) found_focus: bool,
}

pub(super) fn collect_tab_neighbors(
    element: &Element,
    list_id: ElementId,
    focused: ElementId,
    neighbors: &mut TabNeighbors,
) {
    if element.is_display_none()
        || element.is_visibility_hidden()
        || (element.runtime_id != list_id
            && element.accessibility.role == AccessibilityRole::TabList)
    {
        return;
    }
    if element.accessibility.role == AccessibilityRole::Tab
        && element.focusable
        && element.clickable
        && !element.accessibility.disabled
    {
        let id = element.runtime_id;
        neighbors.first.get_or_insert(id);
        if neighbors.found_focus && neighbors.next.is_none() {
            neighbors.next = Some(id);
        } else if id == focused {
            neighbors.found_focus = true;
        } else if !neighbors.found_focus {
            neighbors.previous = Some(id);
        }
        neighbors.last = Some(id);
    }
    for child in &element.children {
        collect_tab_neighbors(child, list_id, focused, neighbors);
    }
}

pub(super) fn radio_group_tab_stop(group: &Element) -> Option<ElementId> {
    fn visit(
        element: &Element,
        group_id: ElementId,
        first: &mut Option<ElementId>,
        selected: &mut Option<ElementId>,
    ) {
        if element.is_display_none()
            || element.is_visibility_hidden()
            || (element.runtime_id != group_id
                && element.accessibility.role == AccessibilityRole::RadioGroup)
        {
            return;
        }
        if element.accessibility.role == AccessibilityRole::RadioButton
            && element.focusable
            && !element.accessibility.disabled
            && element.tab_index >= 0
        {
            first.get_or_insert(element.runtime_id);
            if selected.is_none() && element.accessibility.toggled == Some(ToggleState::On) {
                *selected = Some(element.runtime_id);
            }
        }
        for child in &element.children {
            visit(child, group_id, first, selected);
        }
    }

    let mut first = None;
    let mut selected = None;
    visit(group, group.runtime_id, &mut first, &mut selected);
    selected.or(first)
}

pub(super) fn radio_group_for_focused(element: &Element, focused: ElementId) -> Option<&Element> {
    fn visit(element: &Element, focused: ElementId) -> (bool, Option<&Element>) {
        if element.is_display_none() || element.is_visibility_hidden() {
            return (false, None);
        }
        let mut contains = element.runtime_id == focused;
        for child in &element.children {
            let (child_contains, child_group) = visit(child, focused);
            if let Some(group) = child_group {
                return (true, Some(group));
            }
            contains |= child_contains;
        }
        if contains && element.accessibility.role == AccessibilityRole::RadioGroup {
            (true, Some(element))
        } else {
            (contains, None)
        }
    }

    visit(element, focused).1
}

#[derive(Default)]
pub(super) struct RadioNeighbors {
    pub(super) first: Option<ElementId>,
    pub(super) previous: Option<ElementId>,
    pub(super) next: Option<ElementId>,
    pub(super) last: Option<ElementId>,
    pub(super) found_focus: bool,
}

pub(super) fn collect_radio_neighbors(
    element: &Element,
    group_id: ElementId,
    focused: ElementId,
    neighbors: &mut RadioNeighbors,
) {
    if element.is_display_none()
        || element.is_visibility_hidden()
        || (element.runtime_id != group_id
            && element.accessibility.role == AccessibilityRole::RadioGroup)
    {
        return;
    }
    if element.accessibility.role == AccessibilityRole::RadioButton
        && element.focusable
        && element.clickable
        && !element.accessibility.disabled
    {
        let id = element.runtime_id;
        neighbors.first.get_or_insert(id);
        if neighbors.found_focus && neighbors.next.is_none() {
            neighbors.next = Some(id);
        } else if id == focused {
            neighbors.found_focus = true;
        } else if !neighbors.found_focus {
            neighbors.previous = Some(id);
        }
        neighbors.last = Some(id);
    }
    for child in &element.children {
        collect_radio_neighbors(child, group_id, focused, neighbors);
    }
}

pub(super) struct DispatchMetadata<'a> {
    pub(super) parents: &'a mut HashMap<ElementId, ElementId>,
    pub(super) key_contexts: &'a mut HashMap<ElementId, KeyContext>,
    pub(super) activation_targets: &'a mut HashMap<ElementId, ElementId>,
    pub(super) invalid_ids: &'a mut HashSet<ElementId>,
    pub(super) form_ids: &'a mut HashSet<ElementId>,
    pub(super) form_submitter_ids: &'a mut HashSet<ElementId>,
    pub(super) context_menu_ids: &'a mut HashSet<ElementId>,
    pub(super) mouse_listener_bindings: &'a mut Vec<MouseListenerBinding>,
    pub(super) mouse_listener_ranges: &'a mut HashMap<ElementId, Range<usize>>,
    pub(super) mouse_listener_elements: &'a mut Vec<ElementId>,
    pub(super) key_listener_bindings: &'a mut Vec<KeyListenerBinding>,
    pub(super) key_listener_ranges: &'a mut HashMap<ElementId, Range<usize>>,
    pub(super) action_listener_bindings: &'a mut Vec<ActionListenerBinding>,
    pub(super) action_listener_ranges: &'a mut HashMap<ElementId, Range<usize>>,
    pub(super) scroll_wheel_ids: &'a mut HashSet<ElementId>,
    pub(super) touch_ids: &'a mut HashSet<ElementId>,
    pub(super) mouse_pressure_ids: &'a mut HashSet<ElementId>,
    pub(super) pinch_ids: &'a mut HashSet<ElementId>,
    pub(super) rotation_ids: &'a mut HashSet<ElementId>,
    pub(super) smart_magnify_ids: &'a mut HashSet<ElementId>,
}

pub(super) fn collect_dispatch_metadata(
    element: &Element,
    parent: Option<ElementId>,
    metadata: &mut DispatchMetadata<'_>,
) {
    if element.is_display_none() || element.is_visibility_hidden() {
        return;
    }
    if let Some(parent) = parent {
        metadata.parents.insert(element.runtime_id, parent);
    }
    if let Some(context) = &element.key_context {
        metadata
            .key_contexts
            .insert(element.runtime_id, context.clone());
    }
    if let Some(target) = element.activation_target
        && !element.accessibility.disabled
        && target != element.runtime_id
    {
        metadata
            .activation_targets
            .insert(element.runtime_id, target);
    }
    if element.accessibility.invalid {
        metadata.invalid_ids.insert(element.runtime_id);
    }
    if element.form {
        metadata.form_ids.insert(element.runtime_id);
    }
    if element.form_submitter && !element.accessibility.disabled {
        metadata.form_submitter_ids.insert(element.runtime_id);
    }
    if element.context_menu_listener && !element.accessibility.disabled {
        metadata.context_menu_ids.insert(element.runtime_id);
    }
    if let Some(listeners) = &element.mouse_listeners
        && !element.accessibility.disabled
    {
        let start = metadata.mouse_listener_bindings.len();
        metadata
            .mouse_listener_bindings
            .extend(listeners.iter().copied());
        let end = metadata.mouse_listener_bindings.len();
        metadata
            .mouse_listener_ranges
            .insert(element.runtime_id, start..end);
        metadata.mouse_listener_elements.push(element.runtime_id);
    }
    if let Some(listeners) = &element.key_listeners
        && !element.accessibility.disabled
    {
        let start = metadata.key_listener_bindings.len();
        metadata
            .key_listener_bindings
            .extend(listeners.iter().copied());
        let end = metadata.key_listener_bindings.len();
        metadata
            .key_listener_ranges
            .insert(element.runtime_id, start..end);
    }
    if let Some(listeners) = &element.action_listeners
        && !element.accessibility.disabled
    {
        let start = metadata.action_listener_bindings.len();
        metadata
            .action_listener_bindings
            .extend(listeners.iter().copied());
        let end = metadata.action_listener_bindings.len();
        metadata
            .action_listener_ranges
            .insert(element.runtime_id, start..end);
    }
    if element.scroll_wheel_listener && !element.accessibility.disabled {
        metadata.scroll_wheel_ids.insert(element.runtime_id);
    }
    if element.touch_listener && !element.accessibility.disabled {
        metadata.touch_ids.insert(element.runtime_id);
    }
    if element.mouse_pressure_listener && !element.accessibility.disabled {
        metadata.mouse_pressure_ids.insert(element.runtime_id);
    }
    if element.pinch_listener && !element.accessibility.disabled {
        metadata.pinch_ids.insert(element.runtime_id);
    }
    if element.rotation_listener && !element.accessibility.disabled {
        metadata.rotation_ids.insert(element.runtime_id);
    }
    if element.smart_magnify_listener && !element.accessibility.disabled {
        metadata.smart_magnify_ids.insert(element.runtime_id);
    }
    for child in &element.children {
        collect_dispatch_metadata(child, Some(element.runtime_id), metadata);
    }
}

pub(super) fn collect_drop_predicates(
    element: &Element,
    predicates: &mut HashMap<(ElementId, TypeId), DropPredicateCallback>,
) {
    if element.is_display_none() || element.is_visibility_hidden() {
        return;
    }
    for predicate in &element.drop_predicates {
        predicates.insert(
            (element.runtime_id, predicate.type_id),
            Arc::clone(&predicate.callback),
        );
    }
    for child in &element.children {
        collect_drop_predicates(child, predicates);
    }
}

#[cfg(target_os = "macos")]
pub(super) fn drop_acceptance(
    predicates: &HashMap<(ElementId, TypeId), DropPredicateCallback>,
    id: ElementId,
    value_type: TypeId,
) -> ExternalDropAcceptance {
    predicates
        .get(&(id, value_type))
        .map(|predicate| ExternalDropAcceptance::Predicate(Arc::clone(predicate)))
        .unwrap_or(ExternalDropAcceptance::Always)
}

pub(super) fn collect_tooltips(
    element: &Element,
    tooltips: &mut HashMap<ElementId, Tooltip>,
) -> Result<(), UiError> {
    if element.is_display_none() || element.is_visibility_hidden() {
        return Ok(());
    }
    if let Some(tooltip) = &element.tooltip
        && !element.accessibility.disabled
    {
        if tooltips.len() >= MAX_TOOLTIPS_PER_WINDOW {
            return Err(UiError::TooManyTooltips);
        }
        tooltips.insert(element.runtime_id, tooltip.clone());
    }
    for child in &element.children {
        collect_tooltips(child, tooltips)?;
    }
    Ok(())
}

pub(super) fn find_auto_focus(element: &Element) -> Option<ElementId> {
    if element.is_display_none() || element.is_visibility_hidden() {
        return None;
    }
    if element.auto_focus && element.is_keyboard_focusable() {
        return Some(element.runtime_id);
    }
    element.children.iter().find_map(find_auto_focus)
}

pub(super) fn find_auto_focus_in(
    element: &Element,
    focusable_ids: &HashSet<ElementId>,
) -> Option<ElementId> {
    if element.is_display_none() || element.is_visibility_hidden() {
        return None;
    }
    if element.auto_focus && focusable_ids.contains(&element.runtime_id) {
        return Some(element.runtime_id);
    }
    element
        .children
        .iter()
        .find_map(|child| find_auto_focus_in(child, focusable_ids))
}
