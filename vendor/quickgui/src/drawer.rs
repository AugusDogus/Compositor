use std::sync::Arc;
use web_time::Instant;

use crate::{
    AccessibilityPopover, AccessibilityRole, Dialog, DismissListener, Element, ElementId,
    EventContext, MAX_WINDOW_LOGICAL_DIMENSION, PointerEvent, PointerListener, PointerPhase,
    StateAccessor, ViewContext, div,
};

/// Maximum snap points one drawer retains.
///
/// Snap points live in a fixed-size array, so this is the drawer's retained size rather than a
/// policy check.
pub const MAX_DRAWER_SNAP_POINTS: usize = 8;

/// Maximum drawers one composition may nest.
///
/// Each nested drawer adds a focus scope and an overlay plane, so the depth is declared and
/// bounded instead of being discovered at runtime.
pub const MAX_NESTED_DRAWERS: usize = 8;

/// Default flick speed, in logical pixels per millisecond, that dismisses a drawer.
pub const DEFAULT_DRAWER_DISMISS_VELOCITY: f32 = 0.5;

/// Largest flick speed a drawer may require, in logical pixels per millisecond.
pub const MAX_DRAWER_DISMISS_VELOCITY: f32 = 32.0;

const DRAWER_VIEWPORT_ID_TAG: u64 = 0x35a7_c1e9_046b_d872;
const DRAWER_CONTENT_ID_TAG: u64 = 0xbe40_92d5_71fa_38c6;
const DRAWER_SWIPE_AREA_ID_TAG: u64 = 0x7f18_6a3e_c94d_50b1;
const DRAWER_TRIGGER_ID_TAG: u64 = 0x0c92_45fd_8b17_e6a3;

/// How much of the window one drawer takes over while it is open.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DrawerModality {
    /// Contain focus, project modal semantics, and dismiss from the backdrop.
    #[default]
    Modal,
    /// Contain focus without projecting modal semantics.
    TrapFocus,
    /// Neither contain focus nor project modal semantics.
    NonModal,
}

impl DrawerModality {
    pub const fn traps_focus(self) -> bool {
        matches!(self, Self::Modal | Self::TrapFocus)
    }

    pub const fn is_modal(self) -> bool {
        matches!(self, Self::Modal)
    }
}

/// The edge a drawer is swiped toward to dismiss it.
///
/// The name is the direction of the dismissing gesture, matching Base UI: a bottom sheet uses
/// [`SwipeDirection::Down`], a left navigation drawer uses [`SwipeDirection::Left`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SwipeDirection {
    Up,
    #[default]
    Down,
    Left,
    Right,
}

impl SwipeDirection {
    pub const fn is_vertical(self) -> bool {
        matches!(self, Self::Up | Self::Down)
    }

    /// The sign that turns pointer movement into dismissing progress.
    const fn sign(self) -> f32 {
        match self {
            Self::Down | Self::Right => 1.0,
            Self::Up | Self::Left => -1.0,
        }
    }
}

/// What one captured drawer gesture changed.
///
/// The owner runs its `on_snap_point_change` when [`Self::snap_point_changed`] is set and its
/// `on_open_change` when [`Self::closed`] is set, which keeps both callbacks out of the retained
/// state and out of every frame that only moves the sheet.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DrawerGesture {
    /// Anything the application paints from changed.
    pub changed: bool,
    /// The active snap point moved.
    pub snap_point_changed: bool,
    /// The gesture dismissed the drawer.
    pub closed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct DrawerDrag {
    origin: f32,
    last: f32,
    last_at: Instant,
    velocity: f32,
}

/// Controlled, allocation-free state for one drawer.
///
/// The state owns the open value, the bounded snap points, the active snap point, and the captured
/// swipe: its live offset, whether a swipe is in progress, and the flick velocity that decides
/// between snapping and dismissing. The application owns every visual declaration and applies
/// [`Self::swipe_offset`] as a paint-only transform, so a drag never relayouts and never starts an
/// animation source. Nothing here observes, schedules, or animates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawerState {
    open: bool,
    direction: SwipeDirection,
    snap_points: [f32; MAX_DRAWER_SNAP_POINTS],
    snap_count: usize,
    snap_index: usize,
    default_snap: Option<usize>,
    swiping: bool,
    swipe_offset: f32,
    dismiss_velocity: f32,
    disable_pointer_dismissal: bool,
    drag: Option<DrawerDrag>,
}

impl Default for DrawerState {
    fn default() -> Self {
        Self::new(SwipeDirection::Down)
    }
}

impl DrawerState {
    /// Declare a closed drawer dismissed by swiping toward `direction`.
    pub fn new(direction: SwipeDirection) -> Self {
        let mut snap_points = [0.0; MAX_DRAWER_SNAP_POINTS];
        snap_points[0] = 1.0;
        Self {
            open: false,
            direction,
            snap_points,
            snap_count: 1,
            snap_index: 0,
            default_snap: None,
            swiping: false,
            swipe_offset: 0.0,
            dismiss_velocity: DEFAULT_DRAWER_DISMISS_VELOCITY,
            disable_pointer_dismissal: false,
            drag: None,
        }
    }

    /// Declare the drawer's snap points, smallest first.
    ///
    /// A value of `1.0` or less is a fraction of the viewport extent; a larger value is an
    /// absolute logical-pixel extent. Values are sorted, de-duplicated, and clamped to
    /// [`MAX_WINDOW_LOGICAL_DIMENSION`].
    ///
    /// # Panics
    ///
    /// Panics with more than [`MAX_DRAWER_SNAP_POINTS`] points or with no usable point at all.
    #[must_use]
    pub fn snap_points(mut self, snap_points: &[f32]) -> Self {
        assert!(
            snap_points.len() <= MAX_DRAWER_SNAP_POINTS,
            "a drawer retains at most {MAX_DRAWER_SNAP_POINTS} snap points"
        );
        let mut points: Vec<f32> = snap_points
            .iter()
            .copied()
            .filter(|point| point.is_finite() && *point > 0.0)
            .map(|point| point.min(MAX_WINDOW_LOGICAL_DIMENSION))
            .collect();
        points.sort_by(|left, right| left.partial_cmp(right).expect("finite snap points"));
        points.dedup();
        assert!(
            !points.is_empty(),
            "a drawer needs at least one positive snap point"
        );
        self.snap_points = [0.0; MAX_DRAWER_SNAP_POINTS];
        for (index, point) in points.iter().enumerate() {
            self.snap_points[index] = *point;
        }
        self.snap_count = points.len();
        self.snap_index = self.snap_index.min(self.snap_count - 1);
        self
    }

    /// Choose which snap point an opening drawer lands on. The default is the largest.
    #[must_use]
    pub const fn default_snap_point(mut self, index: usize) -> Self {
        self.default_snap = Some(index);
        self
    }

    /// The snap point an opening drawer lands on.
    pub const fn default_snap_point_index(&self) -> usize {
        match self.default_snap {
            Some(index) if index < self.snap_count => index,
            _ => self.snap_count - 1,
        }
    }

    /// Replace the flick speed, in logical pixels per millisecond, that dismisses the drawer.
    #[must_use]
    pub fn dismiss_velocity(mut self, velocity: f32) -> Self {
        self.dismiss_velocity = if velocity.is_finite() && velocity > 0.0 {
            velocity.min(MAX_DRAWER_DISMISS_VELOCITY)
        } else {
            DEFAULT_DRAWER_DISMISS_VELOCITY
        };
        self
    }

    /// Refuse dismissal from a backdrop press or a swipe, leaving Escape and the close control.
    #[must_use]
    pub const fn disable_pointer_dismissal(mut self, disable: bool) -> Self {
        self.disable_pointer_dismissal = disable;
        self
    }

    pub const fn is_open(&self) -> bool {
        self.open
    }

    pub const fn direction(&self) -> SwipeDirection {
        self.direction
    }

    pub const fn is_swiping(&self) -> bool {
        self.swiping
    }

    /// The live dismissing displacement of the sheet, in logical pixels.
    ///
    /// It is never negative: dragging away from the dismissing edge holds the sheet in place
    /// rather than tearing it off its anchor.
    pub const fn swipe_offset(&self) -> f32 {
        self.swipe_offset
    }

    pub const fn is_pointer_dismissal_disabled(&self) -> bool {
        self.disable_pointer_dismissal
    }

    /// The declared snap points, smallest first.
    pub fn snap_point_values(&self) -> &[f32] {
        &self.snap_points[..self.snap_count]
    }

    /// The active snap point's index.
    pub const fn snap_point(&self) -> usize {
        self.snap_index
    }

    /// The active snap point's declared value.
    pub const fn snap_point_value(&self) -> f32 {
        self.snap_points[self.snap_index]
    }

    /// Resolve one snap point against the viewport extent the application laid out.
    pub fn resolved_snap_point(&self, index: usize, extent: f32) -> f32 {
        let extent = if extent.is_finite() && extent > 0.0 {
            extent
        } else {
            0.0
        };
        let point = self.snap_points[index.min(self.snap_count - 1)];
        if point <= 1.0 { point * extent } else { point }
    }

    /// The active snap point resolved against the viewport extent.
    pub fn resolved_size(&self, extent: f32) -> f32 {
        self.resolved_snap_point(self.snap_index, extent)
    }

    /// Move to one snap point, returning whether it changed.
    pub fn set_snap_point(&mut self, index: usize) -> bool {
        let index = index.min(self.snap_count - 1);
        if self.snap_index == index {
            return false;
        }
        self.snap_index = index;
        true
    }

    /// Open the drawer at its declared default snap point, returning whether it changed.
    pub fn open(&mut self) -> bool {
        let changed = !self.open;
        self.open = true;
        self.swiping = false;
        self.swipe_offset = 0.0;
        self.drag = None;
        changed | self.set_snap_point(self.default_snap_point_index())
    }

    /// Close the drawer and discard any in-flight swipe, returning whether it changed.
    pub fn close(&mut self) -> bool {
        let changed = self.open || self.swiping || self.swipe_offset != 0.0;
        self.open = false;
        self.swiping = false;
        self.swipe_offset = 0.0;
        self.drag = None;
        changed
    }

    /// Force the open value, returning whether it changed.
    pub fn set_open(&mut self, open: bool) -> bool {
        if open { self.open() } else { self.close() }
    }

    /// Apply one captured pointer event from the swipe area or the popup.
    ///
    /// `extent` is the viewport extent along the swipe axis the application laid out — the same
    /// number snap-point fractions resolve against. A press starts the gesture, moves accumulate
    /// the dismissing displacement and an exact velocity, and the release either snaps to the
    /// nearest declared point or dismisses the drawer.
    pub fn apply_pointer(
        &mut self,
        event: &PointerEvent,
        extent: f32,
        now: Instant,
    ) -> DrawerGesture {
        let mut gesture = DrawerGesture::default();
        if !self.open {
            return gesture;
        }
        let position = if self.direction.is_vertical() {
            event.position.y
        } else {
            event.position.x
        };
        let position = if position.is_finite() { position } else { 0.0 };
        match event.phase {
            PointerPhase::Down => {
                self.drag = Some(DrawerDrag {
                    origin: position,
                    last: position,
                    last_at: now,
                    velocity: 0.0,
                });
                gesture.changed = !self.swiping;
                self.swiping = true;
                self.swipe_offset = 0.0;
                gesture
            }
            PointerPhase::Move => {
                let Some(drag) = &mut self.drag else {
                    return gesture;
                };
                let elapsed = now.saturating_duration_since(drag.last_at).as_secs_f32() * 1_000.0;
                if elapsed > 0.0 {
                    drag.velocity = (position - drag.last) * self.direction.sign() / elapsed;
                }
                drag.last = position;
                drag.last_at = now;
                let offset = ((position - drag.origin) * self.direction.sign()).max(0.0);
                if self.swipe_offset != offset {
                    self.swipe_offset = offset;
                    gesture.changed = true;
                }
                gesture
            }
            PointerPhase::Up => {
                let Some(drag) = self.drag.take() else {
                    return gesture;
                };
                self.swiping = false;
                let offset = self.swipe_offset;
                self.swipe_offset = 0.0;
                gesture.changed = true;
                if self.disable_pointer_dismissal && self.snap_count == 1 {
                    return gesture;
                }
                let flicked = drag.velocity >= self.dismiss_velocity;
                let projected = (self.resolved_size(extent) - offset).max(0.0);
                let target = if flicked {
                    self.snap_index.checked_sub(1)
                } else {
                    Some(self.nearest_snap_point(projected, extent))
                };
                match target {
                    Some(index)
                        if !flicked
                            && projected < self.resolved_snap_point(0, extent) * 0.5
                            && index == 0 =>
                    {
                        gesture.closed = !self.disable_pointer_dismissal && self.close();
                        if self.disable_pointer_dismissal {
                            gesture.snap_point_changed = self.set_snap_point(0);
                        }
                    }
                    Some(index) => gesture.snap_point_changed = self.set_snap_point(index),
                    None => {
                        if self.disable_pointer_dismissal {
                            gesture.snap_point_changed = self.set_snap_point(0);
                        } else {
                            gesture.closed = self.close();
                        }
                    }
                }
                gesture
            }
            PointerPhase::Cancel => {
                let dragging = self.drag.take().is_some();
                gesture.changed = dragging || self.swipe_offset != 0.0 || self.swiping;
                self.swiping = false;
                self.swipe_offset = 0.0;
                gesture
            }
        }
    }

    fn nearest_snap_point(&self, size: f32, extent: f32) -> usize {
        let mut best = 0;
        let mut distance = f32::INFINITY;
        for index in 0..self.snap_count {
            let candidate = (self.resolved_snap_point(index, extent) - size).abs();
            if candidate < distance {
                distance = candidate;
                best = index;
            }
        }
        best
    }
}

/// A copyable declaration for one controlled, unstyled drawer.
///
/// A drawer is a dialog anchored to a window edge that can also be swiped away. QuickGUI supplies
/// stable part identities, the viewport portal, focus containment and restoration reused from
/// [`Dialog`], Escape and backdrop dismissal, and the exact drawer accessibility semantics; the
/// application owns every visual declaration, the sheet's size and edge, and the paint-only
/// transform it applies from [`DrawerState::swipe_offset`].
///
/// The portal covers the window viewport on every modality so placement and native-view occlusion
/// stay framework-owned; [`DrawerModality`] selects focus containment and modal semantics.
///
/// The descriptor retains no allocation, task, timer, observer, or idle scheduler source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "a Drawer descriptor has no effect until its parts are mounted"]
pub struct Drawer {
    id: ElementId,
    dialog: Dialog,
    modality: DrawerModality,
    direction: SwipeDirection,
    depth: usize,
}

impl Drawer {
    /// Declare a drawer over a caller-owned identity.
    pub fn new(id: impl Into<ElementId>, open: bool) -> Self {
        let id = id.into();
        Self {
            id,
            dialog: Dialog::new(id, open),
            modality: DrawerModality::Modal,
            direction: SwipeDirection::Down,
            depth: 0,
        }
    }

    /// Declare a drawer from its retained state, inheriting the swipe direction and dismissal.
    pub fn from_state(id: impl Into<ElementId>, state: &DrawerState) -> Self {
        Self::new(id, state.is_open())
            .swipe_direction(state.direction())
            .disable_pointer_dismissal(state.is_pointer_dismissal_disabled())
    }

    pub const fn modal(mut self, modality: DrawerModality) -> Self {
        self.modality = modality;
        self
    }

    pub const fn swipe_direction(mut self, direction: SwipeDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Refuse dismissal from a backdrop press, leaving Escape and the close control.
    pub const fn disable_pointer_dismissal(mut self, disable: bool) -> Self {
        self.dialog = self.dialog.dismiss_on_backdrop(!disable);
        self
    }

    /// Prefer one mounted descendant when the drawer opens.
    pub fn initial_focus(mut self, focus: impl Into<ElementId>) -> Self {
        self.dialog = self.dialog.initial_focus(focus);
        self
    }

    /// Restore focus to one stable control after every dismissal path.
    pub fn restore_focus_to(mut self, focus: impl Into<ElementId>) -> Self {
        self.dialog = self.dialog.restore_focus_to(focus);
        self
    }

    /// Declare this drawer's nesting depth.
    ///
    /// # Panics
    ///
    /// Panics at or beyond [`MAX_NESTED_DRAWERS`], so a recursive composition fails at the
    /// declaration rather than by exhausting focus scopes and overlay planes.
    pub fn depth(mut self, depth: usize) -> Self {
        assert!(
            depth < MAX_NESTED_DRAWERS,
            "a drawer may nest at most {MAX_NESTED_DRAWERS} deep"
        );
        self.depth = depth;
        self
    }

    pub const fn nesting_depth(self) -> usize {
        self.depth
    }

    pub const fn is_open(self) -> bool {
        self.dialog.is_open()
    }

    pub const fn modality(self) -> DrawerModality {
        self.modality
    }

    pub const fn swipe_axis(self) -> SwipeDirection {
        self.direction
    }

    /// The composed dialog, for callers that need its remaining behavior directly.
    pub const fn dialog(self) -> Dialog {
        self.dialog
    }

    pub const fn root_id(self) -> ElementId {
        self.id
    }

    pub fn trigger_id(self) -> ElementId {
        derived_drawer_id(self.id, DRAWER_TRIGGER_ID_TAG)
    }

    pub fn portal_id(self) -> ElementId {
        self.dialog.root_id()
    }

    pub fn backdrop_id(self) -> ElementId {
        self.dialog.backdrop_id()
    }

    pub fn viewport_id(self) -> ElementId {
        derived_drawer_id(self.id, DRAWER_VIEWPORT_ID_TAG)
    }

    pub fn popup_id(self) -> ElementId {
        self.dialog.popover_id()
    }

    pub fn content_id(self) -> ElementId {
        derived_drawer_id(self.id, DRAWER_CONTENT_ID_TAG)
    }

    pub fn swipe_area_id(self) -> ElementId {
        derived_drawer_id(self.id, DRAWER_SWIPE_AREA_ID_TAG)
    }

    pub fn title_id(self) -> ElementId {
        self.dialog.title_id()
    }

    pub fn description_id(self) -> ElementId {
        self.dialog.description_id()
    }

    pub fn close_id(self) -> ElementId {
        self.dialog.close_id()
    }

    /// Request the declared initial focus in the same event that mounts the drawer.
    pub fn focus_initial(self, cx: &mut EventContext) {
        self.dialog.focus_initial(cx);
    }

    /// Restore the declared focus after an explicit close action.
    pub fn focus_restore(self, cx: &mut EventContext) {
        self.dialog.focus_restore(cx);
    }

    /// Decorate the optional application-owned structural wrapper.
    pub fn root_with(self, root: Element) -> Element {
        root.id(self.id).app_region_no_drag()
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(self) -> Element {
        self.root_with(crate::div())
    }

    /// Decorate an application-owned trigger without adding appearance.
    pub fn trigger_with(self, trigger: Element) -> Element {
        let trigger = trigger
            .id(self.trigger_id())
            .focusable()
            .accessibility_role(AccessibilityRole::Button)
            .accessibility_has_popover(AccessibilityPopover::Dialog)
            .accessibility_expanded(self.is_open())
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

    /// Decorate the full-window portal and, for a containing modality, the focus boundary.
    pub fn portal_with(self, portal: Element) -> Element {
        if self.modality.traps_focus() {
            return self.dialog.root_with(portal);
        }
        portal
            .id(self.portal_id())
            .overlay()
            .inset_0()
            .size_full()
            .app_region_no_drag()
            .cursor_default()
    }
    /// Create the unstyled portal part. Use [`Self::portal_with`] to supply an existing element.
    pub fn portal(self) -> Element {
        self.portal_with(crate::div())
    }

    /// Decorate the caller-owned visual backdrop.
    pub fn backdrop_with(self, backdrop: Element) -> Element {
        self.dialog.backdrop_with(backdrop)
    }
    /// Create the unstyled backdrop part. Use [`Self::backdrop_with`] to supply an existing element.
    pub fn backdrop(self) -> Element {
        self.backdrop_with(crate::div())
    }

    /// Decorate the container that aligns the sheet against its edge.
    ///
    /// QuickGUI adds no alignment of its own: the application declares the flex or grid alignment
    /// that puts a bottom sheet at the bottom and a side drawer against a side.
    pub fn viewport_with(self, viewport: Element) -> Element {
        viewport
            .id(self.viewport_id())
            .size_full()
            .app_region_no_drag()
            .cursor_default()
    }
    /// Create the unstyled viewport part. Use [`Self::viewport_with`] to supply an existing element.
    pub fn viewport(self) -> Element {
        self.viewport_with(crate::div())
    }

    /// Decorate the caller-owned sheet.
    ///
    /// The sheet is the dialog surface: it carries the drawer role, the title and description
    /// relationships, Escape dismissal, backdrop dismissal unless it is disabled, and focus
    /// restoration. Apply [`DrawerState::swipe_offset`] to it as a paint-only transform.
    pub fn popup_with(self, popup: Element) -> Element {
        self.dialog
            .popup_with(popup)
            .accessibility_modal(self.modality.is_modal())
    }
    /// Create the unstyled popup part. Use [`Self::popup_with`] to supply an existing element.
    pub fn popup(self) -> Element {
        self.popup_with(crate::div())
    }

    /// Decorate the caller-owned scrollable body of the sheet.
    pub fn content_with(self, content: Element) -> Element {
        content.id(self.content_id())
    }
    /// Create the unstyled content part. Use [`Self::content_with`] to supply an existing element.
    pub fn content(self) -> Element {
        self.content_with(crate::div())
    }

    /// Decorate the caller-owned grab handle that starts a swipe.
    ///
    /// Attach a [`crate::ViewContext::pointer_listener`] registered for [`Self::swipe_area_id`]
    /// and forward the event to [`DrawerState::apply_pointer`], or use [`Self::on_swipe`].
    pub fn swipe_area_with(self, swipe_area: Element) -> Element {
        swipe_area
            .id(self.swipe_area_id())
            .accessibility_hidden(true)
            .app_region_no_drag()
            .user_select_none()
            .cursor_default()
    }
    /// Create the unstyled swipe area part. Use [`Self::swipe_area_with`] to supply an existing element.
    pub fn swipe_area(self) -> Element {
        self.swipe_area_with(crate::div())
    }

    /// Assign the stable visible label target used by the sheet.
    pub fn title_with(self, title: Element) -> Element {
        self.dialog.title_with(title)
    }
    /// Create the unstyled title part. Use [`Self::title_with`] to supply an existing element.
    pub fn title(self) -> Element {
        self.title_with(crate::div())
    }

    /// Assign the stable visible description target used by the sheet.
    pub fn description_with(self, description: Element) -> Element {
        self.dialog.description_with(description)
    }
    /// Create the unstyled description part. Use [`Self::description_with`] to supply an existing element.
    pub fn description(self) -> Element {
        self.description_with(crate::div())
    }

    /// Decorate a caller-owned close control with button behavior and no visual defaults.
    pub fn close_with(self, label: impl Into<Arc<str>>, close: Element) -> Element {
        self.dialog.close_with(label, close)
    }
    /// Create the unstyled close part. Use [`Self::close_with`] to supply an existing element.
    pub fn close(self, label: impl Into<Arc<str>>) -> Element {
        self.close_with(label, crate::button())
    }

    /// Build the swipe area's captured pointer behavior.
    ///
    /// `extent` is the viewport extent along the swipe axis the application laid out. Attach the
    /// returned handle with [`crate::Element::on_pointer`].
    pub fn on_swipe<V: 'static, Snap, Open>(
        self,
        cx: &mut ViewContext<'_, V>,
        extent: f32,
        access: fn(&mut V) -> &mut DrawerState,
        on_snap_point_change: Snap,
        on_open_change: Open,
    ) -> PointerListener<V>
    where
        Snap: Fn(&mut V, usize, &mut EventContext) + 'static,
        Open: Fn(&mut V, bool, &mut EventContext) + 'static,
    {
        self.on_swipe_with(
            cx,
            extent,
            StateAccessor::from(access),
            on_snap_point_change,
            on_open_change,
        )
    }

    /// Build the swipe behavior against a per-instance state accessor.
    pub fn on_swipe_with<V: 'static, Snap, Open>(
        self,
        cx: &mut ViewContext<'_, V>,
        extent: f32,
        access: StateAccessor<V, DrawerState>,
        on_snap_point_change: Snap,
        on_open_change: Open,
    ) -> PointerListener<V>
    where
        Snap: Fn(&mut V, usize, &mut EventContext) + 'static,
        Open: Fn(&mut V, bool, &mut EventContext) + 'static,
    {
        cx.pointer_listener(self.swipe_area_id(), move |view, event, cx| {
            let gesture = access
                .get(view)
                .apply_pointer(event, extent, Instant::now());
            if gesture.snap_point_changed {
                let index = access.get(view).snap_point();
                on_snap_point_change(view, index, cx);
            }
            if gesture.closed {
                on_open_change(view, false, cx);
            }
            if gesture.changed || gesture.snap_point_changed || gesture.closed {
                cx.invalidate();
            }
        })
    }

    /// Build the sheet's dismissal behavior for Escape and backdrop presses.
    pub fn on_dismiss<V: 'static, Open>(
        self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut DrawerState,
        on_open_change: Open,
    ) -> DismissListener<V>
    where
        Open: Fn(&mut V, bool, &mut EventContext) + 'static,
    {
        self.on_dismiss_with(cx, StateAccessor::from(access), on_open_change)
    }

    /// Build the sheet's dismissal behavior against a per-instance state accessor.
    pub fn on_dismiss_with<V: 'static, Open>(
        self,
        cx: &mut ViewContext<'_, V>,
        access: StateAccessor<V, DrawerState>,
        on_open_change: Open,
    ) -> DismissListener<V>
    where
        Open: Fn(&mut V, bool, &mut EventContext) + 'static,
    {
        cx.dismiss_listener(self.popup_id(), move |view, cx| {
            if access.get(view).close() {
                on_open_change(view, false, cx);
                cx.invalidate();
            }
        })
    }
}

/// Create an unstyled drawer sheet root.
///
/// This shorthand is equivalent to `Drawer::from_state(id, state).popup_with(div())`.
pub fn drawer_popup(id: impl Into<ElementId>, state: &DrawerState) -> Element {
    Drawer::from_state(id, state).popup_with(div())
}

fn derived_drawer_id(scope: ElementId, tag: u64) -> ElementId {
    let mut hash = scope.as_u64().rotate_left(43) ^ tag;
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    if hash == 0 || hash == u64::MAX || hash == scope.as_u64() {
        hash ^= tag.rotate_left(29);
    }
    ElementId::new(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Color, IntoElement, Modifiers, MouseButton, Point, Size, TestAppContext, Vector, View,
        button, text,
    };
    use web_time::Duration;

    fn pointer_event(phase: PointerPhase, y: f32) -> PointerEvent {
        PointerEvent {
            size: Size::new(400.0, 300.0),
            phase,
            position: Point::new(10.0, y),
            origin: Point::new(10.0, 0.0),
            local_position: Point::new(10.0, y),
            local_origin: Point::new(10.0, 0.0),
            delta: Vector::ZERO,
            button: MouseButton::Left,
            modifiers: Modifiers::empty(),
        }
    }

    #[test]
    fn snap_points_are_bounded_sorted_and_resolved() {
        let state = DrawerState::new(SwipeDirection::Down).snap_points(&[
            0.9,
            0.25,
            f32::NAN,
            -1.0,
            0.25,
            600.0,
        ]);
        assert_eq!(state.snap_point_values(), &[0.25, 0.9, 600.0]);
        assert_eq!(state.resolved_snap_point(0, 800.0), 200.0);
        assert_eq!(state.resolved_snap_point(1, 800.0), 720.0);
        assert_eq!(
            state.resolved_snap_point(2, 800.0),
            600.0,
            "a value above one is an absolute extent"
        );
        assert_eq!(state.resolved_snap_point(9, 800.0), 600.0);
        assert_eq!(state.resolved_snap_point(0, f32::NAN), 0.0);

        assert_eq!(state.default_snap_point_index(), 2);
        let mut opened = state.default_snap_point(1);
        assert!(opened.open());
        assert!(opened.is_open());
        assert_eq!(opened.snap_point(), 1);
        assert_eq!(opened.snap_point_value(), 0.9);
        assert!(!opened.set_snap_point(1));
        assert!(opened.set_snap_point(99));
        assert_eq!(opened.snap_point(), 2);
        assert!(opened.close());
        assert!(!opened.close());
        assert!(opened.set_open(true));
        assert_eq!(DrawerState::default().direction(), SwipeDirection::Down);
        assert!(!SwipeDirection::Left.is_vertical());
    }

    #[test]
    #[should_panic(expected = "at most")]
    fn too_many_snap_points_are_rejected() {
        let _ =
            DrawerState::new(SwipeDirection::Down).snap_points(&[0.1; MAX_DRAWER_SNAP_POINTS + 1]);
    }

    #[test]
    #[should_panic(expected = "at least one positive")]
    fn empty_snap_points_are_rejected() {
        let _ = DrawerState::new(SwipeDirection::Down).snap_points(&[0.0, -3.0]);
    }

    #[test]
    #[should_panic(expected = "at most")]
    fn deep_nesting_is_rejected() {
        let _ = Drawer::new("sheet", true).depth(MAX_NESTED_DRAWERS);
    }

    #[test]
    fn captured_swipes_snap_flick_and_dismiss() {
        let start = Instant::now();
        let extent = 800.0;
        let mut state = DrawerState::new(SwipeDirection::Down).snap_points(&[0.25, 1.0]);
        state.open();
        assert_eq!(state.snap_point(), 1);
        assert_eq!(state.resolved_size(extent), 800.0);

        // A slow drag toward the lower snap point lands on it.
        assert!(
            state
                .apply_pointer(&pointer_event(PointerPhase::Down, 0.0), extent, start)
                .changed
        );
        assert!(state.is_swiping());
        let slow = start + Duration::from_millis(400);
        assert!(
            state
                .apply_pointer(&pointer_event(PointerPhase::Move, 500.0), extent, slow)
                .changed
        );
        assert_eq!(state.swipe_offset(), 500.0);
        // Dragging back past the anchor never produces a negative offset.
        let back = slow + Duration::from_millis(400);
        state.apply_pointer(&pointer_event(PointerPhase::Move, -200.0), extent, back);
        assert_eq!(state.swipe_offset(), 0.0);
        let settle = back + Duration::from_millis(400);
        state.apply_pointer(&pointer_event(PointerPhase::Move, 550.0), extent, settle);
        let release = settle + Duration::from_millis(400);
        let gesture = state.apply_pointer(&pointer_event(PointerPhase::Up, 550.0), extent, release);
        assert!(gesture.snap_point_changed);
        assert!(!gesture.closed);
        assert_eq!(state.snap_point(), 0);
        assert!(!state.is_swiping());
        assert_eq!(state.swipe_offset(), 0.0);

        // A fast flick from the lowest snap point dismisses.
        let flick_start = release + Duration::from_secs(1);
        state.apply_pointer(&pointer_event(PointerPhase::Down, 0.0), extent, flick_start);
        let flick_move = flick_start + Duration::from_millis(10);
        state.apply_pointer(&pointer_event(PointerPhase::Move, 40.0), extent, flick_move);
        let flick_end = flick_move + Duration::from_millis(1);
        let gesture =
            state.apply_pointer(&pointer_event(PointerPhase::Up, 44.0), extent, flick_end);
        assert!(gesture.closed);
        assert!(!state.is_open());

        // Cancelling a swipe restores the sheet without changing the snap point.
        state.open();
        state.apply_pointer(&pointer_event(PointerPhase::Down, 0.0), extent, flick_end);
        state.apply_pointer(&pointer_event(PointerPhase::Move, 100.0), extent, flick_end);
        let cancelled = state.apply_pointer(
            &pointer_event(PointerPhase::Cancel, 100.0),
            extent,
            flick_end,
        );
        assert!(cancelled.changed);
        assert!(state.is_open());
        assert_eq!(state.swipe_offset(), 0.0);

        // A closed drawer answers no pointer at all.
        state.close();
        assert_eq!(
            state.apply_pointer(&pointer_event(PointerPhase::Down, 0.0), extent, flick_end),
            DrawerGesture::default()
        );

        // Pointer dismissal can be refused entirely.
        let mut protected = DrawerState::new(SwipeDirection::Down).disable_pointer_dismissal(true);
        protected.open();
        protected.apply_pointer(&pointer_event(PointerPhase::Down, 0.0), extent, flick_end);
        protected.apply_pointer(&pointer_event(PointerPhase::Move, 700.0), extent, flick_end);
        let refused =
            protected.apply_pointer(&pointer_event(PointerPhase::Up, 700.0), extent, flick_end);
        assert!(!refused.closed);
        assert!(protected.is_open());

        // A horizontal drawer measures the other axis.
        let mut side = DrawerState::new(SwipeDirection::Left);
        side.open();
        side.apply_pointer(&pointer_event(PointerPhase::Down, 0.0), extent, flick_end);
        assert!(side.is_swiping());
    }

    #[test]
    fn parts_add_exact_behavior_without_appearance() {
        let drawer = Drawer::new("sheet", true)
            .swipe_direction(SwipeDirection::Down)
            .restore_focus_to("open-sheet")
            .depth(1);
        assert_eq!(drawer.nesting_depth(), 1);
        assert_eq!(drawer.swipe_axis(), SwipeDirection::Down);
        assert_eq!(drawer.modality(), DrawerModality::Modal);

        let trigger = drawer.trigger_with(button());
        assert_eq!(trigger.explicit_id, Some(drawer.trigger_id()));
        assert_eq!(trigger.accessibility.expanded, Some(true));
        assert_eq!(
            trigger.accessibility.relations.controls(),
            Some(drawer.popup_id())
        );

        let portal = drawer.portal_with(div());
        assert!(portal.portal);
        assert!(portal.focus_trap);
        assert!(portal.restore_previous_focus);
        assert_eq!(portal.visual.background, None);

        let open = Drawer::new("sheet", true).modal(DrawerModality::NonModal);
        let loose = open.portal_with(div());
        assert!(loose.portal);
        assert!(!loose.focus_trap);
        assert!(!open.popup_with(div()).accessibility.modal);
        assert!(
            Drawer::new("sheet", true)
                .modal(DrawerModality::TrapFocus)
                .portal_with(div())
                .focus_trap
        );

        let popup = drawer.popup_with(div().bg(Color::rgb8(4, 5, 6)));
        assert_eq!(popup.explicit_id, Some(drawer.popup_id()));
        assert_eq!(popup.accessibility.role, AccessibilityRole::Dialog);
        assert!(popup.accessibility.modal);
        assert!(popup.dismiss_policy.on_escape());
        assert!(popup.dismiss_policy.on_pointer_outside());
        assert_eq!(popup.visual.background, Some(Color::rgb8(4, 5, 6)));

        let protected = Drawer::new("sheet", true).disable_pointer_dismissal(true);
        assert!(
            !protected
                .popup_with(div())
                .dismiss_policy
                .on_pointer_outside()
        );
        assert!(protected.popup_with(div()).dismiss_policy.on_escape());

        let viewport = drawer.viewport_with(div());
        assert_eq!(viewport.explicit_id, Some(drawer.viewport_id()));
        let content = drawer.content_with(div());
        assert_eq!(content.explicit_id, Some(drawer.content_id()));
        let swipe = drawer.swipe_area_with(div().h(20.0));
        assert_eq!(swipe.explicit_id, Some(drawer.swipe_area_id()));
        assert!(swipe.accessibility.hidden);
        let backdrop = drawer.backdrop_with(div());
        assert_eq!(backdrop.explicit_id, Some(drawer.backdrop_id()));
        assert_eq!(drawer.root_with(div()).explicit_id, Some("sheet".into()));
        assert_eq!(
            drawer.title_with(div()).explicit_id,
            Some(drawer.title_id())
        );
        assert_eq!(
            drawer.description_with(div()).explicit_id,
            Some(drawer.description_id())
        );
        assert_eq!(
            drawer.close_with("Close", div()).explicit_id,
            Some(drawer.close_id())
        );
        assert_eq!(drawer.dialog().popover_id(), drawer.popup_id());

        let ids = [
            drawer.root_id(),
            drawer.trigger_id(),
            drawer.portal_id(),
            drawer.backdrop_id(),
            drawer.viewport_id(),
            drawer.popup_id(),
            drawer.content_id(),
            drawer.swipe_area_id(),
            drawer.title_id(),
            drawer.description_id(),
            drawer.close_id(),
        ];
        for (index, id) in ids.iter().enumerate() {
            assert_ne!(*id, ElementId::new(0));
            assert_ne!(*id, ElementId::new(u64::MAX));
            assert!(!ids[..index].contains(id));
        }

        let state = DrawerState::new(SwipeDirection::Down);
        let shorthand = drawer_popup("sheet", &state);
        assert_eq!(shorthand.accessibility.role, AccessibilityRole::Dialog);
        assert!(shorthand.children.is_empty());
    }

    const EXTENT: f32 = 300.0;

    struct DrawerView {
        sheet: DrawerState,
        snaps: Vec<usize>,
        opens: Vec<bool>,
    }

    impl DrawerView {
        fn sheet(view: &mut Self) -> &mut DrawerState {
            &mut view.sheet
        }
    }

    impl View for DrawerView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let drawer = Drawer::from_state("sheet", &self.sheet)
                .initial_focus("sheet-first")
                .restore_focus_to(Drawer::new("sheet", false).trigger_id());
            let open = cx.listener(drawer.trigger_id(), move |view, cx| {
                if view.sheet.open() {
                    view.opens.push(true);
                    drawer.focus_initial(cx);
                    cx.invalidate();
                }
            });
            let close = cx.listener(drawer.close_id(), move |view, cx| {
                if view.sheet.close() {
                    view.opens.push(false);
                    drawer.focus_restore(cx);
                    cx.invalidate();
                }
            });
            let dismiss = drawer.on_dismiss(cx, Self::sheet, |view, open, _| {
                view.opens.push(open);
            });
            let swipe = drawer.on_swipe(
                cx,
                EXTENT,
                Self::sheet,
                |view, index, _| view.snaps.push(index),
                |view, open, _| view.opens.push(open),
            );

            let mut root = drawer
                .root_with(div().size_full().relative())
                .child(drawer.trigger_with(button().child("Open")).on_click(open))
                .child(button().id("outside").child("Outside"));
            if drawer.is_open() {
                root = root.child(
                    drawer
                        .portal_with(div())
                        .child(drawer.backdrop_with(div()))
                        .child(
                            drawer.viewport_with(div().flex_col().justify_end()).child(
                                drawer
                                    .popup_with(div().w(400.0).h(EXTENT))
                                    .translate(0.0, self.sheet.swipe_offset())
                                    .on_dismiss(dismiss)
                                    .child(drawer.swipe_area_with(div().h(20.0).on_pointer(swipe)))
                                    .child(drawer.title_with(text("Filters")))
                                    .child(drawer.description_with(text("Narrow the results")))
                                    .child(
                                        drawer
                                            .content_with(div())
                                            .child(button().id("sheet-first").child("First"))
                                            .child(button().id("sheet-second").child("Second")),
                                    )
                                    .child(
                                        drawer.close_with("Close filters", div()).on_click(close),
                                    ),
                            ),
                        ),
                );
            }
            root
        }
    }

    #[test]
    fn controlled_drawer_traps_focus_swipes_and_sleeps() {
        let (mut cx, view) = TestAppContext::new(DrawerView {
            sheet: DrawerState::new(SwipeDirection::Down).snap_points(&[0.4, 1.0]),
            snaps: Vec::new(),
            opens: Vec::new(),
        })
        .unwrap();
        let window = view.window_handle();
        let drawer = Drawer::new("sheet", true);

        cx.click(window, drawer.trigger_id()).unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some("sheet-first".into()));
        assert!(cx.focus(window, "outside").is_err(), "focus is contained");

        cx.simulate_keystrokes(window, "tab").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some("sheet-second".into()));

        // The swipe area is mounted and hidden from assistive technology while the sheet is open.
        assert!(cx.contains_element(window, drawer.swipe_area_id()).unwrap());
        assert!(cx.contains_element(window, drawer.content_id()).unwrap());
        assert!(cx.read(view, |view| view.sheet.is_open()).unwrap());

        cx.simulate_keystrokes(window, "escape").unwrap();
        assert!(!cx.read(view, |view| view.sheet.is_open()).unwrap());
        assert_eq!(cx.focused(window).unwrap(), Some(drawer.trigger_id()));

        let update = cx.accessibility_update(window).unwrap();
        let trigger = update
            .nodes
            .iter()
            .find_map(|(id, node)| (id.0 == drawer.trigger_id().as_u64()).then_some(node))
            .expect("drawer trigger accessibility node");
        assert_eq!(trigger.role(), accesskit::Role::Button);
        assert_eq!(trigger.is_expanded(), Some(false));

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }
}
