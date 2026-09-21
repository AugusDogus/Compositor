use std::{cell::RefCell, fmt, rc::Rc, sync::Arc, time::Duration};

use crate::{
    AccessibilityRole, AnchorAlign, AnchorPlacement, AnchorPlacementHandle, AnchorSide,
    AsyncViewContext, Color, Element, ElementId, EventContext, IntoElement, LayoutBoundsHandle,
    MouseButton, Point, StateAccessor, Task, ViewContext, anchor_placement, div, text,
};

/// Native-style delay before a newly hovered tooltip becomes visible.
pub const DEFAULT_TOOLTIP_DELAY: Duration = Duration::from_millis(500);

/// Longest delay retained by one tooltip declaration.
pub const MAX_TOOLTIP_DELAY: Duration = Duration::from_secs(10);

/// Maximum detached element count accepted by one tooltip.
///
/// Tooltips are deliberately small transient surfaces. This keeps accidental dynamic content from
/// turning pointer hover into an unbounded layout operation.
pub const MAX_TOOLTIP_CONTENT_NODES: usize = 256;

/// Maximum tooltip declarations indexed for one rendered window.
pub const MAX_TOOLTIPS_PER_WINDOW: usize = 4_096;

/// A lazily displayed, pointer-passive GPU overlay attached to an element.
///
/// The content is an ordinary QuickGUI element tree. It is laid out only when its trigger survives
/// the hover delay, then retained until the tooltip hides or the application view changes.
#[derive(Clone)]
pub struct Tooltip {
    pub(crate) content: Rc<Element>,
    pub(crate) placement: AnchorPlacement,
    pub(crate) delay: Duration,
    pub(crate) gap: f32,
    pub(crate) viewport_margin: f32,
    pub(crate) accessibility_description: Option<Arc<str>>,
}

impl Tooltip {
    /// Create a tooltip from an arbitrary GPU element tree.
    pub fn new(content: impl IntoElement) -> Self {
        let content = content.into_element();
        let nodes = tooltip_node_count(&content, MAX_TOOLTIP_CONTENT_NODES + 1);
        assert!(
            nodes <= MAX_TOOLTIP_CONTENT_NODES,
            "a tooltip cannot contain more than {MAX_TOOLTIP_CONTENT_NODES} elements"
        );
        Self {
            content: Rc::new(content),
            placement: AnchorPlacement::Top,
            delay: DEFAULT_TOOLTIP_DELAY,
            gap: 7.0,
            viewport_margin: 8.0,
            accessibility_description: None,
        }
    }

    /// Create a compact native-style text tooltip.
    pub fn text(label: impl Into<Arc<str>>) -> Self {
        let label = label.into();
        Self::new(
            div()
                .max_w(360.0)
                .px_3()
                .py_2()
                .rounded_md()
                .border(1.0, Color::rgba8(255, 255, 255, 30))
                .bg(Color::rgb8(42, 45, 53))
                .shadow_md()
                .accessibility_role(AccessibilityRole::Tooltip)
                .child(
                    text(label.clone())
                        .wrap()
                        .text_sm()
                        .text_color(Color::rgb8(244, 245, 247)),
                ),
        )
        .accessibility_description(label)
    }

    pub fn placement(mut self, placement: AnchorPlacement) -> Self {
        self.placement = placement;
        self
    }

    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay.min(MAX_TOOLTIP_DELAY);
        self
    }

    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = finite_nonnegative(gap);
        self
    }

    pub fn viewport_margin(mut self, margin: f32) -> Self {
        self.viewport_margin = finite_nonnegative(margin);
        self
    }

    /// Text exposed as the trigger's native accessibility description while the visual tooltip
    /// stays transient.
    pub fn accessibility_description(mut self, description: impl Into<Arc<str>>) -> Self {
        let description = description.into();
        self.accessibility_description = (!description.is_empty()).then_some(description);
        self
    }

    pub(crate) fn content_mut(&mut self) -> &mut Element {
        Rc::make_mut(&mut self.content)
    }

    pub(crate) fn content_identity(&self) -> *const Element {
        Rc::as_ptr(&self.content)
    }
}

impl fmt::Debug for Tooltip {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Tooltip")
            .field("placement", &self.placement)
            .field("delay", &self.delay)
            .field("gap", &self.gap)
            .field("viewport_margin", &self.viewport_margin)
            .field("accessibility_description", &self.accessibility_description)
            .finish_non_exhaustive()
    }
}

impl From<Element> for Tooltip {
    fn from(content: Element) -> Self {
        Self::new(content)
    }
}

impl From<&str> for Tooltip {
    fn from(label: &str) -> Self {
        Self::text(Arc::<str>::from(label))
    }
}

impl From<String> for Tooltip {
    fn from(label: String) -> Self {
        Self::text(Arc::<str>::from(label))
    }
}

impl From<Arc<str>> for Tooltip {
    fn from(label: Arc<str>) -> Self {
        Self::text(label)
    }
}

fn tooltip_node_count(element: &Element, stop_after: usize) -> usize {
    let mut count = 1;
    for child in &element.children {
        count += tooltip_node_count(child, stop_after.saturating_sub(count));
        if count >= stop_after {
            break;
        }
    }
    count
}

fn finite_nonnegative(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

/// Default delay before a hovered trigger opens its Base UI-shaped tooltip.
pub const DEFAULT_TOOLTIP_HOVER_DELAY: Duration = Duration::from_millis(600);

/// Default interval after a grouped tooltip closes during which the next one opens instantly.
pub const DEFAULT_TOOLTIP_GROUP_TIMEOUT: Duration = Duration::from_millis(400);

/// Longest grouping interval one [`TooltipProvider`] may declare.
pub const MAX_TOOLTIP_GROUP_TIMEOUT: Duration = Duration::from_secs(10);

/// Maximum distance between a tooltip trigger and its positioner.
pub const MAX_TOOLTIP_SIDE_OFFSET: f32 = 256.0;

/// Maximum collision padding kept between a tooltip and the window viewport edge.
pub const MAX_TOOLTIP_COLLISION_PADDING: f32 = 512.0;

const TOOLTIP_POSITIONER_ID_TAG: u64 = 0x51ab_c07e_29d4_6f18;
const TOOLTIP_ARROW_ID_TAG: u64 = 0xc3d7_1a45_8b62_e097;

/// Which axes a tooltip follows the pointer along, Base UI's `trackCursorAxis`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TooltipCursorAxis {
    /// Stay anchored to the trigger rectangle.
    #[default]
    None,
    /// Follow the pointer horizontally, keeping the trigger's edge vertically.
    X,
    /// Follow the pointer vertically, keeping the trigger's edge horizontally.
    Y,
    /// Follow the pointer on both axes.
    Both,
}

impl TooltipCursorAxis {
    const fn tracks_x(self) -> bool {
        matches!(self, Self::X | Self::Both)
    }

    const fn tracks_y(self) -> bool {
        matches!(self, Self::Y | Self::Both)
    }

    const fn tracks_any(self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Debug, Default)]
struct TooltipGroup {
    delay: Duration,
    close_delay: Duration,
    timeout: Duration,
    warm: bool,
    generation: u64,
    cooldown: Option<Task<()>>,
}

/// Shared open timing for a group of tooltips, Base UI's `Tooltip.Provider`.
///
/// Store one on the view and hand it to every [`TooltipState`] that should behave as one group.
/// The first tooltip waits for the full delay; while the group stays warm — a tooltip is open, or
/// one closed less than [`Self::timeout`] ago — the next adjacent trigger opens instantly, which is
/// what makes scanning a toolbar feel like one control rather than a row of independent waits.
///
/// The warm window is an exact one-shot deadline, not a clock poll: closing a grouped tooltip
/// schedules one task, and opening another cancels it. A group with no tooltip open and no warm
/// window outstanding owns nothing.
#[derive(Clone, Default)]
pub struct TooltipProvider(Rc<RefCell<TooltipGroup>>);

impl TooltipProvider {
    /// Create a group with the default 600 ms delay, immediate close, and 400 ms warm window.
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(TooltipGroup {
            delay: DEFAULT_TOOLTIP_HOVER_DELAY,
            close_delay: Duration::ZERO,
            timeout: DEFAULT_TOOLTIP_GROUP_TIMEOUT,
            warm: false,
            generation: 0,
            cooldown: None,
        })))
    }

    /// Set the group's open delay, Base UI's Provider `delay`.
    pub fn delay(self, delay: Duration) -> Self {
        self.0.borrow_mut().delay = delay.min(MAX_TOOLTIP_DELAY);
        self
    }

    /// Set the group's close delay, Base UI's Provider `closeDelay`.
    pub fn close_delay(self, delay: Duration) -> Self {
        self.0.borrow_mut().close_delay = delay.min(MAX_TOOLTIP_DELAY);
        self
    }

    /// Set how long the group stays warm after a tooltip closes, Base UI's Provider `timeout`.
    pub fn timeout(self, timeout: Duration) -> Self {
        self.0.borrow_mut().timeout = timeout.min(MAX_TOOLTIP_GROUP_TIMEOUT);
        self
    }

    /// The group's open delay.
    pub fn delay_value(&self) -> Duration {
        self.0.borrow().delay
    }

    /// The group's close delay.
    pub fn close_delay_value(&self) -> Duration {
        self.0.borrow().close_delay
    }

    /// The group's warm window.
    pub fn timeout_value(&self) -> Duration {
        self.0.borrow().timeout
    }

    /// Whether the next tooltip in this group opens without waiting.
    pub fn is_warm(&self) -> bool {
        self.0.borrow().warm
    }

    /// Enter the warm window immediately and cancel any outstanding cooldown.
    fn hold_warm(&self) {
        let mut group = self.0.borrow_mut();
        group.warm = true;
        group.generation = group.generation.wrapping_add(1);
        if let Some(task) = group.cooldown.take() {
            task.cancel();
        }
    }

    /// Start the exact one-shot window during which the next tooltip still opens instantly.
    fn start_cooldown<V: 'static>(&self, cx: &mut EventContext) {
        let timeout = {
            let mut group = self.0.borrow_mut();
            group.warm = true;
            group.generation = group.generation.wrapping_add(1);
            if let Some(task) = group.cooldown.take() {
                task.cancel();
            }
            group.timeout
        };
        let generation = self.0.borrow().generation;
        if timeout.is_zero() {
            self.cool(generation);
            return;
        }
        let provider = self.clone();
        let spawned = cx.spawn::<V, _, _, _>(move |task_cx: AsyncViewContext<V>| async move {
            if task_cx.sleep(timeout).await.is_err() {
                return;
            }
            let _ = task_cx
                .update(move |_view: &mut V, _cx| provider.cool(generation))
                .await;
        });
        match spawned {
            Ok(task) => self.0.borrow_mut().cooldown = Some(task),
            Err(_) => self.cool(generation),
        }
    }

    fn cool(&self, generation: u64) {
        let mut group = self.0.borrow_mut();
        if group.generation != generation {
            return;
        }
        group.warm = false;
        group.cooldown = None;
    }
}

impl fmt::Debug for TooltipProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let group = self.0.borrow();
        formatter
            .debug_struct("TooltipProvider")
            .field("delay", &group.delay)
            .field("close_delay", &group.close_delay)
            .field("timeout", &group.timeout)
            .field("warm", &group.warm)
            .finish_non_exhaustive()
    }
}

impl PartialEq for TooltipProvider {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TooltipPhase {
    Open,
    Close,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TooltipHoverPart {
    Trigger,
    Popup,
}

/// A copyable render-state snapshot for one Base UI-shaped tooltip.
///
/// Base UI publishes this as `data-open`, `data-side`, `data-align`, and `data-instant`; QuickGUI
/// has no style sheet, so the same facts arrive as fields the application styles from.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TooltipPartState {
    /// Whether the tooltip is currently open.
    pub open: bool,
    /// Whether opening skipped the delay because the group was still warm.
    pub instant: bool,
    /// Whether the trigger refuses to open at all.
    pub disabled: bool,
    /// The side of the anchor the popup was actually placed on.
    pub side: AnchorSide,
    /// The cross-axis alignment the popup actually used.
    pub align: AnchorAlign,
    /// Whether the anchor left the collision viewport entirely.
    pub anchor_hidden: bool,
}

/// Bounded controlled state for one unstyled tooltip built from Base UI's compound parts.
///
/// This is the compound-part counterpart of the existing [`Tooltip`] declaration attached with
/// `Element::tooltip(...)`. That form stays the shortest path to a native-style hint and keeps
/// working unchanged; use `TooltipState` when the application needs to own the popup's content
/// tree, keep it hoverable, follow the pointer, or share timing across a group of triggers.
///
/// QuickGUI owns the hover deadlines, the tooltip role and description relationship, anchored
/// placement with flip and shift, the resolved-placement report the arrow follows, and Escape
/// dismissal. The application owns every visual declaration and, as always, the open flag lives in
/// this state rather than in a framework registry.
///
/// A closed tooltip owns no task, timer, observer, or idle scheduler source. An open one owns at
/// most a single pending close deadline.
pub struct TooltipState {
    trigger_id: ElementId,
    popup_id: ElementId,
    open: bool,
    instant: bool,
    disabled: bool,
    hoverable: bool,
    close_on_click: bool,
    track_cursor_axis: TooltipCursorAxis,
    placement: AnchorPlacement,
    side_offset: f32,
    collision_padding: f32,
    delay: Option<Duration>,
    close_delay: Option<Duration>,
    provider: TooltipProvider,
    placement_handle: AnchorPlacementHandle,
    /// The trigger's painted rectangle, so a cursor-tracking tooltip can pin its other axis to
    /// the trigger's edge from the very first frame it opens.
    trigger_bounds: LayoutBoundsHandle,
    cursor: Option<Point>,
    trigger_hovered: bool,
    popup_hovered: bool,
    pending: Option<TooltipPhase>,
    generation: u64,
    task: Option<Task<()>>,
}

impl fmt::Debug for TooltipState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TooltipState")
            .field("trigger_id", &self.trigger_id)
            .field("popup_id", &self.popup_id)
            .field("open", &self.open)
            .field("disabled", &self.disabled)
            .field("placement", &self.placement)
            .field("track_cursor_axis", &self.track_cursor_axis)
            .finish_non_exhaustive()
    }
}

impl TooltipState {
    /// Create a closed tooltip owning its own timing group.
    ///
    /// Pass [`Self::provider`] a shared [`TooltipProvider`] to make several triggers behave as one
    /// group.
    pub fn new(trigger_id: impl Into<ElementId>, popup_id: impl Into<ElementId>) -> Self {
        Self {
            trigger_id: trigger_id.into(),
            popup_id: popup_id.into(),
            open: false,
            instant: false,
            disabled: false,
            hoverable: false,
            close_on_click: true,
            track_cursor_axis: TooltipCursorAxis::None,
            placement: AnchorPlacement::Top,
            side_offset: 7.0,
            collision_padding: 8.0,
            delay: None,
            close_delay: None,
            provider: TooltipProvider::new(),
            placement_handle: AnchorPlacementHandle::new(),
            trigger_bounds: LayoutBoundsHandle::new(),
            cursor: None,
            trigger_hovered: false,
            popup_hovered: false,
            pending: None,
            generation: 0,
            task: None,
        }
    }

    /// Join a shared timing group so adjacent tooltips open instantly.
    pub fn provider(mut self, provider: &TooltipProvider) -> Self {
        self.provider = provider.clone();
        self
    }

    /// Override the group's open delay for this trigger, Base UI's `delay`.
    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = Some(delay.min(MAX_TOOLTIP_DELAY));
        self
    }

    /// Override the group's close delay for this trigger, Base UI's `closeDelay`.
    pub fn close_delay(mut self, delay: Duration) -> Self {
        self.close_delay = Some(delay.min(MAX_TOOLTIP_DELAY));
        self
    }

    /// Refuse to open this tooltip, Base UI's `disabled`.
    ///
    /// A disabled tooltip closes immediately if it was already open and cancels any deadline.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        if disabled {
            self.cancel_pending();
            self.open = false;
        }
        self
    }

    /// Choose whether the pointer may rest on the popup itself, Base UI's Popup `hoverable`.
    ///
    /// A tooltip is a passive help tag by default, as AppKit's is: the popup never takes the
    /// pointer, so reaching it closes the tooltip. Base UI defaults the other way; opt in for a
    /// popup that carries something worth hovering.
    pub const fn hoverable(mut self, hoverable: bool) -> Self {
        self.hoverable = hoverable;
        self
    }

    /// Choose whether pressing the trigger closes the tooltip, Base UI's `closeOnClick`.
    pub const fn close_on_click(mut self, close_on_click: bool) -> Self {
        self.close_on_click = close_on_click;
        self
    }

    /// Follow the pointer along one or both axes, Base UI's `trackCursorAxis`.
    ///
    /// Tracking is only applied while the tooltip is open, and only after the first painted frame
    /// has reported the trigger rectangle the untracked axis stays pinned to.
    pub const fn track_cursor_axis(mut self, axis: TooltipCursorAxis) -> Self {
        self.track_cursor_axis = axis;
        self
    }

    /// Prefer a placement for the positioner.
    pub const fn placement(mut self, placement: AnchorPlacement) -> Self {
        self.placement = placement;
        self
    }

    /// Prefer one side of the trigger, keeping the current cross-axis alignment.
    pub const fn side(mut self, side: AnchorSide) -> Self {
        self.placement = anchor_placement(side, AnchorAlign::of(self.placement));
        self
    }

    /// Prefer a cross-axis alignment, keeping the current side.
    pub const fn align(mut self, align: AnchorAlign) -> Self {
        self.placement = anchor_placement(AnchorSide::of(self.placement), align);
        self
    }

    /// Set the distance between the trigger and the positioner, Base UI's `sideOffset`.
    pub fn side_offset(mut self, offset: f32) -> Self {
        self.side_offset = finite_clamped(offset, 0.0, MAX_TOOLTIP_SIDE_OFFSET, 7.0);
        self
    }

    /// Set the collision padding kept inside the window viewport, Base UI's `collisionPadding`.
    pub fn collision_padding(mut self, padding: f32) -> Self {
        self.collision_padding = finite_clamped(padding, 0.0, MAX_TOOLTIP_COLLISION_PADDING, 8.0);
        self
    }

    pub const fn is_open(&self) -> bool {
        self.open
    }

    pub const fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// Whether a hover deadline is outstanding.
    pub const fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub const fn trigger_id(&self) -> ElementId {
        self.trigger_id
    }

    pub const fn popup_id(&self) -> ElementId {
        self.popup_id
    }

    /// Stable identity of the positioner part.
    pub fn positioner_id(&self) -> ElementId {
        derived_tooltip_id(self.popup_id, self.trigger_id, TOOLTIP_POSITIONER_ID_TAG)
    }

    /// Stable identity of the arrow part.
    pub fn arrow_id(&self) -> ElementId {
        derived_tooltip_id(self.popup_id, self.trigger_id, TOOLTIP_ARROW_ID_TAG)
    }

    /// The side the popup actually opened on, or the declared preference before the first frame.
    pub fn resolved_side(&self) -> AnchorSide {
        AnchorSide::of(self.resolved_placement())
    }

    /// The cross-axis alignment the popup actually used, or the declared preference.
    pub fn resolved_align(&self) -> AnchorAlign {
        AnchorAlign::of(self.resolved_placement())
    }

    fn resolved_placement(&self) -> AnchorPlacement {
        self.placement_handle.placement_or(self.placement)
    }

    /// A copyable render-state snapshot the application styles from.
    pub fn state(&self) -> TooltipPartState {
        TooltipPartState {
            open: self.open,
            instant: self.instant,
            disabled: self.disabled,
            side: self.resolved_side(),
            align: self.resolved_align(),
            anchor_hidden: self
                .placement_handle
                .resolved()
                .is_some_and(|resolved| resolved.anchor_hidden),
        }
    }

    /// Open immediately, cancelling any outstanding deadline.
    ///
    /// Returns whether the open state changed. A disabled tooltip never opens.
    pub fn open_now(&mut self) -> bool {
        self.cancel_pending();
        if self.disabled {
            return false;
        }
        self.instant = true;
        self.provider.hold_warm();
        !std::mem::replace(&mut self.open, true)
    }

    /// Close immediately, cancelling any outstanding deadline.
    pub fn close_now(&mut self) -> bool {
        self.cancel_pending();
        self.instant = false;
        std::mem::replace(&mut self.open, false)
    }

    /// Drop any outstanding hover deadline without changing the open state.
    pub fn cancel_pending(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.pending = None;
        if let Some(task) = self.task.take() {
            task.cancel();
        }
    }

    /// Decorate a caller-owned trigger.
    ///
    /// This is the `fn`-pointer entry point for a view that owns one tooltip per field; a host that
    /// renders many declared tooltips through one view uses [`Self::trigger_with_accessor`].
    pub fn trigger_with<V: 'static>(
        &self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut Self,
        trigger: Element,
    ) -> Element {
        self.trigger_with_accessor(cx, StateAccessor::from(access), trigger)
    }
    /// Create the unstyled trigger part. Use [`Self::trigger_with`] to supply an existing element.
    pub fn trigger<V: 'static>(
        &self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut Self,
    ) -> Element {
        self.trigger_with(cx, access, crate::button())
    }

    /// Decorate a caller-owned trigger through a per-instance accessor.
    pub fn trigger_with_accessor<V: 'static>(
        &self,
        cx: &mut ViewContext<'_, V>,
        access: StateAccessor<V, Self>,
        trigger: Element,
    ) -> Element {
        let hovered = self.hover_listener(cx, self.trigger_id, TooltipHoverPart::Trigger, &access);
        let mut trigger = trigger
            .id(self.trigger_id)
            .app_region_no_drag()
            .on_hover(hovered);
        if self.open {
            trigger = trigger.accessibility_described_by(self.popup_id);
        }
        if self.close_on_click {
            let closing = access.clone();
            let pressed = cx.mouse_down_listener(self.trigger_id, move |view, _event, cx| {
                if closing.get(view).close_now() {
                    cx.invalidate();
                }
            });
            trigger = trigger.on_mouse_down(MouseButton::Left, pressed);
        }
        if self.track_cursor_axis.tracks_any() {
            let tracking = access.clone();
            // The pointer is tracked while it rests on the trigger, open or not, so the popup
            // appears at the pointer instead of jumping there on the first move after opening.
            let moved = cx.mouse_move_listener(self.trigger_id, move |view, event, cx| {
                let position = event.position;
                let state = tracking.get(view);
                if state.cursor == Some(position) {
                    return;
                }
                state.cursor = Some(position);
                if state.open {
                    cx.invalidate();
                }
            });
            trigger = trigger
                .on_mouse_move(moved)
                .report_bounds(self.trigger_bounds.clone());
        }
        trigger
    }

    /// Decorate the caller-owned portal/positioner and publish the placement it resolves to.
    ///
    /// QuickGUI's retained overlay node is itself the portal, so Base UI's Portal and Positioner
    /// are one element here; [`Self::portal_with`] is the same decorator under Base UI's other
    /// name.
    pub fn positioner_with(&self, positioner: Element) -> Element {
        let positioner = positioner
            .id(self.positioner_id())
            .app_region_no_drag()
            .report_anchor_placement(self.placement_handle.clone());
        let positioner = match self.cursor_anchor() {
            Some(point) => positioner.anchor_at(point, self.placement),
            None => positioner.anchor_to(self.trigger_id, self.placement),
        };
        positioner
            .anchor_gap(self.side_offset)
            .viewport_margin(self.collision_padding)
    }
    /// Create the unstyled positioner part. Use [`Self::positioner_with`] to supply an existing element.
    pub fn positioner(&self) -> Element {
        self.positioner_with(crate::div())
    }

    /// Base UI's Portal name for the combined portal/positioner part.
    pub fn portal_with(&self, portal: Element) -> Element {
        self.positioner_with(portal)
    }
    /// Create the unstyled portal part. Use [`Self::portal_with`] to supply an existing element.
    pub fn portal(&self) -> Element {
        self.portal_with(crate::div())
    }

    /// Decorate the caller-owned popup.
    ///
    /// The popup carries the Tooltip role, is dismissed by Escape through
    /// [`crate::Event::Dismiss`] under [`Self::popup_id`], and — unless
    /// [`Self::hoverable`] was disabled — keeps the tooltip open while the pointer rests on it.
    pub fn popup_with<V: 'static>(
        &self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut Self,
        popup: Element,
    ) -> Element {
        self.popup_with_accessor(cx, StateAccessor::from(access), popup)
    }
    /// Create the unstyled popup part. Use [`Self::popup_with`] to supply an existing element.
    pub fn popup<V: 'static>(
        &self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut Self,
    ) -> Element {
        self.popup_with(cx, access, crate::div())
    }

    /// Decorate the caller-owned popup through a per-instance accessor.
    pub fn popup_with_accessor<V: 'static>(
        &self,
        cx: &mut ViewContext<'_, V>,
        access: StateAccessor<V, Self>,
        popup: Element,
    ) -> Element {
        let dismissed = access.clone();
        let dismiss = cx.dismiss_listener(self.popup_id, move |view, cx| {
            if dismissed.get(view).close_now() {
                cx.invalidate();
            }
        });
        let mut popup = popup
            .id(self.popup_id)
            .accessibility_role(AccessibilityRole::Tooltip)
            .app_region_no_drag()
            .cursor_default()
            .dismiss_on_escape()
            .on_dismiss(dismiss);
        if self.hoverable {
            let hovered = self.hover_listener(cx, self.popup_id, TooltipHoverPart::Popup, &access);
            return popup.on_hover(hovered);
        }
        // A passive popup must not occlude what is underneath it: it can never keep itself open,
        // so taking the pointer would only steal hover from the controls it floats over.
        popup.blocks_pointer = false;
        popup
    }

    /// Position a caller-owned arrow on the edge the tooltip actually opened against.
    ///
    /// Size, shape, rotation, and color stay application-owned, and the arrow is hidden from
    /// assistive technology because the popup already carries the description relationship.
    pub fn arrow_with(&self, arrow: Element) -> Element {
        let arrow = arrow
            .id(self.arrow_id())
            .absolute()
            .accessibility_hidden(true)
            .app_region_no_drag();
        match self.resolved_side() {
            AnchorSide::Bottom => arrow.top(0.0),
            AnchorSide::Top => arrow.bottom(0.0),
            AnchorSide::Right => arrow.left(0.0),
            AnchorSide::Left => arrow.right(0.0),
        }
    }
    /// Create the unstyled arrow part. Use [`Self::arrow_with`] to supply an existing element.
    pub fn arrow(&self) -> Element {
        self.arrow_with(crate::div())
    }

    /// The point a cursor-tracking tooltip anchors to, once the trigger rectangle is known.
    fn cursor_anchor(&self) -> Option<Point> {
        if !self.track_cursor_axis.tracks_any() {
            return None;
        }
        let cursor = self.cursor?;
        // The trigger's own painted rectangle is known before the tooltip ever opens; the
        // resolved placement is the fallback for a trigger painted before it reported bounds.
        let anchor = self.trigger_bounds.bounds().or_else(|| {
            self.placement_handle
                .resolved()
                .map(|resolved| resolved.anchor)
        })?;
        let side = self.resolved_side();
        let pinned_x = match side {
            AnchorSide::Left => anchor.x,
            AnchorSide::Right => anchor.right(),
            AnchorSide::Top | AnchorSide::Bottom => anchor.x + anchor.width * 0.5,
        };
        let pinned_y = match side {
            AnchorSide::Top => anchor.y,
            AnchorSide::Bottom => anchor.bottom(),
            AnchorSide::Left | AnchorSide::Right => anchor.y + anchor.height * 0.5,
        };
        Some(Point::new(
            if self.track_cursor_axis.tracks_x() {
                cursor.x
            } else {
                pinned_x
            },
            if self.track_cursor_axis.tracks_y() {
                cursor.y
            } else {
                pinned_y
            },
        ))
    }

    fn hover_listener<V: 'static>(
        &self,
        cx: &mut ViewContext<'_, V>,
        id: ElementId,
        part: TooltipHoverPart,
        access: &StateAccessor<V, Self>,
    ) -> crate::HoverListener<V> {
        let access = access.clone();
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
        part: TooltipHoverPart,
        hovered: bool,
        access: StateAccessor<V, Self>,
        cx: &mut EventContext,
    ) {
        match part {
            TooltipHoverPart::Trigger => self.trigger_hovered = hovered,
            TooltipHoverPart::Popup => self.popup_hovered = hovered,
        }
        if !hovered && part == TooltipHoverPart::Trigger && !self.track_cursor_axis.tracks_any() {
            self.cursor = None;
        }
        let wants_open =
            !self.disabled && (self.trigger_hovered || (self.hoverable && self.popup_hovered));
        let phase = if wants_open {
            TooltipPhase::Open
        } else {
            TooltipPhase::Close
        };
        let settled = match phase {
            TooltipPhase::Open => self.open,
            TooltipPhase::Close => !self.open,
        };
        if settled {
            if self.pending.is_some() {
                self.cancel_pending();
            }
            return;
        }
        if self.pending == Some(phase) {
            return;
        }
        let delay = match phase {
            TooltipPhase::Open => {
                if self.provider.is_warm() {
                    Duration::ZERO
                } else {
                    self.delay.unwrap_or_else(|| self.provider.delay_value())
                }
            }
            TooltipPhase::Close => self
                .close_delay
                .unwrap_or_else(|| self.provider.close_delay_value()),
        };
        self.schedule(phase, delay, access, cx);
    }

    fn schedule<V: 'static>(
        &mut self,
        phase: TooltipPhase,
        delay: Duration,
        access: StateAccessor<V, Self>,
        cx: &mut EventContext,
    ) {
        self.cancel_pending();
        if delay.is_zero() {
            if self.apply(phase, true) {
                self.settle_group::<V>(phase, cx);
                cx.invalidate();
            }
            return;
        }
        self.pending = Some(phase);
        let generation = self.generation;
        let deferred = access.clone();
        let spawned = cx.spawn::<V, _, _, _>(move |task_cx: AsyncViewContext<V>| async move {
            if task_cx.sleep(delay).await.is_err() {
                return;
            }
            let _ = task_cx
                .update(move |view, cx| {
                    let state = deferred.get(view);
                    if state.generation != generation || state.pending != Some(phase) {
                        return;
                    }
                    state.pending = None;
                    state.task = None;
                    if state.apply(phase, false) {
                        state.settle_group::<V>(phase, cx);
                        cx.invalidate();
                    }
                })
                .await;
        });
        match spawned {
            Ok(task) => self.task = Some(task),
            Err(_) => {
                self.pending = None;
                if self.apply(phase, true) {
                    self.settle_group::<V>(phase, cx);
                    cx.invalidate();
                }
            }
        }
    }

    fn apply(&mut self, phase: TooltipPhase, instant: bool) -> bool {
        let open = phase == TooltipPhase::Open;
        self.instant = open && instant;
        std::mem::replace(&mut self.open, open) != open
    }

    /// Keep the group warm while a tooltip is open, and start the exact window when one closes.
    fn settle_group<V: 'static>(&mut self, phase: TooltipPhase, cx: &mut EventContext) {
        match phase {
            TooltipPhase::Open => self.provider.hold_warm(),
            TooltipPhase::Close => self.provider.start_cooldown::<V>(cx),
        }
    }
}

fn derived_tooltip_id(parent: ElementId, avoid: ElementId, tag: u64) -> ElementId {
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
    unreachable!("five distinct candidates cannot all match four reserved tooltip IDs")
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
    use taffy::style_helpers::length;

    #[test]
    fn text_tooltips_are_accessible_bounded_and_sanitized() {
        let tooltip = Tooltip::text("Save file")
            .delay(Duration::from_secs(60))
            .gap(f32::NAN)
            .viewport_margin(f32::INFINITY);

        assert_eq!(tooltip.delay, MAX_TOOLTIP_DELAY);
        assert_eq!(tooltip.gap, 0.0);
        assert_eq!(tooltip.viewport_margin, 0.0);
        assert_eq!(
            tooltip.accessibility_description.as_deref(),
            Some("Save file")
        );
    }

    #[test]
    #[should_panic(expected = "a tooltip cannot contain more than 256 elements")]
    fn oversized_tooltip_trees_fail_at_the_api_boundary() {
        let mut content = div();
        for _ in 0..MAX_TOOLTIP_CONTENT_NODES {
            content = content.child(div());
        }
        let _ = Tooltip::new(content);
    }

    #[test]
    fn compound_tooltip_parts_are_bounded_unstyled_and_related() {
        let tooltip = TooltipState::new("trigger", "popup")
            .side(AnchorSide::Right)
            .align(AnchorAlign::End)
            .side_offset(1.0e9)
            .collision_padding(f32::NAN)
            .close_on_click(false);
        assert_eq!(tooltip.trigger_id(), "trigger".into());
        assert_eq!(tooltip.popup_id(), "popup".into());
        assert_eq!(tooltip.resolved_side(), AnchorSide::Right);
        assert_eq!(tooltip.resolved_align(), AnchorAlign::End);
        assert!(!tooltip.is_open());
        assert!(!tooltip.is_disabled());

        let positioner = tooltip.positioner_with(div());
        assert_eq!(positioner.explicit_id, Some(tooltip.positioner_id()));
        assert!(positioner.reports_anchor_placement());
        let anchor = positioner.anchor.expect("tooltip positioner anchor");
        assert_eq!(anchor.placement, AnchorPlacement::RightEnd);
        assert_eq!(anchor.gap, MAX_TOOLTIP_SIDE_OFFSET);
        // A non-finite padding falls back to the declared default rather than to a bound.
        assert_eq!(anchor.viewport_margin, 8.0);
        assert_eq!(positioner.visual.background, None);
        assert_eq!(positioner.visual.border_color, None);
        assert_eq!(
            tooltip.portal_with(div()).explicit_id,
            Some(tooltip.positioner_id())
        );

        let arrow = tooltip.arrow_with(div());
        assert_eq!(arrow.explicit_id, Some(tooltip.arrow_id()));
        assert!(arrow.accessibility.hidden);
        assert_eq!(arrow.visual.background, None);
        // A right-side popup carries its arrow on its own leading edge.
        assert_eq!(arrow.layout.inset.left, length(0.0));
        assert_ne!(tooltip.arrow_id(), tooltip.positioner_id());
        assert_ne!(tooltip.arrow_id(), tooltip.popup_id());
        assert_ne!(tooltip.positioner_id(), tooltip.trigger_id());

        let flipped = TooltipState::new("trigger", "popup").side(AnchorSide::Left);
        assert_eq!(flipped.arrow_with(div()).layout.inset.right, length(0.0));
        let below = TooltipState::new("trigger", "popup").side(AnchorSide::Bottom);
        assert_eq!(below.arrow_with(div()).layout.inset.top, length(0.0));
        let above = TooltipState::new("trigger", "popup").side(AnchorSide::Top);
        assert_eq!(above.arrow_with(div()).layout.inset.bottom, length(0.0));

        let state = tooltip.state();
        assert_eq!(
            state,
            TooltipPartState {
                open: false,
                instant: false,
                disabled: false,
                side: AnchorSide::Right,
                align: AnchorAlign::End,
                anchor_hidden: false,
            }
        );
    }

    #[test]
    fn provider_timing_is_shared_bounded_and_group_warmth_is_explicit() {
        let provider = TooltipProvider::new();
        assert_eq!(provider.delay_value(), DEFAULT_TOOLTIP_HOVER_DELAY);
        assert_eq!(provider.close_delay_value(), Duration::ZERO);
        assert_eq!(provider.timeout_value(), DEFAULT_TOOLTIP_GROUP_TIMEOUT);
        assert!(!provider.is_warm());
        assert!(format!("{provider:?}").contains("TooltipProvider"));

        let bounded = TooltipProvider::new()
            .delay(Duration::from_secs(3_600))
            .close_delay(Duration::from_secs(3_600))
            .timeout(Duration::from_secs(3_600));
        assert_eq!(bounded.delay_value(), MAX_TOOLTIP_DELAY);
        assert_eq!(bounded.close_delay_value(), MAX_TOOLTIP_DELAY);
        assert_eq!(bounded.timeout_value(), MAX_TOOLTIP_GROUP_TIMEOUT);

        // Joining a provider shares one group; the default state owns a private one.
        let shared = provider.clone();
        assert_eq!(shared, provider);
        assert_ne!(TooltipProvider::new(), provider);
        let first = TooltipState::new("a", "a-popup").provider(&provider);
        let second = TooltipState::new("b", "b-popup").provider(&provider);
        assert_eq!(first.provider, second.provider);

        // An immediate open holds the group warm without scheduling anything.
        let mut third = TooltipState::new("c", "c-popup").provider(&provider);
        assert!(third.open_now());
        assert!(provider.is_warm());
        assert!(third.close_now());
        assert!(provider.is_warm());

        // A disabled tooltip refuses to open and drops an outstanding deadline.
        let mut disabled = TooltipState::new("d", "d-popup").disabled(true);
        assert!(!disabled.open_now());
        assert!(!disabled.is_open());
        let mut turned_off = TooltipState::new("e", "e-popup");
        assert!(turned_off.open_now());
        let turned_off = turned_off.disabled(true);
        assert!(!turned_off.is_open());
        assert!(!turned_off.is_pending());
    }

    struct TooltipGroupView {
        provider: TooltipProvider,
        left: TooltipState,
        right: TooltipState,
    }

    impl Default for TooltipGroupView {
        fn default() -> Self {
            let provider = TooltipProvider::new()
                .delay(Duration::from_millis(300))
                .close_delay(Duration::from_millis(80))
                .timeout(Duration::from_millis(400));
            Self {
                left: TooltipState::new("left", "left-popup")
                    .provider(&provider)
                    .side(AnchorSide::Bottom),
                right: TooltipState::new("right", "right-popup")
                    .provider(&provider)
                    .side(AnchorSide::Bottom),
                provider,
            }
        }
    }

    impl crate::View for TooltipGroupView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let left_trigger = self.left.trigger_with(
                cx,
                |view| &mut view.left,
                div().absolute().left(0.0).top(0.0).w(80.0).h(24.0),
            );
            let right_trigger = self.right.trigger_with(
                cx,
                |view| &mut view.right,
                div().absolute().left(200.0).top(0.0).w(80.0).h(24.0),
            );
            let mut root = div()
                .size_full()
                .relative()
                .child(left_trigger)
                .child(right_trigger);
            if self.left.is_open() {
                let popup = self
                    .left
                    .popup_with(cx, |view| &mut view.left, div().w(120.0).h(40.0));
                root = root.child(self.left.positioner_with(div().child(popup)));
            }
            if self.right.is_open() {
                let popup =
                    self.right
                        .popup_with(cx, |view| &mut view.right, div().w(120.0).h(40.0));
                root = root.child(self.right.positioner_with(div().child(popup)));
            }
            root
        }
    }

    #[test]
    fn grouped_tooltips_wait_once_then_open_instantly_inside_the_warm_window() {
        let (mut cx, view) = crate::TestAppContext::new(TooltipGroupView::default()).unwrap();
        let window = view.window_handle();

        cx.visual(window)
            .unwrap()
            .move_pointer(crate::Point::new(10.0, 10.0))
            .unwrap();
        assert!(cx.read(view, |view| view.left.is_pending()).unwrap());
        cx.advance_time(Duration::from_millis(299)).unwrap();
        assert!(!cx.read(view, |view| view.left.is_open()).unwrap());
        cx.advance_time(Duration::from_millis(1)).unwrap();
        assert!(cx.read(view, |view| view.left.is_open()).unwrap());
        assert!(cx.contains_element(window, "left-popup").unwrap());
        assert!(cx.read(view, |view| view.provider.is_warm()).unwrap());
        // The first tooltip waited, so it is not an instant one.
        assert!(!cx.read(view, |view| view.left.state().instant).unwrap());

        // Moving to the adjacent trigger closes the first after its delay and opens the second
        // immediately, because the group is still warm.
        cx.visual(window)
            .unwrap()
            .move_pointer(crate::Point::new(220.0, 10.0))
            .unwrap();
        assert!(cx.read(view, |view| view.right.is_open()).unwrap());
        assert!(cx.read(view, |view| view.right.state().instant).unwrap());
        assert!(cx.read(view, |view| view.left.is_pending()).unwrap());
        cx.advance_time(Duration::from_millis(80)).unwrap();
        assert!(!cx.read(view, |view| view.left.is_open()).unwrap());
        assert!(cx.contains_element(window, "right-popup").unwrap());

        // Leaving the group closes the tooltip and, one exact timeout later, cools it down.
        cx.visual(window)
            .unwrap()
            .move_pointer(crate::Point::new(600.0, 400.0))
            .unwrap();
        cx.advance_time(Duration::from_millis(80)).unwrap();
        assert!(!cx.read(view, |view| view.right.is_open()).unwrap());
        assert!(cx.read(view, |view| view.provider.is_warm()).unwrap());
        cx.advance_time(Duration::from_millis(399)).unwrap();
        assert!(cx.read(view, |view| view.provider.is_warm()).unwrap());
        cx.advance_time(Duration::from_millis(1)).unwrap();
        assert!(!cx.read(view, |view| view.provider.is_warm()).unwrap());

        // A settled group owns no timer, task, or idle frame.
        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);

        // The cooled group waits again.
        cx.visual(window)
            .unwrap()
            .move_pointer(crate::Point::new(10.0, 10.0))
            .unwrap();
        cx.advance_time(Duration::from_millis(299)).unwrap();
        assert!(!cx.read(view, |view| view.left.is_open()).unwrap());
        cx.advance_time(Duration::from_millis(1)).unwrap();
        assert!(cx.read(view, |view| view.left.is_open()).unwrap());
    }

    struct CursorTooltipView {
        tooltip: TooltipState,
    }

    impl Default for CursorTooltipView {
        fn default() -> Self {
            Self {
                tooltip: TooltipState::new("trigger", "popup")
                    .delay(Duration::ZERO)
                    .close_delay(Duration::ZERO)
                    .side(AnchorSide::Top)
                    .side_offset(6.0)
                    .track_cursor_axis(TooltipCursorAxis::X)
                    .hoverable(false),
            }
        }
    }

    impl crate::View for CursorTooltipView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let trigger = self.tooltip.trigger_with(
                cx,
                |view| &mut view.tooltip,
                div().absolute().left(100.0).top(200.0).w(200.0).h(40.0),
            );
            let mut root = div().size_full().relative().child(trigger);
            if self.tooltip.is_open() {
                let popup =
                    self.tooltip
                        .popup_with(cx, |view| &mut view.tooltip, div().w(80.0).h(30.0));
                root = root.child(self.tooltip.positioner_with(div().child(popup)));
            }
            root
        }
    }

    #[test]
    fn a_cursor_tracking_tooltip_follows_one_axis_and_pins_the_other_to_the_trigger_edge() {
        let (mut cx, view) = crate::TestAppContext::new(CursorTooltipView::default()).unwrap();
        let window = view.window_handle();

        cx.visual(window)
            .unwrap()
            .move_pointer(crate::Point::new(120.0, 210.0))
            .unwrap();
        assert!(cx.read(view, |view| view.tooltip.is_open()).unwrap());
        // A hover alone anchors the first open to the trigger; the pointer's own position only
        // arrives with a move event, which the live runtime delivers alongside the hover.
        let first = cx.element_bounds(window, "popup").unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(first.bottom(), 200.0 - 6.0);

        // Once the trigger rectangle is known, the horizontal axis follows the pointer while the
        // vertical axis stays pinned to the trigger's top edge.
        cx.simulate_mouse_move(
            window,
            "trigger",
            crate::MouseMoveEvent {
                position: crate::Point::new(280.0, 215.0),
                pressed_button: None,
                modifiers: crate::Modifiers::empty(),
            },
        )
        .unwrap();
        let tracked = cx.element_bounds(window, "popup").unwrap();
        assert_eq!(tracked.bottom(), 200.0 - 6.0);
        assert_eq!(tracked.x, 280.0 - 40.0);
        assert!(tracked.x > first.x);

        // A passive popup never takes the pointer away from what it floats over.
        cx.visual(window)
            .unwrap()
            .move_pointer(crate::Point::new(600.0, 400.0))
            .unwrap();
        assert!(!cx.read(view, |view| view.tooltip.is_open()).unwrap());
        assert!(!cx.contains_element(window, "popup").unwrap());

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }

    #[test]
    fn escape_and_trigger_presses_close_the_compound_tooltip() {
        let (mut cx, view) = crate::TestAppContext::new(CursorTooltipView::default()).unwrap();
        let window = view.window_handle();

        cx.visual(window)
            .unwrap()
            .move_pointer(crate::Point::new(120.0, 210.0))
            .unwrap();
        assert!(cx.read(view, |view| view.tooltip.is_open()).unwrap());
        cx.simulate_keystrokes(window, "escape").unwrap();
        assert!(!cx.read(view, |view| view.tooltip.is_open()).unwrap());

        cx.update(view, |view, cx| {
            view.tooltip.open_now();
            cx.invalidate();
        })
        .unwrap();
        assert!(cx.read(view, |view| view.tooltip.is_open()).unwrap());
        cx.simulate_mouse_down(
            window,
            "trigger",
            crate::MouseDownEvent {
                button: crate::MouseButton::Left,
                position: crate::Point::new(120.0, 210.0),
                modifiers: crate::Modifiers::empty(),
                click_count: 1,
                first_mouse: false,
            },
        )
        .unwrap();
        assert!(!cx.read(view, |view| view.tooltip.is_open()).unwrap());
    }
}
