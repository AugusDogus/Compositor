use std::{fmt, sync::Arc};

use crate::{AccessibilityRole, AccessibilityValueRange, Element, ElementId, div};

const PROGRESS_LABEL_ID_TAG: u64 = 0x2f81_bd06_5c39_7ae4;
const PROGRESS_VALUE_ID_TAG: u64 = 0x9e40_37cc_18b5_d26f;
const PROGRESS_TRACK_ID_TAG: u64 = 0x6ad2_5f13_e708_49bb;
const PROGRESS_INDICATOR_ID_TAG: u64 = 0xb174_c9e2_3a56_08fd;

/// How far along a task one [`Progress`] indicator reports being.
///
/// Base UI publishes the same three states as `data-progressing`, `data-complete`, and
/// `data-indeterminate`. QuickGUI has no style sheet, so the application reads the value and
/// chooses its own presentation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProgressStatus {
    /// A determinate indicator that has not reached its maximum.
    #[default]
    Progressing,
    /// A determinate indicator that reached its maximum.
    Complete,
    /// Work whose completion is unknown.
    Indeterminate,
}

impl ProgressStatus {
    pub const fn is_progressing(self) -> bool {
        matches!(self, Self::Progressing)
    }

    pub const fn is_complete(self) -> bool {
        matches!(self, Self::Complete)
    }

    pub const fn is_indeterminate(self) -> bool {
        matches!(self, Self::Indeterminate)
    }
}

/// A copyable render-state snapshot for one unstyled progress indicator.
///
/// Build one with [`Progress::state`]. It carries what Base UI exposes as `data-*` attributes so an
/// application can style a track, fill, label, and value without re-deriving them from the numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProgressPartState {
    /// Which of the three Base UI states this indicator is in.
    pub status: ProgressStatus,
    /// The clamped value, or `None` while indeterminate.
    pub value: Option<f64>,
    /// The upper bound of the range.
    pub maximum: f64,
    /// The `0.0..=1.0` completion fraction, or `None` while indeterminate.
    pub completion: Option<f32>,
}

/// A bounded value formatter shared by [`Progress`] and [`Meter`].
///
/// This is QuickGUI's counterpart of Base UI's `format` prop. QuickGUI never renders the result
/// itself: the application puts it inside its own `value_with`, and assistive technology reads it
/// unless an explicit [`Progress::value_text`] overrides it, which is Base UI's `getAriaValueText`
/// precedence.
#[derive(Clone)]
pub struct ValueFormat(Arc<dyn Fn(f64, f64) -> Arc<str>>);

impl ValueFormat {
    /// Build a formatter from `value` and the range's upper bound.
    pub fn new(format: impl Fn(f64, f64) -> Arc<str> + 'static) -> Self {
        Self(Arc::new(format))
    }

    /// Format a whole-number percentage of the range, the Base UI default shape.
    pub fn percent() -> Self {
        Self::new(|value, maximum| {
            let ratio = if maximum > 0.0 { value / maximum } else { 0.0 };
            Arc::from(format!("{:.0}%", (ratio * 100.0).clamp(0.0, 100.0)))
        })
    }

    /// Format the raw value against its bound, such as `"3 of 12"`.
    pub fn fraction() -> Self {
        Self::new(|value, maximum| Arc::from(format!("{value:.0} of {maximum:.0}")))
    }

    /// Apply the formatter.
    pub fn apply(&self, value: f64, maximum: f64) -> Arc<str> {
        (self.0)(value, maximum)
    }
}

impl fmt::Debug for ValueFormat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ValueFormat")
            .finish_non_exhaustive()
    }
}

impl PartialEq for ValueFormat {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Determinate or indeterminate task-completion semantics for one unstyled progress indicator.
///
/// The application owns the track, fill geometry, colors, radii, label, and any motion. QuickGUI
/// supplies the progress role, exact numeric value and bounds, and an accessibility-hidden
/// indicator part. QuickGUI never animates an indeterminate indicator: a moving barber pole is
/// product motion, and a framework-owned animation would keep an otherwise settled window awake.
///
/// The descriptor retains no allocation beyond one optional shared value string, and no task,
/// timer, observer, or idle scheduler source.
#[derive(Clone, Debug, PartialEq)]
#[must_use = "a Progress descriptor has no effect until one of its parts is mounted"]
pub struct Progress {
    id: Option<ElementId>,
    value: Option<f64>,
    maximum: f64,
    value_text: Option<Arc<str>>,
    format: Option<ValueFormat>,
}

impl Progress {
    /// Create a determinate indicator between `0.0` and `maximum`.
    ///
    /// A non-finite or non-positive maximum falls back to `1.0`; the value is clamped into range.
    pub fn new(value: f64, maximum: f64) -> Self {
        let maximum = normalized_maximum(maximum);
        Self {
            id: None,
            value: Some(clamped(value, 0.0, maximum)),
            maximum,
            value_text: None,
            format: None,
        }
    }

    /// Create a determinate indicator from a `0.0..=1.0` completion fraction.
    pub fn fraction(fraction: f64) -> Self {
        Self::new(fraction, 1.0)
    }

    /// Create an indicator for work whose completion is unknown.
    pub fn indeterminate() -> Self {
        Self {
            id: None,
            value: None,
            maximum: 1.0,
            value_text: None,
            format: None,
        }
    }

    /// Attach a human-readable value such as `"3 of 12 files"`.
    ///
    /// Assistive technology prefers this over the raw number when present. An empty string clears
    /// it. The text is not rendered; the application owns every visible label.
    pub fn value_text(mut self, text: impl Into<Arc<str>>) -> Self {
        let text = text.into();
        self.value_text = (!text.is_empty()).then_some(text);
        self
    }

    /// Give this indicator a stable identity so its label and value parts can be related to it.
    ///
    /// Without an identity the parts are still decorated, but QuickGUI cannot point the root's
    /// accessible name and description at them, so the application supplies its own
    /// `accessibility_label`. Existing code that never declares an identity is unchanged.
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Attach a bounded value formatter, Base UI's `format` prop.
    ///
    /// The result is what [`Self::display_value`] returns for a caller-owned value part, and what
    /// assistive technology reads unless [`Self::value_text`] overrides it.
    pub fn format(mut self, format: ValueFormat) -> Self {
        self.format = Some(format);
        self
    }

    /// Which of Base UI's three progress states this indicator is in.
    pub fn status(&self) -> ProgressStatus {
        match self.value {
            None => ProgressStatus::Indeterminate,
            Some(value) if value >= self.maximum => ProgressStatus::Complete,
            Some(_) => ProgressStatus::Progressing,
        }
    }

    /// A copyable snapshot of what Base UI exposes as `data-*` attributes.
    pub fn state(&self) -> ProgressPartState {
        ProgressPartState {
            status: self.status(),
            value: self.value,
            maximum: self.maximum,
            completion: self.completion(),
        }
    }

    /// The formatted text for a caller-owned value part, or `None` while indeterminate.
    pub fn display_value(&self) -> Option<Arc<str>> {
        let value = self.value?;
        self.format
            .as_ref()
            .map(|format| format.apply(value, self.maximum))
            .or_else(|| self.value_text.clone())
    }

    /// The text assistive technology reads for the value, if any.
    ///
    /// An explicit [`Self::value_text`] wins over [`Self::format`], matching Base UI's
    /// `getAriaValueText` precedence.
    pub fn accessible_value(&self) -> Option<Arc<str>> {
        self.value_text.clone().or_else(|| {
            let value = self.value?;
            self.format
                .as_ref()
                .map(|format| format.apply(value, self.maximum))
        })
    }

    /// Stable identity of the label part, when this indicator declares one.
    pub fn label_id(&self) -> Option<ElementId> {
        self.id
            .map(|id| derived_progress_id(id, PROGRESS_LABEL_ID_TAG))
    }

    /// Stable identity of the value part, when this indicator declares one.
    pub fn value_id(&self) -> Option<ElementId> {
        self.id
            .map(|id| derived_progress_id(id, PROGRESS_VALUE_ID_TAG))
    }

    /// Stable identity of the track part, when this indicator declares one.
    pub fn track_id(&self) -> Option<ElementId> {
        self.id
            .map(|id| derived_progress_id(id, PROGRESS_TRACK_ID_TAG))
    }

    /// Stable identity of the indicator part, when this indicator declares one.
    pub fn indicator_id(&self) -> Option<ElementId> {
        self.id
            .map(|id| derived_progress_id(id, PROGRESS_INDICATOR_ID_TAG))
    }

    pub const fn current_value(&self) -> Option<f64> {
        self.value
    }

    pub const fn maximum(&self) -> f64 {
        self.maximum
    }

    pub const fn is_indeterminate(&self) -> bool {
        self.value.is_none()
    }

    /// The `0.0..=1.0` completion fraction, or `None` while indeterminate.
    ///
    /// Applications use this to size a caller-owned fill.
    pub fn completion(&self) -> Option<f32> {
        let value = self.value?;
        if self.maximum <= 0.0 {
            return Some(0.0);
        }
        Some(((value / self.maximum) as f32).clamp(0.0, 1.0))
    }

    /// Decorate an application-owned root without adding layout or appearance.
    pub fn root_with(&self, root: Element) -> Element {
        let range = match self.value {
            Some(value) => AccessibilityValueRange::new(value, 0.0, self.maximum),
            None => AccessibilityValueRange::indeterminate(0.0, self.maximum),
        };
        let mut root = root
            .accessibility_role(AccessibilityRole::ProgressIndicator)
            .accessibility_value_range(range);
        if let Some(id) = self.id {
            root = root.id(id);
        }
        if let Some(label) = self.label_id() {
            root = root.accessibility_labelled_by(label);
        }
        if let Some(value) = self.value_id() {
            root = root.accessibility_described_by(value);
        }
        match self.accessible_value() {
            Some(text) => root.accessibility_value(text),
            None => root,
        }
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(&self) -> Element {
        self.root_with(crate::div())
    }

    /// Decorate the caller-owned track that the fill is measured inside.
    ///
    /// The track is decoration: the root already carries the numeric value and bounds, so the track
    /// and everything under it stay out of the accessible name.
    pub fn track_with(&self, track: Element) -> Element {
        let track = track.accessibility_hidden(true);
        match self.track_id() {
            Some(id) => track.id(id),
            None => track,
        }
    }
    /// Create the unstyled track part. Use [`Self::track_with`] to supply an existing element.
    pub fn track(&self) -> Element {
        self.track_with(crate::div())
    }

    /// Hide an application-owned fill or animation from the accessible name.
    pub fn indicator_with(&self, indicator: Element) -> Element {
        let indicator = indicator.accessibility_hidden(true);
        match self.indicator_id() {
            Some(id) => indicator.id(id),
            None => indicator,
        }
    }
    /// Create the unstyled indicator part. Use [`Self::indicator_with`] to supply an existing element.
    pub fn indicator(&self) -> Element {
        self.indicator_with(crate::div())
    }

    /// Assign the stable mounted label target the root points at.
    pub fn label_with(&self, label: Element) -> Element {
        match self.label_id() {
            Some(id) => label.id(id),
            None => label,
        }
    }
    /// Create the unstyled label part. Use [`Self::label_with`] to supply an existing element.
    pub fn label(&self) -> Element {
        self.label_with(crate::div())
    }

    /// Assign the stable mounted value target the root points at.
    ///
    /// Put [`Self::display_value`] inside it; QuickGUI never renders the text itself.
    /// Create the unstyled value text part.
    pub fn value(&self) -> Element {
        self.value_with(crate::div())
    }

    pub fn value_with(&self, value: Element) -> Element {
        match self.value_id() {
            Some(id) => value.id(id),
            None => value,
        }
    }
}

/// Static measurement semantics for one unstyled meter.
///
/// A meter reports a level inside a known range—disk usage, battery charge, a score—rather than
/// the progress of a task. Optional low, high, and optimum markers let an application color the
/// gauge without inventing thresholds inside the framework.
#[derive(Clone, Debug, PartialEq)]
#[must_use = "a Meter descriptor has no effect until one of its parts is mounted"]
pub struct Meter {
    id: Option<ElementId>,
    value: f64,
    minimum: f64,
    maximum: f64,
    low: Option<f64>,
    high: Option<f64>,
    optimum: Option<f64>,
    value_text: Option<Arc<str>>,
    format: Option<ValueFormat>,
}

impl Meter {
    /// Create a meter. Non-finite bounds fall back to `0.0..=1.0` and an inverted range is swapped.
    pub fn new(value: f64, minimum: f64, maximum: f64) -> Self {
        let (minimum, maximum) = if !minimum.is_finite() || !maximum.is_finite() {
            (0.0, 1.0)
        } else if maximum < minimum {
            (maximum, minimum)
        } else {
            (minimum, maximum)
        };
        Self {
            id: None,
            value: clamped(value, minimum, maximum),
            minimum,
            maximum,
            low: None,
            high: None,
            optimum: None,
            value_text: None,
            format: None,
        }
    }

    /// Mark the upper end of the low range. Values outside the meter bounds are clamped.
    pub fn low(mut self, low: f64) -> Self {
        self.low = Some(clamped(low, self.minimum, self.maximum));
        self
    }

    /// Mark the lower end of the high range.
    pub fn high(mut self, high: f64) -> Self {
        self.high = Some(clamped(high, self.minimum, self.maximum));
        self
    }

    /// Mark the most favorable value inside the range.
    pub fn optimum(mut self, optimum: f64) -> Self {
        self.optimum = Some(clamped(optimum, self.minimum, self.maximum));
        self
    }

    /// Give this meter a stable identity so its label and value parts can be related to it.
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Attach a bounded value formatter, Base UI's `format` prop.
    pub fn format(mut self, format: ValueFormat) -> Self {
        self.format = Some(format);
        self
    }

    /// Attach a human-readable value that assistive technology prefers over the raw number.
    ///
    /// This is Base UI's `getAriaValueText`; an empty string clears it. It wins over
    /// [`Self::format`].
    pub fn value_text(mut self, text: impl Into<Arc<str>>) -> Self {
        let text = text.into();
        self.value_text = (!text.is_empty()).then_some(text);
        self
    }

    /// The formatted text for a caller-owned value part.
    pub fn display_value(&self) -> Option<Arc<str>> {
        self.format
            .as_ref()
            .map(|format| format.apply(self.value, self.maximum))
            .or_else(|| self.value_text.clone())
    }

    /// The text assistive technology reads for the value, if any.
    pub fn accessible_value(&self) -> Option<Arc<str>> {
        self.value_text.clone().or_else(|| {
            self.format
                .as_ref()
                .map(|format| format.apply(self.value, self.maximum))
        })
    }

    /// Stable identity of the label part, when this meter declares one.
    pub fn label_id(&self) -> Option<ElementId> {
        self.id
            .map(|id| derived_progress_id(id, PROGRESS_LABEL_ID_TAG))
    }

    /// Stable identity of the value part, when this meter declares one.
    pub fn value_id(&self) -> Option<ElementId> {
        self.id
            .map(|id| derived_progress_id(id, PROGRESS_VALUE_ID_TAG))
    }

    /// Stable identity of the track part, when this meter declares one.
    pub fn track_id(&self) -> Option<ElementId> {
        self.id
            .map(|id| derived_progress_id(id, PROGRESS_TRACK_ID_TAG))
    }

    /// Stable identity of the indicator part, when this meter declares one.
    pub fn indicator_id(&self) -> Option<ElementId> {
        self.id
            .map(|id| derived_progress_id(id, PROGRESS_INDICATOR_ID_TAG))
    }

    pub const fn current_value(&self) -> f64 {
        self.value
    }

    pub const fn minimum(&self) -> f64 {
        self.minimum
    }

    pub const fn maximum(&self) -> f64 {
        self.maximum
    }

    pub const fn low_value(&self) -> Option<f64> {
        self.low
    }

    pub const fn high_value(&self) -> Option<f64> {
        self.high
    }

    pub const fn optimum_value(&self) -> Option<f64> {
        self.optimum
    }

    /// The `0.0..=1.0` position of the value inside the meter's range.
    pub fn completion(&self) -> f32 {
        let span = self.maximum - self.minimum;
        if span <= 0.0 {
            return 0.0;
        }
        (((self.value - self.minimum) / span) as f32).clamp(0.0, 1.0)
    }

    /// Whether the value falls at or below [`Self::low`].
    pub fn is_low(&self) -> bool {
        self.low.is_some_and(|low| self.value <= low)
    }

    /// Whether the value falls at or above [`Self::high`].
    pub fn is_high(&self) -> bool {
        self.high.is_some_and(|high| self.value >= high)
    }

    /// Decorate an application-owned root without adding layout or appearance.
    pub fn root_with(&self, root: Element) -> Element {
        let mut root = root
            .accessibility_role(AccessibilityRole::Meter)
            .accessibility_value_range(AccessibilityValueRange::new(
                self.value,
                self.minimum,
                self.maximum,
            ));
        if let Some(id) = self.id {
            root = root.id(id);
        }
        if let Some(label) = self.label_id() {
            root = root.accessibility_labelled_by(label);
        }
        if let Some(value) = self.value_id() {
            root = root.accessibility_described_by(value);
        }
        match self.accessible_value() {
            Some(text) => root.accessibility_value(text),
            None => root,
        }
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(&self) -> Element {
        self.root_with(crate::div())
    }

    /// Decorate the caller-owned track that the fill is measured inside.
    pub fn track_with(&self, track: Element) -> Element {
        let track = track.accessibility_hidden(true);
        match self.track_id() {
            Some(id) => track.id(id),
            None => track,
        }
    }
    /// Create the unstyled track part. Use [`Self::track_with`] to supply an existing element.
    pub fn track(&self) -> Element {
        self.track_with(crate::div())
    }

    /// Hide an application-owned fill from the accessible name.
    pub fn indicator_with(&self, indicator: Element) -> Element {
        let indicator = indicator.accessibility_hidden(true);
        match self.indicator_id() {
            Some(id) => indicator.id(id),
            None => indicator,
        }
    }
    /// Create the unstyled indicator part. Use [`Self::indicator_with`] to supply an existing element.
    pub fn indicator(&self) -> Element {
        self.indicator_with(crate::div())
    }

    /// Assign the stable mounted label target the root points at.
    pub fn label_with(&self, label: Element) -> Element {
        match self.label_id() {
            Some(id) => label.id(id),
            None => label,
        }
    }
    /// Create the unstyled label part. Use [`Self::label_with`] to supply an existing element.
    pub fn label(&self) -> Element {
        self.label_with(crate::div())
    }

    /// Assign the stable mounted value target the root points at.
    /// Create the unstyled value text part.
    pub fn value(&self) -> Element {
        self.value_with(crate::div())
    }

    pub fn value_with(&self, value: Element) -> Element {
        match self.value_id() {
            Some(id) => value.id(id),
            None => value,
        }
    }
}

/// Create an unstyled determinate progress root.
///
/// This shorthand is equivalent to `Progress::new(value, maximum).root_with(div())`.
pub fn progress(value: f64, maximum: f64) -> Element {
    Progress::new(value, maximum).root_with(div())
}

/// Create an unstyled meter root.
///
/// This shorthand is equivalent to `Meter::new(value, minimum, maximum).root_with(div())`.
pub fn meter(value: f64, minimum: f64, maximum: f64) -> Element {
    Meter::new(value, minimum, maximum).root_with(div())
}

fn derived_progress_id(parent: ElementId, tag: u64) -> ElementId {
    let mut hash = parent.as_u64() ^ tag;
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    for _ in 0..4 {
        if hash != 0 && hash != parent.as_u64() && hash != u64::MAX {
            return ElementId::new(hash);
        }
        hash = hash.wrapping_add(tag | 1);
    }
    unreachable!("four distinct candidates cannot all match three reserved progress IDs")
}

fn normalized_maximum(maximum: f64) -> f64 {
    if maximum.is_finite() && maximum > 0.0 {
        maximum
    } else {
        1.0
    }
}

fn clamped(value: f64, minimum: f64, maximum: f64) -> f64 {
    if value.is_finite() {
        value.clamp(minimum, maximum)
    } else {
        minimum
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, ElementId, IntoElement, TestAppContext, View, ViewContext, text};

    #[test]
    fn progress_values_are_bounded_and_unstyled() {
        let determinate = Progress::new(3.0, 12.0).value_text("3 of 12 files");
        assert_eq!(determinate.current_value(), Some(3.0));
        assert_eq!(determinate.maximum(), 12.0);
        assert_eq!(determinate.completion(), Some(0.25));
        assert!(!determinate.is_indeterminate());

        assert_eq!(Progress::new(-5.0, 10.0).current_value(), Some(0.0));
        assert_eq!(Progress::new(50.0, 10.0).current_value(), Some(10.0));
        assert_eq!(Progress::new(1.0, f64::NAN).maximum(), 1.0);
        assert_eq!(Progress::new(1.0, -4.0).maximum(), 1.0);
        assert_eq!(
            Progress::new(f64::INFINITY, 10.0).current_value(),
            Some(0.0)
        );
        assert_eq!(Progress::fraction(0.5).completion(), Some(0.5));

        let indeterminate = Progress::indeterminate();
        assert!(indeterminate.is_indeterminate());
        assert_eq!(indeterminate.completion(), None);

        let root = determinate.root_with(div().w(200.0).bg(Color::rgb8(1, 2, 3)));
        assert_eq!(
            root.accessibility.role,
            AccessibilityRole::ProgressIndicator
        );
        assert_eq!(
            root.accessibility.value_range.as_deref(),
            Some(&AccessibilityValueRange::new(3.0, 0.0, 12.0))
        );
        assert_eq!(root.accessibility.value.as_deref(), Some("3 of 12 files"));
        assert_eq!(root.visual.background, Some(Color::rgb8(1, 2, 3)));
        assert!(!root.focusable);
        assert!(!root.clickable);
        assert!(root.transition.is_none());
        assert!(root.animation.is_none());

        let empty_text = Progress::new(1.0, 2.0).value_text("");
        assert!(empty_text.root_with(div()).accessibility.value.is_none());

        let unknown = indeterminate.root_with(div());
        let range = unknown
            .accessibility
            .value_range
            .as_deref()
            .expect("indeterminate range");
        assert_eq!(range.value, None);
        assert_eq!(range.min, Some(0.0));
        assert_eq!(range.max, Some(1.0));

        let indicator = determinate.indicator_with(div().bg(Color::rgb8(4, 5, 6)));
        assert!(indicator.accessibility.hidden);
        assert_eq!(indicator.visual.background, Some(Color::rgb8(4, 5, 6)));
    }

    #[test]
    fn meter_thresholds_are_clamped_and_optional() {
        let meter = Meter::new(72.0, 0.0, 100.0)
            .low(20.0)
            .high(80.0)
            .optimum(50.0);
        assert_eq!(meter.current_value(), 72.0);
        assert_eq!(meter.low_value(), Some(20.0));
        assert_eq!(meter.high_value(), Some(80.0));
        assert_eq!(meter.optimum_value(), Some(50.0));
        assert!(!meter.is_low());
        assert!(!meter.is_high());
        assert_eq!(meter.completion(), 0.72);

        let full = Meter::new(200.0, 0.0, 100.0).high(80.0);
        assert_eq!(full.current_value(), 100.0);
        assert!(full.is_high());
        assert_eq!(Meter::new(0.0, 5.0, 5.0).completion(), 0.0);
        let inverted = Meter::new(1.0, 10.0, -10.0);
        assert_eq!((inverted.minimum(), inverted.maximum()), (-10.0, 10.0));
        let broken = Meter::new(0.5, f64::NAN, 4.0);
        assert_eq!((broken.minimum(), broken.maximum()), (0.0, 1.0));
        assert_eq!(Meter::new(0.0, 0.0, 10.0).low(-4.0).low_value(), Some(0.0));

        let root = meter.root_with(div().bg(Color::rgb8(7, 8, 9)));
        assert_eq!(root.accessibility.role, AccessibilityRole::Meter);
        assert_eq!(
            root.accessibility.value_range.as_deref(),
            Some(&AccessibilityValueRange::new(72.0, 0.0, 100.0))
        );
        assert_eq!(root.visual.background, Some(Color::rgb8(7, 8, 9)));
        assert!(meter.indicator_with(div()).accessibility.hidden);
    }

    struct FeedbackView;

    impl View for FeedbackView {
        fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let download = Progress::new(40.0, 100.0).value_text("40 percent");
            let unknown = Progress::indeterminate();
            let disk = Meter::new(72.0, 0.0, 100.0).low(20.0).high(80.0);
            // The identified indicator takes its accessible name and description from its own
            // mounted label and value parts instead of a copied string.
            let upload = Progress::new(6.0, 8.0)
                .id("upload")
                .format(ValueFormat::fraction());
            div()
                .child(
                    download
                        .root_with(div().id("download").accessibility_label("Download"))
                        .child(download.indicator_with(div().w(80.0))),
                )
                .child(
                    unknown
                        .root_with(div().id("scanning").accessibility_label("Scanning"))
                        .child(text("Scanning")),
                )
                .child(disk.root_with(div().id("disk").accessibility_label("Disk usage")))
                .child(
                    upload
                        .root_with(div())
                        .child(upload.label_with(text("Upload")))
                        .child(upload.value_with(text(
                            upload.display_value().unwrap_or_else(|| Arc::from("")),
                        )))
                        .child(
                            upload
                                .track_with(div().w(200.0))
                                .child(upload.indicator_with(div().w(150.0))),
                        ),
                )
        }
    }

    #[test]
    fn roles_and_values_reach_the_native_tree_without_idle_work() {
        let (mut cx, view) = TestAppContext::new(FeedbackView).unwrap();
        let window = view.window_handle();
        let update = cx.accessibility_update(window).unwrap();
        let node = |id: ElementId| {
            update
                .nodes
                .iter()
                .find_map(|(node_id, node)| (node_id.0 == id.as_u64()).then_some(node))
                .expect("feedback accessibility node")
        };

        let download = node("download".into());
        assert_eq!(download.role(), accesskit::Role::ProgressIndicator);
        assert_eq!(download.numeric_value(), Some(40.0));
        assert_eq!(download.min_numeric_value(), Some(0.0));
        assert_eq!(download.max_numeric_value(), Some(100.0));
        assert_eq!(download.value(), Some("40 percent"));
        assert_eq!(download.label(), Some("Download"));

        let scanning = node("scanning".into());
        assert_eq!(scanning.role(), accesskit::Role::ProgressIndicator);
        assert_eq!(scanning.numeric_value(), None);
        assert_eq!(scanning.max_numeric_value(), Some(1.0));

        let disk = node("disk".into());
        assert_eq!(disk.role(), accesskit::Role::Meter);
        assert_eq!(disk.numeric_value(), Some(72.0));

        let identified = Progress::new(6.0, 8.0)
            .id("upload")
            .format(ValueFormat::fraction());
        let upload = node("upload".into());
        assert_eq!(upload.role(), accesskit::Role::ProgressIndicator);
        assert_eq!(upload.numeric_value(), Some(6.0));
        assert_eq!(upload.max_numeric_value(), Some(8.0));
        assert_eq!(upload.value(), Some("6 of 8"));
        assert_eq!(identified.status(), ProgressStatus::Progressing);
        let label = identified.label_id().expect("declared label identity");
        let value = identified.value_id().expect("declared value identity");
        assert_eq!(
            upload.labelled_by(),
            &[accesskit::NodeId(label.as_u64())][..]
        );
        assert_eq!(
            upload.described_by(),
            &[accesskit::NodeId(value.as_u64())][..]
        );
        assert_eq!(node(label).role(), accesskit::Role::Label);
        assert!(cx.contains_element(window, value).unwrap());

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }

    #[test]
    fn shorthands_are_semantic_unstyled_roots() {
        let bar = progress(0.5, 1.0);
        assert_eq!(bar.accessibility.role, AccessibilityRole::ProgressIndicator);
        assert!(bar.children.is_empty());
        assert_eq!(bar.visual.background, None);

        let gauge = meter(1.0, 0.0, 4.0);
        assert_eq!(gauge.accessibility.role, AccessibilityRole::Meter);
        assert!(gauge.children.is_empty());
    }

    #[test]
    fn progress_status_covers_the_three_base_ui_states() {
        assert_eq!(
            Progress::new(3.0, 12.0).status(),
            ProgressStatus::Progressing
        );
        assert_eq!(Progress::new(12.0, 12.0).status(), ProgressStatus::Complete);
        // The value is clamped first, so an over-large value is complete rather than out of range.
        assert_eq!(Progress::new(99.0, 12.0).status(), ProgressStatus::Complete);
        assert_eq!(
            Progress::indeterminate().status(),
            ProgressStatus::Indeterminate
        );
        assert!(ProgressStatus::Progressing.is_progressing());
        assert!(ProgressStatus::Complete.is_complete());
        assert!(ProgressStatus::Indeterminate.is_indeterminate());
        assert_eq!(ProgressStatus::default(), ProgressStatus::Progressing);

        let state = Progress::new(3.0, 12.0).state();
        assert_eq!(
            state,
            ProgressPartState {
                status: ProgressStatus::Progressing,
                value: Some(3.0),
                maximum: 12.0,
                completion: Some(0.25),
            }
        );
        let unknown = Progress::indeterminate().state();
        assert_eq!(unknown.status, ProgressStatus::Indeterminate);
        assert_eq!(unknown.value, None);
        assert_eq!(unknown.completion, None);
    }

    #[test]
    fn value_formatters_are_bounded_and_aria_text_wins_over_formatting() {
        let percent = Progress::new(3.0, 12.0).format(ValueFormat::percent());
        assert_eq!(percent.display_value().as_deref(), Some("25%"));
        assert_eq!(percent.accessible_value().as_deref(), Some("25%"));

        let fraction = Progress::new(3.0, 12.0).format(ValueFormat::fraction());
        assert_eq!(fraction.display_value().as_deref(), Some("3 of 12"));

        // An explicit value text is Base UI's `getAriaValueText` and wins for assistive technology,
        // while the visible value part keeps the formatted string.
        let both = Progress::new(3.0, 12.0)
            .format(ValueFormat::percent())
            .value_text("3 of 12 files");
        assert_eq!(both.display_value().as_deref(), Some("25%"));
        assert_eq!(both.accessible_value().as_deref(), Some("3 of 12 files"));
        assert_eq!(
            both.root_with(div()).accessibility.value.as_deref(),
            Some("3 of 12 files")
        );

        // An indeterminate indicator has no value to format.
        assert_eq!(
            Progress::indeterminate()
                .format(ValueFormat::percent())
                .display_value(),
            None
        );

        // A custom formatter is an ordinary bounded closure and never runs on an idle frame.
        let custom = Progress::new(2.0, 4.0).format(ValueFormat::new(|value, maximum| {
            Arc::from(format!("{value}/{maximum}"))
        }));
        assert_eq!(custom.display_value().as_deref(), Some("2/4"));
        let shared = ValueFormat::percent();
        assert_eq!(shared, shared.clone());
        assert_ne!(shared, ValueFormat::percent());
        assert!(format!("{shared:?}").contains("ValueFormat"));
        assert_eq!(ValueFormat::percent().apply(1.0, 0.0).as_ref(), "0%");
    }

    #[test]
    fn identified_progress_parts_relate_without_adding_appearance() {
        let progress = Progress::new(40.0, 100.0)
            .id("download")
            .format(ValueFormat::percent());
        let label = progress.label_id().expect("declared label identity");
        let value = progress.value_id().expect("declared value identity");
        let track = progress.track_id().expect("declared track identity");
        let indicator = progress
            .indicator_id()
            .expect("declared indicator identity");
        for (index, id) in [label, value, track, indicator].iter().enumerate() {
            assert_ne!(*id, "download".into());
            assert_ne!(*id, ElementId::new(0));
            assert_ne!(*id, ElementId::new(u64::MAX));
            assert!(![label, value, track, indicator][..index].contains(id));
        }

        let root = progress.root_with(div());
        assert_eq!(root.explicit_id, Some("download".into()));
        assert_eq!(root.accessibility.relations.labelled_by(), Some(label));
        assert_eq!(root.accessibility.relations.described_by(), Some(value));
        assert_eq!(root.accessibility.value.as_deref(), Some("40%"));
        assert_eq!(root.visual.background, None);

        let track_with = progress.track_with(div().bg(Color::rgb8(1, 2, 3)));
        assert_eq!(track_with.explicit_id, Some(track));
        assert!(track_with.accessibility.hidden);
        assert_eq!(track_with.visual.background, Some(Color::rgb8(1, 2, 3)));
        let indicator_with = progress.indicator_with(div());
        assert_eq!(indicator_with.explicit_id, Some(indicator));
        assert!(indicator_with.accessibility.hidden);
        assert_eq!(
            progress.label_with(text("Download")).explicit_id,
            Some(label)
        );
        assert_eq!(progress.value_with(text("40%")).explicit_id, Some(value));

        // An indicator that declares no identity keeps the original unrelated decoration.
        let plain = Progress::new(1.0, 2.0);
        assert_eq!(plain.label_id(), None);
        let root = plain.root_with(div());
        assert_eq!(root.explicit_id, None);
        assert_eq!(root.accessibility.relations.labelled_by(), None);
        assert_eq!(plain.label_with(text("Loading")).explicit_id, None);
        assert!(plain.track_with(div()).accessibility.hidden);
    }

    #[test]
    fn meter_parts_share_the_progress_shape_without_a_task_status() {
        let meter = Meter::new(72.0, 0.0, 100.0)
            .id("disk")
            .low(20.0)
            .high(80.0)
            .format(ValueFormat::percent());
        assert_eq!(meter.display_value().as_deref(), Some("72%"));
        assert_eq!(meter.accessible_value().as_deref(), Some("72%"));

        let labelled = meter.clone().value_text("72 percent full");
        assert_eq!(
            labelled.accessible_value().as_deref(),
            Some("72 percent full")
        );
        assert_eq!(labelled.display_value().as_deref(), Some("72%"));
        assert_eq!(
            Meter::new(1.0, 0.0, 2.0).value_text("").accessible_value(),
            None
        );

        let root = meter.root_with(div());
        assert_eq!(root.explicit_id, Some("disk".into()));
        assert_eq!(root.accessibility.role, AccessibilityRole::Meter);
        assert_eq!(root.accessibility.relations.labelled_by(), meter.label_id());
        assert_eq!(
            root.accessibility.relations.described_by(),
            meter.value_id()
        );
        assert_eq!(root.accessibility.value.as_deref(), Some("72%"));
        assert!(meter.track_with(div()).accessibility.hidden);
        assert!(meter.indicator_with(div()).accessibility.hidden);
        assert_eq!(
            meter.label_with(text("Disk usage")).explicit_id,
            meter.label_id()
        );
        assert_eq!(meter.value_with(text("72%")).explicit_id, meter.value_id());
        assert_eq!(Meter::new(1.0, 0.0, 2.0).label_id(), None);
    }
}
