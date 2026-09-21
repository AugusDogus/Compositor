use crate::{
    AccessibilityOrientation, AccessibilityRole, AccessibilityValueRange, CursorStyle, Element,
    ElementId, KeyBinding, PointerEvent, PointerPhase, StateAccessor, ViewContext,
};

/// Maximum panes managed by one splitter.
///
/// Panes and their handles are retained as fixed-size arrays so a splitter stays copyable and
/// allocation-free. Deeper layouts nest splitters instead of growing one.
pub const MAX_SPLITTER_PANES: usize = 16;

const SPLITTER_KEY_CONTEXT: &str = "Splitter";
const SPLITTER_PANE_ID_TAG: u64 = 0x2b91_44d7_0fa6_e315;
const SPLITTER_HANDLE_ID_TAG: u64 = 0x74c2_e8a1_5d30_9b6f;
const DEFAULT_KEYBOARD_STEP: f32 = 16.0;

/// The window-space anchor and pane-prefix position captured when one handle is pressed.
#[derive(Clone, Copy, Debug, PartialEq)]
struct SplitterDrag {
    handle: usize,
    pointer: f32,
    position: f32,
}

/// Grow the pane before the focused splitter handle.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SplitterIncrease;
/// Shrink the pane before the focused splitter handle.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SplitterDecrease;
/// Shrink the pane before the focused splitter handle to its minimum.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SplitterMinimum;
/// Grow the pane before the focused splitter handle to its maximum.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SplitterMaximum;
/// Collapse or restore the pane before the focused splitter handle.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SplitterCollapse;

/// Contextual bindings used by [`SplitterHandle::key_with`].
pub fn splitter_key_bindings() -> [KeyBinding; 7] {
    [
        KeyBinding::new("right", SplitterIncrease, Some(SPLITTER_KEY_CONTEXT)),
        KeyBinding::new("down", SplitterIncrease, Some(SPLITTER_KEY_CONTEXT)),
        KeyBinding::new("left", SplitterDecrease, Some(SPLITTER_KEY_CONTEXT)),
        KeyBinding::new("up", SplitterDecrease, Some(SPLITTER_KEY_CONTEXT)),
        KeyBinding::new("home", SplitterMinimum, Some(SPLITTER_KEY_CONTEXT)),
        KeyBinding::new("end", SplitterMaximum, Some(SPLITTER_KEY_CONTEXT)),
        KeyBinding::new("enter", SplitterCollapse, Some(SPLITTER_KEY_CONTEXT)),
    ]
}

/// Pane layout axis for one splitter.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SplitterOrientation {
    /// Panes sit side by side and each handle moves along the x axis.
    #[default]
    Horizontal,
    /// Panes stack and each handle moves along the y axis.
    Vertical,
}

impl SplitterOrientation {
    /// The accessibility orientation of the handle itself.
    ///
    /// A row of panes is divided by vertical handles, matching the WAI-ARIA window-splitter
    /// pattern where `aria-orientation` describes the separator rather than the pane axis.
    const fn handle_accessibility(self) -> AccessibilityOrientation {
        match self {
            Self::Horizontal => AccessibilityOrientation::Vertical,
            Self::Vertical => AccessibilityOrientation::Horizontal,
        }
    }

    const fn handle_cursor(self) -> CursorStyle {
        match self {
            Self::Horizontal => CursorStyle::ResizeColumn,
            Self::Vertical => CursorStyle::ResizeRow,
        }
    }
}

/// Controlled, allocation-free pane geometry for one splitter.
///
/// The application owns every visual declaration, including the handle's width and hit area.
/// QuickGUI owns the numeric contract: pane sizes always sum to the splitter's total, minimum
/// sizes are never violated by a drag or a key, and collapsing remembers the size to restore.
///
/// The state retains no allocation, task, timer, observer, or idle scheduler source.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SplitterState {
    orientation: SplitterOrientation,
    sizes: [f32; MAX_SPLITTER_PANES],
    minimums: [f32; MAX_SPLITTER_PANES],
    initial: [f32; MAX_SPLITTER_PANES],
    restore: [f32; MAX_SPLITTER_PANES],
    collapsible: [bool; MAX_SPLITTER_PANES],
    panes: usize,
    keyboard_step: f32,
    /// The handle position captured at pointer-down, retained until release or cancellation.
    drag: Option<SplitterDrag>,
}

impl SplitterState {
    /// Create a splitter from initial pane sizes in logical pixels.
    ///
    /// # Panics
    ///
    /// Panics with fewer than two panes or more than [`MAX_SPLITTER_PANES`].
    pub fn new(orientation: SplitterOrientation, sizes: &[f32]) -> Self {
        assert!(sizes.len() >= 2, "a splitter needs at least two panes");
        assert!(
            sizes.len() <= MAX_SPLITTER_PANES,
            "a splitter retains at most {MAX_SPLITTER_PANES} panes"
        );
        let mut state = Self {
            orientation,
            sizes: [0.0; MAX_SPLITTER_PANES],
            minimums: [0.0; MAX_SPLITTER_PANES],
            initial: [0.0; MAX_SPLITTER_PANES],
            restore: [0.0; MAX_SPLITTER_PANES],
            collapsible: [false; MAX_SPLITTER_PANES],
            panes: sizes.len(),
            keyboard_step: DEFAULT_KEYBOARD_STEP,
            drag: None,
        };
        for (index, size) in sizes.iter().enumerate() {
            state.sizes[index] = finite_nonnegative(*size);
            state.initial[index] = state.sizes[index];
            state.restore[index] = state.sizes[index];
        }
        state
    }

    /// Give one pane a minimum size in logical pixels.
    ///
    /// Minimums are clamped so their total can never exceed the splitter's own total.
    #[must_use]
    pub fn min_size(mut self, index: usize, minimum: f32) -> Self {
        if index < self.panes {
            self.minimums[index] = finite_nonnegative(minimum);
            self.enforce_minimums();
        }
        self
    }

    /// Allow one pane to collapse to zero from its handle.
    #[must_use]
    pub fn collapsible(mut self, index: usize, collapsible: bool) -> Self {
        if index < self.panes {
            self.collapsible[index] = collapsible;
        }
        self
    }

    /// Replace the logical pixels moved by one arrow keypress. The default is 16.
    #[must_use]
    pub fn keyboard_step(mut self, step: f32) -> Self {
        let step = finite_nonnegative(step);
        self.keyboard_step = if step > 0.0 {
            step
        } else {
            DEFAULT_KEYBOARD_STEP
        };
        self
    }

    pub const fn axis(&self) -> SplitterOrientation {
        self.orientation
    }

    pub const fn pane_count(&self) -> usize {
        self.panes
    }

    /// The number of movable handles, which is one fewer than the pane count.
    pub const fn handle_count(&self) -> usize {
        self.panes - 1
    }

    pub fn sizes(&self) -> &[f32] {
        &self.sizes[..self.panes]
    }

    pub fn size(&self, index: usize) -> Option<f32> {
        (index < self.panes).then(|| self.sizes[index])
    }

    pub fn minimum(&self, index: usize) -> Option<f32> {
        (index < self.panes).then(|| self.minimums[index])
    }

    pub fn is_collapsible(&self, index: usize) -> bool {
        index < self.panes && self.collapsible[index]
    }

    /// Whether one pane is currently collapsed to zero.
    pub fn is_collapsed(&self, index: usize) -> bool {
        index < self.panes && self.sizes[index] <= 0.0
    }

    /// The total size of every pane along the split axis.
    pub fn total(&self) -> f32 {
        self.sizes[..self.panes].iter().sum()
    }

    pub const fn keyboard_step_value(&self) -> f32 {
        self.keyboard_step
    }

    /// Whether a captured pointer is currently dragging one of this splitter's handles.
    ///
    /// [`Self::apply_pointer`] raises the flag on the press that starts a capture and lowers it
    /// on the release or cancel that ends it; keyboard resizing never sets it. A controlled owner
    /// that learns of sizes asynchronously uses it to tell a size the pointer is still moving
    /// from one the gesture settled on.
    pub const fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The inclusive bounds one handle can move its preceding pane between.
    pub fn handle_bounds(&self, handle: usize) -> Option<(f32, f32)> {
        if handle >= self.handle_count() {
            return None;
        }
        let span = self.sizes[handle] + self.sizes[handle + 1];
        let lower = if self.collapsible[handle] {
            0.0
        } else {
            self.minimums[handle]
        };
        let upper = (span
            - if self.collapsible[handle + 1] {
                0.0
            } else {
                self.minimums[handle + 1]
            })
        .max(lower);
        Some((lower.min(span), upper))
    }

    /// Move one handle by `delta` logical pixels, returning whether any size changed.
    ///
    /// Positive deltas grow the preceding pane. Movement stops at the minimum size of either
    /// neighbor, so a drag that runs past the limit is clamped instead of redistributing further.
    pub fn resize(&mut self, handle: usize, delta: f32) -> bool {
        let Some(current) = self.size(handle) else {
            return false;
        };
        let delta = if delta.is_finite() { delta } else { 0.0 };
        self.set_handle(handle, current + delta)
    }

    /// Place one handle so its preceding pane has exactly `size` logical pixels.
    pub fn set_handle(&mut self, handle: usize, size: f32) -> bool {
        let Some((lower, upper)) = self.handle_bounds(handle) else {
            return false;
        };
        let size = if size.is_finite() { size } else { lower };
        let size = size.clamp(lower, upper);
        if self.sizes[handle] == size {
            return false;
        }
        let span = self.sizes[handle] + self.sizes[handle + 1];
        if self.sizes[handle] > 0.0 {
            self.restore[handle] = self.sizes[handle];
        }
        self.sizes[handle] = size;
        self.sizes[handle + 1] = span - size;
        true
    }

    /// Apply one captured pointer event for a handle, anchored to its press position.
    ///
    /// Window coordinates stay stable while the handle itself moves. Keeping the handle's pane
    /// prefix from pointer-down means a layout normalization between events cannot accumulate as
    /// cursor drift, and the splitter still needs no container geometry. The event's phase also
    /// maintains [`Self::is_dragging`]. Returns whether any size changed.
    pub fn apply_pointer(&mut self, handle: usize, event: &PointerEvent) -> bool {
        if handle >= self.handle_count() {
            return false;
        }
        let axis = |point: crate::Point| match self.orientation {
            SplitterOrientation::Horizontal => point.x,
            SplitterOrientation::Vertical => point.y,
        };
        let pointer = axis(event.position);
        let position = || self.sizes[..=handle].iter().sum::<f32>();

        match event.phase {
            PointerPhase::Down => {
                self.drag = Some(SplitterDrag {
                    handle,
                    pointer,
                    position: position(),
                });
                false
            }
            PointerPhase::Move | PointerPhase::Up => {
                // A direct Move remains useful to callers constructing a captured stream by hand:
                // infer the preceding pointer position from its delta, then stay absolute from it.
                let drag = self
                    .drag
                    .filter(|drag| drag.handle == handle)
                    .unwrap_or_else(|| {
                        let delta = match self.orientation {
                            SplitterOrientation::Horizontal => event.delta.x,
                            SplitterOrientation::Vertical => event.delta.y,
                        };
                        SplitterDrag {
                            handle,
                            pointer: pointer - delta,
                            position: position(),
                        }
                    });
                let before = self.sizes[..handle].iter().sum::<f32>();
                let changed =
                    self.set_handle(handle, drag.position + (pointer - drag.pointer) - before);
                self.drag = (event.phase == PointerPhase::Move).then_some(drag);
                changed
            }
            PointerPhase::Cancel => {
                self.drag = None;
                false
            }
        }
    }

    /// Move one handle by the keyboard step, returning whether any size changed.
    pub fn step(&mut self, handle: usize, forward: bool) -> bool {
        let step = self.keyboard_step;
        self.resize(handle, if forward { step } else { -step })
    }

    /// Move one handle to the smallest size its preceding pane allows.
    pub fn to_minimum(&mut self, handle: usize) -> bool {
        let Some((lower, _)) = self.handle_bounds(handle) else {
            return false;
        };
        self.set_handle(handle, lower)
    }

    /// Move one handle to the largest size its preceding pane allows.
    pub fn to_maximum(&mut self, handle: usize) -> bool {
        let Some((_, upper)) = self.handle_bounds(handle) else {
            return false;
        };
        self.set_handle(handle, upper)
    }

    /// Collapse the pane before one handle, or restore it when already collapsed.
    ///
    /// Returns `false` for a pane that is not collapsible, so Enter stays inert instead of
    /// silently resizing.
    pub fn toggle_collapse(&mut self, handle: usize) -> bool {
        if handle >= self.handle_count() || !self.collapsible[handle] {
            return false;
        }
        if self.sizes[handle] > 0.0 {
            self.restore[handle] = self.sizes[handle];
            self.set_handle(handle, 0.0)
        } else {
            let restore = self.restore[handle].max(self.minimums[handle]);
            self.set_handle(handle, restore)
        }
    }

    /// Restore the sizes this splitter was created with, returning whether anything changed.
    ///
    /// Applications bind this to a handle double-click; QuickGUI does not assume that gesture.
    pub fn reset(&mut self) -> bool {
        let mut changed = false;
        for index in 0..self.panes {
            if self.sizes[index] != self.initial[index] {
                self.sizes[index] = self.initial[index];
                self.restore[index] = self.initial[index];
                changed = true;
            }
        }
        changed
    }

    /// Rescale every pane so the total matches a new container size.
    ///
    /// Sizes keep their proportions and then honor minimums. Call this from a
    /// [`crate::container_query`] when the surrounding layout changes.
    pub fn set_total(&mut self, total: f32) -> bool {
        let total = finite_nonnegative(total);
        let current = self.total();
        if current <= 0.0 || (current - total).abs() < f32::EPSILON {
            return false;
        }
        let scale = total / current;
        let mut changed = false;
        for index in 0..self.panes {
            let size = self.sizes[index] * scale;
            if self.sizes[index] != size {
                self.sizes[index] = size;
                changed = true;
            }
        }
        self.enforce_minimums();
        changed
    }

    fn enforce_minimums(&mut self) {
        let total = self.total();
        let required: f32 = self.minimums[..self.panes].iter().sum();
        if required <= 0.0 || required > total {
            return;
        }
        for index in 0..self.panes {
            let deficit = self.minimums[index] - self.sizes[index];
            if deficit <= 0.0 {
                continue;
            }
            self.sizes[index] = self.minimums[index];
            let mut remaining = deficit;
            for other in (0..self.panes).rev() {
                if other == index || remaining <= 0.0 {
                    continue;
                }
                let available = (self.sizes[other] - self.minimums[other]).max(0.0);
                let taken = available.min(remaining);
                self.sizes[other] -= taken;
                remaining -= taken;
            }
        }
    }
}

fn finite_nonnegative(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

/// A controlled, unstyled resizable-pane descriptor.
///
/// The application owns pane content, handle appearance, borders, and colors. QuickGUI supplies
/// stable identities, the pane sizes required for the behavior to exist, captured drag arithmetic,
/// typed keyboard resizing, and Splitter accessibility semantics with numeric value and bounds.
#[derive(Clone, Copy, Debug, PartialEq)]
#[must_use = "a Splitter descriptor has no effect until its parts are mounted"]
pub struct Splitter {
    root_id: ElementId,
    state: SplitterState,
}

impl Splitter {
    pub fn new(root_id: impl Into<ElementId>, state: &SplitterState) -> Self {
        Self {
            root_id: root_id.into(),
            state: *state,
        }
    }

    pub const fn root_id(self) -> ElementId {
        self.root_id
    }

    pub const fn state(self) -> SplitterState {
        self.state
    }

    pub fn pane_id(self, index: usize) -> ElementId {
        derived_splitter_id(self.root_id, SPLITTER_PANE_ID_TAG, index as u64)
    }

    pub fn handle_id(self, index: usize) -> ElementId {
        derived_splitter_id(self.root_id, SPLITTER_HANDLE_ID_TAG, index as u64)
    }

    /// Decorate an application-owned root without adding appearance.
    ///
    /// The root becomes a flex container on the split axis because pane order along that axis is
    /// the behavior itself. Colors, gaps, borders, and padding stay caller-owned.
    pub fn root_with(self, root: Element) -> Element {
        let root = root
            .id(self.root_id)
            .accessibility_role(AccessibilityRole::Group)
            .accessibility_orientation(match self.state.orientation {
                SplitterOrientation::Horizontal => AccessibilityOrientation::Horizontal,
                SplitterOrientation::Vertical => AccessibilityOrientation::Vertical,
            });
        match self.state.orientation {
            SplitterOrientation::Horizontal => root.flex_row(),
            SplitterOrientation::Vertical => root.flex_col(),
        }
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(self) -> Element {
        self.root_with(crate::div())
    }

    /// Describe one pane.
    pub fn pane(self, index: usize) -> Option<SplitterPane> {
        let size = self.state.size(index)?;
        Some(SplitterPane {
            splitter: self,
            index,
            size,
        })
    }

    /// Describe the handle that sits after pane `index`.
    pub fn handle(self, index: usize) -> Option<SplitterHandle> {
        let (lower, upper) = self.state.handle_bounds(index)?;
        Some(SplitterHandle {
            splitter: self,
            index,
            value: self.state.sizes[index],
            lower,
            upper,
        })
    }
}

/// A copyable declaration for one splitter pane.
#[derive(Clone, Copy, Debug, PartialEq)]
#[must_use = "a SplitterPane descriptor has no effect until its part is mounted"]
pub struct SplitterPane {
    splitter: Splitter,
    index: usize,
    size: f32,
}

impl SplitterPane {
    pub const fn index(self) -> usize {
        self.index
    }

    pub const fn size(self) -> f32 {
        self.size
    }

    pub fn is_collapsed(self) -> bool {
        self.splitter.state.is_collapsed(self.index)
    }

    pub fn pane_id(self) -> ElementId {
        self.splitter.pane_id(self.index)
    }

    /// Decorate an application-owned pane with its identity and current size.
    ///
    /// The size along the split axis is framework-owned structural geometry: without it the
    /// resize behavior would not exist. Every other layout and paint declaration is caller-owned.
    pub fn pane_with(self, pane: Element) -> Element {
        let pane = pane.id(self.pane_id()).flex_none().overflow_hidden();
        match self.splitter.state.orientation {
            SplitterOrientation::Horizontal => pane.w(self.size).min_w(0.0),
            SplitterOrientation::Vertical => pane.h(self.size).min_h(0.0),
        }
    }
    /// Create the unstyled pane part. Use [`Self::pane_with`] to supply an existing element.
    pub fn pane(self) -> Element {
        self.pane_with(crate::div())
    }
}

/// A copyable declaration for one splitter handle.
#[derive(Clone, Copy, Debug, PartialEq)]
#[must_use = "a SplitterHandle descriptor has no effect until its part is mounted"]
pub struct SplitterHandle {
    splitter: Splitter,
    index: usize,
    value: f32,
    lower: f32,
    upper: f32,
}

impl SplitterHandle {
    pub const fn index(self) -> usize {
        self.index
    }

    /// The current size of the pane this handle moves.
    pub const fn value(self) -> f32 {
        self.value
    }

    pub const fn bounds(self) -> (f32, f32) {
        (self.lower, self.upper)
    }

    /// Whether a captured pointer is currently dragging this splitter's handles.
    pub const fn is_dragging(self) -> bool {
        self.splitter.state.is_dragging()
    }

    pub fn is_collapsible(self) -> bool {
        self.splitter.state.is_collapsible(self.index)
    }

    pub fn handle_id(self) -> ElementId {
        self.splitter.handle_id(self.index)
    }

    /// Decorate an application-owned handle without adding appearance.
    ///
    /// The handle is a focusable Splitter with a numeric value, its bounds, the split axis, and a
    /// relationship to the pane it resizes. The caller owns its thickness, hit area, and paint;
    /// an explicit `.cursor(...)` overrides the default resize cursor.
    pub fn handle_with(self, handle: Element) -> Element {
        let cursor = handle.cursor_style_explicit;
        let handle = handle
            .id(self.handle_id())
            .accessibility_role(AccessibilityRole::SplitterHandle)
            .accessibility_orientation(self.splitter.state.orientation.handle_accessibility())
            .accessibility_value_range(AccessibilityValueRange::new(
                f64::from(self.value),
                f64::from(self.lower),
                f64::from(self.upper),
            ))
            .accessibility_controls(self.splitter.pane_id(self.index))
            .focusable()
            .tab_index(0)
            .key_context(SPLITTER_KEY_CONTEXT)
            .flex_none()
            .app_region_no_drag()
            .user_select_none();
        if cursor {
            handle
        } else {
            handle.cursor(self.splitter.state.orientation.handle_cursor())
        }
    }
    /// Create the unstyled handle part. Use [`Self::handle_with`] to supply an existing element.
    pub fn handle(self) -> Element {
        self.handle_with(crate::div())
    }

    /// Attach QuickGUI's typed splitter keyboard actions to this handle.
    ///
    /// Install [`splitter_key_bindings`] once on the application keymap.
    pub fn key_with<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        handle: Element,
        access: fn(&mut V) -> &mut SplitterState,
    ) -> Element {
        self.key_with_accessor(cx, handle, StateAccessor::from(access))
    }
    /// Create the unstyled key part. Use [`Self::key_with`] to supply an existing element.
    pub fn key<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut SplitterState,
    ) -> Element {
        self.key_with(cx, crate::div(), access)
    }

    /// Attach the typed splitter keyboard actions against a per-instance state accessor.
    ///
    /// A host that renders many declared splitters through one view passes an accessor that
    /// captures which [`SplitterState`] this handle belongs to.
    pub fn key_with_accessor<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        handle: Element,
        access_source: StateAccessor<V, SplitterState>,
    ) -> Element {
        let id = self.handle_id();
        let index = self.index;
        let access = access_source.clone();
        let increase = cx.action_listener(id, move |view, _: &SplitterIncrease, cx| {
            if access.get(view).step(index, true) {
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let decrease = cx.action_listener(id, move |view, _: &SplitterDecrease, cx| {
            if access.get(view).step(index, false) {
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let minimum = cx.action_listener(id, move |view, _: &SplitterMinimum, cx| {
            if access.get(view).to_minimum(index) {
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let maximum = cx.action_listener(id, move |view, _: &SplitterMaximum, cx| {
            if access.get(view).to_maximum(index) {
                cx.invalidate();
            }
        });
        let access = access_source;
        let collapse = cx.action_listener(id, move |view, _: &SplitterCollapse, cx| {
            if access.get(view).toggle_collapse(index) {
                cx.invalidate();
            }
        });
        handle
            .on_action(increase)
            .on_action(decrease)
            .on_action(minimum)
            .on_action(maximum)
            .on_action(collapse)
    }
}

fn derived_splitter_id(scope: ElementId, tag: u64, index: u64) -> ElementId {
    let mut hash = scope
        .as_u64()
        .rotate_left(17)
        .wrapping_add(index.rotate_right(5))
        ^ tag;
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    if hash == 0 || hash == u64::MAX || hash == scope.as_u64() {
        hash ^= tag.rotate_left(23);
    }
    ElementId::new(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppRegion, Application, Color, IntoElement, Modifiers, MouseButton, Point, PointerPhase,
        UserSelect, Vector, View, WindowOptions, div, text,
    };

    fn drag(delta: f32) -> PointerEvent {
        PointerEvent {
            size: crate::Size::ZERO,
            phase: PointerPhase::Move,
            position: Point::new(0.0, 0.0),
            origin: Point::new(0.0, 0.0),
            local_position: Point::new(0.0, 0.0),
            local_origin: Point::new(0.0, 0.0),
            delta: Vector::new(delta, delta),
            button: MouseButton::Left,
            modifiers: Modifiers::empty(),
        }
    }

    #[test]
    fn sizes_stay_conserved_bounded_and_reversible() {
        let mut state = SplitterState::new(SplitterOrientation::Horizontal, &[200.0, 300.0])
            .min_size(0, 120.0)
            .min_size(1, 80.0);
        assert_eq!(state.pane_count(), 2);
        assert_eq!(state.handle_count(), 1);
        assert_eq!(state.total(), 500.0);
        assert_eq!(state.handle_bounds(0), Some((120.0, 420.0)));
        assert_eq!(state.handle_bounds(1), None);

        assert!(state.resize(0, 60.0));
        assert_eq!(state.sizes(), &[260.0, 240.0]);
        assert_eq!(state.total(), 500.0);
        assert!(state.resize(0, -1_000.0));
        assert_eq!(state.sizes(), &[120.0, 380.0]);
        assert!(!state.resize(0, -10.0));
        assert!(state.resize(0, 5_000.0));
        assert_eq!(state.sizes(), &[420.0, 80.0]);
        assert!(!state.resize(0, f32::NAN));
        assert!(!state.resize(9, 10.0));

        assert!(state.to_minimum(0));
        assert_eq!(state.sizes(), &[120.0, 380.0]);
        assert!(state.to_maximum(0));
        assert_eq!(state.sizes(), &[420.0, 80.0]);
        assert!(state.reset());
        assert_eq!(state.sizes(), &[200.0, 300.0]);
        assert!(!state.reset());

        assert!(state.step(0, true));
        assert_eq!(state.sizes(), &[216.0, 284.0]);
        assert!(state.step(0, false));
        assert_eq!(state.sizes(), &[200.0, 300.0]);

        assert!(state.apply_pointer(0, &drag(24.0)));
        assert_eq!(state.sizes(), &[224.0, 276.0]);
        assert!(!state.apply_pointer(0, &drag(0.0)));

        // A pane that is not collapsible ignores Enter instead of resizing silently.
        assert!(!state.toggle_collapse(0));

        let mut collapsible = SplitterState::new(SplitterOrientation::Vertical, &[150.0, 250.0])
            .min_size(0, 100.0)
            .collapsible(0, true);
        assert_eq!(collapsible.handle_bounds(0), Some((0.0, 400.0)));
        assert!(collapsible.toggle_collapse(0));
        assert!(collapsible.is_collapsed(0));
        assert_eq!(collapsible.sizes(), &[0.0, 400.0]);
        assert!(collapsible.toggle_collapse(0));
        assert_eq!(collapsible.sizes(), &[150.0, 250.0]);
        assert!(!collapsible.is_collapsed(0));

        let mut three = SplitterState::new(SplitterOrientation::Horizontal, &[100.0, 100.0, 100.0])
            .min_size(1, 40.0);
        assert!(three.resize(0, 80.0));
        assert_eq!(three.sizes(), &[160.0, 40.0, 100.0]);
        assert_eq!(three.total(), 300.0);
        assert!(three.set_total(600.0));
        assert_eq!(three.sizes(), &[320.0, 80.0, 200.0]);
        assert!(!three.set_total(600.0));
        assert!(three.set_total(0.0));
        assert_eq!(three.total(), 0.0);
        assert!(!three.set_total(10.0));

        let stepped = SplitterState::new(SplitterOrientation::Horizontal, &[10.0, 10.0])
            .keyboard_step(f32::NAN);
        assert_eq!(stepped.keyboard_step_value(), 16.0);
        let negative = SplitterState::new(SplitterOrientation::Horizontal, &[-4.0, 10.0]);
        assert_eq!(negative.sizes(), &[0.0, 10.0]);
    }

    #[test]
    fn parts_add_exact_semantics_without_appearance() {
        let state = SplitterState::new(SplitterOrientation::Horizontal, &[200.0, 300.0])
            .min_size(0, 120.0)
            .min_size(1, 80.0);
        let splitter = Splitter::new("workspace", &state);
        let root = splitter.root_with(div().bg(Color::rgb8(1, 2, 3)));
        assert_eq!(root.explicit_id, Some("workspace".into()));
        assert_eq!(root.accessibility.role, AccessibilityRole::Group);
        assert_eq!(
            root.accessibility.orientation,
            Some(AccessibilityOrientation::Horizontal)
        );
        assert_eq!(root.visual.background, Some(Color::rgb8(1, 2, 3)));

        let pane = splitter.pane(0).expect("first pane");
        assert_eq!(pane.size(), 200.0);
        assert!(!pane.is_collapsed());
        let pane_element = pane.pane_with(div().bg(Color::rgb8(4, 5, 6)));
        assert_eq!(pane_element.explicit_id, Some(splitter.pane_id(0)));
        assert_eq!(pane_element.visual.background, Some(Color::rgb8(4, 5, 6)));
        assert!(splitter.pane(2).is_none());

        let handle = splitter.handle(0).expect("first handle");
        assert_eq!(handle.value(), 200.0);
        assert_eq!(handle.bounds(), (120.0, 420.0));
        assert!(!handle.is_collapsible());
        let handle_element = handle.handle_with(div().w(6.0).bg(Color::rgb8(7, 8, 9)));
        assert_eq!(handle_element.explicit_id, Some(splitter.handle_id(0)));
        assert_eq!(
            handle_element.accessibility.role,
            AccessibilityRole::SplitterHandle
        );
        assert_eq!(
            handle_element.accessibility.orientation,
            Some(AccessibilityOrientation::Vertical)
        );
        assert_eq!(
            handle_element.accessibility.value_range.as_deref(),
            Some(&AccessibilityValueRange::new(200.0, 120.0, 420.0))
        );
        assert_eq!(
            handle_element.accessibility.relations.controls(),
            Some(splitter.pane_id(0))
        );
        assert!(handle_element.focusable);
        assert_eq!(handle_element.tab_index, 0);
        assert_eq!(handle_element.cursor_style, Some(CursorStyle::ResizeColumn));
        assert_eq!(handle_element.app_region, Some(AppRegion::NoDrag));
        assert_eq!(handle_element.user_select, UserSelect::None);
        assert_eq!(handle_element.visual.background, Some(Color::rgb8(7, 8, 9)));
        assert!(splitter.handle(1).is_none());

        let overridden = handle.handle_with(div().cursor(CursorStyle::PointingHand));
        assert_eq!(overridden.cursor_style, Some(CursorStyle::PointingHand));

        let vertical_state = SplitterState::new(SplitterOrientation::Vertical, &[100.0, 100.0]);
        let vertical = Splitter::new("stack", &vertical_state);
        let vertical_handle = vertical
            .handle(0)
            .expect("vertical handle")
            .handle_with(div());
        assert_eq!(
            vertical_handle.accessibility.orientation,
            Some(AccessibilityOrientation::Horizontal)
        );
        assert_eq!(vertical_handle.cursor_style, Some(CursorStyle::ResizeRow));

        let ids = [
            splitter.root_id(),
            splitter.pane_id(0),
            splitter.pane_id(1),
            splitter.handle_id(0),
        ];
        for (index, id) in ids.iter().enumerate() {
            assert!(!ids[..index].contains(id));
        }
    }

    struct SplitterView {
        panes: SplitterState,
    }

    impl Default for SplitterView {
        fn default() -> Self {
            Self {
                panes: SplitterState::new(SplitterOrientation::Horizontal, &[200.0, 300.0])
                    .min_size(0, 120.0)
                    .min_size(1, 80.0)
                    .collapsible(0, true),
            }
        }
    }

    impl SplitterView {
        fn panes(view: &mut Self) -> &mut SplitterState {
            &mut view.panes
        }
    }

    impl View for SplitterView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let splitter = Splitter::new("workspace", &self.panes);
            let sidebar = splitter.pane(0).expect("sidebar");
            let content = splitter.pane(1).expect("content");
            let handle = splitter.handle(0).expect("handle");
            let drag = cx.pointer_listener(handle.handle_id(), |view, event, cx| {
                if view.panes.apply_pointer(0, event) {
                    cx.invalidate();
                }
            });
            splitter.root_with(
                div()
                    .child(sidebar.pane_with(div().child(text("Sidebar"))))
                    .child(handle.key_with(
                        cx,
                        handle.handle_with(div().w(6.0).on_pointer(drag)),
                        Self::panes,
                    ))
                    .child(content.pane_with(div().child(text("Content")))),
            )
        }
    }

    #[test]
    fn keyboard_and_accessibility_paths_stay_deterministic() {
        let (mut cx, view) = Application::new()
            .bind_keys(splitter_key_bindings())
            .into_test_context(WindowOptions::default(), SplitterView::default())
            .unwrap();
        let window = view.window_handle();
        let state = SplitterState::new(SplitterOrientation::Horizontal, &[200.0, 300.0]);
        let splitter = Splitter::new("workspace", &state);

        cx.simulate_keystrokes(window, "tab").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(splitter.handle_id(0)));
        cx.simulate_keystrokes(window, "right").unwrap();
        assert_eq!(
            cx.read(view, |view| view.panes.sizes().to_vec()).unwrap(),
            vec![216.0, 284.0]
        );
        cx.simulate_keystrokes(window, "left left").unwrap();
        assert_eq!(
            cx.read(view, |view| view.panes.sizes().to_vec()).unwrap(),
            vec![184.0, 316.0]
        );
        cx.simulate_keystrokes(window, "end").unwrap();
        assert_eq!(
            cx.read(view, |view| view.panes.sizes().to_vec()).unwrap(),
            vec![420.0, 80.0]
        );
        cx.simulate_keystrokes(window, "enter").unwrap();
        assert!(cx.read(view, |view| view.panes.is_collapsed(0)).unwrap());
        cx.simulate_keystrokes(window, "enter").unwrap();
        assert_eq!(
            cx.read(view, |view| view.panes.sizes().to_vec()).unwrap(),
            vec![420.0, 80.0]
        );
        cx.simulate_keystrokes(window, "home").unwrap();
        assert_eq!(
            cx.read(view, |view| view.panes.sizes().to_vec()).unwrap(),
            vec![0.0, 500.0]
        );

        let update = cx.accessibility_update(window).unwrap();
        let node = |id: ElementId| {
            update
                .nodes
                .iter()
                .find_map(|(node_id, node)| (node_id.0 == id.as_u64()).then_some(node))
                .expect("splitter accessibility node")
        };
        let handle = node(splitter.handle_id(0));
        assert_eq!(handle.role(), accesskit::Role::Splitter);
        assert_eq!(handle.numeric_value(), Some(0.0));
        assert_eq!(handle.min_numeric_value(), Some(0.0));
        assert_eq!(handle.max_numeric_value(), Some(420.0));
        assert_eq!(handle.orientation(), Some(accesskit::Orientation::Vertical));
        assert_eq!(
            handle.controls(),
            &[accesskit::NodeId(splitter.pane_id(0).as_u64())]
        );

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }

    #[test]
    fn bindings_are_contextual_and_complete() {
        let bindings = splitter_key_bindings();
        assert_eq!(bindings.len(), 7);
        assert!(
            bindings.iter().all(
                |binding| binding.context_predicate().is_some_and(|context| context
                    .depth_of(&[crate::KeyContext::parse(SPLITTER_KEY_CONTEXT).unwrap()])
                    .is_some())
            )
        );
    }

    #[test]
    fn a_captured_drag_is_reported_from_its_press_to_its_release() {
        let mut state = SplitterState::new(SplitterOrientation::Horizontal, &[200.0, 300.0]);
        let event = |phase, x, delta| PointerEvent {
            phase,
            position: Point::new(x, 0.0),
            delta: Vector::new(delta, 0.0),
            ..drag(0.0)
        };
        assert!(!state.is_dragging());
        assert!(!state.apply_pointer(0, &event(PointerPhase::Down, 203.0, 0.0)));
        assert!(state.is_dragging());
        assert!(state.apply_pointer(0, &event(PointerPhase::Move, 213.0, 10.0)));
        assert!(state.is_dragging());
        assert_eq!(state.sizes(), &[210.0, 290.0]);
        assert!(
            Splitter::new("workspace", &state)
                .handle(0)
                .expect("handle")
                .is_dragging()
        );
        assert!(!state.apply_pointer(0, &event(PointerPhase::Up, 213.0, 0.0)));
        assert!(!state.is_dragging());
        assert!(!state.apply_pointer(0, &event(PointerPhase::Down, 213.0, 0.0)));
        assert!(state.is_dragging());
        assert!(!state.apply_pointer(0, &event(PointerPhase::Cancel, 213.0, 0.0)));
        assert!(!state.is_dragging());
        // A handle that does not exist neither resizes nor starts a drag, and a key never does.
        assert!(!state.apply_pointer(1, &event(PointerPhase::Down, 213.0, 0.0)));
        assert!(!state.is_dragging());
        assert!(state.step(0, true));
        assert!(!state.is_dragging());
    }

    #[test]
    fn a_captured_drag_stays_anchored_when_layout_normalizes_the_panes() {
        let mut state = SplitterState::new(SplitterOrientation::Horizontal, &[100.0, 100.0, 100.0]);
        let event = |phase, x, delta| PointerEvent {
            phase,
            position: Point::new(x, 0.0),
            delta: Vector::new(delta, 0.0),
            ..drag(0.0)
        };

        // The second handle starts at pane-prefix position 200. A painted-bounds pass then
        // rescales every pane before the next pointer event, moving that prefix back to 180.
        assert!(!state.apply_pointer(1, &event(PointerPhase::Down, 203.0, 0.0)));
        assert!(state.set_total(270.0));
        assert_eq!(state.sizes(), &[90.0, 90.0, 90.0]);

        // The pointer travelled 20 pixels from its press. The active pane absorbs both that
        // travel and the preceding pane's layout shift, putting the handle prefix at 220 exactly.
        assert!(state.apply_pointer(1, &event(PointerPhase::Move, 223.0, 20.0)));
        assert_eq!(state.sizes(), &[90.0, 130.0, 50.0]);
        assert_eq!(state.sizes()[..=1].iter().sum::<f32>(), 220.0);
        assert!(!state.apply_pointer(1, &event(PointerPhase::Up, 223.0, 0.0)));
        assert!(!state.is_dragging());
    }

    #[test]
    #[should_panic(expected = "at least two panes")]
    fn one_pane_is_rejected() {
        let _ = SplitterState::new(SplitterOrientation::Horizontal, &[100.0]);
    }

    #[test]
    #[should_panic(expected = "at most")]
    fn too_many_panes_are_rejected() {
        let _ = SplitterState::new(
            SplitterOrientation::Horizontal,
            &[10.0; MAX_SPLITTER_PANES + 1],
        );
    }
}
