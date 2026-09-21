use std::sync::Arc;

use crate::{
    AccessibilityOrientation, AccessibilityRole, AccessibilityValueRange, Element, ElementId,
    KeyBinding, PointerEvent, PointerPhase, Size, StateAccessor, ViewContext, div,
};

/// Maximum thumbs retained by one slider.
///
/// A slider is a fixed-size value; the bound keeps the retained array small enough to copy while
/// still covering multi-handle range selection.
pub const MAX_SLIDER_THUMBS: usize = 8;

const SLIDER_KEY_CONTEXT: &str = "Slider";
const SLIDER_TRACK_ID_TAG: u64 = 0x5f3a_11c8_9d47_2b60;
const SLIDER_RANGE_ID_TAG: u64 = 0x18d6_7b04_ee31_a5c9;
const SLIDER_LABEL_ID_TAG: u64 = 0x4c1b_7e93_a0d6_28f5;
const SLIDER_VALUE_ID_TAG: u64 = 0xd370_29ae_51fc_6b48;
const SLIDER_CONTROL_ID_TAG: u64 = 0x83e6_05b1_9c7d_f420;
const SLIDER_THUMB_ID_TAG: u64 = 0xc70e_2a95_4413_86fd;

/// Move the active slider thumb one step toward the maximum.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SliderIncrement;
/// Move the active slider thumb one step toward the minimum.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SliderDecrement;
/// Move the active slider thumb one large step toward the maximum.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SliderLargeIncrement;
/// Move the active slider thumb one large step toward the minimum.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SliderLargeDecrement;
/// Move the active slider thumb to its minimum.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SliderMinimum;
/// Move the active slider thumb to its maximum.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SliderMaximum;

/// Contextual bindings used by [`Slider::key_with`] and [`SliderThumb::key_with`].
///
/// `Shift` selects the large step on the same arrow keys, matching the desktop convention that a
/// modified arrow moves by the same amount as PageUp and PageDown.
pub fn slider_key_bindings() -> [KeyBinding; 14] {
    [
        KeyBinding::new("right", SliderIncrement, Some(SLIDER_KEY_CONTEXT)),
        KeyBinding::new("up", SliderIncrement, Some(SLIDER_KEY_CONTEXT)),
        KeyBinding::new("left", SliderDecrement, Some(SLIDER_KEY_CONTEXT)),
        KeyBinding::new("down", SliderDecrement, Some(SLIDER_KEY_CONTEXT)),
        KeyBinding::new(
            "shift-right",
            SliderLargeIncrement,
            Some(SLIDER_KEY_CONTEXT),
        ),
        KeyBinding::new("shift-up", SliderLargeIncrement, Some(SLIDER_KEY_CONTEXT)),
        KeyBinding::new("shift-left", SliderLargeDecrement, Some(SLIDER_KEY_CONTEXT)),
        KeyBinding::new("shift-down", SliderLargeDecrement, Some(SLIDER_KEY_CONTEXT)),
        KeyBinding::new("pageup", SliderLargeIncrement, Some(SLIDER_KEY_CONTEXT)),
        KeyBinding::new("pagedown", SliderLargeDecrement, Some(SLIDER_KEY_CONTEXT)),
        KeyBinding::new("home", SliderMinimum, Some(SLIDER_KEY_CONTEXT)),
        KeyBinding::new("end", SliderMaximum, Some(SLIDER_KEY_CONTEXT)),
        KeyBinding::new("platform-left", SliderMinimum, Some(SLIDER_KEY_CONTEXT)),
        KeyBinding::new("platform-right", SliderMaximum, Some(SLIDER_KEY_CONTEXT)),
    ]
}

/// Where a thumb sits relative to the value position along the track.
///
/// This is Base UI's `thumbAlignment`. It changes only the geometry an application derives from
/// [`SliderThumb::offset`]; the numeric contract is identical either way.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SliderThumbAlignment {
    /// Center the thumb on the value, so it overhangs the track at both ends.
    #[default]
    Center,
    /// Inset the thumb so its own box always stays inside the track.
    Edge,
}

/// What one captured pointer event did to a slider.
///
/// [`SliderState::apply_pointer`] keeps its original `bool` return; this is the richer result an
/// application needs to fire Base UI's `onValueCommitted` exactly once per drag and to restyle a
/// thumb while it is being dragged.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SliderPointerChange {
    /// Whether a value, the active thumb, or the dragging flag changed.
    ///
    /// This is what an application invalidates on, because a drag that changed no value still
    /// changes how the thumb is styled.
    pub changed: bool,
    /// Whether a value or the active thumb changed, which is what
    /// [`SliderState::apply_pointer`] returns.
    pub values_changed: bool,
    /// Whether this event ended a drag, which is when a committed value is final.
    pub committed: bool,
}

/// A copyable render-state snapshot for one slider thumb.
///
/// Base UI publishes the same facts as `data-index`, `data-dragging`, `data-disabled`, and the
/// thumb's own value; QuickGUI has no style sheet, so the application reads them here.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SliderThumbState {
    /// The thumb's position in the declared value list, Base UI's `data-index`.
    pub index: usize,
    /// The thumb's current value.
    pub value: f64,
    /// The `0.0..=1.0` position of the value along the track.
    pub fraction: f32,
    /// Whether typed keyboard actions target this thumb.
    pub active: bool,
    /// Whether a captured pointer is currently dragging the slider.
    pub dragging: bool,
    /// Whether the slider refuses changes.
    pub disabled: bool,
}

/// Layout and keyboard axis for one slider.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SliderOrientation {
    #[default]
    Horizontal,
    Vertical,
}

impl SliderOrientation {
    const fn accessibility(self) -> AccessibilityOrientation {
        match self {
            Self::Horizontal => AccessibilityOrientation::Horizontal,
            Self::Vertical => AccessibilityOrientation::Vertical,
        }
    }
}

/// Controlled, allocation-free bounded value state for one slider.
///
/// The state is a plain copyable value. It owns the numeric contract—bounds, step snapping, thumb
/// ordering, and the active thumb—while the application owns every visual declaration and decides
/// when a change invalidates its window. It retains no allocation, task, timer, observer, or idle
/// scheduler source.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliderState {
    minimum: f64,
    maximum: f64,
    step: f64,
    large_step: f64,
    values: [f64; MAX_SLIDER_THUMBS],
    thumbs: usize,
    active: usize,
    min_steps_between: u16,
    thumb_alignment: SliderThumbAlignment,
    orientation: SliderOrientation,
    disabled: bool,
    dragging: bool,
}

impl Default for SliderState {
    fn default() -> Self {
        Self::new(0.0, 1.0, 0.0)
    }
}

impl SliderState {
    /// Create a single-thumb slider.
    ///
    /// Non-finite bounds fall back to `0.0..=1.0`, an inverted range is swapped, and an empty
    /// range keeps one degenerate value at the minimum.
    pub fn new(minimum: f64, maximum: f64, value: f64) -> Self {
        let (minimum, maximum) = normalized_bounds(minimum, maximum);
        let mut state = Self {
            minimum,
            maximum,
            step: 1.0,
            large_step: 0.0,
            values: [minimum; MAX_SLIDER_THUMBS],
            thumbs: 1,
            active: 0,
            min_steps_between: 0,
            thumb_alignment: SliderThumbAlignment::Center,
            orientation: SliderOrientation::Horizontal,
            disabled: false,
            dragging: false,
        };
        state.values[0] = state.snap(value);
        state
    }

    /// Create a multi-thumb range slider.
    ///
    /// # Panics
    ///
    /// Panics when `values` is empty or longer than [`MAX_SLIDER_THUMBS`].
    pub fn range(minimum: f64, maximum: f64, values: &[f64]) -> Self {
        assert!(!values.is_empty(), "a slider needs at least one thumb");
        assert!(
            values.len() <= MAX_SLIDER_THUMBS,
            "a slider retains at most {MAX_SLIDER_THUMBS} thumbs"
        );
        let mut state = Self::new(minimum, maximum, values[0]);
        state.thumbs = values.len();
        for (index, value) in values.iter().enumerate() {
            state.values[index] = state.snap(*value);
        }
        state.enforce_order(0);
        state
    }

    /// Replace the step used by arrow keys and pointer snapping.
    ///
    /// A non-finite or non-positive step selects continuous movement, which snaps nothing.
    #[must_use]
    pub fn step(mut self, step: f64) -> Self {
        self.step = if step.is_finite() && step > 0.0 {
            step
        } else {
            0.0
        };
        for index in 0..self.thumbs {
            self.values[index] = self.snap(self.values[index]);
        }
        self.enforce_order(0);
        self
    }

    /// Replace the amount moved by PageUp, PageDown, and shifted arrows.
    ///
    /// The default is ten ordinary steps.
    #[must_use]
    pub fn large_step(mut self, large_step: f64) -> Self {
        self.large_step = if large_step.is_finite() && large_step > 0.0 {
            large_step
        } else {
            0.0
        };
        self
    }

    /// Keep adjacent thumbs at least this many steps apart, Base UI's `minStepsBetweenValues`.
    ///
    /// The gap is measured in whole steps, so a continuous slider — one with no step — ignores it.
    /// Existing values are re-ordered outward from the first thumb, and every later movement is
    /// clamped so the gap can never close.
    #[must_use]
    pub fn min_steps_between_values(mut self, steps: usize) -> Self {
        self.min_steps_between = steps.min(u16::MAX as usize) as u16;
        self.enforce_order(0);
        self
    }

    /// Choose where a thumb sits relative to its value, Base UI's `thumbAlignment`.
    #[must_use]
    pub const fn thumb_alignment(mut self, alignment: SliderThumbAlignment) -> Self {
        self.thumb_alignment = alignment;
        self
    }

    #[must_use]
    pub const fn orientation(mut self, orientation: SliderOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    #[must_use]
    pub const fn vertical(self) -> Self {
        self.orientation(SliderOrientation::Vertical)
    }

    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub const fn minimum(&self) -> f64 {
        self.minimum
    }

    pub const fn maximum(&self) -> f64 {
        self.maximum
    }

    pub const fn step_value(&self) -> f64 {
        self.step
    }

    /// The amount one large step moves.
    ///
    /// Without an explicit [`Self::large_step`] this is ten ordinary steps, or one tenth of the
    /// range for a continuous slider.
    pub const fn large_step_value(&self) -> f64 {
        if self.large_step > 0.0 {
            self.large_step
        } else if self.step > 0.0 {
            self.step * 10.0
        } else {
            (self.maximum - self.minimum) / 10.0
        }
    }

    pub const fn axis(&self) -> SliderOrientation {
        self.orientation
    }

    pub const fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// The minimum whole-step gap kept between adjacent thumbs.
    pub const fn min_steps_between_values_value(&self) -> usize {
        self.min_steps_between as usize
    }

    /// Where a thumb sits relative to its value.
    pub const fn thumb_alignment_value(&self) -> SliderThumbAlignment {
        self.thumb_alignment
    }

    /// Whether a captured pointer is currently dragging this slider, Base UI's `data-dragging`.
    pub const fn is_dragging(&self) -> bool {
        self.dragging
    }

    /// End a drag started by [`Self::apply_pointer`], returning whether the flag changed.
    ///
    /// A captured pointer normally clears this itself; call it when the application cancels a drag
    /// for its own reasons.
    pub fn end_drag(&mut self) -> bool {
        std::mem::replace(&mut self.dragging, false)
    }

    /// The distance between adjacent thumbs required by [`Self::min_steps_between_values`].
    fn minimum_gap(&self) -> f64 {
        if self.step > 0.0 {
            self.step * f64::from(self.min_steps_between)
        } else {
            0.0
        }
    }

    /// The leading-edge offset of one thumb along the track, honoring the thumb alignment.
    ///
    /// `track_length` and `thumb_length` are measured along the slider's own axis, and the result
    /// runs in that axis's visual direction — left to right for a horizontal slider, top to bottom
    /// for a vertical one, so a vertical minimum sits at the bottom.
    pub fn thumb_offset(&self, index: usize, track_length: f32, thumb_length: f32) -> f32 {
        let fraction = match self.orientation {
            SliderOrientation::Horizontal => self.fraction(index),
            SliderOrientation::Vertical => 1.0 - self.fraction(index),
        };
        let track_length = if track_length.is_finite() {
            track_length.max(0.0)
        } else {
            0.0
        };
        let thumb_length = if thumb_length.is_finite() {
            thumb_length.max(0.0)
        } else {
            0.0
        };
        match self.thumb_alignment {
            SliderThumbAlignment::Center => fraction * track_length - thumb_length * 0.5,
            SliderThumbAlignment::Edge => fraction * (track_length - thumb_length).max(0.0),
        }
    }

    pub const fn thumb_count(&self) -> usize {
        self.thumbs
    }

    pub const fn active_thumb(&self) -> usize {
        self.active
    }

    /// The first thumb's value, which is the whole value of a single-thumb slider.
    pub const fn value(&self) -> f64 {
        self.values[0]
    }

    pub fn values(&self) -> &[f64] {
        &self.values[..self.thumbs]
    }

    /// Read one thumb value, returning `None` past [`Self::thumb_count`].
    pub fn thumb_value(&self, index: usize) -> Option<f64> {
        (index < self.thumbs).then(|| self.values[index])
    }

    /// The inclusive bounds one thumb may move between, honoring its neighbors.
    pub fn thumb_bounds(&self, index: usize) -> Option<(f64, f64)> {
        if index >= self.thumbs {
            return None;
        }
        let gap = self.minimum_gap();
        let lower = if index == 0 {
            self.minimum
        } else {
            self.values[index - 1] + gap
        };
        let upper = if index + 1 == self.thumbs {
            self.maximum
        } else {
            self.values[index + 1] - gap
        };
        // A range too narrow for the declared gap collapses to the lower bound rather than
        // inverting, so a thumb can never be asked to move past its neighbour.
        Some((lower.min(self.maximum), upper.max(lower).min(self.maximum)))
    }

    /// Make one thumb the target of keyboard actions, returning whether it changed.
    pub fn set_active_thumb(&mut self, index: usize) -> bool {
        if index >= self.thumbs || self.active == index {
            return false;
        }
        self.active = index;
        true
    }

    /// Replace one thumb value, returning whether it changed.
    ///
    /// The value is clamped into the slider bounds, snapped to the step, and then clamped between
    /// its neighboring thumbs so ordering can never invert.
    pub fn set_thumb_value(&mut self, index: usize, value: f64) -> bool {
        if index >= self.thumbs || self.disabled {
            return false;
        }
        let Some((lower, upper)) = self.thumb_bounds(index) else {
            return false;
        };
        let value = self.snap(value).clamp(lower, upper);
        if self.values[index] == value {
            return false;
        }
        self.values[index] = value;
        true
    }

    /// Replace the first thumb value, returning whether it changed.
    pub fn set_value(&mut self, value: f64) -> bool {
        self.set_thumb_value(0, value)
    }

    /// Move the active thumb by `steps` ordinary steps, returning whether it changed.
    ///
    /// A continuous slider uses one percent of its range per step.
    pub fn step_active(&mut self, steps: f64) -> bool {
        let step = if self.step > 0.0 {
            self.step
        } else {
            (self.maximum - self.minimum) / 100.0
        };
        let index = self.active;
        let Some(current) = self.thumb_value(index) else {
            return false;
        };
        self.set_thumb_value(index, current + step * steps)
    }

    /// Move the active thumb by one large step, returning whether it changed.
    pub fn large_step_active(&mut self, forward: bool) -> bool {
        let delta = self.large_step_value();
        let index = self.active;
        let Some(current) = self.thumb_value(index) else {
            return false;
        };
        self.set_thumb_value(index, current + if forward { delta } else { -delta })
    }

    /// Move the active thumb to its lowest reachable value, returning whether it changed.
    pub fn active_to_minimum(&mut self) -> bool {
        let index = self.active;
        self.set_thumb_value(index, self.minimum)
    }

    /// Move the active thumb to its highest reachable value, returning whether it changed.
    pub fn active_to_maximum(&mut self) -> bool {
        let index = self.active;
        self.set_thumb_value(index, self.maximum)
    }

    /// The `0.0..=1.0` position of one thumb along its track.
    ///
    /// Applications use this to place a caller-owned thumb and size a caller-owned range fill.
    /// A vertical slider still returns zero at its minimum; invert it in the caller's layout.
    pub fn fraction(&self, index: usize) -> f32 {
        self.thumb_value(index)
            .map(|value| self.fraction_of(value))
            .unwrap_or(0.0)
    }

    /// The `0.0..=1.0` position of an arbitrary value along the track.
    pub fn fraction_of(&self, value: f64) -> f32 {
        let span = self.maximum - self.minimum;
        if span <= 0.0 {
            return 0.0;
        }
        (((value - self.minimum) / span) as f32).clamp(0.0, 1.0)
    }

    /// The snapped value at one `0.0..=1.0` track position.
    pub fn value_for_fraction(&self, fraction: f32) -> f64 {
        self.value_for_exact_fraction(f64::from(fraction))
    }

    fn value_for_exact_fraction(&self, fraction: f64) -> f64 {
        let fraction = if fraction.is_finite() {
            fraction.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.snap(self.minimum + fraction * (self.maximum - self.minimum))
    }

    /// The snapped value under one pointer offset measured along a track of `length` pixels.
    ///
    /// A vertical track measures from its top, where the maximum lives, matching desktop sliders.
    pub fn value_at(&self, offset: f32, length: f32) -> f64 {
        if !length.is_finite() || length <= 0.0 {
            return self.minimum;
        }
        let raw = (f64::from(offset) / f64::from(length)).clamp(0.0, 1.0);
        let fraction = match self.orientation {
            SliderOrientation::Horizontal => raw,
            SliderOrientation::Vertical => 1.0 - raw,
        };
        self.value_for_exact_fraction(fraction)
    }

    /// The thumb whose value is closest to `value`, preferring the lower index on a tie.
    pub fn nearest_thumb(&self, value: f64) -> usize {
        let mut best = 0;
        let mut distance = f64::INFINITY;
        for index in 0..self.thumbs {
            let candidate = (self.values[index] - value).abs();
            if candidate < distance {
                distance = candidate;
                best = index;
            }
        }
        best
    }

    /// Apply one captured pointer event over a track of the given size.
    ///
    /// The application owns the track's layout, so it passes the size it declared. A press picks
    /// the nearest thumb, makes it active, and jumps it to the pointer; subsequent moves drag that
    /// same thumb even outside the track. Returns whether any value or the active thumb changed.
    pub fn apply_pointer(&mut self, event: &PointerEvent, track: Size) -> bool {
        // The dragging flag is maintained either way; this entry point keeps reporting only
        // whether a value or the active thumb moved, exactly as it always has.
        self.apply_pointer_change(event, track).values_changed
    }

    /// Apply one captured pointer event and report whether the drag also ended.
    ///
    /// This is the same arithmetic as [`Self::apply_pointer`] with Base UI's `onValueCommitted`
    /// boundary exposed: `committed` is true exactly on the event that releases or cancels the
    /// capture, which is when the application persists the value it has been previewing. The
    /// dragging flag is maintained here too, so [`Self::is_dragging`] can restyle the thumb.
    pub fn apply_pointer_change(
        &mut self,
        event: &PointerEvent,
        track: Size,
    ) -> SliderPointerChange {
        if self.disabled {
            return SliderPointerChange::default();
        }
        let ends = matches!(event.phase, PointerPhase::Up | PointerPhase::Cancel);
        let dragging = match event.phase {
            PointerPhase::Down | PointerPhase::Move => true,
            PointerPhase::Up | PointerPhase::Cancel => false,
        };
        let dragging_changed = std::mem::replace(&mut self.dragging, dragging) != dragging;
        let values_changed = self.apply_pointer_values(event, track);
        SliderPointerChange {
            changed: values_changed || dragging_changed,
            values_changed,
            committed: ends,
        }
    }

    fn apply_pointer_values(&mut self, event: &PointerEvent, track: Size) -> bool {
        if self.disabled {
            return false;
        }
        let (offset, length) = match self.orientation {
            SliderOrientation::Horizontal => (event.local_position.x, track.width),
            SliderOrientation::Vertical => (event.local_position.y, track.height),
        };
        let value = self.value_at(offset, length);
        let mut changed = false;
        if event.phase == PointerPhase::Down {
            changed |= self.set_active_thumb(self.nearest_thumb(value));
        }
        if matches!(
            event.phase,
            PointerPhase::Down | PointerPhase::Move | PointerPhase::Up
        ) {
            let index = self.active;
            changed |= self.set_thumb_value(index, value);
        }
        changed
    }

    fn snap(&self, value: f64) -> f64 {
        if !value.is_finite() {
            return self.minimum;
        }
        let value = value.clamp(self.minimum, self.maximum);
        if self.step <= 0.0 {
            return value;
        }
        let steps = ((value - self.minimum) / self.step).round();
        (self.minimum + steps * self.step).clamp(self.minimum, self.maximum)
    }

    fn enforce_order(&mut self, from: usize) {
        let gap = self.minimum_gap();
        for index in from.max(1)..self.thumbs {
            let floor = (self.values[index - 1] + gap).min(self.maximum);
            if self.values[index] < floor {
                self.values[index] = floor;
            }
        }
    }
}

fn normalized_bounds(minimum: f64, maximum: f64) -> (f64, f64) {
    if !minimum.is_finite() || !maximum.is_finite() {
        return (0.0, 1.0);
    }
    if maximum < minimum {
        (maximum, minimum)
    } else {
        (minimum, maximum)
    }
}

/// A controlled, unstyled slider descriptor.
///
/// The application owns the track, range fill, thumb, tick marks, labels, colors, and motion.
/// QuickGUI supplies stable part identities, the Slider role with numeric value/min/max/step and
/// orientation, captured pointer arithmetic, and typed keyboard actions.
///
/// A single-thumb slider projects the Slider role on its root. A multi-thumb slider projects a
/// group root and one Slider role per thumb, each bounded by its neighbors, matching the WAI-ARIA
/// multi-thumb pattern.
#[derive(Clone, Copy, Debug, PartialEq)]
#[must_use = "a Slider descriptor has no effect until its parts are mounted"]
pub struct Slider {
    root_id: ElementId,
    state: SliderState,
}

impl Slider {
    pub fn new(root_id: impl Into<ElementId>, state: &SliderState) -> Self {
        Self {
            root_id: root_id.into(),
            state: *state,
        }
    }

    pub const fn root_id(self) -> ElementId {
        self.root_id
    }

    pub const fn state(self) -> SliderState {
        self.state
    }

    pub fn track_id(self) -> ElementId {
        derived_slider_id(self.root_id, SLIDER_TRACK_ID_TAG, 0)
    }

    pub fn range_id(self) -> ElementId {
        derived_slider_id(self.root_id, SLIDER_RANGE_ID_TAG, 0)
    }

    pub fn thumb_id(self, index: usize) -> ElementId {
        derived_slider_id(self.root_id, SLIDER_THUMB_ID_TAG, index as u64)
    }

    /// Stable identity of the label part.
    pub fn label_id(self) -> ElementId {
        derived_slider_id(self.root_id, SLIDER_LABEL_ID_TAG, 0)
    }

    /// Stable identity of the value part.
    pub fn value_id(self) -> ElementId {
        derived_slider_id(self.root_id, SLIDER_VALUE_ID_TAG, 0)
    }

    /// Stable identity of the control part.
    pub fn control_id(self) -> ElementId {
        derived_slider_id(self.root_id, SLIDER_CONTROL_ID_TAG, 0)
    }

    /// Whether this slider projects its value on the root instead of on each thumb.
    pub const fn is_single_thumb(self) -> bool {
        self.state.thumbs == 1
    }

    /// Decorate an application-owned root without adding layout or appearance.
    ///
    /// A single-thumb slider becomes the focusable Slider itself. Pair it with
    /// [`Self::key_with`] to attach the typed keyboard actions.
    pub fn root_with(self, root: Element) -> Element {
        let disabled = self.state.disabled || root.accessibility.disabled;
        let root = root
            .id(self.root_id)
            .cursor_default()
            .app_region_no_drag()
            .user_select_none()
            .disabled(disabled);
        let root = root
            .accessibility_labelled_by(self.label_id())
            .accessibility_described_by(self.value_id());
        if self.is_single_thumb() {
            root.accessibility_role(AccessibilityRole::Slider)
                .accessibility_orientation(self.state.orientation.accessibility())
                .accessibility_value_range(
                    AccessibilityValueRange::new(
                        self.state.values[0],
                        self.state.minimum,
                        self.state.maximum,
                    )
                    .step(self.state.step),
                )
                .focusable()
                .tab_index(0)
                .key_context(SLIDER_KEY_CONTEXT)
        } else {
            root.accessibility_role(AccessibilityRole::Group)
                .accessibility_orientation(self.state.orientation.accessibility())
        }
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(self) -> Element {
        self.root_with(crate::div())
    }

    /// Attach QuickGUI's typed slider keyboard actions to a single-thumb root.
    ///
    /// Install [`slider_key_bindings`] once on the application keymap. Multi-thumb sliders use
    /// [`SliderThumb::key_with`] instead so each thumb owns its own focus.
    pub fn key_with<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        root: Element,
        access: fn(&mut V) -> &mut SliderState,
    ) -> Element {
        self.key_with_accessor(cx, root, StateAccessor::from(access))
    }
    /// Create the unstyled key part. Use [`Self::key_with`] to supply an existing element.
    pub fn key<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut SliderState,
    ) -> Element {
        self.key_with(cx, crate::div(), access)
    }

    /// Attach the typed slider keyboard actions against a per-instance state accessor.
    pub fn key_with_accessor<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        root: Element,
        access: StateAccessor<V, SliderState>,
    ) -> Element {
        bind_slider_actions(cx, root, self.root_id, &access, None)
    }

    /// Decorate an application-owned track.
    ///
    /// Base UI uses [`Self::control_with`] as the larger interactive surface. A declaration that
    /// omits Control can keep the earlier behavior by attaching a
    /// [`crate::ViewContext::pointer_listener`] registered for [`Self::track_id`] and forwarding
    /// the event to [`SliderState::apply_pointer`] with the size the caller laid out.
    pub fn track_with(self, track: Element) -> Element {
        track
            .id(self.track_id())
            .accessibility_hidden(true)
            .cursor_default()
            .app_region_no_drag()
            .user_select_none()
    }
    /// Create the unstyled track part. Use [`Self::track_with`] to supply an existing element.
    pub fn track(self) -> Element {
        self.track_with(crate::div())
    }

    /// Decorate the application-owned fill between the slider's minimum and its value.
    pub fn range_with(self, range: Element) -> Element {
        range.id(self.range_id()).accessibility_hidden(true)
    }
    /// Create the unstyled range part. Use [`Self::range_with`] to supply an existing element.
    pub fn range(self) -> Element {
        self.range_with(crate::div())
    }

    /// Base UI's Indicator name for the filled part of the track.
    ///
    /// This is the same decorator as [`Self::range_with`]; both names stay supported.
    pub fn indicator_with(self, indicator: Element) -> Element {
        self.range_with(indicator)
    }
    /// Create the unstyled indicator part. Use [`Self::indicator_with`] to supply an existing element.
    pub fn indicator(self) -> Element {
        self.indicator_with(crate::div())
    }

    /// Decorate the caller-owned interactive area that carries the slider's pointer capture.
    ///
    /// Base UI separates the Control — the region a press acts on — from the Track it paints.
    /// Attach a [`crate::ViewContext::pointer_listener`] registered for [`Self::control_id`] and
    /// forward the event to [`SliderState::apply_pointer_change`] with the size the caller laid
    /// out. A caller that omits this part can keep pointer capture on [`Self::track_with`].
    pub fn control_with(self, control: Element) -> Element {
        control
            .id(self.control_id())
            .accessibility_hidden(true)
            .cursor_default()
            .app_region_no_drag()
            .user_select_none()
    }
    /// Create the unstyled control part. Use [`Self::control_with`] to supply an existing element.
    pub fn control(self) -> Element {
        self.control_with(crate::div())
    }

    /// Assign the stable mounted label target the slider points at.
    ///
    /// A single-thumb slider takes its accessible name from this part. A multi-thumb slider names
    /// its group from it, and each thumb still needs its own accessible name.
    pub fn label_with(self, label: Element) -> Element {
        label.id(self.label_id())
    }
    /// Create the unstyled label part. Use [`Self::label_with`] to supply an existing element.
    pub fn label(self) -> Element {
        self.label_with(crate::div())
    }

    /// Assign the stable mounted value target the slider points at.
    ///
    /// Put [`Self::display_value`] inside it; QuickGUI never renders the text itself.
    pub fn value_with(self, value: Element) -> Element {
        value.id(self.value_id())
    }
    /// Create the unstyled value part. Use [`Self::value_with`] to supply an existing element.
    pub fn value(self) -> Element {
        self.value_with(crate::div())
    }

    /// Format every thumb value into one string for a caller-owned value part.
    ///
    /// This is Base UI's Root `format` prop applied through the shared [`crate::ValueFormat`].
    /// A range slider joins its thumbs with an en dash, which is the shape Base UI's Value renders
    /// by default.
    pub fn display_value(self, format: &crate::ValueFormat) -> Arc<str> {
        let mut text = String::new();
        for index in 0..self.state.thumbs {
            if index > 0 {
                text.push_str(" – ");
            }
            text.push_str(&format.apply(self.state.values[index], self.state.maximum));
        }
        Arc::from(text)
    }

    /// Describe one thumb.
    ///
    /// Indices at or past [`SliderState::thumb_count`] return `None` so a caller-driven loop stays
    /// bounded by the state instead of by its own arithmetic.
    pub fn thumb(self, index: usize) -> Option<SliderThumb> {
        let (lower, upper) = self.state.thumb_bounds(index)?;
        Some(SliderThumb {
            slider: self,
            index,
            value: self.state.values[index],
            lower,
            upper,
        })
    }
}

/// A copyable declaration for one slider thumb.
#[derive(Clone, Copy, Debug, PartialEq)]
#[must_use = "a SliderThumb descriptor has no effect until its part is mounted"]
pub struct SliderThumb {
    slider: Slider,
    index: usize,
    value: f64,
    lower: f64,
    upper: f64,
}

impl SliderThumb {
    pub const fn index(self) -> usize {
        self.index
    }

    pub const fn value(self) -> f64 {
        self.value
    }

    /// The inclusive bounds this thumb may move between, honoring its neighbors.
    pub const fn bounds(self) -> (f64, f64) {
        (self.lower, self.upper)
    }

    /// The `0.0..=1.0` position of this thumb along the track.
    pub fn fraction(self) -> f32 {
        self.slider.state.fraction_of(self.value)
    }

    pub const fn is_active(self) -> bool {
        self.slider.state.active == self.index
    }

    pub fn thumb_id(self) -> ElementId {
        self.slider.thumb_id(self.index)
    }

    /// Whether a captured pointer is currently dragging this slider, Base UI's `data-dragging`.
    pub const fn is_dragging(self) -> bool {
        self.slider.state.dragging
    }

    /// A copyable render-state snapshot the application styles this thumb from.
    pub fn state(self) -> SliderThumbState {
        SliderThumbState {
            index: self.index,
            value: self.value,
            fraction: self.fraction(),
            active: self.is_active(),
            dragging: self.slider.state.dragging,
            disabled: self.slider.state.disabled,
        }
    }

    /// The leading-edge offset of this thumb along the track, honoring the thumb alignment.
    ///
    /// `track_length` and `thumb_length` are measured along the slider's own axis. This is the one
    /// place Base UI's `thumbAlignment` changes anything: `Center` lets the thumb overhang both
    /// ends, `Edge` keeps its box inside the track.
    pub fn offset(self, track_length: f32, thumb_length: f32) -> f32 {
        self.slider
            .state
            .thumb_offset(self.index, track_length, thumb_length)
    }

    /// Format this thumb's value, Base UI's Thumb `getAriaValueText` equivalent.
    ///
    /// Pass the result to `Element::accessibility_value` on the thumb; QuickGUI never renders it.
    pub fn value_text(self, format: &crate::ValueFormat) -> Arc<str> {
        format.apply(self.value, self.slider.state.maximum)
    }

    /// Decorate an application-owned thumb without adding layout or appearance.
    ///
    /// A single-thumb slider keeps its value on the root, so its thumb is decorative and hidden
    /// from assistive technology. Every thumb of a multi-thumb slider is an independently
    /// focusable Slider bounded by its neighbors.
    pub fn thumb_with(self, thumb: Element) -> Element {
        let disabled = self.slider.state.disabled || thumb.accessibility.disabled;
        let thumb = thumb.id(self.thumb_id());
        if self.slider.is_single_thumb() {
            return thumb.accessibility_hidden(true);
        }
        thumb
            .accessibility_role(AccessibilityRole::Slider)
            .accessibility_orientation(self.slider.state.orientation.accessibility())
            .accessibility_value_range(
                AccessibilityValueRange::new(self.value, self.lower, self.upper)
                    .step(self.slider.state.step),
            )
            .focusable()
            .tab_index(0)
            .key_context(SLIDER_KEY_CONTEXT)
            .cursor_default()
            .app_region_no_drag()
            .user_select_none()
            .disabled(disabled)
    }
    /// Create the unstyled thumb part. Use [`Self::thumb_with`] to supply an existing element.
    pub fn thumb(self) -> Element {
        self.thumb_with(crate::div())
    }

    /// Attach QuickGUI's typed slider keyboard actions to this thumb.
    ///
    /// Focusing the thumb also makes it the active thumb, so arrows move the thumb the user sees.
    pub fn key_with<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        thumb: Element,
        access: fn(&mut V) -> &mut SliderState,
    ) -> Element {
        self.key_with_accessor(cx, thumb, StateAccessor::from(access))
    }
    /// Create the unstyled key part. Use [`Self::key_with`] to supply an existing element.
    pub fn key<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut SliderState,
    ) -> Element {
        self.key_with(cx, crate::div(), access)
    }

    /// Attach this thumb's typed keyboard actions against a per-instance state accessor.
    pub fn key_with_accessor<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        thumb: Element,
        access: StateAccessor<V, SliderState>,
    ) -> Element {
        bind_slider_actions(cx, thumb, self.thumb_id(), &access, Some(self.index))
    }
}

fn activate_slider_thumb<V: 'static>(
    view: &mut V,
    access: &StateAccessor<V, SliderState>,
    thumb: Option<usize>,
) {
    if let Some(index) = thumb {
        access.get(view).set_active_thumb(index);
    }
}

fn bind_slider_actions<V: 'static>(
    cx: &mut ViewContext<'_, V>,
    element: Element,
    id: ElementId,
    access_source: &StateAccessor<V, SliderState>,
    thumb: Option<usize>,
) -> Element {
    let access = access_source.clone();
    let increment = cx.action_listener(id, move |view, _: &SliderIncrement, cx| {
        activate_slider_thumb(view, &access, thumb);
        if access.get(view).step_active(1.0) {
            cx.invalidate();
        }
    });
    let access = access_source.clone();
    let decrement = cx.action_listener(id, move |view, _: &SliderDecrement, cx| {
        activate_slider_thumb(view, &access, thumb);
        if access.get(view).step_active(-1.0) {
            cx.invalidate();
        }
    });
    let access = access_source.clone();
    let large_increment = cx.action_listener(id, move |view, _: &SliderLargeIncrement, cx| {
        activate_slider_thumb(view, &access, thumb);
        if access.get(view).large_step_active(true) {
            cx.invalidate();
        }
    });
    let access = access_source.clone();
    let large_decrement = cx.action_listener(id, move |view, _: &SliderLargeDecrement, cx| {
        activate_slider_thumb(view, &access, thumb);
        if access.get(view).large_step_active(false) {
            cx.invalidate();
        }
    });
    let access = access_source.clone();
    let minimum = cx.action_listener(id, move |view, _: &SliderMinimum, cx| {
        activate_slider_thumb(view, &access, thumb);
        if access.get(view).active_to_minimum() {
            cx.invalidate();
        }
    });
    let access = access_source.clone();
    let maximum = cx.action_listener(id, move |view, _: &SliderMaximum, cx| {
        activate_slider_thumb(view, &access, thumb);
        if access.get(view).active_to_maximum() {
            cx.invalidate();
        }
    });
    element
        .on_action(increment)
        .on_action(decrement)
        .on_action(large_increment)
        .on_action(large_decrement)
        .on_action(minimum)
        .on_action(maximum)
}

/// Create an unstyled controlled single-thumb slider root.
///
/// This shorthand is equivalent to `Slider::new(id, state).root_with(div())`.
pub fn slider(id: impl Into<ElementId>, state: &SliderState) -> Element {
    Slider::new(id, state).root_with(div())
}

fn derived_slider_id(scope: ElementId, tag: u64, index: u64) -> ElementId {
    let mut hash = scope
        .as_u64()
        .rotate_left(23)
        .wrapping_add(index.rotate_right(11))
        ^ tag;
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    if hash == 0 || hash == u64::MAX || hash == scope.as_u64() {
        hash ^= tag.rotate_left(31);
    }
    ElementId::new(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppRegion, Application, Color, CursorStyle, IntoElement, Modifiers, MouseButton, Point,
        UserSelect, Vector, View, WindowOptions, text,
    };

    fn pointer_event(phase: PointerPhase, x: f32, y: f32) -> PointerEvent {
        PointerEvent {
            size: Size::new(200.0, 20.0),
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

    #[test]
    fn captured_pointer_selects_then_drags_one_thumb() {
        let mut state = SliderState::range(0.0, 100.0, &[20.0, 80.0]).step(10.0);
        let track = Size::new(200.0, 20.0);

        assert!(state.apply_pointer(&pointer_event(PointerPhase::Down, 150.0, 10.0), track));
        assert_eq!(state.active_thumb(), 1);
        assert_eq!(state.values(), &[20.0, 80.0]);

        assert!(state.apply_pointer(&pointer_event(PointerPhase::Move, 190.0, 10.0), track));
        assert_eq!(state.values(), &[20.0, 100.0]);

        // Capture continues outside the track and stays clamped by the lower thumb.
        assert!(state.apply_pointer(&pointer_event(PointerPhase::Move, -400.0, 10.0), track));
        assert_eq!(state.values(), &[20.0, 20.0]);
        assert!(!state.apply_pointer(&pointer_event(PointerPhase::Up, -400.0, 10.0), track));

        let mut vertical = SliderState::new(0.0, 100.0, 0.0).step(10.0).vertical();
        // Pressing the bottom of a vertical track is already the minimum, so nothing changes.
        assert!(!vertical.apply_pointer(&pointer_event(PointerPhase::Down, 5.0, 20.0), track));
        assert_eq!(vertical.value(), 0.0);
        assert!(vertical.apply_pointer(&pointer_event(PointerPhase::Down, 5.0, 5.0), track));
        assert_eq!(vertical.value(), 80.0);

        let mut disabled = SliderState::new(0.0, 1.0, 0.5).disabled(true);
        assert!(!disabled.apply_pointer(&pointer_event(PointerPhase::Down, 200.0, 0.0), track));
        assert!(!state.apply_pointer(&pointer_event(PointerPhase::Cancel, 0.0, 0.0), track));
    }

    #[test]
    fn state_snaps_clamps_and_orders_bounded_values() {
        let mut state = SliderState::new(0.0, 100.0, 42.0).step(5.0);
        assert_eq!(state.minimum(), 0.0);
        assert_eq!(state.maximum(), 100.0);
        assert_eq!(state.value(), 40.0);
        assert!(state.set_value(97.0));
        assert_eq!(state.value(), 95.0);
        assert!(!state.set_value(96.0));
        assert!(state.set_value(1_000.0));
        assert_eq!(state.value(), 100.0);
        assert!(state.set_value(f64::NAN));
        assert_eq!(state.value(), 0.0);

        assert!(state.step_active(1.0));
        assert_eq!(state.value(), 5.0);
        assert!(state.large_step_active(true));
        assert_eq!(state.value(), 55.0);
        assert!(state.large_step_active(false));
        assert_eq!(state.value(), 5.0);
        assert!(state.active_to_maximum());
        assert_eq!(state.value(), 100.0);
        assert!(state.active_to_minimum());
        assert_eq!(state.value(), 0.0);
        assert!(!state.active_to_minimum());

        let inverted = SliderState::new(10.0, -10.0, 0.0);
        assert_eq!(inverted.minimum(), -10.0);
        assert_eq!(inverted.maximum(), 10.0);
        let broken = SliderState::new(f64::NAN, f64::INFINITY, 0.5);
        assert_eq!((broken.minimum(), broken.maximum()), (0.0, 1.0));

        let mut range = SliderState::range(0.0, 10.0, &[8.0, 2.0]);
        assert_eq!(range.thumb_count(), 2);
        assert_eq!(range.values(), &[8.0, 8.0]);
        assert!(range.set_thumb_value(0, 3.0));
        assert_eq!(range.values(), &[3.0, 8.0]);
        assert!(range.set_thumb_value(1, 1.0));
        assert_eq!(range.values(), &[3.0, 3.0]);
        assert_eq!(range.thumb_bounds(0), Some((0.0, 3.0)));
        assert_eq!(range.thumb_bounds(1), Some((3.0, 10.0)));
        assert_eq!(range.thumb_bounds(2), None);
        assert!(!range.set_thumb_value(2, 5.0));
        assert!(range.set_active_thumb(1));
        assert!(!range.set_active_thumb(1));
        assert!(!range.set_active_thumb(9));
        assert_eq!(range.active_thumb(), 1);

        let disabled = &mut SliderState::new(0.0, 1.0, 0.5).disabled(true);
        assert!(!disabled.set_value(0.75));
        assert!(disabled.is_disabled());
    }

    #[test]
    fn pointer_geometry_is_orientation_aware_and_bounded() {
        let horizontal = SliderState::new(0.0, 200.0, 0.0).step(0.0);
        assert_eq!(horizontal.value_at(50.0, 100.0), 100.0);
        assert_eq!(horizontal.value_at(-40.0, 100.0), 0.0);
        assert_eq!(horizontal.value_at(400.0, 100.0), 200.0);
        assert_eq!(horizontal.value_at(50.0, 0.0), 0.0);
        assert_eq!(horizontal.fraction_of(50.0), 0.25);

        let vertical = SliderState::new(0.0, 200.0, 0.0).step(0.0).vertical();
        assert_eq!(vertical.value_at(0.0, 100.0), 200.0);
        assert_eq!(vertical.value_at(100.0, 100.0), 0.0);
        assert_eq!(vertical.value_at(25.0, 100.0), 150.0);

        let degenerate = SliderState::new(5.0, 5.0, 5.0);
        assert_eq!(degenerate.fraction(0), 0.0);
        assert_eq!(degenerate.value_for_fraction(1.0), 5.0);
        assert_eq!(degenerate.value_for_fraction(f32::NAN), 5.0);
    }

    #[test]
    fn parts_add_exact_semantics_without_appearance() {
        let state = SliderState::new(0.0, 10.0, 4.0).step(2.0);
        let slider = Slider::new("volume", &state);
        let root = slider.root_with(div().w(240.0).bg(Color::rgb8(1, 2, 3)));
        assert_eq!(root.explicit_id, Some("volume".into()));
        assert_eq!(root.accessibility.role, AccessibilityRole::Slider);
        assert_eq!(
            root.accessibility.orientation,
            Some(AccessibilityOrientation::Horizontal)
        );
        assert_eq!(
            root.accessibility.value_range.as_deref(),
            Some(&AccessibilityValueRange::new(4.0, 0.0, 10.0).step(2.0))
        );
        assert!(root.focusable);
        assert_eq!(root.tab_index, 0);
        assert_eq!(root.cursor_style, Some(CursorStyle::Arrow));
        assert_eq!(root.app_region, Some(AppRegion::NoDrag));
        assert_eq!(root.user_select, UserSelect::None);
        assert_eq!(root.visual.background, Some(Color::rgb8(1, 2, 3)));
        assert!(root.transition.is_none());

        let track = slider.track_with(div().h(4.0).bg(Color::rgb8(4, 5, 6)));
        assert_eq!(track.explicit_id, Some(slider.track_id()));
        assert!(track.accessibility.hidden);
        assert_eq!(track.visual.background, Some(Color::rgb8(4, 5, 6)));

        let range = slider.range_with(div().bg(Color::rgb8(7, 8, 9)));
        assert_eq!(range.explicit_id, Some(slider.range_id()));
        assert!(range.accessibility.hidden);

        let thumb = slider.thumb(0).expect("first thumb");
        assert_eq!(thumb.value(), 4.0);
        assert_eq!(thumb.fraction(), 0.4);
        assert!(thumb.is_active());
        let thumb_element = thumb.thumb_with(div().size(12.0, 12.0));
        assert_eq!(thumb_element.explicit_id, Some(slider.thumb_id(0)));
        assert!(thumb_element.accessibility.hidden);
        assert!(slider.thumb(1).is_none());

        let ids = [
            slider.root_id(),
            slider.track_id(),
            slider.range_id(),
            slider.thumb_id(0),
            slider.thumb_id(1),
        ];
        for (index, id) in ids.iter().enumerate() {
            assert!(!ids[..index].contains(id));
        }

        let range_state = SliderState::range(0.0, 100.0, &[20.0, 80.0]).step(10.0);
        let range_slider = Slider::new("price", &range_state);
        let range_root = range_slider.root_with(div());
        assert_eq!(range_root.accessibility.role, AccessibilityRole::Group);
        assert!(!range_root.focusable);
        let lower = range_slider.thumb(0).expect("lower thumb");
        let lower_element = lower.thumb_with(div());
        assert_eq!(lower_element.accessibility.role, AccessibilityRole::Slider);
        assert_eq!(
            lower_element.accessibility.value_range.as_deref(),
            Some(&AccessibilityValueRange::new(20.0, 0.0, 80.0).step(10.0))
        );
        assert!(lower_element.focusable);
        let upper = range_slider.thumb(1).expect("upper thumb");
        assert_eq!(upper.bounds(), (20.0, 100.0));
        assert!(!upper.is_active());

        let disabled_state = SliderState::new(0.0, 1.0, 0.5).disabled(true);
        let disabled = Slider::new("muted", &disabled_state).root_with(div());
        assert!(disabled.accessibility.disabled);
    }

    #[derive(Default)]
    struct SliderView {
        volume: SliderState,
        price: SliderState,
    }

    impl SliderView {
        fn build() -> Self {
            Self {
                volume: SliderState::new(0.0, 100.0, 40.0).step(5.0),
                price: SliderState::range(0.0, 100.0, &[20.0, 80.0]).step(10.0),
            }
        }

        fn volume(view: &mut Self) -> &mut SliderState {
            &mut view.volume
        }

        fn price(view: &mut Self) -> &mut SliderState {
            &mut view.price
        }
    }

    impl View for SliderView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let volume = Slider::new("volume", &self.volume);
            let volume_root = volume.key_with(cx, volume.root_with(div()), Self::volume);
            let drag = cx.pointer_listener(volume.track_id(), |view, event, cx| {
                if view.volume.apply_pointer(event, Size::new(200.0, 20.0)) {
                    cx.invalidate();
                }
            });
            let volume_thumb = volume.thumb(0).expect("volume thumb");

            let price = Slider::new("price", &self.price);
            let lower = price.thumb(0).expect("lower thumb");
            let upper = price.thumb(1).expect("upper thumb");
            let lower_element = lower.key_with(cx, lower.thumb_with(div()), Self::price);
            let upper_element = upper.key_with(cx, upper.thumb_with(div()), Self::price);

            div()
                .child(text("Slider gallery"))
                .child(
                    volume_root.child(
                        volume
                            .track_with(div().w(200.0).h(20.0).on_pointer(drag))
                            .child(volume.range_with(div()))
                            .child(volume_thumb.thumb_with(div())),
                    ),
                )
                .child(
                    price
                        .root_with(div())
                        .child(price.track_with(div().w(200.0).h(20.0)))
                        .child(lower_element)
                        .child(upper_element),
                )
        }
    }

    #[test]
    fn keyboard_pointer_and_accessibility_paths_stay_deterministic() {
        let (mut cx, view) = Application::new()
            .bind_keys(slider_key_bindings())
            .into_test_context(WindowOptions::default(), SliderView::build())
            .unwrap();
        let window = view.window_handle();
        let volume = Slider::new("volume", &SliderState::new(0.0, 100.0, 40.0));
        let price = Slider::new("price", &SliderState::range(0.0, 100.0, &[20.0, 80.0]));

        cx.focus(window, volume.root_id()).unwrap();
        cx.simulate_keystrokes(window, "right").unwrap();
        assert_eq!(cx.read(view, |view| view.volume.value()).unwrap(), 45.0);
        cx.simulate_keystrokes(window, "left left").unwrap();
        assert_eq!(cx.read(view, |view| view.volume.value()).unwrap(), 35.0);
        cx.simulate_keystrokes(window, "pageup").unwrap();
        assert_eq!(cx.read(view, |view| view.volume.value()).unwrap(), 85.0);
        cx.simulate_keystrokes(window, "shift-down").unwrap();
        assert_eq!(cx.read(view, |view| view.volume.value()).unwrap(), 35.0);
        cx.simulate_keystrokes(window, "home").unwrap();
        assert_eq!(cx.read(view, |view| view.volume.value()).unwrap(), 0.0);
        cx.simulate_keystrokes(window, "end").unwrap();
        assert_eq!(cx.read(view, |view| view.volume.value()).unwrap(), 100.0);

        cx.focus(window, price.thumb_id(1)).unwrap();
        cx.simulate_keystrokes(window, "right").unwrap();
        assert_eq!(
            cx.read(view, |view| view.price.values().to_vec()).unwrap(),
            vec![20.0, 90.0]
        );
        cx.simulate_keystrokes(window, "home").unwrap();
        assert_eq!(
            cx.read(view, |view| view.price.values().to_vec()).unwrap(),
            vec![20.0, 20.0]
        );

        let update = cx.accessibility_update(window).unwrap();
        let node = |id: ElementId| {
            update
                .nodes
                .iter()
                .find_map(|(node_id, node)| (node_id.0 == id.as_u64()).then_some(node))
                .expect("slider accessibility node")
        };
        let root = node(volume.root_id());
        assert_eq!(root.role(), accesskit::Role::Slider);
        assert_eq!(root.numeric_value(), Some(100.0));
        assert_eq!(root.min_numeric_value(), Some(0.0));
        assert_eq!(root.max_numeric_value(), Some(100.0));
        assert_eq!(root.numeric_value_step(), Some(5.0));
        assert_eq!(root.orientation(), Some(accesskit::Orientation::Horizontal));
        let upper = node(price.thumb_id(1));
        assert_eq!(upper.role(), accesskit::Role::Slider);
        assert_eq!(upper.numeric_value(), Some(20.0));
        assert_eq!(upper.min_numeric_value(), Some(20.0));
        assert_eq!(upper.max_numeric_value(), Some(100.0));

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }

    /// One view that renders a data-driven list of sliders through the accessor entry point.
    ///
    /// This is exactly the shape a host renderer has: one `View` implementation, many declared
    /// instances, and no way to hand each one a distinct non-capturing `fn` pointer.
    struct SliderListView {
        sliders: Vec<SliderState>,
    }

    impl SliderListView {
        fn slider_id(index: usize) -> ElementId {
            ElementId::new(index as u64 + 1)
        }
    }

    impl View for SliderListView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let mut root = div();
            for index in 0..self.sliders.len() {
                let state = self.sliders[index];
                let slider = Slider::new(Self::slider_id(index), &state);
                root = root.child(slider.key_with_accessor(
                    cx,
                    slider.root_with(div()),
                    StateAccessor::new(move |view: &mut Self| &mut view.sliders[index]),
                ));
            }
            root
        }
    }

    #[test]
    fn per_instance_accessors_keep_declared_instances_independent() {
        let (mut cx, view) = Application::new()
            .bind_keys(slider_key_bindings())
            .into_test_context(
                WindowOptions::default(),
                SliderListView {
                    sliders: vec![
                        SliderState::new(0.0, 100.0, 10.0).step(5.0),
                        SliderState::new(0.0, 100.0, 60.0).step(5.0),
                        SliderState::new(0.0, 100.0, 90.0).step(5.0),
                    ],
                },
            )
            .unwrap();
        let window = view.window_handle();
        let values = |cx: &mut crate::TestAppContext| {
            cx.read(view, |view| {
                view.sliders
                    .iter()
                    .map(SliderState::value)
                    .collect::<Vec<_>>()
            })
            .unwrap()
        };

        cx.focus(window, SliderListView::slider_id(0)).unwrap();
        cx.simulate_keystrokes(window, "right right").unwrap();
        assert_eq!(values(&mut cx), vec![20.0, 60.0, 90.0]);

        cx.focus(window, SliderListView::slider_id(2)).unwrap();
        cx.simulate_keystrokes(window, "left").unwrap();
        assert_eq!(values(&mut cx), vec![20.0, 60.0, 85.0]);

        cx.focus(window, SliderListView::slider_id(1)).unwrap();
        cx.simulate_keystrokes(window, "home").unwrap();
        assert_eq!(values(&mut cx), vec![20.0, 0.0, 85.0]);

        // The accessor adds no idle source: a settled window still renders nothing extra.
        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }

    #[test]
    fn bindings_are_contextual_and_complete() {
        let bindings = slider_key_bindings();
        assert_eq!(bindings.len(), 14);
        assert!(
            bindings.iter().all(
                |binding| binding.context_predicate().is_some_and(|context| context
                    .depth_of(&[crate::KeyContext::parse(SLIDER_KEY_CONTEXT).unwrap()])
                    .is_some())
            )
        );
    }

    #[test]
    #[should_panic(expected = "a slider needs at least one thumb")]
    fn empty_range_is_rejected() {
        let _ = SliderState::range(0.0, 1.0, &[]);
    }

    #[test]
    #[should_panic(expected = "at most")]
    fn too_many_thumbs_are_rejected() {
        let _ = SliderState::range(0.0, 1.0, &[0.0; MAX_SLIDER_THUMBS + 1]);
    }

    #[test]
    fn shorthand_is_a_semantic_unstyled_root() {
        let state = SliderState::new(0.0, 1.0, 0.25);
        let element = slider("brightness", &state);
        assert_eq!(element.accessibility.role, AccessibilityRole::Slider);
        assert!(element.children.is_empty());
        assert_eq!(element.visual.background, None);
    }

    #[test]
    fn minimum_steps_between_values_hold_a_gap_open_in_both_directions() {
        let state = SliderState::range(0.0, 100.0, &[10.0, 20.0, 30.0])
            .step(5.0)
            .min_steps_between_values(4);
        assert_eq!(state.min_steps_between_values_value(), 4);
        // The declared values are re-ordered outward from the first thumb to open the gap.
        assert_eq!(state.values(), &[10.0, 30.0, 50.0]);
        assert_eq!(state.thumb_bounds(0), Some((0.0, 10.0)));
        assert_eq!(state.thumb_bounds(1), Some((30.0, 30.0)));
        assert_eq!(state.thumb_bounds(2), Some((50.0, 100.0)));

        let mut state = SliderState::range(0.0, 100.0, &[0.0, 50.0])
            .step(5.0)
            .min_steps_between_values(2);
        assert!(state.set_thumb_value(0, 100.0));
        assert_eq!(state.values(), &[40.0, 50.0]);
        // The second thumb is already sitting on its own floor, so it cannot move down at all.
        assert!(!state.set_thumb_value(1, 0.0));
        assert_eq!(state.values(), &[40.0, 50.0]);

        // A continuous slider has no step to count, so the gap is inert.
        let continuous = SliderState::range(0.0, 100.0, &[20.0, 40.0])
            .step(f64::NAN)
            .min_steps_between_values(3);
        assert_eq!(continuous.step_value(), 0.0);
        assert_eq!(continuous.thumb_bounds(0), Some((0.0, 40.0)));
        assert_eq!(continuous.thumb_bounds(1), Some((20.0, 100.0)));

        // The bound is retained rather than trusted.
        assert_eq!(
            SliderState::new(0.0, 1.0, 0.0)
                .min_steps_between_values(usize::MAX)
                .min_steps_between_values_value(),
            u16::MAX as usize
        );
    }

    #[test]
    fn thumb_alignment_only_changes_the_offset_the_application_lays_out() {
        let centered = SliderState::new(0.0, 100.0, 25.0);
        assert_eq!(
            centered.thumb_alignment_value(),
            SliderThumbAlignment::Center
        );
        // 0.25 * 200 - 16/2
        assert_eq!(centered.thumb_offset(0, 200.0, 16.0), 42.0);
        assert_eq!(centered.thumb_offset(0, 200.0, 0.0), 50.0);

        let edged = centered.thumb_alignment(SliderThumbAlignment::Edge);
        // 0.25 * (200 - 16)
        assert_eq!(edged.thumb_offset(0, 200.0, 16.0), 46.0);
        assert_eq!(edged.value(), centered.value());
        assert_eq!(edged.thumb_bounds(0), centered.thumb_bounds(0));

        // A vertical slider runs its offset downward, so the minimum sits at the bottom.
        let vertical = SliderState::new(0.0, 100.0, 25.0).vertical();
        assert_eq!(vertical.thumb_offset(0, 200.0, 0.0), 150.0);
        let vertical_edge = vertical.thumb_alignment(SliderThumbAlignment::Edge);
        assert_eq!(vertical_edge.thumb_offset(0, 200.0, 20.0), 135.0);

        // Non-finite geometry never escapes into layout.
        assert_eq!(centered.thumb_offset(0, f32::NAN, 16.0), -8.0);
        assert_eq!(edged.thumb_offset(0, 10.0, 40.0), 0.0);

        let slider = Slider::new("range", &SliderState::range(0.0, 100.0, &[20.0, 80.0]));
        let thumb = slider.thumb(1).expect("second thumb");
        assert_eq!(thumb.offset(200.0, 10.0), 155.0);
    }

    #[test]
    fn a_captured_drag_reports_its_commit_boundary_and_dragging_state() {
        let track = Size::new(200.0, 20.0);
        let mut state = SliderState::new(0.0, 100.0, 0.0).step(1.0);
        assert!(!state.is_dragging());

        let down =
            state.apply_pointer_change(&pointer_event(PointerPhase::Down, 100.0, 10.0), track);
        assert!(down.changed);
        assert!(down.values_changed);
        assert!(!down.committed);
        assert!(state.is_dragging());
        assert_eq!(state.value(), 50.0);

        let moved =
            state.apply_pointer_change(&pointer_event(PointerPhase::Move, 150.0, 10.0), track);
        assert!(moved.values_changed);
        assert!(!moved.committed);
        assert!(state.is_dragging());
        assert_eq!(state.value(), 75.0);

        // Releasing without moving still commits, and it is the only event that does.
        let up = state.apply_pointer_change(&pointer_event(PointerPhase::Up, 150.0, 10.0), track);
        assert!(!up.values_changed);
        assert!(up.changed);
        assert!(up.committed);
        assert!(!state.is_dragging());

        let cancelled = {
            let mut state = SliderState::new(0.0, 100.0, 0.0);
            state.apply_pointer_change(&pointer_event(PointerPhase::Down, 100.0, 10.0), track);
            state.apply_pointer_change(&pointer_event(PointerPhase::Cancel, 100.0, 10.0), track)
        };
        assert!(cancelled.committed);

        // A disabled slider never drags and never commits.
        let mut disabled = SliderState::new(0.0, 100.0, 10.0).disabled(true);
        let change =
            disabled.apply_pointer_change(&pointer_event(PointerPhase::Down, 100.0, 10.0), track);
        assert_eq!(change, SliderPointerChange::default());
        assert!(!disabled.is_dragging());

        let mut ended = SliderState::new(0.0, 1.0, 0.0);
        ended.apply_pointer_change(&pointer_event(PointerPhase::Down, 10.0, 10.0), track);
        assert!(ended.end_drag());
        assert!(!ended.end_drag());
    }

    #[test]
    fn base_ui_slider_parts_carry_stable_identities_relations_and_no_appearance() {
        let mut state = SliderState::range(0.0, 100.0, &[20.0, 80.0]).step(1.0);
        state.apply_pointer_change(
            &pointer_event(PointerPhase::Down, 40.0, 10.0),
            Size::new(200.0, 20.0),
        );
        let slider = Slider::new("volume", &state);

        let ids = [
            slider.label_id(),
            slider.value_id(),
            slider.control_id(),
            slider.track_id(),
            slider.range_id(),
            slider.thumb_id(0),
            slider.thumb_id(1),
        ];
        for (index, id) in ids.iter().enumerate() {
            assert_ne!(*id, slider.root_id());
            assert!(!ids[..index].contains(id));
        }

        let root = slider.root_with(div());
        assert_eq!(
            root.accessibility.relations.labelled_by(),
            Some(slider.label_id())
        );
        assert_eq!(
            root.accessibility.relations.described_by(),
            Some(slider.value_id())
        );
        assert_eq!(root.visual.background, None);

        let control = slider.control_with(div().bg(Color::rgb8(1, 2, 3)));
        assert_eq!(control.explicit_id, Some(slider.control_id()));
        assert!(control.accessibility.hidden);
        assert_eq!(control.app_region, Some(AppRegion::NoDrag));
        assert_eq!(control.visual.background, Some(Color::rgb8(1, 2, 3)));
        assert_eq!(control.user_select, UserSelect::None);

        // Indicator is Base UI's name for the existing range part.
        assert_eq!(
            slider.indicator_with(div()).explicit_id,
            Some(slider.range_id())
        );
        assert!(slider.indicator_with(div()).accessibility.hidden);
        assert_eq!(
            slider.label_with(text("Volume")).explicit_id,
            Some(slider.label_id())
        );
        assert_eq!(
            slider.value_with(text("20 – 80")).explicit_id,
            Some(slider.value_id())
        );

        let format = crate::ValueFormat::percent();
        assert_eq!(slider.display_value(&format).as_ref(), "20% – 80%");
        let single = Slider::new("one", &SliderState::new(0.0, 100.0, 30.0));
        assert_eq!(single.display_value(&format).as_ref(), "30%");

        let thumb = slider.thumb(1).expect("second thumb");
        assert_eq!(thumb.value_text(&format).as_ref(), "80%");
        assert_eq!(
            thumb.state(),
            SliderThumbState {
                index: 1,
                value: 80.0,
                fraction: 0.8,
                active: false,
                dragging: true,
                disabled: false,
            }
        );
        assert!(thumb.is_dragging());
        let active = slider.thumb(0).expect("first thumb");
        assert!(active.state().active);
        assert_eq!(active.state().index, 0);
    }
}
