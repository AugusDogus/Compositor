use thiserror::Error;

use crate::{AccessibilityRole, Element, ElementId};

/// Maximum simultaneously open values retained by one multiple accordion.
///
/// Closed values are not retained. Lookup is logarithmic in the number of open values and
/// mutation occurs only in direct response to application actions.
pub const MAX_ACCORDION_OPEN_ITEMS: usize = 4_096;

const COLLAPSIBLE_TRIGGER_ID_TAG: u64 = 0x14c2_94d0_a71f_30e1;
const COLLAPSIBLE_PANEL_ID_TAG: u64 = 0xa41d_0c67_5f3a_9b22;
const ACCORDION_ITEM_ID_TAG: u64 = 0x9779_c49e_3cd9_1227;
const ACCORDION_HEADER_ID_TAG: u64 = 0xb863_73cc_41f0_5d18;
const ACCORDION_TRIGGER_ID_TAG: u64 = 0x5c50_058b_700c_2ce9;
const ACCORDION_PANEL_ID_TAG: u64 = 0xe379_2ec2_240d_a34f;
const DEFAULT_ACCORDION_HEADING_LEVEL: usize = 3;
const MAX_ACCORDION_HEADING_LEVEL: usize = 6;

/// Caller-owned state projected across one unstyled collapsible's parts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CollapsibleState {
    pub open: bool,
    pub disabled: bool,
}

/// A controlled, unstyled disclosure descriptor.
///
/// The application owns the open boolean, toggle listener, content, layout, iconography, colors,
/// typography, and motion. QuickGUI supplies stable part identities, button semantics,
/// open/controls accessibility state, disabled interaction, and optional closed-panel retention.
///
/// This descriptor retains no allocation, task, timer, observer, animation, or idle scheduler
/// source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "a Collapsible descriptor has no effect until its parts are mounted"]
pub struct Collapsible {
    root_id: ElementId,
    state: CollapsibleState,
    keep_mounted: bool,
}

impl Collapsible {
    pub fn new(root_id: impl Into<ElementId>, open: bool) -> Self {
        Self {
            root_id: root_id.into(),
            state: CollapsibleState {
                open,
                disabled: false,
            },
            keep_mounted: false,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.state.disabled = disabled;
        self
    }

    /// Retain the closed panel as `display: none` instead of omitting it from the mounted tree.
    pub const fn keep_mounted(mut self, keep_mounted: bool) -> Self {
        self.keep_mounted = keep_mounted;
        self
    }

    pub const fn state(self) -> CollapsibleState {
        self.state
    }

    pub const fn root_id(self) -> ElementId {
        self.root_id
    }

    pub fn trigger_id(self) -> ElementId {
        derived_disclosure_id(self.root_id, self.root_id, COLLAPSIBLE_TRIGGER_ID_TAG)
    }

    pub fn panel_id(self) -> ElementId {
        derived_disclosure_id(self.root_id, self.root_id, COLLAPSIBLE_PANEL_ID_TAG)
    }

    pub const fn is_open(self) -> bool {
        self.state.open
    }

    pub const fn is_disabled(self) -> bool {
        self.state.disabled
    }

    pub const fn keeps_panel_mounted(self) -> bool {
        self.keep_mounted
    }

    pub const fn panel_is_mounted(self) -> bool {
        self.state.open || self.keep_mounted
    }

    /// Decorate an application-owned structural root without adding role or appearance.
    pub fn root_with(self, root: Element) -> Element {
        root.id(self.root_id)
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(self) -> Element {
        self.root_with(crate::div())
    }

    /// Decorate an application-owned trigger with disclosure-button behavior.
    pub fn trigger_with(self, trigger: Element) -> Element {
        disclosure_trigger(trigger, self.trigger_id(), self.panel_id(), self.state)
    }
    /// Create the unstyled trigger part. Use [`Self::trigger_with`] to supply an existing element.
    pub fn trigger(self) -> Element {
        self.trigger_with(crate::button())
    }

    /// Decorate the panel when it should be mounted.
    ///
    /// The default closed state returns `None`. Pass the result to [`Element::children`] so the
    /// `Option<Element>` composes directly. With [`Self::keep_mounted`], a closed panel remains in
    /// the retained tree as `display: none` and therefore contributes no layout, paint, input, or
    /// accessibility node.
    pub fn panel_with(self, panel: Element) -> Option<Element> {
        disclosure_panel(
            panel.id(self.panel_id()),
            self.state.open,
            self.keep_mounted,
        )
    }
    /// Create the unstyled panel part. Use [`Self::panel_with`] to supply an existing element.
    pub fn panel(self) -> Option<Element> {
        self.panel_with(crate::div())
    }
}

/// A controlled accordion value violated its bounded single/multiple contract.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AccordionStateError {
    #[error("an accordion retains at most {limit} simultaneously open items")]
    TooManyOpenItems { limit: usize },
    #[error("a single accordion accepts at most one open item")]
    MultipleOpenItemsInSingleMode,
    #[error("accordion open values contain duplicate ID {id:?}")]
    DuplicateOpenItem { id: ElementId },
}

/// Bounded application-owned open-value state for an unstyled accordion.
///
/// Values are stored as a sorted compact vector. A fresh empty state allocates nothing; capacity
/// grows only when a value opens and is then reused across toggles. Reads are logarithmic and
/// writes are user-action-driven. No item registry, task, timer, or idle work is retained.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AccordionState {
    multiple: bool,
    open: Vec<ElementId>,
}

impl AccordionState {
    pub const fn new() -> Self {
        Self {
            multiple: false,
            open: Vec::new(),
        }
    }

    pub fn with_multiple(mut self, multiple: bool) -> Self {
        self.set_multiple(multiple);
        self
    }

    pub const fn allows_multiple(&self) -> bool {
        self.multiple
    }

    /// Change single/multiple behavior.
    ///
    /// Switching to single mode deterministically retains the lowest stable open ID.
    pub fn set_multiple(&mut self, multiple: bool) -> bool {
        if self.multiple == multiple {
            return false;
        }
        self.multiple = multiple;
        if !multiple {
            self.open.truncate(1);
        }
        true
    }

    pub fn open_ids(&self) -> &[ElementId] {
        &self.open
    }

    pub fn is_open(&self, id: impl Into<ElementId>) -> bool {
        self.open_index(id.into()).is_ok()
    }

    /// Atomically replace the controlled open values.
    pub fn replace_open<I, E>(&mut self, ids: I) -> Result<bool, AccordionStateError>
    where
        I: IntoIterator<Item = E>,
        E: Into<ElementId>,
    {
        let mut next = Vec::new();
        for id in ids {
            if next.len() == MAX_ACCORDION_OPEN_ITEMS {
                return Err(AccordionStateError::TooManyOpenItems {
                    limit: MAX_ACCORDION_OPEN_ITEMS,
                });
            }
            next.push(id.into());
        }
        next.sort_unstable_by_key(|id| id.as_u64());
        if let Some(pair) = next.windows(2).find(|pair| pair[0] == pair[1]) {
            return Err(AccordionStateError::DuplicateOpenItem { id: pair[0] });
        }
        if !self.multiple && next.len() > 1 {
            return Err(AccordionStateError::MultipleOpenItemsInSingleMode);
        }
        if self.open == next {
            return Ok(false);
        }
        self.open = next;
        Ok(true)
    }

    /// Set one value's open state, returning whether retained state changed.
    pub fn set_open(
        &mut self,
        id: impl Into<ElementId>,
        open: bool,
    ) -> Result<bool, AccordionStateError> {
        let id = id.into();
        match (self.open_index(id), open) {
            (Ok(_), true) | (Err(_), false) => Ok(false),
            (Ok(index), false) => {
                self.open.remove(index);
                Ok(true)
            }
            (Err(_), true) if !self.multiple => {
                self.open.clear();
                self.open.push(id);
                Ok(true)
            }
            (Err(_), true) if self.open.len() == MAX_ACCORDION_OPEN_ITEMS => {
                Err(AccordionStateError::TooManyOpenItems {
                    limit: MAX_ACCORDION_OPEN_ITEMS,
                })
            }
            (Err(index), true) => {
                self.open.insert(index, id);
                Ok(true)
            }
        }
    }

    /// Toggle one value, returning whether retained state changed.
    pub fn toggle(&mut self, id: impl Into<ElementId>) -> Result<bool, AccordionStateError> {
        let id = id.into();
        self.set_open(id, !self.is_open(id))
    }

    pub fn clear(&mut self) -> bool {
        if self.open.is_empty() {
            false
        } else {
            self.open.clear();
            true
        }
    }

    fn open_index(&self, id: ElementId) -> Result<usize, usize> {
        self.open
            .binary_search_by_key(&id.as_u64(), |candidate| candidate.as_u64())
    }
}

/// Caller-visible state shared by one accordion item's unstyled parts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccordionItemState {
    pub index: usize,
    pub open: bool,
    pub disabled: bool,
}

/// A copyable declaration for an unstyled accordion root.
///
/// Current WAI-ARIA guidance keeps every enabled trigger in normal Tab order. QuickGUI therefore
/// adds no roving-focus registry or arrow-key listener. Enter and Space reuse ordinary button
/// activation. The application owns open state and toggle listeners; [`AccordionState`] is the
/// optional bounded helper for single or multiple values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "an Accordion descriptor has no effect until its parts are mounted"]
pub struct Accordion {
    root_id: ElementId,
    disabled: bool,
    keep_mounted: bool,
    heading_level: usize,
}

impl Accordion {
    pub fn new(root_id: impl Into<ElementId>) -> Self {
        Self {
            root_id: root_id.into(),
            disabled: false,
            keep_mounted: false,
            heading_level: DEFAULT_ACCORDION_HEADING_LEVEL,
        }
    }

    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub const fn keep_mounted(mut self, keep_mounted: bool) -> Self {
        self.keep_mounted = keep_mounted;
        self
    }

    pub fn heading_level(mut self, level: usize) -> Self {
        self.heading_level = level.clamp(1, MAX_ACCORDION_HEADING_LEVEL);
        self
    }

    pub const fn root_id(self) -> ElementId {
        self.root_id
    }

    pub fn root_with(self, root: Element) -> Element {
        root.id(self.root_id)
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(self) -> Element {
        self.root_with(crate::div())
    }

    pub fn item(self, value: impl Into<ElementId>, index: usize, open: bool) -> AccordionItem {
        AccordionItem {
            accordion_id: self.root_id,
            value: value.into(),
            state: AccordionItemState {
                index,
                open,
                disabled: self.disabled,
            },
            keep_mounted: self.keep_mounted,
            heading_level: self.heading_level,
        }
    }

    pub fn item_from_state(
        self,
        value: impl Into<ElementId>,
        index: usize,
        state: &AccordionState,
    ) -> AccordionItem {
        let value = value.into();
        self.item(value, index, state.is_open(value))
    }
}

/// A copyable declaration for one controlled, unstyled accordion item.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "an AccordionItem descriptor has no effect until its parts are mounted"]
pub struct AccordionItem {
    accordion_id: ElementId,
    value: ElementId,
    state: AccordionItemState,
    keep_mounted: bool,
    heading_level: usize,
}

impl AccordionItem {
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.state.disabled |= disabled;
        self
    }

    pub const fn keep_mounted(mut self, keep_mounted: bool) -> Self {
        self.keep_mounted = keep_mounted;
        self
    }

    pub fn heading_level(mut self, level: usize) -> Self {
        self.heading_level = level.clamp(1, MAX_ACCORDION_HEADING_LEVEL);
        self
    }

    pub const fn state(self) -> AccordionItemState {
        self.state
    }

    pub const fn value(self) -> ElementId {
        self.value
    }

    /// This item's declared position in the accordion, Base UI's `data-index`.
    pub const fn index(self) -> usize {
        self.state.index
    }

    pub fn root_id(self) -> ElementId {
        derived_disclosure_id(self.accordion_id, self.value, ACCORDION_ITEM_ID_TAG)
    }

    pub fn header_id(self) -> ElementId {
        derived_disclosure_id(self.accordion_id, self.value, ACCORDION_HEADER_ID_TAG)
    }

    pub fn trigger_id(self) -> ElementId {
        derived_disclosure_id(self.accordion_id, self.value, ACCORDION_TRIGGER_ID_TAG)
    }

    pub fn panel_id(self) -> ElementId {
        derived_disclosure_id(self.accordion_id, self.value, ACCORDION_PANEL_ID_TAG)
    }

    pub const fn is_open(self) -> bool {
        self.state.open
    }

    pub const fn is_disabled(self) -> bool {
        self.state.disabled
    }

    pub const fn panel_is_mounted(self) -> bool {
        self.state.open || self.keep_mounted
    }

    pub fn root_with(self, root: Element) -> Element {
        root.id(self.root_id())
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(self) -> Element {
        self.root_with(crate::div())
    }

    /// Decorate the caller-owned heading that contains only this item's trigger.
    pub fn header_with(self, header: Element) -> Element {
        header
            .id(self.header_id())
            .accessibility_role(AccessibilityRole::Heading)
            .accessibility_level(self.heading_level)
    }
    /// Create the unstyled header part. Use [`Self::header_with`] to supply an existing element.
    pub fn header(self) -> Element {
        self.header_with(crate::div())
    }

    pub fn trigger_with(self, trigger: Element) -> Element {
        disclosure_trigger(
            trigger,
            self.trigger_id(),
            self.panel_id(),
            CollapsibleState {
                open: self.state.open,
                disabled: self.state.disabled,
            },
        )
    }
    /// Create the unstyled trigger part. Use [`Self::trigger_with`] to supply an existing element.
    pub fn trigger(self) -> Element {
        self.trigger_with(crate::button())
    }

    /// Decorate the item panel as a named accessibility region when mounted.
    pub fn panel_with(self, panel: Element) -> Option<Element> {
        let panel = panel
            .id(self.panel_id())
            .accessibility_role(AccessibilityRole::Region)
            .accessibility_labelled_by(self.trigger_id());
        disclosure_panel(panel, self.state.open, self.keep_mounted)
    }
    /// Create the unstyled panel part. Use [`Self::panel_with`] to supply an existing element.
    pub fn panel(self) -> Option<Element> {
        self.panel_with(crate::div())
    }
}

fn disclosure_trigger(
    trigger: Element,
    trigger_id: ElementId,
    panel_id: ElementId,
    state: CollapsibleState,
) -> Element {
    let disabled = trigger.accessibility.disabled || state.disabled;
    let mut trigger = trigger
        .id(trigger_id)
        .accessibility_role(AccessibilityRole::Button)
        .clickable()
        .cursor_default()
        .user_select_none()
        .app_region_no_drag()
        .accessibility_expanded(state.open)
        .disabled(disabled);
    if state.open {
        trigger = trigger.accessibility_controls(panel_id);
    }
    trigger
}

fn disclosure_panel(panel: Element, open: bool, keep_mounted: bool) -> Option<Element> {
    if open {
        Some(panel)
    } else if keep_mounted {
        Some(panel.hidden())
    } else {
        None
    }
}

fn derived_disclosure_id(scope: ElementId, value: ElementId, tag: u64) -> ElementId {
    let mut hash = scope
        .as_u64()
        .rotate_left(17)
        .wrapping_add(value.as_u64().rotate_right(11))
        ^ tag;
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    if hash == 0 || hash == u64::MAX || hash == scope.as_u64() || hash == value.as_u64() {
        hash ^= tag.rotate_left(23);
    }
    ElementId::new(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppRegion, Color, CursorStyle, Insets, IntoElement, TestAppContext, UserSelect, View,
        ViewContext, div, text,
    };

    #[test]
    fn collapsible_parts_add_exact_behavior_without_appearance_or_idle_sources() {
        let disclosure = Collapsible::new("recovery", true);
        assert_eq!(
            disclosure.state(),
            CollapsibleState {
                open: true,
                disabled: false,
            }
        );
        let root = disclosure.root_with(div().w(321.0).bg(Color::rgb8(1, 2, 3)));
        assert_eq!(root.explicit_id, Some(disclosure.root_id()));
        assert_eq!(root.visual.background, Some(Color::rgb8(1, 2, 3)));

        let trigger = disclosure.trigger_with(div().border(2.0, Color::rgb8(4, 5, 6)));
        assert_eq!(trigger.explicit_id, Some(disclosure.trigger_id()));
        assert_eq!(trigger.accessibility.role, AccessibilityRole::Button);
        assert_eq!(trigger.accessibility.expanded, Some(true));
        assert_eq!(
            trigger.accessibility.relations.controls(),
            Some(disclosure.panel_id())
        );
        assert!(trigger.clickable);
        assert!(trigger.focusable);
        assert_eq!(trigger.cursor_style, Some(CursorStyle::Arrow));
        assert_eq!(trigger.app_region, Some(AppRegion::NoDrag));
        assert_eq!(trigger.user_select, UserSelect::None);
        assert_eq!(trigger.visual.border_widths, Insets::all(2.0));
        assert!(trigger.transition.is_none());

        let panel = disclosure
            .panel_with(div().bg(Color::rgb8(7, 8, 9)))
            .expect("open panel");
        assert_eq!(panel.explicit_id, Some(disclosure.panel_id()));
        assert_eq!(panel.visual.background, Some(Color::rgb8(7, 8, 9)));

        let closed = Collapsible::new("closed", false);
        assert!(closed.panel_with(div()).is_none());
        let retained = closed
            .keep_mounted(true)
            .panel_with(div())
            .expect("retained closed panel");
        assert!(retained.is_display_none());
        assert_eq!(
            closed
                .keep_mounted(true)
                .trigger_with(div())
                .accessibility
                .relations
                .controls(),
            None
        );

        let disabled = closed.disabled(true).trigger_with(div());
        assert!(disabled.accessibility.disabled);
        assert_eq!(disabled.accessibility.expanded, Some(false));
    }

    #[test]
    fn accordion_state_is_atomic_bounded_and_supports_single_or_multiple_values() {
        let mut state = AccordionState::new();
        assert!(state.set_open("one", true).unwrap());
        assert!(state.is_open("one"));
        assert!(state.set_open("two", true).unwrap());
        assert!(!state.is_open("one"));
        assert!(state.is_open("two"));
        assert!(state.toggle("two").unwrap());
        assert!(state.open_ids().is_empty());
        assert!(!state.clear());

        state.set_multiple(true);
        assert!(state.replace_open(["two", "one"]).unwrap());
        assert!(state.is_open("one"));
        assert!(state.is_open("two"));
        let before = state.clone();
        assert_eq!(
            state.replace_open(["one", "one"]),
            Err(AccordionStateError::DuplicateOpenItem { id: "one".into() })
        );
        assert_eq!(state, before);

        let too_many = (0..=MAX_ACCORDION_OPEN_ITEMS).map(ElementId::from);
        assert_eq!(
            state.replace_open(too_many),
            Err(AccordionStateError::TooManyOpenItems {
                limit: MAX_ACCORDION_OPEN_ITEMS,
            })
        );
        assert_eq!(state, before);

        assert!(state.set_multiple(false));
        assert_eq!(state.open_ids().len(), 1);
        assert_eq!(
            state.replace_open(["one", "two"]),
            Err(AccordionStateError::MultipleOpenItemsInSingleMode)
        );
    }

    #[test]
    fn accordion_parts_project_current_tab_heading_and_region_contract() {
        let accordion = Accordion::new("faq").heading_level(2);
        let item = accordion.item("what", 0, true);
        assert_eq!(
            item.state(),
            AccordionItemState {
                index: 0,
                open: true,
                disabled: false,
            }
        );
        // The declared position is Base UI's `data-index`, reachable without the whole snapshot.
        assert_eq!(item.index(), 0);
        assert_eq!(accordion.item("how", 3, false).index(), 3);
        let ids = [
            item.root_id(),
            item.header_id(),
            item.trigger_id(),
            item.panel_id(),
        ];
        for (index, id) in ids.iter().enumerate() {
            assert_ne!(*id, accordion.root_id());
            assert!(!ids[..index].contains(id));
        }

        let header = item.header_with(div());
        assert_eq!(header.accessibility.role, AccessibilityRole::Heading);
        assert_eq!(header.accessibility.collection.level(), Some(2));

        let trigger = item.trigger_with(div());
        assert_eq!(trigger.accessibility.role, AccessibilityRole::Button);
        assert_eq!(trigger.tab_index, 0);
        assert!(trigger.focusable);
        assert!(trigger.key_listeners.is_none());
        assert_eq!(trigger.accessibility.expanded, Some(true));
        assert_eq!(
            trigger.accessibility.relations.controls(),
            Some(item.panel_id())
        );

        let panel = item.panel_with(div()).expect("open accordion panel");
        assert_eq!(panel.accessibility.role, AccessibilityRole::Region);
        assert_eq!(
            panel.accessibility.relations.labelled_by(),
            Some(item.trigger_id())
        );

        let closed = accordion.item("closed", 1, false).keep_mounted(true);
        assert!(
            closed
                .panel_with(div())
                .expect("retained panel")
                .is_display_none()
        );
        assert_eq!(
            closed
                .trigger_with(div())
                .accessibility
                .relations
                .controls(),
            None
        );

        let disabled = Accordion::new("disabled")
            .disabled(true)
            .item("item", 0, true)
            .trigger_with(div());
        assert!(disabled.accessibility.disabled);
    }

    #[derive(Default)]
    struct DisclosureView {
        collapsible_open: bool,
        accordion: AccordionState,
    }

    impl View for DisclosureView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let disclosure = Collapsible::new("details", self.collapsible_open);
            let toggle_disclosure = cx.listener(disclosure.trigger_id(), |view, cx| {
                view.collapsible_open = !view.collapsible_open;
                cx.invalidate();
            });

            let accordion = Accordion::new("faq");
            let first = accordion.item_from_state("first", 0, &self.accordion);
            let second = accordion.item_from_state("second", 1, &self.accordion);
            let disabled = accordion
                .item_from_state("disabled", 2, &self.accordion)
                .disabled(true);
            let toggle_first = cx.listener(first.trigger_id(), |view, cx| {
                view.accordion.toggle("first").unwrap();
                cx.invalidate();
            });
            let toggle_second = cx.listener(second.trigger_id(), |view, cx| {
                view.accordion.toggle("second").unwrap();
                cx.invalidate();
            });
            let toggle_disabled = cx.listener(disabled.trigger_id(), |_view, _cx| {});

            div()
                .child(
                    disclosure
                        .root_with(
                            div().child(
                                disclosure
                                    .trigger_with(div().child("Details"))
                                    .on_click(toggle_disclosure),
                            ),
                        )
                        .children(disclosure.panel_with(text("Disclosure content"))),
                )
                .child(
                    accordion.root_with(
                        div()
                            .child(
                                first.root_with(
                                    div()
                                        .child(
                                            first.header_with(
                                                div().child(
                                                    first
                                                        .trigger_with(div().child("First"))
                                                        .on_click(toggle_first),
                                                ),
                                            ),
                                        )
                                        .children(first.panel_with(text("First panel"))),
                                ),
                            )
                            .child(
                                second.root_with(
                                    div()
                                        .child(
                                            second.header_with(
                                                div().child(
                                                    second
                                                        .trigger_with(div().child("Second"))
                                                        .on_click(toggle_second),
                                                ),
                                            ),
                                        )
                                        .children(second.panel_with(text("Second panel"))),
                                ),
                            )
                            .child(
                                disabled.root_with(
                                    div()
                                        .child(
                                            disabled.header_with(
                                                div().child(
                                                    disabled
                                                        .trigger_with(div().child("Disabled"))
                                                        .on_click(toggle_disabled),
                                                ),
                                            ),
                                        )
                                        .children(disabled.panel_with(text("Disabled panel"))),
                                ),
                            ),
                    ),
                )
        }
    }

    #[test]
    fn disclosure_keyboard_mounting_accessibility_and_idle_paths_are_deterministic() {
        let (mut cx, view) = TestAppContext::new(DisclosureView::default()).unwrap();
        let window = view.window_handle();
        let disclosure = Collapsible::new("details", false);
        let accordion = Accordion::new("faq");
        let first = accordion.item("first", 0, false);
        let second = accordion.item("second", 1, false);
        let disabled = accordion.item("disabled", 2, false).disabled(true);

        assert!(!cx.contains_element(window, disclosure.panel_id()).unwrap());
        cx.click(window, disclosure.trigger_id()).unwrap();
        assert!(cx.contains_element(window, disclosure.panel_id()).unwrap());
        cx.simulate_keystrokes(window, "space").unwrap();
        assert!(!cx.contains_element(window, disclosure.panel_id()).unwrap());

        cx.simulate_keystrokes(window, "tab").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(first.trigger_id()));
        cx.simulate_keystrokes(window, "enter").unwrap();
        assert!(cx.contains_element(window, first.panel_id()).unwrap());
        cx.simulate_keystrokes(window, "tab").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(second.trigger_id()));
        cx.simulate_keystrokes(window, "space").unwrap();
        assert!(!cx.contains_element(window, first.panel_id()).unwrap());
        assert!(cx.contains_element(window, second.panel_id()).unwrap());
        assert!(cx.click(window, disabled.trigger_id()).is_err());
        cx.simulate_keystrokes(window, "tab").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(disclosure.trigger_id()));

        let update = cx.accessibility_update(window).unwrap();
        let node = |id: ElementId| {
            update
                .nodes
                .iter()
                .find_map(|(node_id, node)| (node_id.0 == id.as_u64()).then_some(node))
                .expect("disclosure accessibility node")
        };
        assert_eq!(node(first.trigger_id()).is_expanded(), Some(false));
        assert!(node(first.trigger_id()).controls().is_empty());
        assert_eq!(node(second.trigger_id()).is_expanded(), Some(true));
        assert_eq!(
            node(second.trigger_id()).controls(),
            &[accesskit::NodeId(second.panel_id().as_u64())]
        );
        assert_eq!(node(second.panel_id()).role(), accesskit::Role::Region);
        assert_eq!(
            node(second.panel_id()).labelled_by(),
            &[accesskit::NodeId(second.trigger_id().as_u64())]
        );
        assert!(node(disabled.trigger_id()).is_disabled());
        assert!(!node(disabled.trigger_id()).supports_action(accesskit::Action::Click));
        assert!(!node(disabled.trigger_id()).supports_action(accesskit::Action::Focus));

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }
}
