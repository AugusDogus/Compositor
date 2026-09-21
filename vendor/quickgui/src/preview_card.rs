use web_time::{Duration, Instant};

use crate::{
    AccessibilityPopover, AccessibilityRole, AnchorPlacement, DismissListener, Element, ElementId,
    EventContext, FocusHandle, HoverListener, Popover, PopoverKind, StateAccessor, ViewContext,
    div,
};

/// Default hover delay before a preview card opens, matching Base UI's `delay`.
pub const DEFAULT_PREVIEW_CARD_DELAY: Duration = Duration::from_millis(600);
/// Default delay before a preview card closes once the pointer leaves it.
pub const DEFAULT_PREVIEW_CARD_CLOSE_DELAY: Duration = Duration::from_millis(300);
/// Longest open or close delay one preview card may declare.
pub const MAX_PREVIEW_CARD_DELAY: Duration = Duration::from_secs(10);

const PREVIEW_CARD_ARROW_ID_TAG: u64 = 0x1e73_44ba_92c0_57fd;

/// Controlled hover/focus state for one preview card.
///
/// The state owns the two exact one-shot deadlines a hover card needs — one to open after the
/// pointer rests on the trigger, one to close after it leaves both the trigger and the popup — plus
/// the pointer and focus bookkeeping that decides when each deadline is armed. It owns no timer,
/// task, or animation source: [`Self::next_deadline`] reports the single instant a repaint is
/// needed, exactly like [`crate::ToastManager::next_deadline`], and a settled card reports none.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewCardState {
    open: bool,
    trigger_hovered: bool,
    popup_hovered: bool,
    trigger_focused: bool,
    delay: Duration,
    close_delay: Duration,
    open_at: Option<Instant>,
    close_at: Option<Instant>,
}

impl Default for PreviewCardState {
    fn default() -> Self {
        Self::new()
    }
}

impl PreviewCardState {
    pub const fn new() -> Self {
        Self {
            open: false,
            trigger_hovered: false,
            popup_hovered: false,
            trigger_focused: false,
            delay: DEFAULT_PREVIEW_CARD_DELAY,
            close_delay: DEFAULT_PREVIEW_CARD_CLOSE_DELAY,
            open_at: None,
            close_at: None,
        }
    }

    /// Replace the hover-to-open delay, clamped to [`MAX_PREVIEW_CARD_DELAY`].
    #[must_use]
    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay.min(MAX_PREVIEW_CARD_DELAY);
        self
    }

    /// Replace the pointer-leave close delay, clamped to [`MAX_PREVIEW_CARD_DELAY`].
    #[must_use]
    pub fn close_delay(mut self, close_delay: Duration) -> Self {
        self.close_delay = close_delay.min(MAX_PREVIEW_CARD_DELAY);
        self
    }

    pub const fn open_delay(&self) -> Duration {
        self.delay
    }

    pub const fn close_delay_value(&self) -> Duration {
        self.close_delay
    }

    pub const fn is_open(&self) -> bool {
        self.open
    }

    pub const fn is_trigger_hovered(&self) -> bool {
        self.trigger_hovered
    }

    pub const fn is_popup_hovered(&self) -> bool {
        self.popup_hovered
    }

    /// Force the open value, cancelling any armed deadline. Returns whether it changed.
    pub fn set_open(&mut self, open: bool) -> bool {
        self.open_at = None;
        self.close_at = None;
        if self.open == open {
            return false;
        }
        self.open = open;
        true
    }

    /// Close the card and require a fresh pointer entry before it may reopen.
    ///
    /// This is the Escape and outside-press path: clearing the retained hover flags is what keeps
    /// a card that is dismissed under the pointer from immediately scheduling itself open again.
    pub fn close(&mut self) -> bool {
        self.trigger_hovered = false;
        self.popup_hovered = false;
        self.trigger_focused = false;
        self.set_open(false)
    }

    /// Report the pointer entering or leaving the trigger. Returns whether anything changed.
    pub fn hover_trigger(&mut self, hovered: bool, now: Instant) -> bool {
        if self.trigger_hovered == hovered {
            return false;
        }
        self.trigger_hovered = hovered;
        self.rearm(now);
        true
    }

    /// Report the pointer entering or leaving the popup. Returns whether anything changed.
    pub fn hover_popup(&mut self, hovered: bool, now: Instant) -> bool {
        if self.popup_hovered == hovered {
            return false;
        }
        self.popup_hovered = hovered;
        self.rearm(now);
        true
    }

    /// Report keyboard focus entering or leaving the trigger.
    ///
    /// Focus opens the card immediately, because a keyboard user has no way to "rest" a pointer,
    /// and blur closes it immediately unless the pointer is keeping it open.
    pub fn focus_trigger(&mut self, focused: bool, now: Instant) -> bool {
        if self.trigger_focused == focused {
            return false;
        }
        self.trigger_focused = focused;
        if focused {
            self.open_at = None;
            self.close_at = None;
            self.open = true;
            return true;
        }
        self.rearm(now);
        true
    }

    /// The single instant a repaint is needed, or `None` while the card is settled.
    pub fn next_deadline(&self) -> Option<Instant> {
        match (self.open_at, self.close_at) {
            (Some(open), Some(close)) => Some(open.min(close)),
            (open, close) => open.or(close),
        }
    }

    /// Apply every elapsed deadline, returning whether the open value changed.
    pub fn poll(&mut self, now: Instant) -> bool {
        let mut changed = false;
        if self.open_at.is_some_and(|deadline| deadline <= now) {
            self.open_at = None;
            changed |= !self.open;
            self.open = true;
        }
        if self.close_at.is_some_and(|deadline| deadline <= now) {
            self.close_at = None;
            changed |= self.open;
            self.open = false;
        }
        changed
    }

    fn rearm(&mut self, now: Instant) {
        let wanted = self.trigger_hovered || self.popup_hovered || self.trigger_focused;
        if wanted {
            self.close_at = None;
            if self.open {
                self.open_at = None;
            } else if self.delay.is_zero() {
                self.open_at = None;
                self.open = true;
            } else if self.open_at.is_none() {
                self.open_at = Some(now + self.delay);
            }
            return;
        }
        self.open_at = None;
        if !self.open {
            self.close_at = None;
        } else if self.close_delay.is_zero() {
            self.close_at = None;
            self.open = false;
        } else if self.close_at.is_none() {
            self.close_at = Some(now + self.close_delay);
        }
    }
}

/// A copyable declaration for one controlled, unstyled preview card.
///
/// A preview card is a link-like trigger whose rich preview opens on hover after a delay and on
/// focus at once. QuickGUI supplies the delayed open/close contract, stable part identities,
/// anchored in-window placement, Escape and outside-press dismissal, and the trigger/popup
/// accessibility relationship; the application owns every visual declaration and the content.
///
/// The parts compose the existing in-window [`Popover`]. QuickGUI's retained overlay node is
/// itself the portal, so [`Self::portal_with`] and [`Self::positioner_with`] decorate the same
/// boundary: mount exactly one of them.
///
/// The descriptor retains no allocation, task, timer, observer, or idle scheduler source.
#[derive(Clone, Copy, Debug, PartialEq)]
#[must_use = "a PreviewCard descriptor has no effect until its parts are mounted"]
pub struct PreviewCard {
    popover: Popover,
}

impl PreviewCard {
    /// Declare a preview card over caller-owned trigger and popup identities.
    pub fn new(
        trigger_id: impl Into<ElementId>,
        popup_id: impl Into<ElementId>,
        open: bool,
    ) -> Self {
        Self {
            popover: Popover::new(trigger_id, popup_id, open)
                .kind(PopoverKind::Dialog)
                .placement(AnchorPlacement::BottomStart),
        }
    }

    /// Declare a preview card from its retained state.
    pub fn from_state(
        trigger_id: impl Into<ElementId>,
        popup_id: impl Into<ElementId>,
        state: &PreviewCardState,
    ) -> Self {
        Self::new(trigger_id, popup_id, state.is_open())
    }

    pub const fn placement(mut self, placement: AnchorPlacement) -> Self {
        self.popover = self.popover.placement(placement);
        self
    }

    /// Set the structural distance between the trigger and the popup.
    pub fn anchor_gap(mut self, gap: f32) -> Self {
        self.popover = self.popover.anchor_gap(gap);
        self
    }

    /// Set the structural collision margin inside the current window viewport.
    pub fn viewport_margin(mut self, margin: f32) -> Self {
        self.popover = self.popover.viewport_margin(margin);
        self
    }

    /// The composed popover, for callers that need its remaining parts directly.
    pub const fn popover(self) -> Popover {
        self.popover
    }

    pub const fn is_open(self) -> bool {
        self.popover.is_open()
    }

    pub const fn trigger_id(self) -> ElementId {
        self.popover.trigger_id()
    }

    pub const fn popup_id(self) -> ElementId {
        self.popover.popover_id()
    }

    pub fn positioner_id(self) -> ElementId {
        self.popover.positioner_id()
    }

    pub fn backdrop_id(self) -> ElementId {
        self.popover.backdrop_id()
    }

    pub fn arrow_id(self) -> ElementId {
        derived_preview_card_id(self.popup_id(), PREVIEW_CARD_ARROW_ID_TAG)
    }

    pub fn trigger_focus(self) -> FocusHandle {
        self.popover.trigger_focus()
    }

    /// Decorate the optional application-owned structural wrapper.
    ///
    /// A preview card needs no wrapper of its own; this part exists so a composition that wants
    /// one keeps the trigger out of the window's native drag region without inventing appearance.
    pub fn root_with(self, root: Element) -> Element {
        root.app_region_no_drag()
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(self) -> Element {
        self.root_with(crate::div())
    }

    /// Decorate the application-owned link-like trigger without adding appearance.
    ///
    /// Unlike a popover trigger, this projects the Link role: a preview card previews a
    /// destination rather than opening a menu, so the trigger stays an ordinary link for the
    /// keyboard and for assistive technology.
    pub fn trigger_with(self, trigger: Element) -> Element {
        let trigger = trigger
            .id(self.trigger_id())
            .focusable()
            .tab_index(0)
            .accessibility_role(AccessibilityRole::Link)
            .accessibility_expanded(self.is_open())
            .accessibility_has_popover(AccessibilityPopover::Dialog)
            .app_region_no_drag()
            .user_select_none();
        if self.is_open() {
            trigger.accessibility_controls(self.popup_id())
        } else {
            trigger
        }
    }
    /// Create the unstyled trigger part. Use [`Self::trigger_with`] to supply an existing element.
    pub fn trigger(self) -> Element {
        self.trigger_with(crate::button())
    }

    /// Decorate the caller-owned portal boundary.
    ///
    /// QuickGUI's retained overlay node is itself the portal, so this is the same boundary as
    /// [`Self::positioner_with`]; mount exactly one of them.
    pub fn portal_with(self, portal: Element) -> Element {
        self.popover.positioner_with(portal)
    }
    /// Create the unstyled portal part. Use [`Self::portal_with`] to supply an existing element.
    pub fn portal(self) -> Element {
        self.portal_with(crate::div())
    }

    /// Decorate the caller-owned positioner without adding appearance.
    pub fn positioner_with(self, positioner: Element) -> Element {
        self.popover.positioner_with(positioner)
    }
    /// Create the unstyled positioner part. Use [`Self::positioner_with`] to supply an existing element.
    pub fn positioner(self) -> Element {
        self.positioner_with(crate::div())
    }

    /// Decorate the application-owned popup without adding layout or appearance.
    ///
    /// The popup emits [`crate::Event::Dismiss`] under [`Self::popup_id`] for Escape and for an
    /// outside pointer press, and restores focus to the trigger.
    pub fn popup_with(self, popup: Element) -> Element {
        self.popover.popup_with(popup)
    }
    /// Create the unstyled popup part. Use [`Self::popup_with`] to supply an existing element.
    pub fn popup(self) -> Element {
        self.popup_with(crate::div())
    }

    /// Decorate the application-owned arrow.
    ///
    /// The arrow is decorative: it carries a stable identity so the application can position and
    /// animate it, and is hidden from assistive technology.
    pub fn arrow_with(self, arrow: Element) -> Element {
        arrow.id(self.arrow_id()).accessibility_hidden(true)
    }
    /// Create the unstyled arrow part. Use [`Self::arrow_with`] to supply an existing element.
    pub fn arrow(self) -> Element {
        self.arrow_with(crate::div())
    }

    /// Decorate an optional caller-painted viewport backdrop.
    pub fn backdrop_with(self, backdrop: Element) -> Element {
        self.popover.backdrop_with(backdrop)
    }
    /// Create the unstyled backdrop part. Use [`Self::backdrop_with`] to supply an existing element.
    pub fn backdrop(self) -> Element {
        self.backdrop_with(crate::div())
    }

    /// Build the trigger's hover behavior.
    ///
    /// Attach the returned handle with [`crate::Element::on_hover`]. `on_open_change` runs only
    /// when the open value actually changes, which for the delayed open is on the frame the
    /// deadline is applied rather than on the pointer event that armed it.
    pub fn on_trigger_hover<V: 'static, Change>(
        self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut PreviewCardState,
        on_open_change: Change,
    ) -> HoverListener<V>
    where
        Change: Fn(&mut V, bool, &mut EventContext) + 'static,
    {
        self.on_trigger_hover_with(cx, StateAccessor::from(access), on_open_change)
    }

    /// Build the trigger's hover behavior against a per-instance state accessor.
    pub fn on_trigger_hover_with<V: 'static, Change>(
        self,
        cx: &mut ViewContext<'_, V>,
        access: StateAccessor<V, PreviewCardState>,
        on_open_change: Change,
    ) -> HoverListener<V>
    where
        Change: Fn(&mut V, bool, &mut EventContext) + 'static,
    {
        cx.hover_listener(self.trigger_id(), move |view, hovered, cx| {
            let hovered = *hovered;
            let before = access.get(view).is_open();
            if access.get(view).hover_trigger(hovered, Instant::now()) {
                let after = access.get(view).is_open();
                if before != after {
                    on_open_change(view, after, cx);
                }
                cx.invalidate();
            }
        })
    }

    /// Build the popup's hover behavior, which keeps an open card open.
    pub fn on_popup_hover<V: 'static, Change>(
        self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut PreviewCardState,
        on_open_change: Change,
    ) -> HoverListener<V>
    where
        Change: Fn(&mut V, bool, &mut EventContext) + 'static,
    {
        self.on_popup_hover_with(cx, StateAccessor::from(access), on_open_change)
    }

    /// Build the popup's hover behavior against a per-instance state accessor.
    pub fn on_popup_hover_with<V: 'static, Change>(
        self,
        cx: &mut ViewContext<'_, V>,
        access: StateAccessor<V, PreviewCardState>,
        on_open_change: Change,
    ) -> HoverListener<V>
    where
        Change: Fn(&mut V, bool, &mut EventContext) + 'static,
    {
        cx.hover_listener(self.popup_id(), move |view, hovered, cx| {
            let hovered = *hovered;
            let before = access.get(view).is_open();
            if access.get(view).hover_popup(hovered, Instant::now()) {
                let after = access.get(view).is_open();
                if before != after {
                    on_open_change(view, after, cx);
                }
                cx.invalidate();
            }
        })
    }

    /// Build the popup's dismissal behavior for Escape and outside presses.
    ///
    /// Attach the returned handle with [`crate::Element::on_dismiss`].
    pub fn on_dismiss<V: 'static, Change>(
        self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut PreviewCardState,
        on_open_change: Change,
    ) -> DismissListener<V>
    where
        Change: Fn(&mut V, bool, &mut EventContext) + 'static,
    {
        self.on_dismiss_with(cx, StateAccessor::from(access), on_open_change)
    }

    /// Build the popup's dismissal behavior against a per-instance state accessor.
    pub fn on_dismiss_with<V: 'static, Change>(
        self,
        cx: &mut ViewContext<'_, V>,
        access: StateAccessor<V, PreviewCardState>,
        on_open_change: Change,
    ) -> DismissListener<V>
    where
        Change: Fn(&mut V, bool, &mut EventContext) + 'static,
    {
        cx.dismiss_listener(self.popup_id(), move |view, cx| {
            if access.get(view).close() {
                on_open_change(view, false, cx);
                cx.invalidate();
            }
        })
    }

    /// Ask for the one repaint an armed open or close deadline needs.
    ///
    /// Call this while rendering. A settled preview card requests nothing, so the window sleeps.
    pub fn schedule<V: 'static>(cx: &mut ViewContext<'_, V>, state: &PreviewCardState) {
        if let Some(deadline) = state.next_deadline() {
            cx.request_repaint_at(deadline);
        }
    }
}

/// Create an unstyled preview-card trigger root.
///
/// This shorthand is equivalent to
/// `PreviewCard::from_state(trigger_id, popup_id, state).trigger_with(div())`.
pub fn preview_card_trigger(
    trigger_id: impl Into<ElementId>,
    popup_id: impl Into<ElementId>,
    state: &PreviewCardState,
) -> Element {
    PreviewCard::from_state(trigger_id, popup_id, state).trigger_with(div())
}

fn derived_preview_card_id(scope: ElementId, tag: u64) -> ElementId {
    let mut hash = scope.as_u64().rotate_left(37) ^ tag;
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    if hash == 0 || hash == u64::MAX || hash == scope.as_u64() {
        hash ^= tag.rotate_left(41);
    }
    ElementId::new(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, IntoElement, TestAppContext, View, text};

    #[test]
    fn hover_and_focus_use_exact_one_shot_deadlines() {
        let start = Instant::now();
        let mut state = PreviewCardState::new();
        assert_eq!(state.open_delay(), DEFAULT_PREVIEW_CARD_DELAY);
        assert_eq!(state.close_delay_value(), DEFAULT_PREVIEW_CARD_CLOSE_DELAY);
        assert_eq!(state.next_deadline(), None);

        assert!(state.hover_trigger(true, start));
        assert!(!state.hover_trigger(true, start));
        assert!(state.is_trigger_hovered());
        assert!(!state.is_open());
        assert_eq!(
            state.next_deadline(),
            Some(start + DEFAULT_PREVIEW_CARD_DELAY)
        );
        assert!(!state.poll(start + Duration::from_millis(599)));
        assert!(state.poll(start + DEFAULT_PREVIEW_CARD_DELAY));
        assert!(state.is_open());
        assert_eq!(state.next_deadline(), None);

        // Leaving the trigger for the popup keeps the card open with no deadline in between.
        let moved = start + Duration::from_secs(1);
        assert!(state.hover_popup(true, moved));
        assert!(state.hover_trigger(false, moved));
        assert_eq!(state.next_deadline(), None);
        assert!(state.is_popup_hovered());

        assert!(state.hover_popup(false, moved));
        assert_eq!(
            state.next_deadline(),
            Some(moved + DEFAULT_PREVIEW_CARD_CLOSE_DELAY)
        );
        // Returning to the trigger before the deadline cancels the close.
        assert!(state.hover_trigger(true, moved));
        assert_eq!(state.next_deadline(), None);
        assert!(state.is_open());

        assert!(state.hover_trigger(false, moved));
        assert!(state.poll(moved + DEFAULT_PREVIEW_CARD_CLOSE_DELAY));
        assert!(!state.is_open());
        assert!(!state.poll(moved + Duration::from_secs(10)));

        // Focus opens without waiting.
        assert!(state.focus_trigger(true, moved));
        assert!(state.is_open());
        assert_eq!(state.next_deadline(), None);
        assert!(state.focus_trigger(false, moved));
        assert!(state.poll(moved + DEFAULT_PREVIEW_CARD_CLOSE_DELAY));
        assert!(!state.is_open());

        // Escape closes and requires a fresh pointer entry.
        assert!(state.hover_trigger(true, moved));
        assert!(state.poll(moved + DEFAULT_PREVIEW_CARD_DELAY));
        assert!(state.close());
        assert!(!state.is_open());
        assert!(!state.is_trigger_hovered());
        assert_eq!(state.next_deadline(), None);

        let instant = &mut PreviewCardState::new()
            .delay(Duration::ZERO)
            .close_delay(Duration::ZERO);
        assert!(instant.hover_trigger(true, start));
        assert!(instant.is_open());
        assert_eq!(instant.next_deadline(), None);
        assert!(instant.hover_trigger(false, start));
        assert!(!instant.is_open());

        let clamped = PreviewCardState::new()
            .delay(Duration::from_secs(600))
            .close_delay(Duration::from_secs(600));
        assert_eq!(clamped.open_delay(), MAX_PREVIEW_CARD_DELAY);
        assert_eq!(clamped.close_delay_value(), MAX_PREVIEW_CARD_DELAY);
        assert_eq!(PreviewCardState::default(), PreviewCardState::new());

        let mut forced = PreviewCardState::new();
        assert!(forced.set_open(true));
        assert!(!forced.set_open(true));
        assert!(forced.set_open(false));
    }

    #[test]
    fn parts_add_exact_semantics_without_appearance() {
        let card = PreviewCard::new("profile-link", "profile-card", true)
            .placement(AnchorPlacement::TopStart)
            .anchor_gap(10.0)
            .viewport_margin(12.0);
        let trigger = card.trigger_with(div().bg(Color::rgb8(1, 2, 3)));
        assert_eq!(trigger.explicit_id, Some("profile-link".into()));
        assert_eq!(trigger.accessibility.role, AccessibilityRole::Link);
        assert_eq!(trigger.accessibility.expanded, Some(true));
        assert_eq!(
            trigger.accessibility.has_popover,
            Some(AccessibilityPopover::Dialog)
        );
        assert_eq!(
            trigger.accessibility.relations.controls(),
            Some(card.popup_id())
        );
        assert!(trigger.focusable);
        assert_eq!(trigger.visual.background, Some(Color::rgb8(1, 2, 3)));

        let closed = PreviewCard::new("profile-link", "profile-card", false).trigger_with(div());
        assert_eq!(closed.accessibility.expanded, Some(false));
        assert_eq!(closed.accessibility.relations.controls(), None);

        let positioner = card.positioner_with(div());
        assert_eq!(positioner.explicit_id, Some(card.positioner_id()));
        let portal = card.portal_with(div());
        assert_eq!(portal.explicit_id, Some(card.positioner_id()));

        let popup = card.popup_with(div().w(280.0));
        assert_eq!(popup.explicit_id, Some(card.popup_id()));
        assert_eq!(popup.accessibility.role, AccessibilityRole::Dialog);
        assert!(popup.dismiss_policy.on_escape());
        assert!(popup.dismiss_policy.on_pointer_outside());
        assert_eq!(popup.visual.background, None);

        let arrow = card.arrow_with(div().size(8.0, 8.0));
        assert_eq!(arrow.explicit_id, Some(card.arrow_id()));
        assert!(arrow.accessibility.hidden);

        let backdrop = card.backdrop_with(div());
        assert_eq!(backdrop.explicit_id, Some(card.backdrop_id()));
        assert!(backdrop.accessibility.hidden);

        assert_eq!(card.root_with(div()).visual.background, None);
        assert_eq!(card.popover().popover_id(), card.popup_id());
        assert!(card.is_open());

        let ids = [
            card.trigger_id(),
            card.popup_id(),
            card.positioner_id(),
            card.backdrop_id(),
            card.arrow_id(),
        ];
        for (index, id) in ids.iter().enumerate() {
            assert_ne!(*id, ElementId::new(0));
            assert_ne!(*id, ElementId::new(u64::MAX));
            assert!(!ids[..index].contains(id));
        }

        let state = PreviewCardState::new();
        let shorthand = preview_card_trigger("profile-link", "profile-card", &state);
        assert_eq!(shorthand.accessibility.role, AccessibilityRole::Link);
        assert!(shorthand.children.is_empty());
    }

    struct PreviewCardView {
        card: PreviewCardState,
        changes: Vec<bool>,
    }

    impl PreviewCardView {
        fn card(view: &mut Self) -> &mut PreviewCardState {
            &mut view.card
        }
    }

    impl View for PreviewCardView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let _ = self.card.poll(Instant::now());
            PreviewCard::schedule(cx, &self.card);
            let card = PreviewCard::from_state("profile-link", "profile-card", &self.card);
            let trigger_hover = card.on_trigger_hover(cx, Self::card, |view, open, _| {
                view.changes.push(open);
            });
            let popup_hover = card.on_popup_hover(cx, Self::card, |view, open, _| {
                view.changes.push(open);
            });
            let dismiss = card.on_dismiss(cx, Self::card, |view, open, _| {
                view.changes.push(open);
            });
            let open = cx.listener("open", |view, cx| {
                if view.card.set_open(true) {
                    view.changes.push(true);
                    cx.invalidate();
                }
            });

            let mut root = card
                .root_with(div().size_full().relative())
                .child(
                    card.trigger_with(div().child(text("Ada Lovelace")))
                        .on_hover(trigger_hover),
                )
                .child(crate::button().id("open").child("Open").on_click(open));
            if card.is_open() {
                root = root.child(
                    card.positioner_with(div()).child(
                        card.popup_with(div().w(240.0).h(120.0))
                            .on_hover(popup_hover)
                            .on_dismiss(dismiss)
                            .child(text("Mathematician"))
                            .child(card.arrow_with(div().size(8.0, 8.0))),
                    ),
                );
            }
            root
        }
    }

    #[test]
    fn controlled_card_opens_dismisses_and_sleeps() {
        let (mut cx, view) = TestAppContext::new(PreviewCardView {
            card: PreviewCardState::new(),
            changes: Vec::new(),
        })
        .unwrap();
        let window = view.window_handle();
        let card = PreviewCard::new("profile-link", "profile-card", true);

        assert!(!cx.contains_element(window, card.popup_id()).unwrap());
        cx.click(window, "open").unwrap();
        assert!(cx.contains_element(window, card.popup_id()).unwrap());

        let update = cx.accessibility_update(window).unwrap();
        let node = |id: ElementId| {
            update
                .nodes
                .iter()
                .find_map(|(node_id, node)| (node_id.0 == id.as_u64()).then_some(node))
                .expect("preview card accessibility node")
        };
        let trigger = node(card.trigger_id());
        assert_eq!(trigger.role(), accesskit::Role::Link);
        assert_eq!(trigger.is_expanded(), Some(true));
        assert_eq!(node(card.popup_id()).role(), accesskit::Role::Dialog);

        cx.simulate_keystrokes(window, "escape").unwrap();
        assert!(!cx.read(view, |view| view.card.is_open()).unwrap());
        assert_eq!(
            cx.read(view, |view| view.changes.clone()).unwrap(),
            vec![true, false]
        );
        assert!(!cx.contains_element(window, card.popup_id()).unwrap());

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }
}
