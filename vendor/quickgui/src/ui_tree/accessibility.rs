use super::*;

pub(super) struct AccessibilitySnapshot {
    pub(super) window_title: String,
    pub(super) accessible_ids: HashSet<ElementId>,
}

pub(super) struct AccessibilityBuildContext<'a> {
    pub(super) element_bounds: &'a HashMap<ElementId, Rect>,
    pub(super) scroll_offsets: &'a HashMap<ElementId, Vector>,
    pub(super) text_inputs: &'a HashMap<ElementId, TextInputState>,
    pub(super) selectable_texts: &'a [SelectableTextEntry],
    pub(super) selectable_text_indices: &'a HashMap<ElementId, usize>,
    pub(super) static_text_selection: Option<StaticTextSelection>,
    pub(super) accessibility_text_ids: &'a HashMap<ElementId, AccessibilityNodeId>,
    pub(super) accessible_ids: &'a HashSet<ElementId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AccessibilityBuildMode {
    Full,
    ScrollContainers,
}

pub(super) fn accessibility_scroll_translation(
    element: &Element,
    scroll_offsets: &HashMap<ElementId, Vector>,
) -> Option<Vector> {
    let overflow_scroll = !matches!(&element.kind, ElementKind::TextInput(_))
        && (element.layout.overflow.x == Overflow::Scroll
            || element.layout.overflow.y == Overflow::Scroll);
    if overflow_scroll {
        let offset = scroll_offsets
            .get(&element.runtime_id)
            .copied()
            .unwrap_or_default();
        // A right-to-left container stores its offset along the inline axis, whose start edge is
        // on the right, so its painted content moves the opposite way to a left-to-right one.
        return Some(if element.resolved_direction.is_rtl() {
            Vector::new(-offset.x, offset.y)
        } else {
            offset
        });
    }
    element.virtual_scroll.as_ref().map(|virtual_scroll| {
        let offset_y = scroll_offsets
            .get(&element.runtime_id)
            .map_or_else(|| virtual_scroll.handle.offset(), |offset| offset.y);
        Vector::new(
            0.0,
            virtual_scroll.handle.presented_offset(offset_y) - virtual_scroll.mount.layout_offset_y,
        )
    })
}

pub(super) fn accessibility_unscrolled_bounds(bounds: Rect, translation: Vector) -> Rect {
    Rect::new(
        bounds.x + translation.x,
        bounds.y + translation.y,
        bounds.width,
        bounds.height,
    )
}

pub(super) fn build_accessibility_nodes(
    element: &Element,
    context: &AccessibilityBuildContext<'_>,
    nodes: &mut Vec<(AccessibilityNodeId, AccessibilityNode)>,
    inherited_scroll_translation: Vector,
    mode: AccessibilityBuildMode,
) {
    if !context.accessible_ids.contains(&element.runtime_id) {
        return;
    }
    let Some(viewport_bounds) = context.element_bounds.get(&element.runtime_id).copied() else {
        return;
    };
    let own_scroll_translation = accessibility_scroll_translation(element, context.scroll_offsets);
    let content_scroll_translation =
        inherited_scroll_translation + own_scroll_translation.unwrap_or_default();
    if mode == AccessibilityBuildMode::ScrollContainers && own_scroll_translation.is_none() {
        for child in &element.children {
            build_accessibility_nodes(child, context, nodes, content_scroll_translation, mode);
        }
        return;
    }
    let bounds = accessibility_unscrolled_bounds(viewport_bounds, content_scroll_translation);
    let mut node = AccessibilityNode::new(accessibility_role(element.accessibility.role));
    node.set_bounds(accessibility_rect(bounds));
    if let Some(scroll) = own_scroll_translation {
        node.set_transform(Affine::translate(AccessibilityVector::new(
            -(scroll.x as f64),
            -(scroll.y as f64),
        )));
        node.set_clips_children();
        node.set_scroll_x(scroll.x as f64);
        node.set_scroll_y(scroll.y as f64);
    }
    // A child only joins the list when this update will also carry its node: a descendant the
    // last frame never painted (its parent clipped it away entirely, or it has not been laid out
    // yet) has no bounds, and an assistive client aborts on a child id it cannot resolve.
    let mut children = element
        .children
        .iter()
        .filter(|child| {
            context.accessible_ids.contains(&child.runtime_id)
                && context.element_bounds.contains_key(&child.runtime_id)
        })
        .map(|child| accessibility_id(child.runtime_id))
        .collect::<Vec<_>>();
    let text_input = context.text_inputs.get(&element.runtime_id).zip(
        context
            .accessibility_text_ids
            .get(&element.runtime_id)
            .copied(),
    );
    let selectable_text = context
        .selectable_text_indices
        .get(&element.runtime_id)
        .and_then(|index| {
            context
                .selectable_texts
                .get(*index)
                .map(|entry| (*index, entry))
        })
        .zip(
            context
                .accessibility_text_ids
                .get(&element.runtime_id)
                .copied(),
        );
    if let Some(text_id) = text_input
        .map(|(_, text_id)| text_id)
        .or_else(|| selectable_text.map(|(_, text_id)| text_id))
    {
        children.push(text_id);
    }
    node.set_children(children);
    if let Some(label) = accessibility_label(element) {
        node.set_label(label);
    }
    if let Some(description) = &element.accessibility.description {
        node.set_description(description.to_string());
    }
    if let Some((state, text_id)) = text_input {
        let password = matches!(
            &element.kind,
            ElementKind::TextInput(input) if input.password
        )
        .then(|| PasswordDisplay::new(state.text()));
        let accessible_text = password
            .as_ref()
            .map_or_else(|| state.shared_text(), |display| display.content.clone());
        let accessible_character_index = |source_index| {
            password.as_ref().map_or_else(
                || state.accessibility_character_index(source_index),
                |display| {
                    accessibility_character_index(
                        &display.content,
                        display.display_index(source_index),
                    )
                },
            )
        };
        let accessible_character_lengths = password.as_ref().map_or_else(
            || state.accessibility_character_lengths(),
            |display| selectable_character_lengths(&display.content),
        );
        node.set_value(accessible_text.to_string());
        if let ElementKind::TextInput(input) = &element.kind
            && !input.placeholder.is_empty()
        {
            node.set_placeholder(input.placeholder.to_string());
        }
        let anchor = TextPosition {
            node: text_id,
            character_index: accessible_character_index(state.anchor()),
        };
        let focus = TextPosition {
            node: text_id,
            character_index: accessible_character_index(state.caret()),
        };
        node.set_text_selection(TextSelection { anchor, focus });
        if !element.accessibility.disabled {
            node.add_action(Action::SetValue);
            node.add_action(Action::SetTextSelection);
        }

        let mut text_node = AccessibilityNode::new(Role::TextRun);
        text_node.set_bounds(accessibility_rect(bounds));
        text_node.set_value(accessible_text.to_string());
        text_node.set_character_lengths(accessible_character_lengths);
        nodes.push((text_id, text_node));
    } else if let Some(((document_index, entry), text_id)) = selectable_text {
        node.set_value(entry.content.to_string());
        let range = static_selection_range_for_entry(
            context.static_text_selection,
            context.selectable_text_indices,
            document_index,
            entry.content.len(),
        )
        .unwrap_or(0..0);
        let reversed = context
            .static_text_selection
            .and_then(|selection| {
                let anchor = (
                    *context.selectable_text_indices.get(&selection.anchor.id)?,
                    selection.anchor.offset,
                );
                let focus = (
                    *context.selectable_text_indices.get(&selection.focus.id)?,
                    selection.focus.offset,
                );
                Some(anchor > focus)
            })
            .unwrap_or(false);
        let (anchor_offset, focus_offset) = if reversed {
            (range.end, range.start)
        } else {
            (range.start, range.end)
        };
        node.set_text_selection(TextSelection {
            anchor: TextPosition {
                node: text_id,
                character_index: accessibility_character_index_from_lengths(
                    &entry.character_lengths,
                    anchor_offset,
                ),
            },
            focus: TextPosition {
                node: text_id,
                character_index: accessibility_character_index_from_lengths(
                    &entry.character_lengths,
                    focus_offset,
                ),
            },
        });
        if !element.accessibility.disabled {
            node.add_action(Action::SetTextSelection);
        }

        let mut text_node = AccessibilityNode::new(Role::TextRun);
        text_node.set_bounds(accessibility_rect(bounds));
        text_node.set_value(entry.content.to_string());
        text_node.set_character_lengths(entry.character_lengths.to_vec());
        nodes.push((text_id, text_node));
    } else if let Some(value) = &element.accessibility.value {
        node.set_value(value.to_string());
    }
    if element.accessibility.disabled {
        node.set_disabled();
    }
    if element.accessibility.required {
        node.set_required();
    }
    if element.accessibility.read_only {
        node.set_read_only();
    }
    if element.accessibility.invalid {
        node.set_invalid(AccessibilityInvalid::True);
        if let Some(message) = &element.accessibility.validation_message {
            node.set_description(message.to_string());
        }
    }
    if element.accessibility.role == AccessibilityRole::Tab {
        node.set_selected(element.accessibility.selected);
    } else if element.accessibility.selected {
        node.set_selected(true);
    }
    if let Some(toggled) = element.accessibility.toggled {
        node.set_toggled(match toggled {
            ToggleState::Off => AccessibilityToggled::False,
            ToggleState::On => AccessibilityToggled::True,
            ToggleState::Mixed => AccessibilityToggled::Mixed,
        });
    }
    if let Some(expanded) = element.accessibility.expanded {
        node.set_expanded(expanded);
    }
    if let Some(target) = element.accessibility.relations.controls()
        && context.accessible_ids.contains(&target)
    {
        node.push_controlled(accessibility_id(target));
    }
    if let Some(target) = element.accessibility.relations.active_descendant()
        && context.accessible_ids.contains(&target)
    {
        node.set_active_descendant(accessibility_id(target));
    }
    if let Some(target) = element.accessibility.relations.labelled_by()
        && target != element.runtime_id
        && context.accessible_ids.contains(&target)
    {
        node.push_labelled_by(accessibility_id(target));
    }
    if let Some(target) = element.accessibility.relations.described_by()
        && target != element.runtime_id
        && context.accessible_ids.contains(&target)
    {
        node.push_described_by(accessibility_id(target));
    }
    if let Some(target) = element.accessibility.relations.described_by_secondary()
        && target != element.runtime_id
        && context.accessible_ids.contains(&target)
    {
        node.push_described_by(accessibility_id(target));
    }
    if let Some(popover) = element.accessibility.has_popover {
        node.set_has_popup(match popover {
            AccessibilityPopover::Menu => AccessibilityHasPopover::Menu,
            AccessibilityPopover::ListBox => AccessibilityHasPopover::Listbox,
            AccessibilityPopover::Tree => AccessibilityHasPopover::Tree,
            AccessibilityPopover::Grid => AccessibilityHasPopover::Grid,
            AccessibilityPopover::Dialog => AccessibilityHasPopover::Dialog,
        });
    }
    if let Some(behavior) = element.accessibility.auto_complete {
        node.set_auto_complete(match behavior {
            AccessibilityAutoComplete::Inline => NativeAccessibilityAutoComplete::Inline,
            AccessibilityAutoComplete::List => NativeAccessibilityAutoComplete::List,
            AccessibilityAutoComplete::Both => NativeAccessibilityAutoComplete::Both,
        });
    }
    if let Some(orientation) = element.accessibility.orientation {
        node.set_orientation(match orientation {
            AccessibilityOrientation::Horizontal => NativeAccessibilityOrientation::Horizontal,
            AccessibilityOrientation::Vertical => NativeAccessibilityOrientation::Vertical,
        });
    }
    if let Some(range) = element.accessibility.value_range.as_deref() {
        if let Some(value) = range.value {
            node.set_numeric_value(value);
        }
        if let Some(minimum) = range.min {
            node.set_min_numeric_value(minimum);
        }
        if let Some(maximum) = range.max {
            node.set_max_numeric_value(maximum);
        }
        if let Some(step) = range.step {
            node.set_numeric_value_step(step);
        }
    }
    if let Some(live) = element.accessibility.live {
        node.set_live(match live {
            crate::AccessibilityLive::Polite => Live::Polite,
            crate::AccessibilityLive::Assertive => Live::Assertive,
        });
    }
    if element.accessibility.modal {
        node.set_modal();
    }
    if element.accessibility.multiselectable {
        node.set_multiselectable();
    }
    let collection = element.accessibility.collection;
    if let Some(value) = collection.row_count() {
        node.set_row_count(value);
    }
    if let Some(value) = collection.column_count() {
        node.set_column_count(value);
    }
    if let Some(value) = collection.row_index() {
        node.set_row_index(value);
    }
    if let Some(value) = collection.column_index() {
        node.set_column_index(value);
    }
    if let Some(value) = collection.level() {
        node.set_level(value);
    }
    if let Some(value) = collection.size_of_set() {
        node.set_size_of_set(value);
    }
    if let Some(value) = collection.position_in_set() {
        node.set_position_in_set(value);
    }
    if let Some(direction) = collection.sort_direction {
        node.set_sort_direction(match direction {
            AccessibilitySortDirection::Ascending => NativeAccessibilitySortDirection::Ascending,
            AccessibilitySortDirection::Descending => NativeAccessibilitySortDirection::Descending,
            AccessibilitySortDirection::Other => NativeAccessibilitySortDirection::Other,
        });
    }
    if element.is_keyboard_focusable() {
        node.add_action(Action::Focus);
        node.add_action(Action::Blur);
    }
    if element.clickable && !element.accessibility.disabled {
        node.add_action(Action::Click);
    }
    nodes.push((accessibility_id(element.runtime_id), node));

    for child in &element.children {
        build_accessibility_nodes(child, context, nodes, content_scroll_translation, mode);
    }
}

pub(super) fn accessibility_label(element: &Element) -> Option<String> {
    if element.is_display_none() || element.is_visibility_hidden() || element.accessibility.hidden {
        return None;
    }
    if let Some(label) = &element.accessibility.label {
        return Some(label.to_string());
    }
    if let ElementKind::Text(content) = &element.kind {
        return Some(content.to_string());
    }
    if let ElementKind::StyledText(content) = &element.kind {
        return Some(content.content().to_string());
    }
    if element.accessibility.role == AccessibilityRole::GenericContainer {
        return None;
    }
    let mut label = String::new();
    collect_text(element, &mut label);
    (!label.is_empty()).then_some(label)
}

pub(super) fn collect_text(element: &Element, output: &mut String) {
    if element.is_display_none() || element.is_visibility_hidden() || element.accessibility.hidden {
        return;
    }
    if let ElementKind::Text(content) = &element.kind {
        if !output.is_empty() {
            output.push(' ');
        }
        output.push_str(content);
    }
    if let ElementKind::StyledText(content) = &element.kind {
        if !output.is_empty() {
            output.push(' ');
        }
        output.push_str(content.content());
    }
    for child in &element.children {
        collect_text(child, output);
    }
}

pub(super) fn accessibility_id(id: ElementId) -> AccessibilityNodeId {
    AccessibilityNodeId(id.value())
}

pub(super) fn accessibility_rect(rect: Rect) -> AccessibilityRect {
    AccessibilityRect {
        x0: rect.x as f64,
        y0: rect.y as f64,
        x1: rect.right() as f64,
        y1: rect.bottom() as f64,
    }
}

pub(super) fn accessibility_role(role: AccessibilityRole) -> Role {
    match role {
        AccessibilityRole::GenericContainer => Role::GenericContainer,
        AccessibilityRole::Label => Role::Label,
        AccessibilityRole::Button => Role::Button,
        AccessibilityRole::Link => Role::Link,
        AccessibilityRole::Image => Role::Image,
        AccessibilityRole::List => Role::List,
        AccessibilityRole::ListItem => Role::ListItem,
        AccessibilityRole::Heading => Role::Heading,
        AccessibilityRole::CheckBox => Role::CheckBox,
        AccessibilityRole::RadioButton => Role::RadioButton,
        AccessibilityRole::RadioGroup => Role::RadioGroup,
        AccessibilityRole::Switch => Role::Switch,
        AccessibilityRole::TextInput => Role::TextInput,
        AccessibilityRole::PasswordInput => Role::PasswordInput,
        AccessibilityRole::MultilineTextInput => Role::MultilineTextInput,
        AccessibilityRole::Dialog => Role::Dialog,
        AccessibilityRole::AlertDialog => Role::AlertDialog,
        AccessibilityRole::Menu => Role::Menu,
        AccessibilityRole::MenuItem => Role::MenuItem,
        AccessibilityRole::MenuItemCheckBox => Role::MenuItemCheckBox,
        AccessibilityRole::MenuItemRadio => Role::MenuItemRadio,
        AccessibilityRole::Separator => Role::Splitter,
        AccessibilityRole::Group => Role::Group,
        AccessibilityRole::Region => Role::Region,
        AccessibilityRole::ListBox => Role::ListBox,
        AccessibilityRole::ListBoxOption => Role::ListBoxOption,
        AccessibilityRole::ComboBox => Role::ComboBox,
        AccessibilityRole::EditableComboBox => Role::EditableComboBox,
        AccessibilityRole::Table => Role::Table,
        AccessibilityRole::Tree => Role::Tree,
        AccessibilityRole::Grid => Role::Grid,
        AccessibilityRole::Row => Role::Row,
        AccessibilityRole::ColumnHeader => Role::ColumnHeader,
        AccessibilityRole::RowHeader => Role::RowHeader,
        AccessibilityRole::GridCell => Role::GridCell,
        AccessibilityRole::TreeItem => Role::TreeItem,
        AccessibilityRole::Tab => Role::Tab,
        AccessibilityRole::TabList => Role::TabList,
        AccessibilityRole::TabPanel => Role::TabPanel,
        AccessibilityRole::Tooltip => Role::Tooltip,
        AccessibilityRole::Form => Role::Form,
        AccessibilityRole::Slider => Role::Slider,
        AccessibilityRole::SpinButton => Role::SpinButton,
        AccessibilityRole::ProgressIndicator => Role::ProgressIndicator,
        AccessibilityRole::Meter => Role::Meter,
        AccessibilityRole::SplitterHandle => Role::Splitter,
        AccessibilityRole::Toolbar => Role::Toolbar,
        AccessibilityRole::ToggleButton => Role::Button,
        AccessibilityRole::MenuBar => Role::MenuBar,
        AccessibilityRole::Alert => Role::Alert,
        AccessibilityRole::Status => Role::Status,
        AccessibilityRole::Navigation => Role::Navigation,
        AccessibilityRole::ScrollView => Role::ScrollView,
        AccessibilityRole::ScrollBar => Role::ScrollBar,
    }
}

pub(super) fn collect_displayed_ids(element: &Element, ids: &mut HashSet<ElementId>) {
    if element.is_display_none() {
        return;
    }
    ids.insert(element.runtime_id);
    for child in &element.children {
        collect_displayed_ids(child, ids);
    }
}

pub(super) fn collect_focus_restorations(
    element: &Element,
    displayed_ids: &HashSet<ElementId>,
    previous: &[FocusRestoration],
    previous_focused: Option<ElementId>,
    restorations: &mut Vec<FocusRestoration>,
) {
    if !displayed_ids.contains(&element.runtime_id) {
        return;
    }
    if let Some(target) = element.restore_focus {
        restorations.push(FocusRestoration {
            surface: element.runtime_id,
            target: target.id(),
        });
    } else if element.restore_previous_focus {
        let target = previous
            .iter()
            .find(|restoration| restoration.surface == element.runtime_id)
            .map(|restoration| restoration.target)
            .or(previous_focused)
            .filter(|target| *target != element.runtime_id && displayed_ids.contains(target));
        if let Some(target) = target {
            restorations.push(FocusRestoration {
                surface: element.runtime_id,
                target,
            });
        }
    }
    for child in &element.children {
        collect_focus_restorations(
            child,
            displayed_ids,
            previous,
            previous_focused,
            restorations,
        );
    }
}

pub(super) fn collect_visible_ids(element: &Element, ids: &mut HashSet<ElementId>) {
    if element.is_display_none() || element.is_visibility_hidden() {
        return;
    }
    ids.insert(element.runtime_id);
    for child in &element.children {
        collect_visible_ids(child, ids);
    }
}

pub(super) fn collect_accessible_ids(
    element: &Element,
    bounds: &HashMap<ElementId, Rect>,
    ids: &mut HashSet<ElementId>,
) {
    if element.is_display_none()
        || element.is_visibility_hidden()
        || element.accessibility.hidden
        || !bounds.contains_key(&element.runtime_id)
    {
        return;
    }
    ids.insert(element.runtime_id);
    for child in &element.children {
        collect_accessible_ids(child, bounds, ids);
    }
}

pub(super) fn accessibility_tree_contains(element: &Element, target: ElementId) -> bool {
    if element.is_display_none() || element.is_visibility_hidden() || element.accessibility.hidden {
        return false;
    }
    element.runtime_id == target
        || element
            .children
            .iter()
            .any(|child| accessibility_tree_contains(child, target))
}

pub(super) fn mix_id(parent: u64, child: u64) -> u64 {
    let mut value = parent ^ child.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

pub(super) fn collect_explicit_ids(
    element: &Element,
    ids: &mut HashSet<ElementId>,
) -> Result<(), UiError> {
    if let Some(id) = element.explicit_id
        && !ids.insert(id)
    {
        if id.value() == ACCESSIBILITY_ROOT_ID.0 {
            return Err(UiError::ReservedId(id));
        }
        return Err(UiError::DuplicateId(id));
    }
    for child in &element.children {
        collect_explicit_ids(child, ids)?;
    }
    Ok(())
}

pub(super) fn validate_style_transition_count(
    element: &Element,
    count: &mut usize,
) -> Result<(), UiError> {
    if element.is_display_none() || element.is_visibility_hidden() {
        return Ok(());
    }
    if element.transition.is_some() {
        if *count == MAX_STYLE_TRANSITIONS_PER_WINDOW {
            return Err(UiError::TooManyStyleTransitions);
        }
        *count += 1;
    }
    for child in &element.children {
        validate_style_transition_count(child, count)?;
    }
    Ok(())
}

pub(super) fn collect_style_transition_ids(element: &Element, ids: &mut HashSet<ElementId>) {
    if element.is_display_none() || element.is_visibility_hidden() {
        return;
    }
    if element.transition.is_some() {
        ids.insert(element.runtime_id);
    }
    for child in &element.children {
        collect_style_transition_ids(child, ids);
    }
}
