use crate::{
    AccessibilityOrientation, AccessibilityRole, AccessibilityValueRange, Element, ElementId,
    GesturePhase, Point, PointerEvent, PointerPhase, ScrollWheelEvent, Size, Vector, div,
};

/// Default line height used to turn a discrete wheel notch into logical pixels.
pub const DEFAULT_SCROLL_AREA_LINE_HEIGHT: f32 = 40.0;
/// Default distance from an edge that still counts as "at the edge".
pub const DEFAULT_SCROLL_AREA_OVERFLOW_THRESHOLD: f32 = 1.0;
/// Largest overflow edge threshold one scroll area may declare.
pub const MAX_SCROLL_AREA_OVERFLOW_THRESHOLD: f32 = 256.0;
/// Smallest thumb extent a scroll area reports, in logical pixels.
///
/// This matches the built-in overlay scrollbar's minimum so a caller-styled scrollbar cannot
/// collapse to an unclickable sliver over very long content.
pub const MIN_SCROLL_AREA_THUMB_LENGTH: f32 = 24.0;

const SCROLL_AREA_VIEWPORT_ID_TAG: u64 = 0x7c41_be0d_2a95_31f6;
const SCROLL_AREA_CONTENT_ID_TAG: u64 = 0x2f68_d914_57ab_c30e;
const SCROLL_AREA_SCROLLBAR_ID_TAG: u64 = 0xa03e_5b72_ce18_47d9;
const SCROLL_AREA_THUMB_ID_TAG: u64 = 0x54d9_812f_0b67_ae23;
const SCROLL_AREA_CORNER_ID_TAG: u64 = 0xe916_37c5_84da_2b70;

/// Which axis one scrollbar, thumb, or overflow query addresses.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ScrollAreaOrientation {
    #[default]
    Vertical,
    Horizontal,
}

impl ScrollAreaOrientation {
    const fn accessibility(self) -> AccessibilityOrientation {
        match self {
            Self::Vertical => AccessibilityOrientation::Vertical,
            Self::Horizontal => AccessibilityOrientation::Horizontal,
        }
    }

    const fn index(self) -> u64 {
        match self {
            Self::Vertical => 0,
            Self::Horizontal => 1,
        }
    }

    pub const fn is_horizontal(self) -> bool {
        matches!(self, Self::Horizontal)
    }
}

/// The `data-`-like render state one scroll area exposes to the application.
///
/// Every field is derived from the retained geometry, never observed or timed, so reading it costs
/// nothing and a settled scroll area produces no work at all. Applications style shadows, fades,
/// and scrollbar visibility from these flags exactly as a web application styles `data-` attributes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ScrollAreaStyleState {
    /// A pointer or wheel gesture is currently moving the viewport.
    pub scrolling: bool,
    /// The pointer is inside the scroll area's root.
    pub hovering: bool,
    /// The content is wider than the viewport.
    pub has_overflow_x: bool,
    /// The content is taller than the viewport.
    pub has_overflow_y: bool,
    /// Content is hidden past the inline start edge.
    pub overflow_x_start: bool,
    /// Content is hidden past the inline end edge.
    pub overflow_x_end: bool,
    /// Content is hidden above the top edge.
    pub overflow_y_start: bool,
    /// Content is hidden below the bottom edge.
    pub overflow_y_end: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ScrollAreaDrag {
    orientation: ScrollAreaOrientation,
    track_length: f32,
    /// The thumb's track origin when the drag started, in logical pixels along the axis.
    start: f32,
}

/// Controlled, allocation-free geometry and interaction state for one scroll area.
///
/// The application owns every visual declaration and declares the viewport and content extents it
/// laid out; QuickGUI owns the clamped offsets, the derived overflow flags, the thumb arithmetic,
/// and the captured pointer contract for thumb drags and track presses. The arithmetic mirrors the
/// built-in overlay scrollbar in `src/ui_tree/pointer.rs`: a wheel delta subtracts from the offset,
/// the thumb length is the viewport-to-content ratio with a minimum, and the thumb travels the
/// remaining track. It retains no allocation, task, timer, observer, or idle scheduler source.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollAreaState {
    viewport: Size,
    content: Size,
    offset: Vector,
    threshold: f32,
    line_height: f32,
    scrolling: bool,
    hovering: bool,
    drag: Option<ScrollAreaDrag>,
}

impl Default for ScrollAreaState {
    fn default() -> Self {
        Self::new()
    }
}

impl ScrollAreaState {
    pub const fn new() -> Self {
        Self {
            viewport: Size::new(0.0, 0.0),
            content: Size::new(0.0, 0.0),
            offset: Vector::ZERO,
            threshold: DEFAULT_SCROLL_AREA_OVERFLOW_THRESHOLD,
            line_height: DEFAULT_SCROLL_AREA_LINE_HEIGHT,
            scrolling: false,
            hovering: false,
            drag: None,
        }
    }

    /// Replace the distance from an edge that still counts as being at that edge.
    ///
    /// A non-finite or negative value falls back to the default; anything larger than
    /// [`MAX_SCROLL_AREA_OVERFLOW_THRESHOLD`] is clamped to it.
    #[must_use]
    pub fn overflow_edge_threshold(mut self, threshold: f32) -> Self {
        self.threshold = if threshold.is_finite() && threshold >= 0.0 {
            threshold.min(MAX_SCROLL_AREA_OVERFLOW_THRESHOLD)
        } else {
            DEFAULT_SCROLL_AREA_OVERFLOW_THRESHOLD
        };
        self
    }

    /// Replace the logical-pixel distance one discrete wheel line moves.
    #[must_use]
    pub fn line_height(mut self, line_height: f32) -> Self {
        self.line_height = if line_height.is_finite() && line_height > 0.0 {
            line_height.min(MAX_SCROLL_AREA_OVERFLOW_THRESHOLD * 16.0)
        } else {
            DEFAULT_SCROLL_AREA_LINE_HEIGHT
        };
        self
    }

    pub const fn viewport(&self) -> Size {
        self.viewport
    }

    pub const fn content(&self) -> Size {
        self.content
    }

    pub const fn offset(&self) -> Vector {
        self.offset
    }

    pub const fn edge_threshold(&self) -> f32 {
        self.threshold
    }

    pub const fn is_scrolling(&self) -> bool {
        self.scrolling
    }

    pub const fn is_hovering(&self) -> bool {
        self.hovering
    }

    /// Whether a captured thumb drag is in progress.
    pub const fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Report the viewport and content extents the application laid out.
    ///
    /// Returns whether anything changed, including a clamp of the retained offset caused by
    /// content shrinking.
    pub fn set_geometry(&mut self, viewport: Size, content: Size) -> bool {
        let viewport = finite_size(viewport);
        let content = finite_size(content);
        let mut changed = false;
        if self.viewport != viewport {
            self.viewport = viewport;
            changed = true;
        }
        if self.content != content {
            self.content = content;
            changed = true;
        }
        changed | self.clamp_offset()
    }

    /// The furthest the viewport may scroll on each axis.
    pub fn max_offset(&self) -> Vector {
        Vector::new(
            (self.content.width - self.viewport.width).max(0.0),
            (self.content.height - self.viewport.height).max(0.0),
        )
    }

    /// Replace the retained offset, clamped to the scrollable range.
    pub fn set_offset(&mut self, offset: Vector) -> bool {
        let max = self.max_offset();
        let next = Vector::new(
            finite_or_zero(offset.x).clamp(0.0, max.x),
            finite_or_zero(offset.y).clamp(0.0, max.y),
        );
        if self.offset == next {
            return false;
        }
        self.offset = next;
        true
    }

    /// Move the retained offset by `delta`, clamped to the scrollable range.
    pub fn scroll_by(&mut self, delta: Vector) -> bool {
        self.set_offset(Vector::new(
            self.offset.x + finite_or_zero(delta.x),
            self.offset.y + finite_or_zero(delta.y),
        ))
    }

    pub fn set_scrolling(&mut self, scrolling: bool) -> bool {
        if self.scrolling == scrolling {
            return false;
        }
        self.scrolling = scrolling;
        true
    }

    pub fn set_hovering(&mut self, hovering: bool) -> bool {
        if self.hovering == hovering {
            return false;
        }
        self.hovering = hovering;
        true
    }

    pub fn has_overflow_x(&self) -> bool {
        self.max_offset().x > 0.0
    }

    pub fn has_overflow_y(&self) -> bool {
        self.max_offset().y > 0.0
    }

    pub fn has_overflow(&self, orientation: ScrollAreaOrientation) -> bool {
        if orientation.is_horizontal() {
            self.has_overflow_x()
        } else {
            self.has_overflow_y()
        }
    }

    /// Whether content is hidden past the inline start edge.
    pub fn overflow_x_start(&self) -> bool {
        self.has_overflow_x() && self.offset.x > self.threshold
    }

    /// Whether content is hidden past the inline end edge.
    pub fn overflow_x_end(&self) -> bool {
        self.has_overflow_x() && self.offset.x < self.max_offset().x - self.threshold
    }

    /// Whether content is hidden above the top edge.
    pub fn overflow_y_start(&self) -> bool {
        self.has_overflow_y() && self.offset.y > self.threshold
    }

    /// Whether content is hidden below the bottom edge.
    pub fn overflow_y_end(&self) -> bool {
        self.has_overflow_y() && self.offset.y < self.max_offset().y - self.threshold
    }

    /// The complete `data-`-like render state, in one copyable snapshot.
    pub fn style_state(&self) -> ScrollAreaStyleState {
        ScrollAreaStyleState {
            scrolling: self.scrolling,
            hovering: self.hovering,
            has_overflow_x: self.has_overflow_x(),
            has_overflow_y: self.has_overflow_y(),
            overflow_x_start: self.overflow_x_start(),
            overflow_x_end: self.overflow_x_end(),
            overflow_y_start: self.overflow_y_start(),
            overflow_y_end: self.overflow_y_end(),
        }
    }

    /// The `0.0..=1.0` share of the content one axis of the viewport shows.
    pub fn visible_fraction(&self, orientation: ScrollAreaOrientation) -> f32 {
        let (viewport, content) = self.axis_extents(orientation);
        if content <= 0.0 || viewport <= 0.0 {
            return 1.0;
        }
        (viewport / content).clamp(0.0, 1.0)
    }

    /// The `0.0..=1.0` position of one axis between its minimum and maximum offset.
    pub fn scroll_fraction(&self, orientation: ScrollAreaOrientation) -> f32 {
        let max = if orientation.is_horizontal() {
            self.max_offset().x
        } else {
            self.max_offset().y
        };
        if max <= 0.0 {
            return 0.0;
        }
        let offset = if orientation.is_horizontal() {
            self.offset.x
        } else {
            self.offset.y
        };
        (offset / max).clamp(0.0, 1.0)
    }

    /// The thumb's extent along a track of `track_length` logical pixels.
    pub fn thumb_length(&self, orientation: ScrollAreaOrientation, track_length: f32) -> f32 {
        let track_length = finite_or_zero(track_length).max(0.0);
        if track_length <= 0.0 {
            return 0.0;
        }
        (track_length * self.visible_fraction(orientation))
            .max(MIN_SCROLL_AREA_THUMB_LENGTH.min(track_length))
            .min(track_length)
    }

    /// The thumb's offset from the track's start, in logical pixels.
    pub fn thumb_offset(&self, orientation: ScrollAreaOrientation, track_length: f32) -> f32 {
        let track_length = finite_or_zero(track_length).max(0.0);
        let travel = (track_length - self.thumb_length(orientation, track_length)).max(0.0);
        travel * self.scroll_fraction(orientation)
    }

    /// Apply one wheel or trackpad event, mirroring the built-in retained scrolling arithmetic.
    ///
    /// Returns whether the retained offset or the scrolling flag changed.
    pub fn apply_scroll_wheel(&mut self, event: &ScrollWheelEvent) -> bool {
        let delta = event.delta.pixel_delta(self.line_height);
        let mut changed = self.scroll_by(Vector::new(-delta.x, -delta.y));
        changed |= match event.phase {
            GesturePhase::Started | GesturePhase::Moved => self.set_scrolling(true),
            GesturePhase::Ended | GesturePhase::Cancelled => self.set_scrolling(false),
        };
        changed
    }

    /// Apply one captured pointer event on a caller-owned scrollbar thumb.
    ///
    /// A press records where inside the thumb the pointer grabbed it, so the thumb does not jump
    /// under the cursor; subsequent moves drag it even outside the track, exactly like the
    /// built-in overlay scrollbar. Returns whether anything changed.
    pub fn apply_thumb_pointer(
        &mut self,
        event: &PointerEvent,
        orientation: ScrollAreaOrientation,
        track_length: f32,
    ) -> bool {
        match event.phase {
            PointerPhase::Down => {
                let track_length = finite_or_zero(track_length).max(0.0);
                self.drag = Some(ScrollAreaDrag {
                    orientation,
                    track_length,
                    start: self.thumb_offset(orientation, track_length),
                });
                self.set_scrolling(true);
                true
            }
            PointerPhase::Move => {
                let Some(drag) = self.drag.filter(|drag| drag.orientation == orientation) else {
                    return false;
                };
                // The thumb itself moves as the offset changes, so its element-local position is
                // useless during a drag. Window coordinates are stable: the thumb origin is where
                // it started plus the pointer's travel since the capture began.
                let travel = self.axis_window(event.position, orientation)
                    - self.axis_window(event.origin, orientation);
                self.set_thumb_origin(orientation, drag.track_length, drag.start + travel)
            }
            PointerPhase::Up | PointerPhase::Cancel => {
                let dragging = self.drag.take().is_some();
                self.set_scrolling(false) | dragging
            }
        }
    }

    /// Apply one pointer press on a caller-owned scrollbar track.
    ///
    /// A press outside the thumb centres the thumb on the pressed position, matching the macOS
    /// "jump to the spot that's clicked" behavior the built-in scrollbar uses.
    pub fn apply_track_pointer(
        &mut self,
        event: &PointerEvent,
        orientation: ScrollAreaOrientation,
        track_length: f32,
    ) -> bool {
        if event.phase != PointerPhase::Down {
            return false;
        }
        let track_length = finite_or_zero(track_length).max(0.0);
        let position = self.axis_position(event, orientation);
        let thumb = self.thumb_length(orientation, track_length);
        self.set_thumb_origin(orientation, track_length, position - thumb * 0.5)
    }

    fn set_thumb_origin(
        &mut self,
        orientation: ScrollAreaOrientation,
        track_length: f32,
        origin: f32,
    ) -> bool {
        let travel = (track_length - self.thumb_length(orientation, track_length)).max(0.0);
        if travel <= 0.0 {
            return false;
        }
        let fraction = (finite_or_zero(origin) / travel).clamp(0.0, 1.0);
        let max = self.max_offset();
        if orientation.is_horizontal() {
            self.set_offset(Vector::new(max.x * fraction, self.offset.y))
        } else {
            self.set_offset(Vector::new(self.offset.x, max.y * fraction))
        }
    }

    fn axis_position(&self, event: &PointerEvent, orientation: ScrollAreaOrientation) -> f32 {
        if orientation.is_horizontal() {
            event.local_position.x
        } else {
            event.local_position.y
        }
    }

    fn axis_window(&self, point: Point, orientation: ScrollAreaOrientation) -> f32 {
        if orientation.is_horizontal() {
            point.x
        } else {
            point.y
        }
    }

    fn axis_extents(&self, orientation: ScrollAreaOrientation) -> (f32, f32) {
        if orientation.is_horizontal() {
            (self.viewport.width, self.content.width)
        } else {
            (self.viewport.height, self.content.height)
        }
    }

    fn clamp_offset(&mut self) -> bool {
        let offset = self.offset;
        self.set_offset(offset)
    }
}

fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

fn finite_size(size: Size) -> Size {
    Size::new(
        finite_or_zero(size.width).max(0.0),
        finite_or_zero(size.height).max(0.0),
    )
}

/// A controlled, unstyled scroll-area descriptor.
///
/// The application owns the viewport's box, the content, every scrollbar's width, color, radius,
/// and visibility policy, and the transform that places the content. QuickGUI supplies stable part
/// identities, the ScrollView and ScrollBar roles with exact numeric positions, the clipped
/// viewport, and the pointer arithmetic in [`ScrollAreaState`].
///
/// This is the caller-styled alternative to QuickGUI's built-in native-style overlay scrollbars,
/// which stay available on any ordinary `overflow_y_scroll` container. A scroll area's viewport is
/// clipped rather than natively scrolled, so the two never fight over the same wheel event.
///
/// The descriptor retains no allocation, task, timer, observer, or idle scheduler source.
#[derive(Clone, Copy, Debug, PartialEq)]
#[must_use = "a ScrollArea descriptor has no effect until its parts are mounted"]
pub struct ScrollArea {
    root_id: ElementId,
    keep_mounted: bool,
}

impl ScrollArea {
    pub fn new(root_id: impl Into<ElementId>) -> Self {
        Self {
            root_id: root_id.into(),
            keep_mounted: false,
        }
    }

    /// Keep a scrollbar mounted while its axis does not overflow.
    ///
    /// The default unmounts it, matching Base UI's `keepMounted={false}`. A kept scrollbar is
    /// hidden from assistive technology while its axis cannot scroll, so a mounted-but-useless
    /// scrollbar is never announced.
    pub const fn keep_mounted(mut self, keep_mounted: bool) -> Self {
        self.keep_mounted = keep_mounted;
        self
    }

    pub const fn is_keep_mounted(self) -> bool {
        self.keep_mounted
    }

    pub const fn root_id(self) -> ElementId {
        self.root_id
    }

    pub fn viewport_id(self) -> ElementId {
        derived_scroll_area_id(self.root_id, SCROLL_AREA_VIEWPORT_ID_TAG, 0)
    }

    pub fn content_id(self) -> ElementId {
        derived_scroll_area_id(self.root_id, SCROLL_AREA_CONTENT_ID_TAG, 0)
    }

    pub fn scrollbar_id(self, orientation: ScrollAreaOrientation) -> ElementId {
        derived_scroll_area_id(
            self.root_id,
            SCROLL_AREA_SCROLLBAR_ID_TAG,
            orientation.index(),
        )
    }

    pub fn thumb_id(self, orientation: ScrollAreaOrientation) -> ElementId {
        derived_scroll_area_id(self.root_id, SCROLL_AREA_THUMB_ID_TAG, orientation.index())
    }

    pub fn corner_id(self) -> ElementId {
        derived_scroll_area_id(self.root_id, SCROLL_AREA_CORNER_ID_TAG, 0)
    }

    /// Whether a scrollbar for `orientation` should be mounted at all.
    pub fn shows_scrollbar(
        self,
        state: &ScrollAreaState,
        orientation: ScrollAreaOrientation,
    ) -> bool {
        self.keep_mounted || state.has_overflow(orientation)
    }

    /// Whether the corner between two scrollbars should be mounted.
    pub fn shows_corner(self, state: &ScrollAreaState) -> bool {
        self.keep_mounted || (state.has_overflow_x() && state.has_overflow_y())
    }

    /// Decorate an application-owned root without adding layout or appearance.
    pub fn root_with(self, root: Element) -> Element {
        root.id(self.root_id)
            .relative()
            .app_region_no_drag()
            .cursor_default()
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(self) -> Element {
        self.root_with(crate::div())
    }

    /// Decorate the application-owned scroll container.
    ///
    /// The viewport clips its content on both axes and carries the ScrollView role. The caller
    /// places the content by translating [`Self::content_with`] with the retained offset, which
    /// keeps scrolling paint-only and keeps QuickGUI's built-in overlay scrollbar out of a
    /// component whose scrollbars the application draws itself.
    pub fn viewport_with(self, viewport: Element) -> Element {
        viewport
            .id(self.viewport_id())
            .overflow_hidden()
            .accessibility_role(AccessibilityRole::ScrollView)
            .app_region_no_drag()
            .cursor_default()
    }
    /// Create the unstyled viewport part. Use [`Self::viewport_with`] to supply an existing element.
    pub fn viewport(self) -> Element {
        self.viewport_with(crate::div())
    }

    /// Decorate the application-owned scrolled content.
    ///
    /// Translate it by the negated [`ScrollAreaState::offset`]; QuickGUI adds no transform of its
    /// own so the application keeps full control of the motion.
    pub fn content_with(self, content: Element) -> Element {
        content.id(self.content_id())
    }
    /// Create the unstyled content part. Use [`Self::content_with`] to supply an existing element.
    pub fn content(self) -> Element {
        self.content_with(crate::div())
    }

    /// Decorate one application-owned scrollbar track.
    ///
    /// The track projects the ScrollBar role with the exact current position, the scrollable
    /// range, and its orientation. It is not focusable: a scrollbar duplicates keyboard scrolling
    /// the viewport already provides, so it stays out of the Tab sequence.
    pub fn scrollbar_with(
        self,
        state: &ScrollAreaState,
        orientation: ScrollAreaOrientation,
        scrollbar: Element,
    ) -> Element {
        let has_overflow = state.has_overflow(orientation);
        let (offset, max) = if orientation.is_horizontal() {
            (state.offset().x, state.max_offset().x)
        } else {
            (state.offset().y, state.max_offset().y)
        };
        scrollbar
            .id(self.scrollbar_id(orientation))
            .relative()
            .accessibility_role(AccessibilityRole::ScrollBar)
            .accessibility_orientation(orientation.accessibility())
            .accessibility_value_range(AccessibilityValueRange::new(
                f64::from(offset),
                0.0,
                f64::from(max),
            ))
            .accessibility_controls(self.viewport_id())
            .accessibility_hidden(!has_overflow)
            .app_region_no_drag()
            .cursor_default()
            .user_select_none()
    }
    /// Create the unstyled scrollbar part. Use [`Self::scrollbar_with`] to supply an existing element.
    pub fn scrollbar(self, state: &ScrollAreaState, orientation: ScrollAreaOrientation) -> Element {
        self.scrollbar_with(state, orientation, crate::div())
    }

    /// Decorate one application-owned scrollbar thumb.
    ///
    /// The thumb carries the captured pointer drag. Attach a
    /// [`crate::ViewContext::pointer_listener`] registered for [`Self::thumb_id`] and forward the
    /// event to [`ScrollAreaState::apply_thumb_pointer`] with the track length the caller laid out.
    pub fn thumb_with(self, orientation: ScrollAreaOrientation, thumb: Element) -> Element {
        thumb
            .id(self.thumb_id(orientation))
            .accessibility_hidden(true)
            .app_region_no_drag()
            .cursor_default()
            .user_select_none()
    }
    /// Create the unstyled thumb part. Use [`Self::thumb_with`] to supply an existing element.
    pub fn thumb(self, orientation: ScrollAreaOrientation) -> Element {
        self.thumb_with(orientation, crate::div())
    }

    /// Decorate and position one application-owned scrollbar thumb inside its scrollbar.
    ///
    /// This is [`Self::thumb_with`] plus the structural geometry a draggable thumb needs: the
    /// thumb is absolutely positioned along the scrollbar's axis at
    /// [`ScrollAreaState::thumb_offset`] with [`ScrollAreaState::thumb_length`], so it follows
    /// the offset without the application re-deriving either value. Cross-axis size and every
    /// visual property stay application-owned.
    pub fn positioned_thumb_with(
        self,
        state: &ScrollAreaState,
        orientation: ScrollAreaOrientation,
        track_length: f32,
        thumb: Element,
    ) -> Element {
        let offset = state.thumb_offset(orientation, track_length);
        let length = state.thumb_length(orientation, track_length);
        let thumb = self.thumb_with(orientation, thumb).absolute();
        if orientation.is_horizontal() {
            thumb.left(offset).top(0.0).w(length)
        } else {
            thumb.top(offset).left(0.0).h(length)
        }
    }
    /// Create the unstyled positioned thumb part. Use [`Self::positioned_thumb_with`] to supply an existing element.
    pub fn positioned_thumb(
        self,
        state: &ScrollAreaState,
        orientation: ScrollAreaOrientation,
        track_length: f32,
    ) -> Element {
        self.positioned_thumb_with(state, orientation, track_length, crate::div())
    }

    /// Decorate the application-owned corner between a horizontal and a vertical scrollbar.
    pub fn corner_with(self, corner: Element) -> Element {
        corner
            .id(self.corner_id())
            .accessibility_hidden(true)
            .app_region_no_drag()
            .cursor_default()
    }
    /// Create the unstyled corner part. Use [`Self::corner_with`] to supply an existing element.
    pub fn corner(self) -> Element {
        self.corner_with(crate::div())
    }
}

/// Create an unstyled scroll-area viewport.
///
/// This shorthand is equivalent to `ScrollArea::new(id).viewport_with(div())`.
pub fn scroll_area_viewport(id: impl Into<ElementId>) -> Element {
    ScrollArea::new(id).viewport_with(div())
}

fn derived_scroll_area_id(scope: ElementId, tag: u64, index: u64) -> ElementId {
    let mut hash = scope
        .as_u64()
        .rotate_left(17)
        .wrapping_add(index.rotate_right(29))
        ^ tag;
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    if hash == 0 || hash == u64::MAX || hash == scope.as_u64() {
        hash ^= tag.rotate_left(13);
    }
    ElementId::new(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Color, IntoElement, Modifiers, MouseButton, Point, ScrollDelta, TestAppContext, View,
        ViewContext, text,
    };

    fn pointer_event(phase: PointerPhase, x: f32, y: f32) -> PointerEvent {
        PointerEvent {
            size: Size::new(12.0, 200.0),
            phase,
            position: Point::new(x, y),
            origin: Point::new(x, y),
            local_position: Point::new(x, y),
            local_origin: Point::new(x, y),
            delta: Vector::ZERO,
            button: MouseButton::Left,
            modifiers: Modifiers::empty(),
        }
    }

    /// A captured event whose window `origin` stays at the press point while the pointer moves.
    fn drag_event(phase: PointerPhase, origin_y: f32, y: f32) -> PointerEvent {
        let mut event = pointer_event(phase, 6.0, y);
        event.origin = Point::new(6.0, origin_y);
        event.local_origin = Point::new(6.0, origin_y);
        event
    }

    fn wheel(delta: Vector, phase: GesturePhase) -> ScrollWheelEvent {
        ScrollWheelEvent {
            position: Point::new(10.0, 10.0),
            delta: ScrollDelta::Pixels(delta),
            phase,
            modifiers: Modifiers::empty(),
        }
    }

    #[test]
    fn geometry_derives_overflow_flags_and_thumb_arithmetic() {
        let mut state = ScrollAreaState::new();
        assert_eq!(state.max_offset(), Vector::ZERO);
        assert!(!state.has_overflow_x());
        assert!(!state.has_overflow_y());
        assert_eq!(state.visible_fraction(ScrollAreaOrientation::Vertical), 1.0);

        assert!(state.set_geometry(Size::new(200.0, 100.0), Size::new(400.0, 500.0)));
        assert!(!state.set_geometry(Size::new(200.0, 100.0), Size::new(400.0, 500.0)));
        assert_eq!(state.max_offset(), Vector::new(200.0, 400.0));
        assert!(state.has_overflow_x());
        assert!(state.has_overflow_y());
        assert!(!state.overflow_y_start());
        assert!(state.overflow_y_end());

        assert!(state.scroll_by(Vector::new(0.0, 400.0)));
        assert_eq!(state.offset(), Vector::new(0.0, 400.0));
        assert!(state.overflow_y_start());
        assert!(!state.overflow_y_end());
        assert!(!state.scroll_by(Vector::new(0.0, 100.0)), "already clamped");

        assert_eq!(state.visible_fraction(ScrollAreaOrientation::Vertical), 0.2);
        assert_eq!(state.scroll_fraction(ScrollAreaOrientation::Vertical), 1.0);
        assert_eq!(
            state.thumb_length(ScrollAreaOrientation::Vertical, 100.0),
            24.0,
            "the minimum thumb keeps a long document's thumb usable"
        );
        assert_eq!(
            state.thumb_offset(ScrollAreaOrientation::Vertical, 100.0),
            76.0
        );
        assert_eq!(
            state.thumb_length(ScrollAreaOrientation::Vertical, 0.0),
            0.0
        );
        assert_eq!(
            state.thumb_length(ScrollAreaOrientation::Vertical, 400.0),
            80.0
        );

        let snapshot = state.style_state();
        assert!(snapshot.has_overflow_x && snapshot.has_overflow_y);
        assert!(snapshot.overflow_y_start && !snapshot.overflow_y_end);
        assert!(!snapshot.scrolling && !snapshot.hovering);
        assert!(state.set_hovering(true));
        assert!(!state.set_hovering(true));
        assert!(state.style_state().hovering);

        // Shrinking the content clamps the retained offset instead of leaving a stale gap.
        assert!(state.set_geometry(Size::new(200.0, 100.0), Size::new(400.0, 150.0)));
        assert_eq!(state.offset().y, 50.0);

        let degenerate = ScrollAreaState::new()
            .overflow_edge_threshold(f32::NAN)
            .line_height(f32::INFINITY);
        assert_eq!(
            degenerate.edge_threshold(),
            DEFAULT_SCROLL_AREA_OVERFLOW_THRESHOLD
        );
        let clamped = ScrollAreaState::new().overflow_edge_threshold(10_000.0);
        assert_eq!(clamped.edge_threshold(), MAX_SCROLL_AREA_OVERFLOW_THRESHOLD);
        assert_eq!(ScrollAreaState::default(), ScrollAreaState::new());
    }

    #[test]
    fn wheel_thumb_and_track_pointers_move_the_viewport() {
        let mut state = ScrollAreaState::new();
        state.set_geometry(Size::new(200.0, 200.0), Size::new(200.0, 1_000.0));

        // A wheel delta subtracts from the offset exactly like retained scrolling.
        assert!(state.apply_scroll_wheel(&wheel(Vector::new(0.0, -120.0), GesturePhase::Started)));
        assert_eq!(state.offset().y, 120.0);
        assert!(state.is_scrolling());
        assert!(state.apply_scroll_wheel(&wheel(Vector::ZERO, GesturePhase::Ended)));
        assert!(!state.is_scrolling());

        // A thumb drag grabs the thumb where it was pressed and never jumps.
        state.set_offset(Vector::ZERO);
        let track = 200.0;
        let thumb = state.thumb_length(ScrollAreaOrientation::Vertical, track);
        assert_eq!(thumb, 40.0);
        assert!(state.apply_thumb_pointer(
            &pointer_event(PointerPhase::Down, 6.0, 10.0),
            ScrollAreaOrientation::Vertical,
            track,
        ));
        assert!(state.is_dragging());
        assert_eq!(state.offset().y, 0.0, "pressing the thumb moves nothing");

        assert!(state.apply_thumb_pointer(
            &drag_event(PointerPhase::Move, 10.0, 90.0),
            ScrollAreaOrientation::Vertical,
            track,
        ));
        assert_eq!(state.offset().y, 400.0);

        // Capture continues past the track and stays clamped.
        assert!(state.apply_thumb_pointer(
            &drag_event(PointerPhase::Move, 10.0, 4_000.0),
            ScrollAreaOrientation::Vertical,
            track,
        ));
        assert_eq!(state.offset().y, 800.0);
        assert!(state.apply_thumb_pointer(
            &drag_event(PointerPhase::Up, 10.0, 4_000.0),
            ScrollAreaOrientation::Vertical,
            track,
        ));
        assert!(!state.is_dragging());

        // A press on the track centres the thumb on the pressed spot.
        state.set_offset(Vector::ZERO);
        assert!(state.apply_track_pointer(
            &pointer_event(PointerPhase::Down, 6.0, 100.0),
            ScrollAreaOrientation::Vertical,
            track,
        ));
        assert_eq!(state.offset().y, 400.0);
        assert!(!state.apply_track_pointer(
            &pointer_event(PointerPhase::Move, 6.0, 20.0),
            ScrollAreaOrientation::Vertical,
            track,
        ));

        // A non-overflowing axis refuses every pointer path.
        let mut flat = ScrollAreaState::new();
        flat.set_geometry(Size::new(200.0, 200.0), Size::new(200.0, 200.0));
        assert!(!flat.apply_track_pointer(
            &pointer_event(PointerPhase::Down, 6.0, 100.0),
            ScrollAreaOrientation::Vertical,
            track,
        ));
        assert!(!flat.apply_thumb_pointer(
            &pointer_event(PointerPhase::Move, 6.0, 100.0),
            ScrollAreaOrientation::Horizontal,
            track,
        ));
    }

    #[test]
    fn parts_add_exact_semantics_without_appearance() {
        let mut state = ScrollAreaState::new();
        state.set_geometry(Size::new(200.0, 200.0), Size::new(200.0, 1_000.0));
        state.set_offset(Vector::new(0.0, 250.0));
        let area = ScrollArea::new("log");

        let root = area.root_with(div().bg(Color::rgb8(1, 2, 3)));
        assert_eq!(root.explicit_id, Some("log".into()));
        assert_eq!(root.visual.background, Some(Color::rgb8(1, 2, 3)));

        let viewport = area.viewport_with(div().size(200.0, 200.0));
        assert_eq!(viewport.explicit_id, Some(area.viewport_id()));
        assert_eq!(viewport.accessibility.role, AccessibilityRole::ScrollView);
        assert_eq!(viewport.visual.background, None);

        let content = area.content_with(div());
        assert_eq!(content.explicit_id, Some(area.content_id()));

        let vertical = area.scrollbar_with(&state, ScrollAreaOrientation::Vertical, div().w(10.0));
        assert_eq!(
            vertical.explicit_id,
            Some(area.scrollbar_id(ScrollAreaOrientation::Vertical))
        );
        assert_eq!(vertical.accessibility.role, AccessibilityRole::ScrollBar);
        assert_eq!(
            vertical.accessibility.orientation,
            Some(AccessibilityOrientation::Vertical)
        );
        assert_eq!(
            vertical.accessibility.value_range.as_deref(),
            Some(&AccessibilityValueRange::new(250.0, 0.0, 800.0))
        );
        assert_eq!(
            vertical.accessibility.relations.controls(),
            Some(area.viewport_id())
        );
        assert!(!vertical.accessibility.hidden);
        assert!(!vertical.focusable);

        let horizontal =
            area.scrollbar_with(&state, ScrollAreaOrientation::Horizontal, div().h(10.0));
        assert!(
            horizontal.accessibility.hidden,
            "an axis that cannot scroll is not announced"
        );

        let thumb = area.thumb_with(ScrollAreaOrientation::Vertical, div());
        assert_eq!(
            thumb.explicit_id,
            Some(area.thumb_id(ScrollAreaOrientation::Vertical))
        );
        assert!(thumb.accessibility.hidden);
        let corner = area.corner_with(div());
        assert_eq!(corner.explicit_id, Some(area.corner_id()));
        assert!(corner.accessibility.hidden);

        assert!(area.shows_scrollbar(&state, ScrollAreaOrientation::Vertical));
        assert!(!area.shows_scrollbar(&state, ScrollAreaOrientation::Horizontal));
        assert!(!area.shows_corner(&state));
        let kept = ScrollArea::new("log").keep_mounted(true);
        assert!(kept.is_keep_mounted());
        assert!(kept.shows_scrollbar(&state, ScrollAreaOrientation::Horizontal));
        assert!(kept.shows_corner(&state));

        let ids = [
            area.root_id(),
            area.viewport_id(),
            area.content_id(),
            area.scrollbar_id(ScrollAreaOrientation::Vertical),
            area.scrollbar_id(ScrollAreaOrientation::Horizontal),
            area.thumb_id(ScrollAreaOrientation::Vertical),
            area.thumb_id(ScrollAreaOrientation::Horizontal),
            area.corner_id(),
        ];
        for (index, id) in ids.iter().enumerate() {
            assert_ne!(*id, ElementId::new(0));
            assert_ne!(*id, ElementId::new(u64::MAX));
            assert!(!ids[..index].contains(id));
        }

        let shorthand = scroll_area_viewport("log");
        assert_eq!(shorthand.accessibility.role, AccessibilityRole::ScrollView);
        assert!(shorthand.children.is_empty());
    }

    struct ScrollAreaView {
        state: ScrollAreaState,
    }

    const TRACK: f32 = 200.0;

    impl View for ScrollAreaView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            self.state
                .set_geometry(Size::new(200.0, 200.0), Size::new(200.0, 1_000.0));
            let area = ScrollArea::new("log");
            let offset = self.state.offset();
            let wheel = cx.scroll_wheel_listener(area.viewport_id(), |view, event, cx| {
                if view.state.apply_scroll_wheel(event) {
                    cx.invalidate();
                }
            });
            let drag = cx.pointer_listener(
                area.thumb_id(ScrollAreaOrientation::Vertical),
                |view, event, cx| {
                    if view
                        .state
                        .apply_thumb_pointer(event, ScrollAreaOrientation::Vertical, TRACK)
                    {
                        cx.invalidate();
                    }
                },
            );
            let thumb_length = self
                .state
                .thumb_length(ScrollAreaOrientation::Vertical, TRACK);
            let thumb_offset = self
                .state
                .thumb_offset(ScrollAreaOrientation::Vertical, TRACK);

            area.root_with(div().size(212.0, 200.0).flex_row())
                .child(
                    area.viewport_with(div().size(200.0, 200.0).on_scroll_wheel(wheel))
                        .child(
                            area.content_with(div().w(200.0).h(1_000.0))
                                .translate(-offset.x, -offset.y)
                                .child(text("Scrolled content")),
                        ),
                )
                .child(
                    area.scrollbar_with(
                        &self.state,
                        ScrollAreaOrientation::Vertical,
                        div().w(12.0).h(TRACK).relative(),
                    )
                    .child(
                        area.thumb_with(
                            ScrollAreaOrientation::Vertical,
                            div()
                                .absolute()
                                .w(12.0)
                                .h(thumb_length)
                                .translate(0.0, thumb_offset)
                                .on_pointer(drag),
                        ),
                    ),
                )
        }
    }

    #[test]
    fn wheel_and_thumb_paths_stay_deterministic_and_sleep() {
        let (mut cx, view) = TestAppContext::new(ScrollAreaView {
            state: ScrollAreaState::new(),
        })
        .unwrap();
        let window = view.window_handle();
        let area = ScrollArea::new("log");

        cx.simulate_scroll_wheel(
            window,
            area.viewport_id(),
            wheel(Vector::new(0.0, -100.0), GesturePhase::Moved),
        )
        .unwrap();
        assert_eq!(cx.read(view, |view| view.state.offset().y).unwrap(), 100.0);

        let update = cx.accessibility_update(window).unwrap();
        let node = |id: ElementId| {
            update
                .nodes
                .iter()
                .find_map(|(node_id, node)| (node_id.0 == id.as_u64()).then_some(node))
                .expect("scroll area accessibility node")
        };
        assert_eq!(node(area.viewport_id()).role(), accesskit::Role::ScrollView);
        let scrollbar = node(area.scrollbar_id(ScrollAreaOrientation::Vertical));
        assert_eq!(scrollbar.role(), accesskit::Role::ScrollBar);
        assert_eq!(scrollbar.numeric_value(), Some(100.0));
        assert_eq!(scrollbar.max_numeric_value(), Some(800.0));
        assert_eq!(
            scrollbar.orientation(),
            Some(accesskit::Orientation::Vertical)
        );

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }
}
