use std::{sync::Arc, time::Duration};

use crate::{
    AccessibilityPopover, AccessibilityRole, AnchorAlign, AnchorPlacement, AnchorPlacementHandle,
    AnchorSide, AsyncViewContext, Color, Element, ElementId, EventContext, FocusHandle,
    MAX_WINDOW_LOGICAL_COORDINATE, MAX_WINDOW_LOGICAL_DIMENSION, Point, PopoverAnchor,
    PopoverConstraintAdjustment, PopoverGravity, PopoverOptions, Rect, ResolvedAnchorPlacement,
    Size, StateAccessor, Task, View, ViewContext, WindowBackgroundAppearance, WindowCommandError,
    WindowHandle, WindowOptions, anchor_placement, button, div,
};

/// Maximum hover open or close delay a popover trigger may declare.
pub const MAX_POPOVER_HOVER_DELAY: Duration = Duration::from_secs(10);
/// Default delay before a hovered popover trigger opens, matching Base UI's 300 ms.
pub const DEFAULT_POPOVER_HOVER_DELAY: Duration = Duration::from_millis(300);

/// Maximum structural distance between a popover trigger and its positioner.
pub const MAX_POPOVER_SIDE_OFFSET: f32 = 256.0;
/// Maximum cross-axis shift applied to a popover before collision handling runs.
pub const MAX_POPOVER_ALIGN_OFFSET: f32 = 4_096.0;
/// Maximum collision padding kept between a popover and the window viewport edge.
pub const MAX_POPOVER_COLLISION_PADDING: f32 = 512.0;
/// Maximum declared edge length of a framework-positioned popover arrow.
pub const MAX_POPOVER_ARROW_SIZE: f32 = 256.0;

const MAX_POPOVER_ANCHOR_GAP: f32 = MAX_POPOVER_SIDE_OFFSET;
const MAX_POPOVER_VIEWPORT_MARGIN: f32 = MAX_POPOVER_COLLISION_PADDING;
const DEFAULT_POPOVER_ANCHOR_GAP: f32 = 6.0;
const DEFAULT_POPOVER_VIEWPORT_MARGIN: f32 = 8.0;
const POPOVER_POSITIONER_ID_TAG: u64 = 0x847d_5a0f_0f9b_31e7;
const POPOVER_BACKDROP_ID_TAG: u64 = 0x663e_a690_8d7f_c442;
const POPOVER_TITLE_ID_TAG: u64 = 0x23ce_95af_9dc3_481b;
const POPOVER_DESCRIPTION_ID_TAG: u64 = 0xa935_070d_46db_78c1;
const POPOVER_CLOSE_ID_TAG: u64 = 0xd55f_271c_bbd9_e6a4;
const POPOVER_ARROW_ID_TAG: u64 = 0x1f2c_6ad8_5e07_9b3d;
const POPOVER_VIEWPORT_ID_TAG: u64 = 0x74b0_e319_c68a_20f5;

/// Semantic content exposed by a controlled [`Popover`].
///
/// The value drives both the surface role and the trigger's AccessKit `has-popup` state. It does
/// not select a different renderer or retain component state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PopoverKind {
    #[default]
    Dialog,
    Menu,
    ListBox,
    Tree,
    Grid,
}

impl PopoverKind {
    const fn accessibility_role(self) -> AccessibilityRole {
        match self {
            Self::Dialog => AccessibilityRole::Dialog,
            Self::Menu => AccessibilityRole::Menu,
            Self::ListBox => AccessibilityRole::ListBox,
            Self::Tree => AccessibilityRole::Tree,
            Self::Grid => AccessibilityRole::Grid,
        }
    }

    const fn accessibility_popover(self) -> AccessibilityPopover {
        match self {
            Self::Dialog => AccessibilityPopover::Dialog,
            Self::Menu => AccessibilityPopover::Menu,
            Self::ListBox => AccessibilityPopover::ListBox,
            Self::Tree => AccessibilityPopover::Tree,
            Self::Grid => AccessibilityPopover::Grid,
        }
    }
}

/// A copyable declaration descriptor for one controlled, unstyled popover.
///
/// The application owns `open`, every visual declaration, and the listener that changes state.
/// QuickGUI owns stable part identities, anchored portal geometry, topmost Escape/outside-press
/// dismissal, click-through prevention, focus movement/restoration, and accessibility relations.
/// Mount either [`Self::positioner_with`] plus [`Self::popup_with`], or the merged
/// [`Self::surface_with`], only while [`Self::is_open`] is true.
///
/// The descriptor retains no allocation, component store, observer, task, timer, animation, or
/// idle scheduler source.
#[derive(Clone, Copy, Debug, PartialEq)]
#[must_use = "a Popover descriptor has no effect until one of its parts is mounted"]
pub struct Popover {
    trigger_id: ElementId,
    surface_id: ElementId,
    open: bool,
    kind: PopoverKind,
    placement: AnchorPlacement,
    anchor: PopoverAnchorSource,
    anchor_gap: f32,
    align_offset: f32,
    viewport_margin: f32,
    sticky: bool,
    modal: bool,
    arrow_size: f32,
    arrow_padding: f32,
    resolved: Option<ResolvedAnchorPlacement>,
    initial_focus: Option<FocusHandle>,
    dismiss_on_escape: bool,
    dismiss_on_pointer_outside: bool,
}

/// What a popover positions itself against.
///
/// Base UI's `anchor` prop lets a popup track something other than the element that opened it — a
/// selected row, a caret rectangle, a pointer position. QuickGUI keeps the trigger as the default
/// and makes the override explicit, so the trigger's own expanded and controls relationships are
/// never affected by where the surface is placed.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum PopoverAnchorSource {
    #[default]
    Trigger,
    Element(ElementId),
    Point(Point),
}

impl Popover {
    pub fn new(
        trigger_id: impl Into<ElementId>,
        surface_id: impl Into<ElementId>,
        open: bool,
    ) -> Self {
        Self {
            trigger_id: trigger_id.into(),
            surface_id: surface_id.into(),
            open,
            kind: PopoverKind::Dialog,
            placement: AnchorPlacement::BottomStart,
            anchor: PopoverAnchorSource::Trigger,
            anchor_gap: DEFAULT_POPOVER_ANCHOR_GAP,
            align_offset: 0.0,
            viewport_margin: DEFAULT_POPOVER_VIEWPORT_MARGIN,
            sticky: true,
            modal: false,
            arrow_size: 0.0,
            arrow_padding: 0.0,
            resolved: None,
            initial_focus: None,
            dismiss_on_escape: true,
            dismiss_on_pointer_outside: true,
        }
    }

    /// Replace the controlled open flag, Base UI's Root `open` prop.
    ///
    /// The descriptor is copied per frame, so a component that retains the open flag itself can
    /// build one descriptor at construction time and stamp the current value onto it here.
    pub const fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    pub const fn kind(mut self, kind: PopoverKind) -> Self {
        self.kind = kind;
        self
    }

    pub const fn placement(mut self, placement: AnchorPlacement) -> Self {
        self.placement = placement;
        self
    }

    /// Prefer one side of the anchor, keeping the current cross-axis alignment.
    ///
    /// This is the side half of [`Self::placement`], matching Base UI's `side` prop. A side is a
    /// preference: QuickGUI flips to the opposite side when the preferred one does not fit, and
    /// [`Self::resolved_side`] reports what actually happened.
    pub const fn side(self, side: AnchorSide) -> Self {
        self.placement(anchor_placement(side, AnchorAlign::of(self.placement)))
    }

    /// Prefer a cross-axis alignment, keeping the current side.
    ///
    /// This is the alignment half of [`Self::placement`], matching Base UI's `align` prop.
    pub const fn align(self, align: AnchorAlign) -> Self {
        self.placement(anchor_placement(AnchorSide::of(self.placement), align))
    }

    /// Set the distance between the anchor and the positioner, Base UI's `sideOffset`.
    ///
    /// This is the Base UI-named alias of [`Self::anchor_gap`]. Both names stay supported and write
    /// the same bounded value.
    pub fn side_offset(self, offset: f32) -> Self {
        self.anchor_gap(offset)
    }

    /// Shift the popover along its cross axis before collision handling, Base UI's `alignOffset`.
    ///
    /// Positive values move right on a top or bottom placement and down on a left or right
    /// placement. The shift runs before the surface is clamped into the viewport, so it can slide a
    /// popup along its trigger but never push it off screen.
    pub fn align_offset(mut self, offset: f32) -> Self {
        self.align_offset = finite_clamped(
            offset,
            -MAX_POPOVER_ALIGN_OFFSET,
            MAX_POPOVER_ALIGN_OFFSET,
            0.0,
        );
        self
    }

    /// Set the collision padding kept inside the window viewport, Base UI's `collisionPadding`.
    ///
    /// This is the Base UI-named alias of [`Self::viewport_margin`]. Both names stay supported.
    pub fn collision_padding(self, padding: f32) -> Self {
        self.viewport_margin(padding)
    }

    /// Choose whether the popover is kept inside the collision viewport, Base UI's `sticky`.
    ///
    /// QuickGUI keeps anchored surfaces on screen by default, which is Base UI's sticky behavior.
    /// Passing `false` locks the popover to its anchor so it travels off screen with a scrolling
    /// row instead of detaching from it.
    pub const fn sticky(mut self, sticky: bool) -> Self {
        self.sticky = sticky;
        self
    }

    /// Position against a different mounted element instead of the trigger, Base UI's `anchor`.
    pub fn anchor_element(mut self, anchor: impl Into<ElementId>) -> Self {
        self.anchor = PopoverAnchorSource::Element(anchor.into());
        self
    }

    /// Position against a logical window point instead of the trigger, Base UI's virtual `anchor`.
    pub fn anchor_point(mut self, anchor: Point) -> Self {
        self.anchor = PopoverAnchorSource::Point(Point::new(
            finite_clamped(
                anchor.x,
                -MAX_WINDOW_LOGICAL_COORDINATE,
                MAX_WINDOW_LOGICAL_COORDINATE,
                0.0,
            ),
            finite_clamped(
                anchor.y,
                -MAX_WINDOW_LOGICAL_COORDINATE,
                MAX_WINDOW_LOGICAL_COORDINATE,
                0.0,
            ),
        ));
        self
    }

    /// Position against the trigger again, undoing an anchor override.
    pub const fn anchor_trigger(mut self) -> Self {
        self.anchor = PopoverAnchorSource::Trigger;
        self
    }

    /// Make the open popover modal, Base UI's `modal` prop.
    ///
    /// A modal popover contains Tab focus inside the popup and expects a mounted
    /// [`Self::backdrop_with`] to absorb outside pointer input. QuickGUI adds no dimming: the
    /// backdrop stays an invisible caller-owned element until the application paints it.
    pub const fn modal(mut self, modal: bool) -> Self {
        self.modal = modal;
        self
    }

    /// Whether this popover contains focus while open.
    pub const fn is_modal(self) -> bool {
        self.modal
    }

    /// Declare the edge length of a framework-positioned arrow.
    ///
    /// [`Self::arrow_with`] centers an arrow of this size on the anchor. Leaving it at zero pins
    /// the arrow to the resolved edge without a cross-axis offset, which is what an application
    /// that centers the arrow with its own layout wants.
    pub fn arrow_size(mut self, size: f32) -> Self {
        self.arrow_size = finite_clamped(size, 0.0, MAX_POPOVER_ARROW_SIZE, 0.0);
        self
    }

    /// Keep a framework-positioned arrow this far from the popup's corners, Base UI's
    /// `arrowPadding`.
    pub fn arrow_padding(mut self, padding: f32) -> Self {
        self.arrow_padding = finite_clamped(padding, 0.0, MAX_POPOVER_ARROW_SIZE, 0.0);
        self
    }

    /// Adopt the placement QuickGUI resolved for this popover on the previous painted frame.
    ///
    /// Store an [`AnchorPlacementHandle`] on the view next to `open`, mount the positioner with
    /// [`Self::tracked_positioner_with`] or [`Self::tracked_surface_with`], and pass the handle
    /// here while declaring the next frame. [`Self::arrow_with`], [`Self::resolved_side`],
    /// [`Self::resolved_align`], and [`Self::state`] then follow the placement the popover really
    /// used instead of the declared preference.
    ///
    /// QuickGUI requests exactly one correcting frame when the resolved placement changes, so a
    /// flipped popover settles immediately and an unflipped one adds no redraw source.
    pub fn track_placement(mut self, placement: &AnchorPlacementHandle) -> Self {
        self.resolved = placement.resolved();
        self
    }

    /// Adopt an already-read placement report.
    ///
    /// Use this when the application keeps the snapshot itself rather than the handle.
    pub const fn resolved_placement(mut self, resolved: Option<ResolvedAnchorPlacement>) -> Self {
        self.resolved = resolved;
        self
    }

    /// The side the popover actually opened on, or the declared preference before the first frame.
    pub fn resolved_side(self) -> AnchorSide {
        AnchorSide::of(self.resolved_anchor_placement())
    }

    /// The cross-axis alignment the popover actually used, or the declared preference.
    pub fn resolved_align(self) -> AnchorAlign {
        AnchorAlign::of(self.resolved_anchor_placement())
    }

    /// The full placement the popover actually used, or the declared preference.
    pub fn resolved_anchor_placement(self) -> AnchorPlacement {
        self.resolved
            .map_or(self.placement, |resolved| resolved.placement)
    }

    /// A copyable snapshot of what Base UI exposes as `data-*` attributes and CSS variables.
    ///
    /// The application styles from this: it picks a transform origin from
    /// [`PopoverPartState::side`], hides a popup whose anchor scrolled away using
    /// [`PopoverPartState::anchor_hidden`], matches the trigger width with
    /// [`PopoverPartState::anchor_width`], and caps a scrolling viewport with
    /// [`PopoverPartState::available_height`]. Every measured field stays zero until the popover
    /// has been painted once with a bound placement handle.
    pub fn state(self) -> PopoverPartState {
        let measured = self.resolved.unwrap_or_default();
        let reported = self.resolved.is_some();
        PopoverPartState {
            open: self.open,
            modal: self.modal,
            side: self.resolved_side(),
            align: self.resolved_align(),
            anchor_hidden: reported && measured.anchor_hidden,
            anchor_width: if reported { measured.anchor.width } else { 0.0 },
            anchor_height: if reported {
                measured.anchor.height
            } else {
                0.0
            },
            available_width: if reported {
                measured.available.width
            } else {
                0.0
            },
            available_height: if reported {
                measured.available.height
            } else {
                0.0
            },
        }
    }

    /// Set the structural distance between the trigger and positioner.
    pub fn anchor_gap(mut self, gap: f32) -> Self {
        self.anchor_gap =
            finite_clamped(gap, 0.0, MAX_POPOVER_ANCHOR_GAP, DEFAULT_POPOVER_ANCHOR_GAP);
        self
    }

    /// Set the structural collision margin inside the current window viewport.
    pub fn viewport_margin(mut self, margin: f32) -> Self {
        self.viewport_margin = finite_clamped(
            margin,
            0.0,
            MAX_POPOVER_VIEWPORT_MARGIN,
            DEFAULT_POPOVER_VIEWPORT_MARGIN,
        );
        self
    }

    /// Prefer a mounted popover descendant instead of the popover root when opening.
    pub fn initial_focus(mut self, focus: impl Into<ElementId>) -> Self {
        self.initial_focus = Some(FocusHandle::new(focus));
        self
    }

    pub const fn dismiss_on_escape(mut self, dismiss: bool) -> Self {
        self.dismiss_on_escape = dismiss;
        self
    }

    pub const fn dismiss_on_pointer_outside(mut self, dismiss: bool) -> Self {
        self.dismiss_on_pointer_outside = dismiss;
        self
    }

    pub const fn is_open(self) -> bool {
        self.open
    }

    pub const fn trigger_id(self) -> ElementId {
        self.trigger_id
    }

    /// Stable popover identity retained under the existing `surface` name.
    pub const fn surface_id(self) -> ElementId {
        self.surface_id
    }

    pub const fn popover_id(self) -> ElementId {
        self.surface_id
    }

    pub fn positioner_id(self) -> ElementId {
        derived_popover_id(self.surface_id, self.trigger_id, POPOVER_POSITIONER_ID_TAG)
    }

    pub fn backdrop_id(self) -> ElementId {
        derived_popover_id(self.surface_id, self.trigger_id, POPOVER_BACKDROP_ID_TAG)
    }

    pub fn title_id(self) -> ElementId {
        derived_popover_id(self.surface_id, self.trigger_id, POPOVER_TITLE_ID_TAG)
    }

    pub fn description_id(self) -> ElementId {
        derived_popover_id(self.surface_id, self.trigger_id, POPOVER_DESCRIPTION_ID_TAG)
    }

    pub fn close_id(self) -> ElementId {
        derived_popover_id(self.surface_id, self.trigger_id, POPOVER_CLOSE_ID_TAG)
    }

    /// Stable identity of the framework-positioned arrow part.
    pub fn arrow_id(self) -> ElementId {
        derived_popover_id(self.surface_id, self.trigger_id, POPOVER_ARROW_ID_TAG)
    }

    /// Stable identity of the scrolling viewport mounted inside the popup.
    pub fn viewport_id(self) -> ElementId {
        derived_popover_id(self.surface_id, self.trigger_id, POPOVER_VIEWPORT_ID_TAG)
    }

    pub fn trigger_focus(self) -> FocusHandle {
        FocusHandle::new(self.trigger_id)
    }

    pub fn surface_focus(self) -> FocusHandle {
        FocusHandle::new(self.surface_id)
    }

    pub fn popover_focus(self) -> FocusHandle {
        self.surface_focus()
    }

    /// Move focus to the declared initial target, or the popover root, in the same controlled update.
    pub fn focus_surface(self, cx: &mut EventContext) {
        cx.focus(self.initial_focus.unwrap_or_else(|| self.popover_focus()));
    }

    /// Group the trigger and floating content in an ordinary, unstyled view.
    pub fn root(self) -> Element {
        div()
    }

    /// Use an existing view as the popover's composition root.
    pub fn root_with(self, root: Element) -> Element {
        root
    }

    /// Decorate an application-owned trigger without adding appearance.
    pub fn trigger_with(self, trigger: Element) -> Element {
        let trigger = trigger
            .id(self.trigger_id)
            .focusable()
            .accessibility_role(AccessibilityRole::Button)
            .user_select_none()
            .app_region_no_drag()
            .cursor_default()
            .accessibility_expanded(self.open)
            .accessibility_has_popover(self.kind.accessibility_popover());
        if self.open {
            trigger.accessibility_controls(self.surface_id)
        } else {
            trigger
        }
    }

    /// Create an unstyled semantic trigger root.
    pub fn trigger(self) -> Element {
        self.trigger_with(button())
    }

    /// Decorate the caller-owned portal/positioner without adding popover appearance.
    ///
    /// QuickGUI's retained overlay node is itself the portal, so this part combines the Base
    /// UI-style Portal and Positioner boundary without introducing a full-window wrapper that
    /// would block unrelated pointer input.
    pub fn positioner_with(self, positioner: Element) -> Element {
        self.anchored(positioner.id(self.positioner_id()))
            .app_region_no_drag()
            .cursor_default()
    }
    /// Create the unstyled positioner part. Use [`Self::positioner_with`] to supply an existing element.
    pub fn positioner(self) -> Element {
        self.positioner_with(crate::div())
    }

    /// Decorate the positioner and publish the placement it resolves to.
    ///
    /// This is [`Self::positioner_with`] plus
    /// [`crate::Element::report_anchor_placement`]: the handle receives the side, alignment, anchor
    /// rectangle, and remaining room the popover actually used, and [`Self::track_placement`] feeds
    /// it back into the descriptor on the next frame.
    pub fn tracked_positioner_with(
        self,
        positioner: Element,
        placement: &AnchorPlacementHandle,
    ) -> Element {
        self.positioner_with(positioner.report_anchor_placement(placement.clone()))
    }
    /// Create the unstyled tracked positioner part. Use [`Self::tracked_positioner_with`] to supply an existing element.
    pub fn tracked_positioner(self, placement: &AnchorPlacementHandle) -> Element {
        self.tracked_positioner_with(crate::div(), placement)
    }

    /// Base UI's Portal name for the combined portal/positioner part.
    ///
    /// QuickGUI's retained overlay node is itself the portal, so Portal and Positioner are one
    /// element; both names decorate it identically.
    pub fn portal_with(self, portal: Element) -> Element {
        self.positioner_with(portal)
    }
    /// Create the unstyled portal part. Use [`Self::portal_with`] to supply an existing element.
    pub fn portal(self) -> Element {
        self.portal_with(crate::div())
    }

    /// Apply the anchored geometry this popover declares to a caller-owned element.
    fn anchored(self, element: Element) -> Element {
        let element = match self.anchor {
            PopoverAnchorSource::Trigger => element.anchor_to(self.trigger_id, self.placement),
            PopoverAnchorSource::Element(anchor) => element.anchor_to(anchor, self.placement),
            PopoverAnchorSource::Point(anchor) => element.anchor_at(anchor, self.placement),
        };
        element
            .anchor_gap(self.anchor_gap)
            .anchor_align_offset(self.align_offset)
            .viewport_margin(self.viewport_margin)
            .anchor_sticky(self.sticky)
    }

    /// Decorate an application-owned popover without adding layout or appearance.
    ///
    /// The popover emits [`crate::Event::Dismiss`] under [`Self::surface_id`] for every enabled
    /// dismissal path. Mounted title/description parts are related without copying their text.
    pub fn popup_with(self, popover: Element) -> Element {
        let mut popover = popover
            .id(self.surface_id)
            .restore_focus_to(self.trigger_focus())
            .track_focus(self.popover_focus())
            .accessibility_role(self.kind.accessibility_role())
            .accessibility_labelled_by(self.title_id())
            .accessibility_described_by(self.description_id())
            .block_pointer()
            .app_region_no_drag()
            .cursor_default();
        if self.dismiss_on_escape {
            popover = popover.dismiss_on_escape();
        }
        if self.dismiss_on_pointer_outside {
            popover = popover.dismiss_on_pointer_outside();
        }
        if self.modal {
            popover = popover.focus_trap();
        }
        popover
    }
    /// Create the unstyled popup part. Use [`Self::popup_with`] to supply an existing element.
    pub fn popup(self) -> Element {
        self.popup_with(crate::div())
    }

    /// Decorate one caller-owned element as both positioner and popover.
    ///
    /// This compact form has the same unstyled contract as composing [`Self::positioner_with`]
    /// around [`Self::popup_with`]. Use separate parts when the application needs to animate or
    /// size the positioner independently from popover presentation.
    pub fn surface_with(self, surface: Element) -> Element {
        self.anchored(self.popup_with(surface))
    }

    /// Decorate the merged surface and publish the placement it resolves to.
    pub fn tracked_surface_with(
        self,
        surface: Element,
        placement: &AnchorPlacementHandle,
    ) -> Element {
        self.surface_with(surface.report_anchor_placement(placement.clone()))
    }
    /// Create the unstyled tracked surface part. Use [`Self::tracked_surface_with`] to supply an existing element.
    pub fn tracked_surface(self, placement: &AnchorPlacementHandle) -> Element {
        self.tracked_surface_with(crate::div(), placement)
    }

    /// Create an unstyled merged positioner/popover root.
    pub fn surface(self) -> Element {
        self.surface_with(div())
    }

    /// Position a caller-owned arrow on the edge the popover actually opened against.
    ///
    /// The arrow is absolutely positioned inside the popup, pinned to the edge that faces the
    /// anchor and centered on the anchor along the cross axis. When [`Self::track_placement`] has
    /// supplied a placement report the offsets follow the real placement, including a flip — never
    /// the declared preference. Before the first painted frame, or without a bound handle, the arrow
    /// follows the declared side and stays centered on the popup, and QuickGUI's one correcting
    /// frame moves it as soon as the real placement is known.
    ///
    /// The application owns the arrow's size, shape, rotation, and color. Declare
    /// [`Self::arrow_size`] when QuickGUI should center an arrow of a known edge length, and
    /// [`Self::arrow_padding`] to keep it clear of the popup's corners. The arrow is hidden from
    /// assistive technology: it is decoration attached to a surface that already carries the
    /// relationship.
    pub fn arrow_with(self, arrow: Element) -> Element {
        let arrow = arrow
            .id(self.arrow_id())
            .absolute()
            .accessibility_hidden(true)
            .app_region_no_drag();
        let side = self.resolved_side();
        let offset = self.arrow_cross_offset(side);
        match (side, offset) {
            // The popup opened below its anchor, so the arrow belongs on the popup's top edge.
            (AnchorSide::Bottom, Some(offset)) => arrow.top(0.0).left(offset),
            (AnchorSide::Bottom, None) => arrow.top(0.0),
            (AnchorSide::Top, Some(offset)) => arrow.bottom(0.0).left(offset),
            (AnchorSide::Top, None) => arrow.bottom(0.0),
            (AnchorSide::Right, Some(offset)) => arrow.left(0.0).top(offset),
            (AnchorSide::Right, None) => arrow.left(0.0),
            (AnchorSide::Left, Some(offset)) => arrow.right(0.0).top(offset),
            (AnchorSide::Left, None) => arrow.right(0.0),
        }
    }
    /// Create the unstyled arrow part. Use [`Self::arrow_with`] to supply an existing element.
    pub fn arrow(self) -> Element {
        self.arrow_with(crate::div())
    }

    /// The popup-local cross-axis offset of a centered arrow, once a placement has been reported.
    fn arrow_cross_offset(self, side: AnchorSide) -> Option<f32> {
        let resolved = self.resolved?;
        let (anchor_center, popup_origin, popup_extent) = if side.is_vertical() {
            (
                resolved.anchor.x + resolved.anchor.width * 0.5,
                resolved.bounds.x,
                resolved.bounds.width,
            )
        } else {
            (
                resolved.anchor.y + resolved.anchor.height * 0.5,
                resolved.bounds.y,
                resolved.bounds.height,
            )
        };
        let leading = anchor_center - popup_origin - self.arrow_size * 0.5;
        let last = popup_extent - self.arrow_size - self.arrow_padding;
        if !leading.is_finite() || !last.is_finite() {
            return None;
        }
        Some(leading.clamp(self.arrow_padding, last.max(self.arrow_padding)))
    }

    /// Decorate a caller-owned scrolling viewport mounted inside the popup.
    ///
    /// Base UI's Viewport clips content that changes size between triggers so the popup can animate
    /// between them. QuickGUI supplies the stable identity and the scroll container; height,
    /// padding, and motion stay application-owned — cap it with
    /// [`PopoverPartState::available_height`] to keep a long popup inside the window.
    pub fn viewport_with(self, viewport: Element) -> Element {
        viewport
            .id(self.viewport_id())
            .overflow_y_scroll()
            .app_region_no_drag()
    }
    /// Create the unstyled viewport part. Use [`Self::viewport_with`] to supply an existing element.
    pub fn viewport(self) -> Element {
        self.viewport_with(crate::div())
    }

    /// Decorate an optional caller-painted viewport backdrop.
    pub fn backdrop_with(self, backdrop: Element) -> Element {
        backdrop
            .id(self.backdrop_id())
            .overlay()
            .inset_0()
            .size_full()
            .app_region_no_drag()
            .cursor_default()
            .accessibility_hidden(true)
    }
    /// Create the unstyled backdrop part. Use [`Self::backdrop_with`] to supply an existing element.
    pub fn backdrop(self) -> Element {
        self.backdrop_with(crate::div())
    }

    /// Assign the stable mounted label target used by the popover.
    pub fn title_with(self, title: Element) -> Element {
        title.id(self.title_id())
    }
    /// Create the unstyled title part. Use [`Self::title_with`] to supply an existing element.
    pub fn title(self) -> Element {
        self.title_with(crate::div())
    }

    /// Assign the stable mounted description target used by the popover.
    pub fn description_with(self, description: Element) -> Element {
        description.id(self.description_id())
    }
    /// Create the unstyled description part. Use [`Self::description_with`] to supply an existing element.
    pub fn description(self) -> Element {
        self.description_with(crate::div())
    }

    /// Decorate a caller-owned close control with button behavior and no visual defaults.
    pub fn close_with(self, label: impl Into<Arc<str>>, close: Element) -> Element {
        close
            .id(self.close_id())
            .clickable()
            .cursor_default()
            .accessibility_role(AccessibilityRole::Button)
            .accessibility_label(label)
            .app_region_no_drag()
            .user_select_none()
    }
    /// Create the unstyled close part. Use [`Self::close_with`] to supply an existing element.
    pub fn close(self, label: impl Into<Arc<str>>) -> Element {
        self.close_with(label, crate::button())
    }
}

/// Which hoverable part of a popover the pointer entered or left.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PopoverHoverPart {
    Trigger,
    Popup,
}

/// The transition a pending hover deadline will apply when it expires.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PopoverHoverPhase {
    Open,
    Close,
}

/// Bounded hover-open state for one controlled popover, Base UI's Trigger `openOnHover`.
///
/// The application still owns whether the popover is open: this state decides *when* that changes
/// and calls back through the accessor the decorators were given. A hovered trigger opens after
/// [`Self::delay`], and the popover closes after [`Self::close_delay`] once neither the trigger nor
/// a hoverable popup is hovered, so the pointer can cross the gap between them.
///
/// Every delay is an exact one-shot deadline: entering and leaving cancels the outstanding task
/// rather than polling, and a settled popover owns no timer, animation, or idle scheduler source.
/// A zero delay applies the change in the same controlled update with no task at all.
#[derive(Debug)]
pub struct PopoverHoverState {
    open: bool,
    delay: Duration,
    close_delay: Duration,
    trigger_hovered: bool,
    popup_hovered: bool,
    hoverable_popup: bool,
    pending: Option<PopoverHoverPhase>,
    generation: u64,
    task: Option<Task<()>>,
}

impl Default for PopoverHoverState {
    fn default() -> Self {
        Self::new()
    }
}

impl PopoverHoverState {
    /// Create a closed hover state with Base UI's default open delay and immediate close.
    pub fn new() -> Self {
        Self {
            open: false,
            delay: DEFAULT_POPOVER_HOVER_DELAY,
            close_delay: Duration::ZERO,
            trigger_hovered: false,
            popup_hovered: false,
            hoverable_popup: true,
            pending: None,
            generation: 0,
            task: None,
        }
    }

    /// Set how long a hovered trigger waits before opening, Base UI's `delay`.
    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay.min(MAX_POPOVER_HOVER_DELAY);
        self
    }

    /// Set how long the popover waits before closing after the pointer leaves, Base UI's
    /// `closeDelay`.
    pub fn close_delay(mut self, delay: Duration) -> Self {
        self.close_delay = delay.min(MAX_POPOVER_HOVER_DELAY);
        self
    }

    /// Choose whether hovering the popup itself keeps the popover open.
    ///
    /// This is Base UI's `hoverable` popup prop. A non-hoverable popup closes as soon as the
    /// pointer leaves the trigger, which is what a passive preview surface wants.
    pub const fn hoverable_popup(mut self, hoverable: bool) -> Self {
        self.hoverable_popup = hoverable;
        self
    }

    pub const fn is_open(&self) -> bool {
        self.open
    }

    /// Whether a hover deadline is outstanding.
    pub const fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Whether the trigger or a hoverable popup currently has the pointer.
    pub const fn is_hovered(&self) -> bool {
        self.trigger_hovered || (self.hoverable_popup && self.popup_hovered)
    }

    /// Open immediately, cancelling any outstanding deadline.
    ///
    /// Returns whether the open state changed, so a click listener can invalidate exactly once.
    pub fn open_now(&mut self) -> bool {
        self.cancel_pending();
        !std::mem::replace(&mut self.open, true)
    }

    /// Close immediately, cancelling any outstanding deadline.
    pub fn close_now(&mut self) -> bool {
        self.cancel_pending();
        std::mem::replace(&mut self.open, false)
    }

    /// Toggle the popover immediately, cancelling any outstanding deadline.
    pub fn toggle(&mut self) -> bool {
        if self.open {
            self.close_now()
        } else {
            self.open_now()
        }
    }

    /// Drop any outstanding hover deadline without changing the open state.
    pub fn cancel_pending(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.pending = None;
        if let Some(task) = self.task.take() {
            task.cancel();
        }
    }

    /// Decorate a caller-owned trigger with hover opening.
    ///
    /// This is the `fn`-pointer entry point for a view that owns one popover per field. A host that
    /// renders many declared popovers through one view uses [`Self::trigger_with_accessor`].
    pub fn trigger_with<V: 'static>(
        &self,
        cx: &mut ViewContext<'_, V>,
        popover: Popover,
        access: fn(&mut V) -> &mut Self,
        trigger: Element,
    ) -> Element {
        self.trigger_with_accessor(cx, popover, StateAccessor::from(access), trigger)
    }
    /// Create the unstyled trigger part. Use [`Self::trigger_with`] to supply an existing element.
    pub fn trigger<V: 'static>(
        &self,
        cx: &mut ViewContext<'_, V>,
        popover: Popover,
        access: fn(&mut V) -> &mut Self,
    ) -> Element {
        self.trigger_with(cx, popover, access, crate::button())
    }

    /// Decorate a caller-owned trigger with hover opening through a per-instance accessor.
    pub fn trigger_with_accessor<V: 'static>(
        &self,
        cx: &mut ViewContext<'_, V>,
        popover: Popover,
        access: StateAccessor<V, Self>,
        trigger: Element,
    ) -> Element {
        let hovered =
            self.hover_listener(cx, popover.trigger_id(), PopoverHoverPart::Trigger, access);
        popover.trigger_with(trigger).on_hover(hovered)
    }

    /// Decorate the caller-owned popup so the pointer may cross into it without closing.
    pub fn popup_with<V: 'static>(
        &self,
        cx: &mut ViewContext<'_, V>,
        popover: Popover,
        access: fn(&mut V) -> &mut Self,
        popup: Element,
    ) -> Element {
        self.popup_with_accessor(cx, popover, StateAccessor::from(access), popup)
    }
    /// Create the unstyled popup part. Use [`Self::popup_with`] to supply an existing element.
    pub fn popup<V: 'static>(
        &self,
        cx: &mut ViewContext<'_, V>,
        popover: Popover,
        access: fn(&mut V) -> &mut Self,
    ) -> Element {
        self.popup_with(cx, popover, access, crate::div())
    }

    /// Decorate the caller-owned popup through a per-instance accessor.
    pub fn popup_with_accessor<V: 'static>(
        &self,
        cx: &mut ViewContext<'_, V>,
        popover: Popover,
        access: StateAccessor<V, Self>,
        popup: Element,
    ) -> Element {
        let hovered =
            self.hover_listener(cx, popover.popover_id(), PopoverHoverPart::Popup, access);
        popover.popup_with(popup).on_hover(hovered)
    }

    fn hover_listener<V: 'static>(
        &self,
        cx: &mut ViewContext<'_, V>,
        id: ElementId,
        part: PopoverHoverPart,
        access: StateAccessor<V, Self>,
    ) -> crate::HoverListener<V> {
        cx.hover_listener(id, move |view, hovered, cx| {
            let access = access.clone();
            let hovered = *hovered;
            access
                .clone()
                .get(view)
                .record_hover(part, hovered, access, cx);
        })
    }

    fn record_hover<V: 'static>(
        &mut self,
        part: PopoverHoverPart,
        hovered: bool,
        access: StateAccessor<V, Self>,
        cx: &mut EventContext,
    ) {
        match part {
            PopoverHoverPart::Trigger => self.trigger_hovered = hovered,
            PopoverHoverPart::Popup => self.popup_hovered = hovered,
        }
        let wanted = if self.is_hovered() {
            PopoverHoverPhase::Open
        } else {
            PopoverHoverPhase::Close
        };
        let already = match wanted {
            PopoverHoverPhase::Open => self.open,
            PopoverHoverPhase::Close => !self.open,
        };
        if already {
            // The pointer returned before the deadline expired: drop it rather than reopening.
            if self.pending.is_some() {
                self.cancel_pending();
            }
            return;
        }
        if self.pending == Some(wanted) {
            return;
        }
        let delay = match wanted {
            PopoverHoverPhase::Open => self.delay,
            PopoverHoverPhase::Close => self.close_delay,
        };
        self.schedule(wanted, delay, access, cx);
    }

    fn schedule<V: 'static>(
        &mut self,
        phase: PopoverHoverPhase,
        delay: Duration,
        access: StateAccessor<V, Self>,
        cx: &mut EventContext,
    ) {
        self.cancel_pending();
        if delay.is_zero() {
            if self.apply(phase) {
                cx.invalidate();
            }
            return;
        }
        self.pending = Some(phase);
        let generation = self.generation;
        let spawned = cx.spawn::<V, _, _, _>(move |task_cx: AsyncViewContext<V>| async move {
            if task_cx.sleep(delay).await.is_err() {
                return;
            }
            let _ = task_cx
                .update(move |view, cx| {
                    let state = access.get(view);
                    if state.generation != generation || state.pending != Some(phase) {
                        return;
                    }
                    state.pending = None;
                    state.task = None;
                    if state.apply(phase) {
                        cx.invalidate();
                    }
                })
                .await;
        });
        match spawned {
            Ok(task) => self.task = Some(task),
            Err(_) => {
                // A window that cannot own another foreground task still gets correct behavior;
                // only the delay is lost.
                self.pending = None;
                if self.apply(phase) {
                    cx.invalidate();
                }
            }
        }
    }

    fn apply(&mut self, phase: PopoverHoverPhase) -> bool {
        let open = phase == PopoverHoverPhase::Open;
        std::mem::replace(&mut self.open, open) != open
    }
}

/// A copyable render-state snapshot for one controlled popover.
///
/// Base UI exposes this information as `data-open`, `data-side`, `data-align`,
/// `data-anchor-hidden`, and the `--anchor-width` / `--available-height` CSS variables. QuickGUI
/// has no style sheet, so the same facts arrive as fields the application reads while declaring
/// its own presentation. Build one with [`Popover::state`].
///
/// The measured fields are zero until the popover has been painted once with a bound
/// [`AnchorPlacementHandle`]; [`Self::is_measured`] says whether they can be trusted.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PopoverPartState {
    /// Whether the application currently declares the popover open.
    pub open: bool,
    /// Whether the popover contains focus and expects a backdrop.
    pub modal: bool,
    /// The side of the anchor the popup was actually placed on.
    pub side: AnchorSide,
    /// The cross-axis alignment the popup actually used.
    pub align: AnchorAlign,
    /// Whether the anchor left the collision viewport entirely.
    pub anchor_hidden: bool,
    /// Width of the anchor rectangle, for a popup that matches its trigger.
    pub anchor_width: f32,
    /// Height of the anchor rectangle.
    pub anchor_height: f32,
    /// Room left for the popup on the cross axis inside the collision viewport.
    pub available_width: f32,
    /// Room left for the popup between the anchor and the viewport edge on the resolved side.
    pub available_height: f32,
}

impl PopoverPartState {
    /// Whether the measured fields come from a real painted frame rather than defaults.
    pub fn is_measured(self) -> bool {
        self.anchor_width > 0.0
            || self.anchor_height > 0.0
            || self.available_width > 0.0
            || self.available_height > 0.0
    }
}

fn derived_popover_id(parent: ElementId, avoid: ElementId, tag: u64) -> ElementId {
    let mut hash = parent.as_u64() ^ tag;
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    for _ in 0..5 {
        if hash != 0 && hash != parent.as_u64() && hash != avoid.as_u64() && hash != u64::MAX {
            return ElementId::new(hash);
        }
        hash = hash.wrapping_add(tag | 1);
    }
    unreachable!("five distinct candidates cannot all match four reserved popover IDs")
}

/// An unstyled parent-owned popover window that may extend beyond its parent window.
///
/// [`Popover`] uses the parent window's existing overlay plane and is therefore physically
/// bounded by that WGPU surface. `SystemPopover` instead opens a borderless native child window
/// with its own WGPU surface. On macOS this is an `NSPanel` ordered above its parent; placement is
/// resolved against the display work area, not the parent content rectangle.
///
/// Popover size is explicit, matching the platform popover contract. The trigger rectangle itself is
/// resolved from retained element geometry by [`EventContext::open_system_popover`], so callers do
/// not duplicate coordinates or install a layout observer. Closing the child restores parent focus
/// to that retained trigger unless the trigger or parent was destroyed with it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SystemPopover {
    size: Size,
    placement: AnchorPlacement,
    gap: f32,
    viewport_margin: f32,
    offset: Point,
    constraints: PopoverConstraintAdjustment,
    dismiss_on_escape: bool,
    dismiss_on_pointer_outside: bool,
    grab: bool,
    accepts_key_focus: bool,
}

impl SystemPopover {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            size: Size::new(
                finite_clamped(width, 1.0, MAX_WINDOW_LOGICAL_DIMENSION, 240.0),
                finite_clamped(height, 1.0, MAX_WINDOW_LOGICAL_DIMENSION, 180.0),
            ),
            placement: AnchorPlacement::BottomStart,
            gap: DEFAULT_POPOVER_ANCHOR_GAP,
            viewport_margin: DEFAULT_POPOVER_VIEWPORT_MARGIN,
            offset: Point::ZERO,
            constraints: PopoverConstraintAdjustment::FIT,
            dismiss_on_escape: true,
            dismiss_on_pointer_outside: true,
            grab: true,
            accepts_key_focus: true,
        }
    }

    pub const fn size(self) -> Size {
        self.size
    }

    pub const fn placement(mut self, placement: AnchorPlacement) -> Self {
        self.placement = placement;
        self
    }

    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = finite_clamped(gap, 0.0, 512.0, 0.0);
        self
    }

    /// Set the collision margin inside the display work area.
    pub fn viewport_margin(mut self, margin: f32) -> Self {
        self.viewport_margin = finite_clamped(
            margin,
            0.0,
            MAX_POPOVER_VIEWPORT_MARGIN,
            DEFAULT_POPOVER_VIEWPORT_MARGIN,
        );
        self
    }

    pub const fn dismiss_on_escape(mut self, dismiss: bool) -> Self {
        self.dismiss_on_escape = dismiss;
        self
    }

    pub const fn dismiss_on_pointer_outside(mut self, dismiss: bool) -> Self {
        self.dismiss_on_pointer_outside = dismiss;
        self
    }

    pub fn offset(mut self, x: f32, y: f32) -> Self {
        self.offset = Point::new(
            finite_clamped(
                x,
                -MAX_WINDOW_LOGICAL_COORDINATE,
                MAX_WINDOW_LOGICAL_COORDINATE,
                0.0,
            ),
            finite_clamped(
                y,
                -MAX_WINDOW_LOGICAL_COORDINATE,
                MAX_WINDOW_LOGICAL_COORDINATE,
                0.0,
            ),
        );
        self
    }

    pub const fn constraint_adjustment(mut self, constraints: PopoverConstraintAdjustment) -> Self {
        self.constraints = constraints;
        self
    }

    /// Select menu-style focus and outside/Escape dismissal.
    ///
    /// Passive tooltips and preview surfaces use `grab(false)` and own no event monitor.
    pub const fn grab(mut self, grab: bool) -> Self {
        self.grab = grab;
        self.dismiss_on_escape = grab;
        self.dismiss_on_pointer_outside = grab;
        if grab {
            self.accepts_key_focus = true;
        }
        self
    }

    /// Allow or forbid native key-window ownership after pointer interaction.
    ///
    /// `false` keeps an interactive child panel permanently non-key and also disables grabbing,
    /// which lets an owner-window text input retain keyboard and IME focus.
    pub const fn accepts_key_focus(mut self, accepts_key_focus: bool) -> Self {
        self.accepts_key_focus = accepts_key_focus;
        if !accepts_key_focus {
            self.grab = false;
            self.dismiss_on_escape = false;
            self.dismiss_on_pointer_outside = false;
        }
        self
    }

    pub fn popover_options(self) -> PopoverOptions {
        let (anchor, gravity) = native_popover_placement(self.placement);
        let mut offset = self.offset;
        match self.placement {
            AnchorPlacement::TopStart | AnchorPlacement::Top | AnchorPlacement::TopEnd => {
                offset.y -= self.gap;
            }
            AnchorPlacement::BottomStart | AnchorPlacement::Bottom | AnchorPlacement::BottomEnd => {
                offset.y += self.gap;
            }
            AnchorPlacement::LeftStart | AnchorPlacement::Left | AnchorPlacement::LeftEnd => {
                offset.x -= self.gap;
            }
            AnchorPlacement::RightStart | AnchorPlacement::Right | AnchorPlacement::RightEnd => {
                offset.x += self.gap;
            }
        }
        PopoverOptions::new(Rect::ZERO)
            .anchor(anchor)
            .gravity(gravity)
            .constraint_adjustment(self.constraints)
            .offset(offset.x, offset.y)
            .grab(self.grab)
            .accepts_key_focus(self.accepts_key_focus)
            .viewport_margin(self.viewport_margin)
            .dismiss_on_escape(self.dismiss_on_escape)
            .dismiss_on_pointer_outside(self.dismiss_on_pointer_outside)
    }

    /// Build transparent, borderless window options without choosing application presentation.
    pub fn window_options(self, title: impl Into<String>) -> WindowOptions {
        WindowOptions::new(title)
            .size(self.size.width, self.size.height)
            .background(Color::TRANSPARENT)
            .window_background(WindowBackgroundAppearance::Transparent)
            .system_popover(self.popover_options())
    }

    /// Open the overflow-capable child using the latest retained bounds of `anchor`.
    pub fn open<V: View>(
        self,
        cx: &mut EventContext,
        anchor: impl Into<ElementId>,
        title: impl Into<String>,
        view: V,
    ) -> Result<WindowHandle, WindowCommandError> {
        cx.open_system_popover(anchor, self.window_options(title), view)
    }
}

impl Default for SystemPopover {
    fn default() -> Self {
        Self::new(240.0, 180.0)
    }
}

fn native_popover_placement(placement: AnchorPlacement) -> (PopoverAnchor, PopoverGravity) {
    match placement {
        AnchorPlacement::TopStart => (PopoverAnchor::TopLeft, PopoverGravity::TopRight),
        AnchorPlacement::Top => (PopoverAnchor::Top, PopoverGravity::Top),
        AnchorPlacement::TopEnd => (PopoverAnchor::TopRight, PopoverGravity::TopLeft),
        AnchorPlacement::BottomStart => (PopoverAnchor::BottomLeft, PopoverGravity::BottomRight),
        AnchorPlacement::Bottom => (PopoverAnchor::Bottom, PopoverGravity::Bottom),
        AnchorPlacement::BottomEnd => (PopoverAnchor::BottomRight, PopoverGravity::BottomLeft),
        AnchorPlacement::LeftStart => (PopoverAnchor::TopLeft, PopoverGravity::BottomLeft),
        AnchorPlacement::Left => (PopoverAnchor::Left, PopoverGravity::Left),
        AnchorPlacement::LeftEnd => (PopoverAnchor::BottomLeft, PopoverGravity::TopLeft),
        AnchorPlacement::RightStart => (PopoverAnchor::TopRight, PopoverGravity::BottomRight),
        AnchorPlacement::Right => (PopoverAnchor::Right, PopoverGravity::Right),
        AnchorPlacement::RightEnd => (PopoverAnchor::BottomRight, PopoverGravity::TopRight),
    }
}

fn finite_clamped(value: f32, minimum: f32, maximum: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(minimum, maximum)
    } else {
        fallback.clamp(minimum, maximum)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::AnchorTarget;
    use crate::{
        AppRegion, Application, Event, IntoElement, TestAppContext, View, ViewContext,
        WindowBounds, WindowKind, WindowOptions,
    };
    use taffy::style_helpers::length;

    #[test]
    fn descriptor_builds_paired_controlled_elements_without_retained_component_state() {
        // The descriptor stays a plain copyable value: identities, bounded geometry, and one
        // optional placement report, with no allocation, handle, or component store inside it.
        assert!(std::mem::size_of::<Popover>() <= 128);
        let closed = Popover::new("trigger", "surface", false).kind(PopoverKind::Menu);
        let closed_trigger = closed.trigger();
        assert_eq!(closed_trigger.explicit_id, Some("trigger".into()));
        assert_eq!(closed_trigger.accessibility.expanded, Some(false));
        assert_eq!(closed_trigger.accessibility.relations.controls(), None);
        assert_eq!(
            closed_trigger.accessibility.has_popover,
            Some(AccessibilityPopover::Menu)
        );
        assert_eq!(closed_trigger.app_region, Some(AppRegion::NoDrag));
        assert_eq!(closed_trigger.cursor_style, Some(crate::CursorStyle::Arrow));

        let open = Popover::new("trigger", "surface", true)
            .kind(PopoverKind::Menu)
            .placement(AnchorPlacement::TopEnd);
        let trigger = open.trigger();
        assert_eq!(trigger.accessibility.expanded, Some(true));
        assert_eq!(
            trigger.accessibility.relations.controls(),
            Some("surface".into())
        );

        let surface = open.surface();
        assert_eq!(surface.explicit_id, Some("surface".into()));
        assert_eq!(surface.accessibility.role, AccessibilityRole::Menu);
        assert_eq!(surface.dismiss_policy, crate::element::DismissPolicy::BOTH);
        assert!(surface.blocks_pointer);
        assert!(surface.focusable);
        assert_eq!(surface.restore_focus, Some(open.trigger_focus()));
        assert_eq!(surface.app_region, Some(AppRegion::NoDrag));
        assert!(surface.portal);
        assert_eq!(surface.visual.background, None);
        assert_eq!(surface.visual.border_color, None);
        assert_eq!(surface.visual.shadows, None);
        let anchor = surface.anchor.expect("popover anchor");
        assert_eq!(anchor.target, AnchorTarget::Element("trigger".into()));
        assert_eq!(anchor.placement, AnchorPlacement::TopEnd);
        assert_eq!(anchor.gap, DEFAULT_POPOVER_ANCHOR_GAP);
        assert_eq!(anchor.viewport_margin, DEFAULT_POPOVER_VIEWPORT_MARGIN);
    }

    #[test]
    fn split_parts_add_exact_behavior_without_appearance_tokens() {
        let popover = Popover::new("trigger", "surface", true)
            .kind(PopoverKind::ListBox)
            .placement(AnchorPlacement::BottomEnd)
            .anchor_gap(12.0)
            .viewport_margin(20.0)
            .initial_focus("first-option")
            .dismiss_on_escape(false);

        let trigger = popover.trigger_with(crate::div());
        assert_eq!(trigger.explicit_id, Some("trigger".into()));
        assert_eq!(trigger.accessibility.role, AccessibilityRole::Button);
        assert!(trigger.focusable);
        assert_eq!(trigger.visual.background, None);
        assert_eq!(trigger.visual.border_color, None);
        assert_eq!(trigger.cursor_style, Some(crate::CursorStyle::Arrow));

        let positioner = popover.positioner_with(crate::div());
        assert_eq!(positioner.explicit_id, Some(popover.positioner_id()));
        assert!(positioner.portal);
        assert!(positioner.blocks_pointer);
        assert_eq!(positioner.visual.background, None);
        assert_eq!(positioner.visual.border_color, None);
        let anchor = positioner.anchor.expect("popover positioner anchor");
        assert_eq!(anchor.target, AnchorTarget::Element("trigger".into()));
        assert_eq!(anchor.placement, AnchorPlacement::BottomEnd);
        assert_eq!(anchor.gap, 12.0);
        assert_eq!(anchor.viewport_margin, 20.0);

        let surface = popover.popup_with(crate::div());
        assert_eq!(surface.explicit_id, Some("surface".into()));
        assert_eq!(surface.accessibility.role, AccessibilityRole::ListBox);
        assert!(!surface.portal);
        assert!(!surface.dismiss_policy.on_escape());
        assert!(surface.dismiss_policy.on_pointer_outside());
        assert!(surface.blocks_pointer);
        assert_eq!(surface.restore_focus, Some(popover.trigger_focus()));
        assert_eq!(
            surface.accessibility.relations.labelled_by(),
            Some(popover.title_id())
        );
        assert_eq!(
            surface.accessibility.relations.described_by(),
            Some(popover.description_id())
        );
        assert_eq!(surface.visual.background, None);
        assert_eq!(surface.visual.border_color, None);
        assert_eq!(surface.visual.shadows, None);
        assert_eq!(surface.visual.radius, 0.0);

        let title = popover.title_with(crate::text("Visible title"));
        let description = popover.description_with(crate::text("Visible description"));
        let close = popover.close_with("Close popover", crate::div());
        let backdrop = popover.backdrop_with(crate::div());
        assert_eq!(title.explicit_id, Some(popover.title_id()));
        assert_eq!(description.explicit_id, Some(popover.description_id()));
        assert_eq!(close.explicit_id, Some(popover.close_id()));
        assert!(close.clickable);
        assert_eq!(close.accessibility.role, AccessibilityRole::Button);
        assert_eq!(close.accessibility.label.as_deref(), Some("Close popover"));
        assert_eq!(close.visual.background, None);
        assert_eq!(backdrop.explicit_id, Some(popover.backdrop_id()));
        assert!(backdrop.portal);
        assert!(backdrop.blocks_pointer);
        assert!(backdrop.accessibility.hidden);
    }

    #[test]
    fn structural_geometry_is_bounded_and_dismissal_paths_are_independent() {
        let popover = Popover::new("trigger", "surface", true)
            .anchor_gap(-4.0)
            .viewport_margin(f32::NAN)
            .dismiss_on_escape(false)
            .dismiss_on_pointer_outside(false);
        let positioner = popover.positioner_with(crate::div());
        let anchor = positioner.anchor.expect("popover positioner anchor");
        assert_eq!(anchor.gap, 0.0);
        assert_eq!(anchor.viewport_margin, DEFAULT_POPOVER_VIEWPORT_MARGIN);

        let popover = popover.popup_with(crate::div());
        assert!(popover.dismiss_policy.is_empty());
        assert!(popover.blocks_pointer);
    }

    #[test]
    fn derived_part_ids_are_stable_distinct_and_avoid_the_declared_pair() {
        let popover = Popover::new(41_u64, 42_u64, true);
        let ids = [
            popover.positioner_id(),
            popover.backdrop_id(),
            popover.title_id(),
            popover.description_id(),
            popover.close_id(),
        ];
        for (index, id) in ids.iter().enumerate() {
            assert_ne!(*id, popover.trigger_id());
            assert_ne!(*id, popover.surface_id());
            assert_ne!(*id, ElementId::new(0));
            assert_ne!(*id, ElementId::new(u64::MAX));
            assert!(!ids[..index].contains(id));
        }
        assert_eq!(
            popover.positioner_id(),
            Popover::new(41_u64, 42_u64, false).positioner_id()
        );
    }

    #[test]
    fn system_descriptor_maps_placement_and_builds_a_transparent_popover_host() {
        let descriptor = SystemPopover::new(320.0, 200.0)
            .placement(AnchorPlacement::TopEnd)
            .gap(8.0)
            .viewport_margin(12.0)
            .offset(3.0, 4.0)
            .dismiss_on_escape(false);
        assert_eq!(descriptor.size(), Size::new(320.0, 200.0));

        let popover = descriptor.popover_options();
        assert_eq!(popover.anchor_rect, Rect::ZERO);
        assert_eq!(popover.anchor, PopoverAnchor::TopRight);
        assert_eq!(popover.gravity, PopoverGravity::TopLeft);
        assert_eq!(popover.offset, Point::new(3.0, -4.0));
        assert_eq!(
            popover.constraint_adjustment,
            PopoverConstraintAdjustment::FIT
        );
        assert_eq!(popover.viewport_margin, 12.0);
        assert!(!popover.dismiss_on_escape);
        assert!(popover.dismiss_on_pointer_outside);
        assert!(popover.grab);
        assert!(popover.accepts_key_focus);

        let options = descriptor.window_options("Unstyled popover");
        assert_eq!(options.kind, WindowKind::SystemPopover);
        assert_eq!(options.size, Size::new(320.0, 200.0));
        assert_eq!(options.background, Color::TRANSPARENT);
        assert_eq!(
            options.window_background,
            WindowBackgroundAppearance::Transparent
        );
        assert!(options.focus);
        assert_eq!(options.popover, Some(popover));

        let never_key = descriptor.accepts_key_focus(false).popover_options();
        assert!(!never_key.grab);
        assert!(!never_key.accepts_key_focus);
    }

    #[derive(Default)]
    struct SystemPopoverLauncher {
        popover: Option<WindowHandle>,
    }

    struct SystemPopoverSurface;

    impl View for SystemPopoverSurface {
        fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            div().size_full()
        }
    }

    impl View for SystemPopoverLauncher {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let open = cx.listener("anchor", |view, cx| {
                view.popover = Some(
                    SystemPopover::new(180.0, 120.0)
                        .gap(6.0)
                        .open(cx, "anchor", "System popover", SystemPopoverSurface)
                        .expect("a mounted window can open a system popover"),
                );
            });
            div()
                .size_full()
                .relative()
                .child(
                    button()
                        .id("anchor")
                        .absolute()
                        .left(280.0)
                        .top(180.0)
                        .w(40.0)
                        .h(30.0)
                        .on_click(open),
                )
                .child(button().id("other").child("Other"))
        }
    }

    #[test]
    fn system_popover_uses_retained_trigger_bounds_and_may_cross_the_parent_edge() {
        let parent_bounds = Rect::new(100.0, 100.0, 320.0, 240.0);
        let (mut cx, launcher) = Application::new()
            .into_test_context(
                WindowOptions::new("Anchor parent")
                    .window_bounds(WindowBounds::Windowed(parent_bounds))
                    .without_minimum_size(),
                SystemPopoverLauncher::default(),
            )
            .unwrap();
        let parent = launcher.window_handle();

        cx.click(parent, "anchor").unwrap();
        let popover = cx
            .read(launcher, |view| view.popover)
            .unwrap()
            .expect("listener retained the popover handle");
        let popover_state = cx.window_state(popover).unwrap();
        let popover_bounds = popover_state.bounds.bounds();

        assert_eq!(popover_state.kind, WindowKind::SystemPopover);
        assert_eq!(popover_bounds, Rect::new(380.0, 316.0, 180.0, 120.0));
        assert!(popover_bounds.right() > parent_bounds.right());
        assert!(popover_bounds.bottom() > parent_bounds.bottom());
        assert_eq!(
            popover_bounds.intersection(cx.primary_display().unwrap().visible_bounds()),
            Some(popover_bounds)
        );

        let renders = cx.render_count(popover).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(popover).unwrap(), renders);

        cx.focus(parent, "other").unwrap();
        assert_eq!(cx.focused(parent).unwrap(), Some("other".into()));
        cx.update(launcher, |_view, cx| cx.close_window_handle(popover))
            .unwrap();
        assert!(!cx.is_window_open(popover));
        assert_eq!(cx.focused(parent).unwrap(), Some("anchor".into()));

        cx.click(parent, "anchor").unwrap();
        let owned_popover = cx
            .read(launcher, |view| view.popover)
            .unwrap()
            .expect("the anchor can reopen its system popover");
        assert!(cx.is_window_open(owned_popover));
        cx.update(launcher, |_view, cx| cx.close_window()).unwrap();
        assert!(cx.windows().is_empty());
    }

    #[derive(Default)]
    struct PopoverView {
        open: bool,
        chosen: bool,
    }

    impl View for PopoverView {
        fn event(&mut self, event: &Event, cx: &mut EventContext) {
            if *event == Event::Dismiss("surface".into()) {
                self.open = false;
                cx.invalidate();
            }
        }

        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let popover = Popover::new("trigger", "surface", self.open).initial_focus("choose");
            let toggle = cx.listener(popover.trigger_id(), move |view, cx| {
                view.open = !view.open;
                if view.open {
                    popover.focus_surface(cx);
                }
                cx.invalidate();
            });
            let choose = cx.listener("choose", move |view, cx| {
                view.chosen = true;
                view.open = false;
                cx.invalidate();
            });

            let mut root = crate::div().children([
                popover.trigger().on_click(toggle).child("Open"),
                crate::button().id("after").child("After"),
            ]);
            if popover.is_open() {
                root = root.child(
                    popover.positioner_with(
                        crate::div().child(
                            popover
                                .popup_with(crate::div())
                                .accessibility_label("Actions")
                                .child(
                                    crate::button()
                                        .id("choose")
                                        .on_click(choose)
                                        .child("Choose"),
                                ),
                        ),
                    ),
                );
            }
            root
        }
    }

    #[test]
    fn controlled_open_focus_dismiss_and_idle_paths_use_the_existing_runtime() {
        let (mut cx, view) = TestAppContext::new(PopoverView::default()).unwrap();
        let window = view.window_handle();
        assert!(!cx.contains_element(window, "surface").unwrap());

        cx.click(window, "trigger").unwrap();
        assert!(cx.read(view, |view| view.open).unwrap());
        assert!(cx.contains_element(window, "surface").unwrap());
        assert_eq!(cx.focused(window).unwrap(), Some("choose".into()));
        let trigger_bounds = cx.element_bounds(window, "trigger").unwrap();
        let positioner_bounds = cx
            .element_bounds(
                window,
                Popover::new("trigger", "surface", true).positioner_id(),
            )
            .unwrap();
        let popover_bounds = cx.element_bounds(window, "surface").unwrap();
        assert_eq!(positioner_bounds, popover_bounds);
        assert_eq!(positioner_bounds.x, DEFAULT_POPOVER_VIEWPORT_MARGIN);
        assert!(positioner_bounds.x > trigger_bounds.x);
        assert_eq!(
            positioner_bounds.y,
            trigger_bounds.bottom() + DEFAULT_POPOVER_ANCHOR_GAP
        );

        cx.simulate_keystrokes(window, "escape").unwrap();
        assert!(!cx.read(view, |view| view.open).unwrap());
        assert!(!cx.contains_element(window, "surface").unwrap());
        assert_eq!(cx.focused(window).unwrap(), Some("trigger".into()));

        cx.click(window, "trigger").unwrap();
        cx.update(view, |view, cx| {
            view.open = !view.open;
            view.open = !view.open;
            cx.invalidate();
        })
        .unwrap();
        assert!(cx.contains_element(window, "surface").unwrap());
        assert_eq!(cx.focused(window).unwrap(), Some("choose".into()));

        cx.update(view, |view, cx| {
            view.open = false;
            cx.invalidate();
        })
        .unwrap();
        assert!(!cx.contains_element(window, "surface").unwrap());
        assert_eq!(cx.focused(window).unwrap(), Some("trigger".into()));

        cx.click(window, "trigger").unwrap();
        cx.click(window, "choose").unwrap();
        assert!(cx.read(view, |view| view.chosen).unwrap());
        assert!(!cx.read(view, |view| view.open).unwrap());
        assert_eq!(cx.focused(window).unwrap(), Some("trigger".into()));

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }

    #[test]
    fn base_ui_positioner_props_map_to_bounded_anchored_geometry() {
        let popover = Popover::new("trigger", "popup", true)
            .side(AnchorSide::Left)
            .align(AnchorAlign::End)
            .side_offset(14.0)
            .align_offset(-9.0)
            .collision_padding(24.0)
            .sticky(false)
            .modal(true);
        assert_eq!(
            popover.resolved_anchor_placement(),
            AnchorPlacement::LeftEnd
        );
        assert!(popover.is_modal());

        let positioner = popover.positioner_with(div());
        let anchor = positioner.anchor.expect("positioner anchor");
        assert_eq!(anchor.target, AnchorTarget::Element("trigger".into()));
        assert_eq!(anchor.placement, AnchorPlacement::LeftEnd);
        assert_eq!(anchor.gap, 14.0);
        assert_eq!(anchor.align_offset, -9.0);
        assert_eq!(anchor.viewport_margin, 24.0);
        assert!(!anchor.sticky);
        assert_eq!(positioner.visual.background, None);
        assert_eq!(positioner.visual.border_color, None);
        assert!(!positioner.reports_anchor_placement());

        // Portal and Positioner are the same retained overlay node in QuickGUI.
        assert_eq!(
            popover.portal_with(div()).explicit_id,
            Some(popover.positioner_id())
        );

        let modal_popup = popover.popup_with(div());
        assert!(modal_popup.focus_trap);
        assert!(
            !Popover::new("trigger", "popup", true)
                .popup_with(div())
                .focus_trap
        );

        // Base UI names and the original QuickGUI names write the same bounded value.
        assert_eq!(
            Popover::new("t", "p", true).side_offset(3.0),
            Popover::new("t", "p", true).anchor_gap(3.0)
        );
        assert_eq!(
            Popover::new("t", "p", true).collision_padding(3.0),
            Popover::new("t", "p", true).viewport_margin(3.0)
        );

        // Every declared distance is clamped, and a non-finite value falls back.
        let clamped = Popover::new("trigger", "popup", true)
            .side_offset(1.0e9)
            .align_offset(f32::NAN)
            .collision_padding(-5.0)
            .arrow_size(1.0e9)
            .arrow_padding(f32::NAN);
        let anchor = clamped.positioner_with(div()).anchor.expect("anchor");
        assert_eq!(anchor.gap, MAX_POPOVER_SIDE_OFFSET);
        assert_eq!(anchor.align_offset, 0.0);
        assert_eq!(anchor.viewport_margin, 0.0);
        // A non-finite distance falls back to the declared default rather than to a bound.
        let broken = Popover::new("trigger", "popup", true)
            .side_offset(f32::INFINITY)
            .collision_padding(f32::NAN);
        let anchor = broken.positioner_with(div()).anchor.expect("anchor");
        assert_eq!(anchor.gap, DEFAULT_POPOVER_ANCHOR_GAP);
        assert_eq!(anchor.viewport_margin, DEFAULT_POPOVER_VIEWPORT_MARGIN);
        assert_eq!(clamped.arrow_size, MAX_POPOVER_ARROW_SIZE);
        assert_eq!(clamped.arrow_padding, 0.0);

        // The anchor override moves placement only; the trigger keeps its own relationships.
        let against_row = popover.anchor_element("row-3");
        let anchor = against_row.positioner_with(div()).anchor.expect("anchor");
        assert_eq!(anchor.target, AnchorTarget::Element("row-3".into()));
        assert_eq!(
            against_row.trigger().accessibility.relations.controls(),
            Some("popup".into())
        );
        let at_point = popover.anchor_point(Point::new(40.0, 50.0));
        let anchor = at_point.positioner_with(div()).anchor.expect("anchor");
        assert_eq!(anchor.target, AnchorTarget::Point(Point::new(40.0, 50.0)));
        assert_eq!(anchor.gap, 14.0);
        assert_eq!(
            at_point
                .anchor_trigger()
                .positioner_with(div())
                .anchor
                .expect("anchor")
                .target,
            AnchorTarget::Element("trigger".into())
        );

        // The viewport part is a scroll container with a stable identity and no appearance.
        let viewport = popover.viewport_with(div());
        assert_eq!(viewport.explicit_id, Some(popover.viewport_id()));
        assert_eq!(viewport.visual.background, None);
        assert_ne!(popover.viewport_id(), popover.arrow_id());
        assert_ne!(popover.viewport_id(), popover.positioner_id());
        assert_ne!(popover.arrow_id(), popover.positioner_id());

        let handle = AnchorPlacementHandle::new();
        assert!(
            popover
                .tracked_positioner_with(div(), &handle)
                .reports_anchor_placement()
        );
        assert!(
            popover
                .tracked_surface_with(div(), &handle)
                .reports_anchor_placement()
        );
    }

    #[test]
    fn the_arrow_and_state_snapshot_follow_the_resolved_placement_not_the_preference() {
        let preferred = Popover::new("trigger", "popup", true)
            .side(AnchorSide::Bottom)
            .arrow_size(10.0)
            .arrow_padding(4.0);
        assert_eq!(preferred.resolved_side(), AnchorSide::Bottom);
        assert_eq!(
            preferred.state(),
            PopoverPartState {
                open: true,
                modal: false,
                side: AnchorSide::Bottom,
                align: AnchorAlign::Start,
                anchor_hidden: false,
                anchor_width: 0.0,
                anchor_height: 0.0,
                available_width: 0.0,
                available_height: 0.0,
            }
        );
        assert!(!preferred.state().is_measured());
        // Without a report the arrow pins to the declared edge and adds no guessed offset.
        let unmeasured = preferred.arrow_with(div());
        assert_eq!(unmeasured.layout.inset.top, length(0.0));
        assert!(unmeasured.accessibility.hidden);

        // The popover really opened above its trigger, so the arrow belongs on the popup's bottom.
        let flipped = preferred.resolved_placement(Some(ResolvedAnchorPlacement {
            placement: AnchorPlacement::TopStart,
            anchor: Rect::new(100.0, 560.0, 40.0, 30.0),
            bounds: Rect::new(100.0, 354.0, 200.0, 200.0),
            available: Size::new(944.0, 546.0),
            anchor_hidden: false,
        }));
        assert_eq!(flipped.resolved_side(), AnchorSide::Top);
        assert_eq!(flipped.resolved_align(), AnchorAlign::Start);
        let arrow = flipped.arrow_with(div());
        assert_eq!(arrow.explicit_id, Some(flipped.arrow_id()));
        assert_eq!(arrow.layout.inset.bottom, length(0.0));
        // 100 + 40/2 - 100 - 10/2 = 15 logical points from the popup's leading edge.
        assert_eq!(arrow.layout.inset.left, length(15.0));

        let state = flipped.state();
        assert_eq!(state.side, AnchorSide::Top);
        assert_eq!(state.anchor_width, 40.0);
        assert_eq!(state.anchor_height, 30.0);
        assert_eq!(state.available_width, 944.0);
        assert_eq!(state.available_height, 546.0);
        assert!(state.is_measured());
        assert!(!state.anchor_hidden);

        // Arrow padding keeps the arrow off the corners even when the anchor sits past them.
        let corner = flipped.resolved_placement(Some(ResolvedAnchorPlacement {
            placement: AnchorPlacement::TopStart,
            anchor: Rect::new(0.0, 560.0, 4.0, 30.0),
            bounds: Rect::new(100.0, 354.0, 200.0, 200.0),
            available: Size::new(944.0, 546.0),
            anchor_hidden: true,
        }));
        assert_eq!(corner.arrow_with(div()).layout.inset.left, length(4.0));
        assert!(corner.state().anchor_hidden);

        // A left placement pins the arrow to the popup's trailing edge and offsets vertically.
        let leftward = preferred.resolved_placement(Some(ResolvedAnchorPlacement {
            placement: AnchorPlacement::Left,
            anchor: Rect::new(400.0, 100.0, 40.0, 60.0),
            bounds: Rect::new(180.0, 90.0, 200.0, 80.0),
            available: Size::new(392.0, 624.0),
            anchor_hidden: false,
        }));
        let arrow = leftward.arrow_with(div());
        assert_eq!(arrow.layout.inset.right, length(0.0));
        // 100 + 60/2 - 90 - 10/2 = 35 from the popup's top edge.
        assert_eq!(arrow.layout.inset.top, length(35.0));
    }

    #[derive(Default)]
    struct FlipView {
        open: bool,
        placement: AnchorPlacementHandle,
    }

    impl View for FlipView {
        fn event(&mut self, event: &Event, cx: &mut EventContext) {
            if *event == Event::Dismiss("popup".into()) {
                self.open = false;
                cx.invalidate();
            }
        }

        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let popover = Popover::new("trigger", "popup", self.open)
                .side(AnchorSide::Bottom)
                .align(AnchorAlign::Start)
                .side_offset(6.0)
                .arrow_size(10.0)
                .arrow_padding(4.0)
                .track_placement(&self.placement);
            let toggle = cx.listener("trigger", |view: &mut Self, cx: &mut EventContext| {
                view.open = !view.open;
                cx.invalidate();
            });
            let mut root = div().size_full().relative().child(
                popover
                    .trigger()
                    .absolute()
                    .left(100.0)
                    .top(560.0)
                    .w(40.0)
                    .h(30.0)
                    .on_click(toggle),
            );
            if popover.is_open() {
                root = root.child(
                    popover.tracked_positioner_with(
                        div().child(
                            popover
                                .popup_with(div())
                                .relative()
                                .w(200.0)
                                .h(200.0)
                                .accessibility_label("Actions")
                                .child(popover.arrow_with(div().w(10.0).h(6.0))),
                        ),
                        &self.placement,
                    ),
                );
            }
            root
        }
    }

    #[test]
    fn a_flipped_popover_reports_its_real_placement_and_repositions_the_arrow() {
        let (mut cx, view) = TestAppContext::new(FlipView::default()).unwrap();
        let window = view.window_handle();
        assert_eq!(
            cx.read(view, |view| view.placement.resolved()).unwrap(),
            None
        );

        cx.click(window, "trigger").unwrap();
        let popup_bounds = cx.element_bounds(window, "popup").unwrap();
        cx.run_until_idle().unwrap();

        let resolved = cx
            .read(view, |view| view.placement.resolved())
            .unwrap()
            .expect("a painted anchored surface publishes its resolved placement");
        // Declared bottom, but only 36 points remain below the trigger for a 200 point popup.
        assert_eq!(resolved.placement, AnchorPlacement::TopStart);
        assert_eq!(resolved.side(), AnchorSide::Top);
        assert_eq!(resolved.align(), AnchorAlign::Start);
        assert_eq!(resolved.anchor, Rect::new(100.0, 560.0, 40.0, 30.0));
        assert_eq!(resolved.bounds, popup_bounds);
        assert_eq!(resolved.available.height, 560.0 - 8.0 - 6.0);
        assert!(!resolved.anchor_hidden);
        assert!(popup_bounds.bottom() <= 560.0);

        let arrow_bounds = cx
            .element_bounds(window, Popover::new("trigger", "popup", true).arrow_id())
            .unwrap();
        assert_eq!(arrow_bounds.bottom(), popup_bounds.bottom());
        assert_eq!(arrow_bounds.x, popup_bounds.x + 15.0);

        // The correcting frame is one-shot: a settled popover schedules no further work.
        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);

        cx.simulate_keystrokes(window, "escape").unwrap();
        assert!(!cx.read(view, |view| view.open).unwrap());
        cx.update(view, |view, _cx| view.placement.clear()).unwrap();
        assert_eq!(
            cx.read(view, |view| view.placement.resolved()).unwrap(),
            None
        );
    }

    struct HoverPopoverView {
        hover: PopoverHoverState,
    }

    impl Default for HoverPopoverView {
        fn default() -> Self {
            Self {
                hover: PopoverHoverState::new()
                    .delay(Duration::from_millis(200))
                    .close_delay(Duration::from_millis(100)),
            }
        }
    }

    impl View for HoverPopoverView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let popover = Popover::new("trigger", "popup", self.hover.is_open());
            let trigger = self.hover.trigger_with(
                cx,
                popover,
                |view| &mut view.hover,
                div().absolute().left(0.0).top(0.0).w(60.0).h(20.0),
            );
            let mut root = div().size_full().relative().child(trigger);
            if popover.is_open() {
                let popup = self.hover.popup_with(
                    cx,
                    popover,
                    |view| &mut view.hover,
                    div().w(100.0).h(60.0),
                );
                root = root.child(popover.positioner_with(div().child(popup)));
            }
            root
        }
    }

    #[test]
    fn hover_opening_uses_exact_one_shot_deadlines_and_survives_the_gap_to_the_popup() {
        let (mut cx, view) = TestAppContext::new(HoverPopoverView::default()).unwrap();
        let window = view.window_handle();
        assert!(!cx.contains_element(window, "popup").unwrap());

        cx.visual(window)
            .unwrap()
            .move_pointer(Point::new(10.0, 10.0))
            .unwrap();
        assert!(cx.read(view, |view| view.hover.is_pending()).unwrap());
        assert!(!cx.read(view, |view| view.hover.is_open()).unwrap());

        cx.advance_time(Duration::from_millis(199)).unwrap();
        assert!(!cx.read(view, |view| view.hover.is_open()).unwrap());
        cx.advance_time(Duration::from_millis(1)).unwrap();
        assert!(cx.read(view, |view| view.hover.is_open()).unwrap());
        assert!(!cx.read(view, |view| view.hover.is_pending()).unwrap());
        assert!(cx.contains_element(window, "popup").unwrap());

        // An opened popover owns no timer: nothing is scheduled once the deadline fired.
        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);

        // Leaving the trigger starts the close deadline; entering the popup cancels it.
        cx.visual(window)
            .unwrap()
            .move_pointer(Point::new(600.0, 400.0))
            .unwrap();
        assert!(cx.read(view, |view| view.hover.is_pending()).unwrap());
        cx.visual(window)
            .unwrap()
            .move_pointer(Point::new(40.0, 50.0))
            .unwrap();
        assert!(!cx.read(view, |view| view.hover.is_pending()).unwrap());
        cx.advance_time(Duration::from_millis(500)).unwrap();
        assert!(cx.read(view, |view| view.hover.is_open()).unwrap());

        // Leaving everything closes after exactly the declared close delay.
        cx.visual(window)
            .unwrap()
            .move_pointer(Point::new(600.0, 400.0))
            .unwrap();
        cx.advance_time(Duration::from_millis(99)).unwrap();
        assert!(cx.read(view, |view| view.hover.is_open()).unwrap());
        cx.advance_time(Duration::from_millis(1)).unwrap();
        assert!(!cx.read(view, |view| view.hover.is_open()).unwrap());
        assert!(!cx.contains_element(window, "popup").unwrap());

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }

    #[test]
    fn hover_state_defaults_are_bounded_and_immediate_changes_cancel_deadlines() {
        let state = PopoverHoverState::new();
        assert!(!state.is_open());
        assert!(!state.is_pending());
        assert_eq!(state.delay, DEFAULT_POPOVER_HOVER_DELAY);
        assert_eq!(state.close_delay, Duration::ZERO);
        assert_eq!(
            PopoverHoverState::new()
                .delay(Duration::from_secs(3_600))
                .delay,
            MAX_POPOVER_HOVER_DELAY
        );
        assert_eq!(
            PopoverHoverState::new()
                .close_delay(Duration::from_secs(3_600))
                .close_delay,
            MAX_POPOVER_HOVER_DELAY
        );

        let mut state = PopoverHoverState::new().hoverable_popup(false);
        assert!(state.open_now());
        assert!(!state.open_now());
        assert!(state.is_open());
        assert!(state.toggle());
        assert!(!state.is_open());
        assert!(state.toggle());
        assert!(state.close_now());
        assert!(!state.close_now());
        state.popup_hovered = true;
        assert!(!state.is_hovered());
    }
}
