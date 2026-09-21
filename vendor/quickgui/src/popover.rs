use bitflags::bitflags;

use crate::{Point, Rect, Size};

/// Maximum nested menu-style native popover grabs retained by one application.
pub const MAX_GRABBING_POPOVERS: usize = 32;

/// The point of a parent-space anchor rectangle used to position a popover.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum PopoverAnchor {
    #[default]
    Center,
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    BottomLeft,
    TopRight,
    BottomRight,
}

impl PopoverAnchor {
    #[cfg(any(target_os = "macos", test))]
    fn flip_x(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
            Self::TopLeft => Self::TopRight,
            Self::BottomLeft => Self::BottomRight,
            Self::TopRight => Self::TopLeft,
            Self::BottomRight => Self::BottomLeft,
            other => other,
        }
    }

    #[cfg(any(target_os = "macos", test))]
    fn flip_y(self) -> Self {
        match self {
            Self::Top => Self::Bottom,
            Self::Bottom => Self::Top,
            Self::TopLeft => Self::BottomLeft,
            Self::BottomLeft => Self::TopLeft,
            Self::TopRight => Self::BottomRight,
            Self::BottomRight => Self::TopRight,
            other => other,
        }
    }

    fn point(self, rect: Rect) -> Point {
        let center_x = rect.x + rect.width * 0.5;
        let center_y = rect.y + rect.height * 0.5;
        match self {
            Self::Center => Point::new(center_x, center_y),
            Self::Top => Point::new(center_x, rect.y),
            Self::Bottom => Point::new(center_x, rect.bottom()),
            Self::Left => Point::new(rect.x, center_y),
            Self::Right => Point::new(rect.right(), center_y),
            Self::TopLeft => Point::new(rect.x, rect.y),
            Self::BottomLeft => Point::new(rect.x, rect.bottom()),
            Self::TopRight => Point::new(rect.right(), rect.y),
            Self::BottomRight => Point::new(rect.right(), rect.bottom()),
        }
    }
}

/// The direction in which a popover extends from its anchor point.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum PopoverGravity {
    #[default]
    Center,
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    BottomLeft,
    TopRight,
    BottomRight,
}

impl PopoverGravity {
    #[cfg(any(target_os = "macos", test))]
    fn flip_x(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
            Self::TopLeft => Self::TopRight,
            Self::BottomLeft => Self::BottomRight,
            Self::TopRight => Self::TopLeft,
            Self::BottomRight => Self::BottomLeft,
            other => other,
        }
    }

    #[cfg(any(target_os = "macos", test))]
    fn flip_y(self) -> Self {
        match self {
            Self::Top => Self::Bottom,
            Self::Bottom => Self::Top,
            Self::TopLeft => Self::BottomLeft,
            Self::BottomLeft => Self::TopLeft,
            Self::TopRight => Self::BottomRight,
            Self::BottomRight => Self::TopRight,
            other => other,
        }
    }

    fn origin(self, anchor: Point, size: Size) -> Point {
        match self {
            Self::Center => Point::new(anchor.x - size.width * 0.5, anchor.y - size.height * 0.5),
            Self::Top => Point::new(anchor.x - size.width * 0.5, anchor.y - size.height),
            Self::Bottom => Point::new(anchor.x - size.width * 0.5, anchor.y),
            Self::Left => Point::new(anchor.x - size.width, anchor.y - size.height * 0.5),
            Self::Right => Point::new(anchor.x, anchor.y - size.height * 0.5),
            Self::TopLeft => Point::new(anchor.x - size.width, anchor.y - size.height),
            Self::BottomLeft => Point::new(anchor.x - size.width, anchor.y),
            Self::TopRight => Point::new(anchor.x, anchor.y - size.height),
            Self::BottomRight => anchor,
        }
    }
}

bitflags! {
    /// Permitted screen-edge corrections for a system popover.
    #[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
    pub struct PopoverConstraintAdjustment: u32 {
        const SLIDE_X = 1 << 0;
        const SLIDE_Y = 1 << 1;
        const FLIP_X = 1 << 2;
        const FLIP_Y = 1 << 3;
        const RESIZE_X = 1 << 4;
        const RESIZE_Y = 1 << 5;

        /// Native-menu-style correction that preserves size while selecting the best side and
        /// sliding the result into the visible screen work area.
        const FIT = Self::SLIDE_X.bits()
            | Self::SLIDE_Y.bits()
            | Self::FLIP_X.bits()
            | Self::FLIP_Y.bits();
    }
}

/// Parent-relative placement and input behavior for a native system popover.
///
/// Coordinates use the parent's logical, top-left content coordinate space—the same space as
/// element bounds. The popover size comes from [`WindowOptions::size`](crate::WindowOptions::size).
#[derive(Clone, Debug, PartialEq)]
pub struct PopoverOptions {
    pub anchor_rect: Rect,
    pub anchor: PopoverAnchor,
    pub gravity: PopoverGravity,
    pub constraint_adjustment: PopoverConstraintAdjustment,
    pub offset: Point,
    /// Logical inset from the display work area used by collision correction.
    pub viewport_margin: f32,
    /// Whether Escape dismisses this system popover while it owns keyboard input.
    pub dismiss_on_escape: bool,
    /// Whether a pointer press outside this system popover dismisses it.
    pub dismiss_on_pointer_outside: bool,
    /// Whether this system popover participates in the native menu-style focus chain.
    ///
    /// [`Self::grab`] remains the compatibility switch that enables or disables both dismissal
    /// paths together. Call the individual dismissal builders afterwards to override either path.
    pub grab: bool,
    /// Whether pointer interaction may make the native popover the key window.
    ///
    /// This remains independent from `grab`: interactive autocomplete suggestions use a
    /// non-grabbing, never-key panel so the owner text input keeps its IME session.
    pub accepts_key_focus: bool,
}

impl PopoverOptions {
    /// Create menu/dropdown placement below the anchor's left edge.
    pub const fn new(anchor_rect: Rect) -> Self {
        Self {
            anchor_rect,
            anchor: PopoverAnchor::BottomLeft,
            gravity: PopoverGravity::BottomRight,
            constraint_adjustment: PopoverConstraintAdjustment::FIT,
            offset: Point::ZERO,
            viewport_margin: 0.0,
            dismiss_on_escape: true,
            dismiss_on_pointer_outside: true,
            grab: true,
            accepts_key_focus: true,
        }
    }

    pub const fn anchor(mut self, anchor: PopoverAnchor) -> Self {
        self.anchor = anchor;
        self
    }

    pub const fn gravity(mut self, gravity: PopoverGravity) -> Self {
        self.gravity = gravity;
        self
    }

    pub const fn constraint_adjustment(mut self, adjustment: PopoverConstraintAdjustment) -> Self {
        self.constraint_adjustment = adjustment;
        self
    }

    pub const fn offset(mut self, x: f32, y: f32) -> Self {
        self.offset = Point::new(x, y);
        self
    }

    pub const fn viewport_margin(mut self, margin: f32) -> Self {
        self.viewport_margin = margin;
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

    pub const fn grab(mut self, grab: bool) -> Self {
        self.grab = grab;
        self.dismiss_on_escape = grab;
        self.dismiss_on_pointer_outside = grab;
        if grab {
            self.accepts_key_focus = true;
        }
        self
    }

    /// Allow or forbid the popover from becoming the native key window after interaction.
    ///
    /// Forbidding key focus also disables menu-style grabbing because a grabbing popover must own
    /// keyboard focus. Pointer delivery remains enabled.
    pub const fn accepts_key_focus(mut self, accepts_key_focus: bool) -> Self {
        self.accepts_key_focus = accepts_key_focus;
        if !accepts_key_focus {
            self.grab = false;
            self.dismiss_on_escape = false;
            self.dismiss_on_pointer_outside = false;
        }
        self
    }

    pub(crate) fn is_valid(&self, coordinate_limit: f32, dimension_limit: f32) -> bool {
        let rect = self.anchor_rect;
        rect.x.is_finite()
            && rect.y.is_finite()
            && rect.width.is_finite()
            && rect.height.is_finite()
            && rect.x.abs() <= coordinate_limit
            && rect.y.abs() <= coordinate_limit
            && rect.width >= 0.0
            && rect.height >= 0.0
            && rect.width <= dimension_limit
            && rect.height <= dimension_limit
            && self.offset.x.is_finite()
            && self.offset.y.is_finite()
            && self.offset.x.abs() <= coordinate_limit
            && self.offset.y.abs() <= coordinate_limit
            && self.viewport_margin.is_finite()
            && self.viewport_margin >= 0.0
            && self.viewport_margin <= dimension_limit
            && (!self.grab || self.accepts_key_focus)
    }
}

fn placed_rect(
    anchor_rect: Rect,
    size: Size,
    anchor: PopoverAnchor,
    gravity: PopoverGravity,
    offset: Point,
) -> Rect {
    let point = anchor.point(anchor_rect);
    let origin = gravity.origin(point, size);
    Rect::new(
        origin.x + offset.x,
        origin.y + offset.y,
        size.width,
        size.height,
    )
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn unconstrained_popover_rect(
    anchor_rect: Rect,
    size: Size,
    options: &PopoverOptions,
) -> Rect {
    placed_rect(
        anchor_rect,
        size,
        options.anchor,
        options.gravity,
        options.offset,
    )
}

#[cfg(any(target_os = "macos", test))]
fn horizontal_overflow(rect: Rect, bounds: Rect) -> f32 {
    (bounds.x - rect.x).max(0.0) + (rect.right() - bounds.right()).max(0.0)
}

#[cfg(any(target_os = "macos", test))]
fn vertical_overflow(rect: Rect, bounds: Rect) -> f32 {
    (bounds.y - rect.y).max(0.0) + (rect.bottom() - bounds.bottom()).max(0.0)
}

#[cfg(any(target_os = "macos", test))]
fn resized_axis(origin: f32, size: f32, minimum: f32, maximum: f32) -> (f32, f32) {
    let start = origin.max(minimum);
    let end = (origin + size).min(maximum);
    if end > start {
        (start, end - start)
    } else {
        let available = (maximum - minimum).max(0.0);
        let size = available.min(1.0);
        if origin < minimum {
            (minimum, size)
        } else {
            (maximum - size, size)
        }
    }
}

/// Resolve placement in one logical top-left screen coordinate space.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn place_popover(
    anchor_rect: Rect,
    size: Size,
    visible_bounds: Rect,
    options: &PopoverOptions,
) -> Rect {
    let margin = options
        .viewport_margin
        .min(visible_bounds.width * 0.5)
        .min(visible_bounds.height * 0.5);
    let visible_bounds = Rect::new(
        visible_bounds.x + margin,
        visible_bounds.y + margin,
        (visible_bounds.width - margin * 2.0).max(0.0),
        (visible_bounds.height - margin * 2.0).max(0.0),
    );
    let mut anchor = options.anchor;
    let mut gravity = options.gravity;
    let mut result = placed_rect(anchor_rect, size, anchor, gravity, options.offset);

    if options
        .constraint_adjustment
        .contains(PopoverConstraintAdjustment::FLIP_X)
        && horizontal_overflow(result, visible_bounds) > 0.0
    {
        let candidate_anchor = anchor.flip_x();
        let candidate_gravity = gravity.flip_x();
        let candidate = placed_rect(
            anchor_rect,
            size,
            candidate_anchor,
            candidate_gravity,
            options.offset,
        );
        if horizontal_overflow(candidate, visible_bounds)
            < horizontal_overflow(result, visible_bounds)
        {
            anchor = candidate_anchor;
            gravity = candidate_gravity;
            result = candidate;
        }
    }

    if options
        .constraint_adjustment
        .contains(PopoverConstraintAdjustment::FLIP_Y)
        && vertical_overflow(result, visible_bounds) > 0.0
    {
        let candidate = placed_rect(
            anchor_rect,
            size,
            anchor.flip_y(),
            gravity.flip_y(),
            options.offset,
        );
        if vertical_overflow(candidate, visible_bounds) < vertical_overflow(result, visible_bounds)
        {
            result = candidate;
        }
    }

    if options
        .constraint_adjustment
        .contains(PopoverConstraintAdjustment::SLIDE_X)
    {
        result.x = if result.width <= visible_bounds.width {
            result
                .x
                .clamp(visible_bounds.x, visible_bounds.right() - result.width)
        } else {
            visible_bounds.x
        };
    }
    if options
        .constraint_adjustment
        .contains(PopoverConstraintAdjustment::SLIDE_Y)
    {
        result.y = if result.height <= visible_bounds.height {
            result
                .y
                .clamp(visible_bounds.y, visible_bounds.bottom() - result.height)
        } else {
            visible_bounds.y
        };
    }
    if options
        .constraint_adjustment
        .contains(PopoverConstraintAdjustment::RESIZE_X)
        && horizontal_overflow(result, visible_bounds) > 0.0
    {
        (result.x, result.width) = resized_axis(
            result.x,
            result.width,
            visible_bounds.x,
            visible_bounds.right(),
        );
    }
    if options
        .constraint_adjustment
        .contains(PopoverConstraintAdjustment::RESIZE_Y)
        && vertical_overflow(result, visible_bounds) > 0.0
    {
        (result.y, result.height) = resized_axis(
            result.y,
            result.height,
            visible_bounds.y,
            visible_bounds.bottom(),
        );
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_menu_placement_grows_below_and_right() {
        let options = PopoverOptions::new(Rect::new(40.0, 20.0, 100.0, 30.0));
        assert_eq!(
            place_popover(
                options.anchor_rect,
                Size::new(180.0, 120.0),
                Rect::new(0.0, 0.0, 800.0, 600.0),
                &options,
            ),
            Rect::new(40.0, 50.0, 180.0, 120.0)
        );
    }

    #[test]
    fn vertical_flip_places_a_menu_above_an_edge_anchor() {
        let options = PopoverOptions::new(Rect::new(40.0, 560.0, 100.0, 30.0));
        assert_eq!(
            place_popover(
                options.anchor_rect,
                Size::new(180.0, 120.0),
                Rect::new(0.0, 0.0, 800.0, 600.0),
                &options,
            ),
            Rect::new(40.0, 440.0, 180.0, 120.0)
        );
    }

    #[test]
    fn horizontal_flip_preserves_the_near_edge_when_it_fits_better() {
        let options = PopoverOptions::new(Rect::new(760.0, 40.0, 30.0, 30.0))
            .anchor(PopoverAnchor::BottomRight)
            .gravity(PopoverGravity::BottomRight);
        assert_eq!(
            place_popover(
                options.anchor_rect,
                Size::new(180.0, 120.0),
                Rect::new(0.0, 0.0, 800.0, 600.0),
                &options,
            ),
            Rect::new(580.0, 70.0, 180.0, 120.0)
        );
    }

    #[test]
    fn slide_and_resize_are_independently_opt_in() {
        let options = PopoverOptions::new(Rect::new(20.0, 20.0, 0.0, 0.0))
            .gravity(PopoverGravity::BottomRight)
            .constraint_adjustment(
                PopoverConstraintAdjustment::SLIDE_X | PopoverConstraintAdjustment::RESIZE_Y,
            );
        assert_eq!(
            place_popover(
                options.anchor_rect,
                Size::new(120.0, 900.0),
                Rect::new(10.0, 10.0, 100.0, 80.0),
                &options,
            ),
            Rect::new(10.0, 20.0, 120.0, 70.0)
        );
    }

    #[test]
    fn viewport_margin_insets_the_collision_bounds() {
        let options = PopoverOptions::new(Rect::new(88.0, 40.0, 4.0, 4.0))
            .constraint_adjustment(PopoverConstraintAdjustment::SLIDE_X)
            .viewport_margin(10.0);
        assert_eq!(
            place_popover(
                options.anchor_rect,
                Size::new(30.0, 20.0),
                Rect::new(0.0, 0.0, 100.0, 100.0),
                &options,
            ),
            Rect::new(60.0, 44.0, 30.0, 20.0)
        );
    }

    #[test]
    fn zero_sized_anchor_and_bounded_offset_are_valid() {
        assert!(
            PopoverOptions::new(Rect::new(10.0, 20.0, 0.0, 0.0))
                .offset(4.0, -2.0)
                .is_valid(1_000.0, 1_000.0)
        );
        assert!(
            !PopoverOptions::new(Rect::new(f32::NAN, 0.0, 0.0, 0.0)).is_valid(1_000.0, 1_000.0)
        );
    }

    #[test]
    fn never_key_popover_is_interactive_without_a_grab() {
        let options = PopoverOptions::new(Rect::ZERO).accepts_key_focus(false);
        assert!(!options.grab);
        assert!(!options.accepts_key_focus);
        assert!(!options.dismiss_on_escape);
        assert!(!options.dismiss_on_pointer_outside);
        assert!(options.is_valid(1_000.0, 1_000.0));

        let grabbing = options.grab(true);
        assert!(grabbing.grab);
        assert!(grabbing.accepts_key_focus);
        assert!(grabbing.dismiss_on_escape);
        assert!(grabbing.dismiss_on_pointer_outside);

        let independently_dismissed = grabbing
            .dismiss_on_escape(false)
            .dismiss_on_pointer_outside(true);
        assert!(!independently_dismissed.dismiss_on_escape);
        assert!(independently_dismissed.dismiss_on_pointer_outside);

        let invalid = PopoverOptions {
            grab: true,
            accepts_key_focus: false,
            ..PopoverOptions::new(Rect::ZERO)
        };
        assert!(!invalid.is_valid(1_000.0, 1_000.0));
    }
}
