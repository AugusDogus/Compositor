use std::sync::Arc;
use web_time::{Duration, Instant};

use crate::{
    AccessibilityRole, AccessibilityValueRange, Element, ElementId, Modifiers, Point, PointerEvent,
    PointerPhase, div, text_input,
};

/// Maximum logical pixels one scrub gesture may travel per step.
pub const MAX_NUMBER_FIELD_SCRUB_SENSITIVITY: f32 = 256.0;

/// Default logical pixels one scrub gesture travels per step.
pub const DEFAULT_NUMBER_FIELD_SCRUB_SENSITIVITY: f32 = 2.0;

/// Maximum UTF-8 bytes retained by one number field's editing text.
///
/// Numbers are short. The bound keeps a paste of arbitrary application or clipboard text from
/// turning one controlled field into an unbounded string.
pub const MAX_NUMBER_FIELD_TEXT_BYTES: usize = 64;

/// Maximum fractional digits one number field formats.
pub const MAX_NUMBER_FIELD_PRECISION: u8 = 15;

/// Delay before a held increment or decrement button starts repeating.
pub const NUMBER_FIELD_REPEAT_DELAY: Duration = Duration::from_millis(400);

/// Interval between repeats while an increment or decrement button stays held.
pub const NUMBER_FIELD_REPEAT_INTERVAL: Duration = Duration::from_millis(60);

const NUMBER_FIELD_INPUT_ID_TAG: u64 = 0x6a2f_c391_bd47_50e8;
const NUMBER_FIELD_INCREMENT_ID_TAG: u64 = 0xb185_7e2c_04af_39d6;
const NUMBER_FIELD_DECREMENT_ID_TAG: u64 = 0x27ce_4a80_f6d1_9b53;
const NUMBER_FIELD_GROUP_ID_TAG: u64 = 0x9d61_38b7_2e0c_a4f5;
const NUMBER_FIELD_SCRUB_AREA_ID_TAG: u64 = 0x4f0a_c68d_71b3_2e97;
const NUMBER_FIELD_SCRUB_CURSOR_ID_TAG: u64 = 0xe258_9134_bd06_7cfa;

/// Locale-shaped parsing and formatting rules for one number field.
///
/// QuickGUI deliberately has no locale database. The application supplies the separators its
/// users expect, which keeps the framework free of an unbounded data table while still supporting
/// `1 234,56` as readily as `1,234.56`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NumberFieldFormat {
    decimal: char,
    group: Option<char>,
    sign: bool,
    exponent: bool,
    precision: Option<u8>,
}

impl Default for NumberFieldFormat {
    fn default() -> Self {
        Self {
            decimal: '.',
            group: None,
            sign: true,
            exponent: false,
            precision: None,
        }
    }
}

impl NumberFieldFormat {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the decimal separator. A digit or ASCII sign is rejected and keeps the default.
    #[must_use]
    pub fn decimal_separator(mut self, separator: char) -> Self {
        if !separator.is_ascii_digit() && separator != '+' && separator != '-' {
            self.decimal = separator;
        }
        self
    }

    /// Accept and emit one grouping separator, such as `,` or a narrow space.
    #[must_use]
    pub fn group_separator(mut self, separator: Option<char>) -> Self {
        self.group = separator.filter(|separator| {
            !separator.is_ascii_digit() && *separator != '+' && *separator != '-'
        });
        self
    }

    /// Accept a leading `+` or `-`. The default is `true`.
    #[must_use]
    pub const fn sign(mut self, sign: bool) -> Self {
        self.sign = sign;
        self
    }

    /// Accept scientific notation such as `1.5e3`. The default is `false`.
    #[must_use]
    pub const fn exponent(mut self, exponent: bool) -> Self {
        self.exponent = exponent;
        self
    }

    /// Format committed values with exactly this many fractional digits.
    #[must_use]
    pub fn precision(mut self, precision: u8) -> Self {
        self.precision = Some(precision.min(MAX_NUMBER_FIELD_PRECISION));
        self
    }

    pub const fn decimal(&self) -> char {
        self.decimal
    }

    pub const fn group(&self) -> Option<char> {
        self.group
    }

    pub const fn allows_sign(&self) -> bool {
        self.sign
    }

    pub const fn allows_exponent(&self) -> bool {
        self.exponent
    }

    pub const fn precision_digits(&self) -> Option<u8> {
        self.precision
    }

    /// Parse one string into a number using these rules.
    ///
    /// Only ASCII digits are accepted. Whitespace around the value is ignored, group separators
    /// are removed, and an empty or malformed string returns `None`.
    pub fn parse(&self, text: &str) -> Option<f64> {
        let trimmed = text.trim();
        if trimmed.is_empty() || trimmed.len() > MAX_NUMBER_FIELD_TEXT_BYTES {
            return None;
        }
        let mut normalized = String::with_capacity(trimmed.len());
        let mut digits = 0_usize;
        let mut seen_decimal = false;
        let mut seen_exponent = false;
        let mut expect_sign = true;
        for character in trimmed.chars() {
            if Some(character) == self.group {
                if !seen_decimal && !seen_exponent && digits > 0 {
                    continue;
                }
                return None;
            }
            if character == self.decimal {
                if seen_decimal || seen_exponent {
                    return None;
                }
                seen_decimal = true;
                normalized.push('.');
                expect_sign = false;
                continue;
            }
            match character {
                '+' | '-' => {
                    if !expect_sign || !self.sign {
                        return None;
                    }
                    normalized.push(character);
                    expect_sign = false;
                }
                'e' | 'E' => {
                    if !self.exponent || seen_exponent || digits == 0 {
                        return None;
                    }
                    seen_exponent = true;
                    normalized.push('e');
                    expect_sign = true;
                }
                digit if digit.is_ascii_digit() => {
                    digits += 1;
                    normalized.push(digit);
                    expect_sign = false;
                }
                _ => return None,
            }
        }
        if digits == 0 {
            return None;
        }
        normalized
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
    }

    /// Render one number using these rules.
    pub fn format(&self, value: f64) -> Arc<str> {
        if !value.is_finite() {
            return Arc::from("");
        }
        let rendered = match self.precision {
            Some(precision) => format!("{value:.*}", usize::from(precision)),
            None => format!("{value}"),
        };
        let (sign, digits) = match rendered.strip_prefix('-') {
            Some(rest) => ("-", rest),
            None => ("", rendered.as_str()),
        };
        let (integer, fraction) = match digits.split_once('.') {
            Some((integer, fraction)) => (integer, Some(fraction)),
            None => (digits, None),
        };
        let mut output = String::with_capacity(rendered.len() + 8);
        output.push_str(sign);
        match self.group {
            Some(separator)
                if integer.len() > 3 && integer.bytes().all(|byte| byte.is_ascii_digit()) =>
            {
                let lead = integer.len() % 3;
                if lead > 0 {
                    output.push_str(&integer[..lead]);
                }
                let mut index = lead;
                while index < integer.len() {
                    if index > 0 {
                        output.push(separator);
                    }
                    output.push_str(&integer[index..index + 3]);
                    index += 3;
                }
            }
            _ => output.push_str(integer),
        }
        if let Some(fraction) = fraction {
            output.push(self.decimal);
            output.push_str(fraction);
        }
        Arc::from(output)
    }
}

/// The axis a number field's scrub area follows, Base UI's ScrubArea `direction`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NumberFieldScrubDirection {
    /// Dragging right increases the value.
    #[default]
    Horizontal,
    /// Dragging up increases the value.
    Vertical,
    /// Either axis contributes, which suits a small square scrub handle.
    Both,
}

/// Which step size one keyboard, wheel, or scrub gesture applies.
///
/// This is Base UI's modifier contract: Shift selects the large step and Alt the small one.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NumberFieldStepSize {
    /// The ordinary declared step.
    #[default]
    Normal,
    /// The Shift-modified step, ten ordinary steps by default.
    Large,
    /// The Alt-modified step, one tenth of an ordinary step by default.
    Small,
}

impl NumberFieldStepSize {
    /// Choose the step size the way Base UI reads keyboard modifiers.
    ///
    /// Shift wins over Alt when both are held, matching the platform spin-button convention that
    /// the coarser gesture takes precedence.
    pub const fn from_modifiers(modifiers: Modifiers) -> Self {
        if modifiers.contains(Modifiers::SHIFT) {
            Self::Large
        } else if modifiers.contains(Modifiers::ALT) {
            Self::Small
        } else {
            Self::Normal
        }
    }
}

/// A copyable render-state snapshot for one unstyled number field.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NumberFieldPartState {
    /// Whether a captured scrub gesture is currently changing the value.
    pub scrubbing: bool,
    /// Whether a stepper is held and repeating.
    pub stepping: bool,
    /// Whether the field refuses every change.
    pub disabled: bool,
    /// Whether the field shows a value that may be read but not changed.
    pub read_only: bool,
    /// Whether the field requires a value before submission.
    pub required: bool,
    /// Whether the current text parses inside the declared range.
    pub valid: bool,
}

/// Which stepper is currently held.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RepeatDirection {
    Increment,
    Decrement,
}

/// Controlled editing text, committed value, and bounded stepping state for one number field.
///
/// The application owns the input's appearance, the stepper buttons, and the repeat task it wires
/// to [`Self::repeat_deadline`]. QuickGUI owns parsing, clamping, formatting, and the exact repeat
/// schedule. Nothing here is a timer: the state only reports the next deadline, so a released
/// stepper leaves the window with no idle source at all.
#[derive(Clone, Debug, PartialEq)]
pub struct NumberFieldState {
    text: Arc<str>,
    value: Option<f64>,
    committed: Option<f64>,
    minimum: f64,
    maximum: f64,
    step: f64,
    small_step: f64,
    large_step: f64,
    snap_on_step: bool,
    allow_wheel_scrub: bool,
    scrub_direction: NumberFieldScrubDirection,
    scrub_sensitivity: f32,
    format: NumberFieldFormat,
    disabled: bool,
    read_only: bool,
    required: bool,
    scrub: Option<ScrubSession>,
    repeat: Option<(RepeatDirection, Instant)>,
}

/// The retained remainder of one captured scrub gesture.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ScrubSession {
    /// Pixels travelled that have not yet become a whole step.
    remainder: f32,
    /// The pointer position, so a caller-owned scrub cursor can follow it.
    position: Point,
}

impl NumberFieldState {
    /// Create a field showing one formatted value.
    pub fn new(value: f64) -> Self {
        let format = NumberFieldFormat::default();
        let value = value.is_finite().then_some(value);
        Self {
            text: value.map_or_else(|| Arc::from(""), |value| format.format(value)),
            value,
            committed: value,
            minimum: f64::NEG_INFINITY,
            maximum: f64::INFINITY,
            step: 1.0,
            small_step: 0.0,
            large_step: 0.0,
            snap_on_step: false,
            allow_wheel_scrub: true,
            scrub_direction: NumberFieldScrubDirection::Horizontal,
            scrub_sensitivity: DEFAULT_NUMBER_FIELD_SCRUB_SENSITIVITY,
            format,
            disabled: false,
            read_only: false,
            required: false,
            scrub: None,
            repeat: None,
        }
    }

    /// Create an empty field.
    pub fn empty() -> Self {
        Self {
            text: Arc::from(""),
            value: None,
            committed: None,
            ..Self::new(0.0)
        }
    }

    /// Constrain committed values. An inverted range is swapped.
    #[must_use]
    pub fn range(mut self, minimum: f64, maximum: f64) -> Self {
        let (minimum, maximum) = if minimum.is_nan() || maximum.is_nan() {
            (f64::NEG_INFINITY, f64::INFINITY)
        } else if maximum < minimum {
            (maximum, minimum)
        } else {
            (minimum, maximum)
        };
        self.minimum = minimum;
        self.maximum = maximum;
        self
    }

    /// Replace the amount one arrow key, wheel notch, or stepper press moves.
    #[must_use]
    pub fn step(mut self, step: f64) -> Self {
        if step.is_finite() && step > 0.0 {
            self.step = step;
        }
        self
    }

    /// Replace the parsing and formatting rules, reformatting any committed value.
    #[must_use]
    pub fn format(mut self, format: NumberFieldFormat) -> Self {
        self.format = format;
        if let Some(value) = self.value {
            self.text = self.format.format(value);
        }
        self
    }

    /// Format committed values with exactly this many fractional digits.
    #[must_use]
    pub fn precision(self, precision: u8) -> Self {
        let format = self.format.precision(precision);
        self.format(format)
    }

    /// Replace the amount an Alt-modified gesture moves, Base UI's `smallStep`.
    ///
    /// The default is one tenth of an ordinary step. A non-finite or non-positive value restores
    /// that default.
    #[must_use]
    pub fn small_step(mut self, small_step: f64) -> Self {
        self.small_step = if small_step.is_finite() && small_step > 0.0 {
            small_step
        } else {
            0.0
        };
        self
    }

    /// Replace the amount a Shift-modified gesture moves, Base UI's `largeStep`.
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

    /// Snap a stepped value onto the step grid, Base UI's `snapOnStep`.
    ///
    /// Without this a step adds to whatever the user typed, so `3` steps to `4` with a step of
    /// `5`. With it the value lands on the nearest multiple of the step measured from the minimum,
    /// or from zero when the field is unbounded below.
    #[must_use]
    pub const fn snap_on_step(mut self, snap: bool) -> Self {
        self.snap_on_step = snap;
        self
    }

    /// Choose whether a focused field steps on scroll-wheel input, Base UI's `allowWheelScrub`.
    ///
    /// QuickGUI has always applied focused wheel input, so this defaults to `true` rather than to
    /// Base UI's `false`; pass `false` to opt out.
    #[must_use]
    pub const fn allow_wheel_scrub(mut self, allow: bool) -> Self {
        self.allow_wheel_scrub = allow;
        self
    }

    /// Choose the axis a mounted scrub area follows.
    #[must_use]
    pub const fn scrub_direction(mut self, direction: NumberFieldScrubDirection) -> Self {
        self.scrub_direction = direction;
        self
    }

    /// Set how far a scrub gesture travels per step, in logical pixels.
    #[must_use]
    pub fn scrub_sensitivity(mut self, pixels_per_step: f32) -> Self {
        self.scrub_sensitivity = if pixels_per_step.is_finite() && pixels_per_step > 0.0 {
            pixels_per_step.min(MAX_NUMBER_FIELD_SCRUB_SENSITIVITY)
        } else {
            DEFAULT_NUMBER_FIELD_SCRUB_SENSITIVITY
        };
        self
    }

    /// Show a value the user may read and copy but not change, Base UI's `readOnly`.
    ///
    /// Unlike [`Self::disabled`], a read-only field stays focusable and stays in the Tab sequence;
    /// it simply refuses every commit, step, wheel notch, and scrub.
    #[must_use]
    pub const fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Require a value before submission, Base UI's `required`.
    #[must_use]
    pub const fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn text(&self) -> &Arc<str> {
        &self.text
    }

    /// The value the current text parses to, or `None` while the text is empty or malformed.
    pub const fn value(&self) -> Option<f64> {
        self.value
    }

    /// The last value committed by [`Self::commit`] or a step.
    ///
    /// Unparseable text is restored to this value on commit, so a half-typed entry cannot lose
    /// the user's previous number.
    pub const fn committed_value(&self) -> Option<f64> {
        self.committed
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

    pub const fn format_rules(&self) -> NumberFieldFormat {
        self.format
    }

    pub const fn is_disabled(&self) -> bool {
        self.disabled
    }

    pub const fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub const fn is_required(&self) -> bool {
        self.required
    }

    /// Whether a captured scrub gesture is currently changing the value.
    pub const fn is_scrubbing(&self) -> bool {
        self.scrub.is_some()
    }

    /// The pointer position of the active scrub gesture, for a caller-owned scrub cursor.
    pub const fn scrub_position(&self) -> Option<Point> {
        match self.scrub {
            Some(session) => Some(session.position),
            None => None,
        }
    }

    /// The axis a mounted scrub area follows.
    pub const fn scrub_direction_value(&self) -> NumberFieldScrubDirection {
        self.scrub_direction
    }

    /// The logical pixels a scrub gesture travels per step.
    pub const fn scrub_sensitivity_value(&self) -> f32 {
        self.scrub_sensitivity
    }

    /// Whether a stepped value lands on the step grid.
    pub const fn snaps_on_step(&self) -> bool {
        self.snap_on_step
    }

    /// Whether focused wheel input steps the value.
    pub const fn allows_wheel_scrub(&self) -> bool {
        self.allow_wheel_scrub
    }

    /// The amount one small (Alt) gesture moves, one tenth of a step by default.
    pub const fn small_step_value(&self) -> f64 {
        if self.small_step > 0.0 {
            self.small_step
        } else {
            self.step * 0.1
        }
    }

    /// The amount one large (Shift) gesture moves, ten steps by default.
    pub const fn large_step_value(&self) -> f64 {
        if self.large_step > 0.0 {
            self.large_step
        } else {
            self.step * 10.0
        }
    }

    /// The amount one gesture of the given size moves.
    pub const fn step_amount(&self, size: NumberFieldStepSize) -> f64 {
        match size {
            NumberFieldStepSize::Normal => self.step,
            NumberFieldStepSize::Large => self.large_step_value(),
            NumberFieldStepSize::Small => self.small_step_value(),
        }
    }

    /// A copyable snapshot of what Base UI exposes as `data-*` attributes.
    pub fn state(&self) -> NumberFieldPartState {
        NumberFieldPartState {
            scrubbing: self.is_scrubbing(),
            stepping: self.is_stepping(),
            disabled: self.disabled,
            read_only: self.read_only,
            required: self.required,
            valid: self.is_valid(),
        }
    }

    /// Whether this field refuses every value change.
    const fn is_locked(&self) -> bool {
        self.disabled || self.read_only
    }

    /// Whether the current text parses to a value inside the field's range.
    ///
    /// An empty field is valid; use [`crate::Field`] for a required-value contract.
    pub fn is_valid(&self) -> bool {
        if self.text.trim().is_empty() {
            return true;
        }
        self.format
            .parse(&self.text)
            .is_some_and(|value| value >= self.minimum && value <= self.maximum)
    }

    /// Replace the editing text from a controlled input listener, returning whether it changed.
    ///
    /// Text is neither clamped nor reformatted while the user types; only [`Self::commit`] does
    /// that. Input longer than [`MAX_NUMBER_FIELD_TEXT_BYTES`] is truncated on a character
    /// boundary rather than retained.
    pub fn set_text(&mut self, text: impl Into<Arc<str>>) -> bool {
        if self.is_locked() {
            return false;
        }
        let text = bounded_text(text.into());
        if self.text == text {
            return false;
        }
        self.text = text;
        self.value = self.format.parse(&self.text);
        true
    }

    /// Clamp and reformat the current text, returning whether anything changed.
    ///
    /// Call this on Return and on blur. Unparseable text restores the last committed value; an
    /// empty field stays empty.
    pub fn commit(&mut self) -> bool {
        if self.is_locked() {
            return false;
        }
        if self.text.trim().is_empty() {
            let changed = self.value.is_some() || self.committed.is_some() || !self.text.is_empty();
            self.value = None;
            self.committed = None;
            self.text = Arc::from("");
            return changed;
        }
        let parsed = self.format.parse(&self.text).or(self.committed);
        let Some(value) = parsed else {
            let changed = !self.text.is_empty();
            self.text = Arc::from("");
            self.value = None;
            return changed;
        };
        let value = value.clamp(self.minimum, self.maximum);
        let text = self.format.format(value);
        let changed = self.value != Some(value) || self.text != text;
        self.value = Some(value);
        self.committed = Some(value);
        self.text = text;
        changed
    }

    /// Move the value by `steps` steps, clamping and reformatting, returning whether it changed.
    ///
    /// An empty field starts from zero clamped into range, matching desktop spin buttons.
    pub fn step_by(&mut self, steps: f64) -> bool {
        self.step_by_amount(steps, self.step)
    }

    /// Move the value by `steps` gestures of the given size.
    ///
    /// This is the Base UI modifier contract: `Large` applies [`Self::large_step_value`] and
    /// `Small` applies [`Self::small_step_value`], so one keyboard, wheel, or scrub path covers
    /// Shift and Alt without the application re-deriving the amounts.
    pub fn step_by_size(&mut self, steps: f64, size: NumberFieldStepSize) -> bool {
        self.step_by_amount(steps, self.step_amount(size))
    }

    /// Move the value by `steps` gestures whose size comes from held keyboard modifiers.
    pub fn step_with_modifiers(&mut self, steps: f64, modifiers: Modifiers) -> bool {
        self.step_by_size(steps, NumberFieldStepSize::from_modifiers(modifiers))
    }

    fn step_by_amount(&mut self, steps: f64, amount: f64) -> bool {
        if self.is_locked() || !steps.is_finite() || !amount.is_finite() {
            return false;
        }
        let current = self
            .format
            .parse(&self.text)
            .or(self.committed)
            .unwrap_or_else(|| 0.0_f64.clamp(self.minimum, self.maximum));
        let moved = current + amount * steps;
        let value = self
            .snapped(moved, amount)
            .clamp(self.minimum, self.maximum);
        let text = self.format.format(value);
        let changed = self.value != Some(value) || self.text != text;
        self.value = Some(value);
        self.committed = Some(value);
        self.text = text;
        changed
    }

    /// Round a stepped value onto the step grid when `snap_on_step` is declared.
    fn snapped(&self, value: f64, amount: f64) -> f64 {
        if !self.snap_on_step || amount <= 0.0 || !value.is_finite() {
            return value;
        }
        let origin = if self.minimum.is_finite() {
            self.minimum
        } else {
            0.0
        };
        origin + ((value - origin) / amount).round() * amount
    }

    /// Step once toward the maximum.
    pub fn increment(&mut self) -> bool {
        self.step_by(1.0)
    }

    /// Step once toward the minimum.
    pub fn decrement(&mut self) -> bool {
        self.step_by(-1.0)
    }

    /// Step from a scroll wheel, which desktop platforms only apply to a focused field.
    ///
    /// `delta` is logical pixels or lines; only its sign is used, so trackpad inertia cannot run
    /// the value away.
    pub fn wheel(&mut self, delta: f32, focused: bool) -> bool {
        self.wheel_with_modifiers(delta, focused, Modifiers::empty())
    }

    /// Step from a scroll wheel, applying the Shift and Alt step sizes.
    ///
    /// Declaring `allow_wheel_scrub(false)` — Base UI's `allowWheelScrub` — makes both wheel entry
    /// points inert.
    pub fn wheel_with_modifiers(
        &mut self,
        delta: f32,
        focused: bool,
        modifiers: Modifiers,
    ) -> bool {
        if !self.allow_wheel_scrub || !focused || !delta.is_finite() || delta == 0.0 {
            return false;
        }
        self.step_with_modifiers(if delta > 0.0 { 1.0 } else { -1.0 }, modifiers)
    }

    /// Press and hold one stepper: steps once and arms the first repeat deadline.
    pub fn press_step(&mut self, forward: bool, now: Instant) -> bool {
        if self.is_locked() {
            return false;
        }
        self.repeat = Some((
            if forward {
                RepeatDirection::Increment
            } else {
                RepeatDirection::Decrement
            },
            now + NUMBER_FIELD_REPEAT_DELAY,
        ));
        self.step_by(if forward { 1.0 } else { -1.0 })
    }

    /// The exact instant at which the held stepper next repeats.
    ///
    /// This is `None` while no stepper is held, so a settled field schedules no wakeup at all.
    /// Sleep until this instant with [`crate::AsyncViewContext::sleep_until`] and then call
    /// [`Self::repeat`].
    pub const fn repeat_deadline(&self) -> Option<Instant> {
        match self.repeat {
            Some((_, deadline)) => Some(deadline),
            None => None,
        }
    }

    /// Apply every repeat that is due at `now` and re-arm the next one.
    ///
    /// Returns whether the value changed. A single late wakeup applies at most one step per
    /// elapsed interval, so a delayed event loop cannot make the value jump unpredictably.
    pub fn repeat(&mut self, now: Instant) -> bool {
        let Some((direction, deadline)) = self.repeat else {
            return false;
        };
        if now < deadline {
            return false;
        }
        let elapsed = now.duration_since(deadline);
        let extra = elapsed.as_nanos() / NUMBER_FIELD_REPEAT_INTERVAL.as_nanos().max(1);
        let steps = 1 + u32::try_from(extra).unwrap_or(u32::MAX);
        let next = deadline + NUMBER_FIELD_REPEAT_INTERVAL * steps;
        self.repeat = Some((direction, next));
        let amount = f64::from(steps)
            * match direction {
                RepeatDirection::Increment => 1.0,
                RepeatDirection::Decrement => -1.0,
            };
        self.step_by(amount)
    }

    /// Release the held stepper, returning whether a repeat was pending.
    ///
    /// After this the field owns no deadline, task, timer, observer, or idle scheduler source.
    pub fn release_step(&mut self) -> bool {
        self.repeat.take().is_some()
    }

    /// Whether a stepper is currently held.
    pub const fn is_stepping(&self) -> bool {
        self.repeat.is_some()
    }

    /// Apply one captured pointer event from a mounted scrub area.
    ///
    /// This is Base UI's ScrubArea: dragging over the area changes the value without touching the
    /// text caret. Motion is accumulated in logical pixels and converted to whole steps at
    /// [`Self::scrub_sensitivity`], so a slow drag still moves exactly one step at a time and a
    /// fast one never loses a fraction. Dragging right increases a horizontal scrub area and
    /// dragging up increases a vertical one; the held modifiers select the small or large step.
    ///
    /// The gesture is pure pointer capture: it schedules no task, timer, or repeat, and a released
    /// scrub leaves the field with no retained session at all. Returns whether anything the
    /// application renders changed, including the scrubbing flag itself.
    pub fn apply_scrub(&mut self, event: &PointerEvent) -> bool {
        if self.is_locked() {
            return false;
        }
        match event.phase {
            PointerPhase::Down => {
                let started = self.scrub.is_none();
                self.scrub = Some(ScrubSession {
                    remainder: 0.0,
                    position: event.position,
                });
                started
            }
            PointerPhase::Move => {
                let Some(mut session) = self.scrub else {
                    return false;
                };
                let travel = match self.scrub_direction {
                    NumberFieldScrubDirection::Horizontal => event.delta.x,
                    NumberFieldScrubDirection::Vertical => -event.delta.y,
                    NumberFieldScrubDirection::Both => event.delta.x - event.delta.y,
                };
                let moved = session.position != event.position;
                session.position = event.position;
                if !travel.is_finite() {
                    self.scrub = Some(session);
                    return moved;
                }
                session.remainder += travel;
                let steps = (session.remainder / self.scrub_sensitivity).trunc();
                session.remainder -= steps * self.scrub_sensitivity;
                self.scrub = Some(session);
                let stepped =
                    steps != 0.0 && self.step_with_modifiers(f64::from(steps), event.modifiers);
                stepped || moved
            }
            PointerPhase::Up | PointerPhase::Cancel => self.scrub.take().is_some(),
        }
    }

    /// End a scrub gesture the application cancelled itself.
    pub fn end_scrub(&mut self) -> bool {
        self.scrub.take().is_some()
    }
}

fn bounded_text(text: Arc<str>) -> Arc<str> {
    if text.len() <= MAX_NUMBER_FIELD_TEXT_BYTES {
        return text;
    }
    let mut end = MAX_NUMBER_FIELD_TEXT_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    Arc::from(&text[..end])
}

/// A controlled, unstyled number-field descriptor.
///
/// The application owns the input's appearance, the stepper glyphs, and the layout. QuickGUI
/// supplies stable identities, the SpinButton role with numeric value, bounds and step, invalid
/// state, and the ordinary controlled text input underneath.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "a NumberField descriptor has no effect until its parts are mounted"]
pub struct NumberField {
    root_id: ElementId,
}

impl NumberField {
    pub fn new(root_id: impl Into<ElementId>) -> Self {
        Self {
            root_id: root_id.into(),
        }
    }

    pub const fn root_id(self) -> ElementId {
        self.root_id
    }

    pub fn input_id(self) -> ElementId {
        derived_number_field_id(self.root_id, NUMBER_FIELD_INPUT_ID_TAG)
    }

    pub fn increment_id(self) -> ElementId {
        derived_number_field_id(self.root_id, NUMBER_FIELD_INCREMENT_ID_TAG)
    }

    pub fn decrement_id(self) -> ElementId {
        derived_number_field_id(self.root_id, NUMBER_FIELD_DECREMENT_ID_TAG)
    }

    /// Stable identity of the group that wraps the steppers and the input.
    pub fn group_id(self) -> ElementId {
        derived_number_field_id(self.root_id, NUMBER_FIELD_GROUP_ID_TAG)
    }

    /// Stable identity of the scrub area.
    pub fn scrub_area_id(self) -> ElementId {
        derived_number_field_id(self.root_id, NUMBER_FIELD_SCRUB_AREA_ID_TAG)
    }

    /// Stable identity of the caller-owned cursor shown while scrubbing.
    pub fn scrub_area_cursor_id(self) -> ElementId {
        derived_number_field_id(self.root_id, NUMBER_FIELD_SCRUB_CURSOR_ID_TAG)
    }

    /// Decorate an application-owned root without adding layout or appearance.
    pub fn root_with(self, root: Element) -> Element {
        root.id(self.root_id)
            .accessibility_role(AccessibilityRole::Group)
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(self) -> Element {
        self.root_with(crate::div())
    }

    /// Decorate the application-owned controlled input.
    ///
    /// Pass a [`crate::text_input`] built from [`NumberFieldState::text`] with the caller's own
    /// [`crate::Element::on_input`] listener. QuickGUI adds the spin-button semantics: the
    /// committed numeric value, the field's bounds and step, and invalid state.
    pub fn input_with(self, state: &NumberFieldState, input: Element) -> Element {
        let mut range = AccessibilityValueRange {
            value: state.value,
            min: state.minimum.is_finite().then_some(state.minimum),
            max: state.maximum.is_finite().then_some(state.maximum),
            step: None,
        };
        range = range.step(state.step);
        input
            .id(self.input_id())
            .accessibility_role(AccessibilityRole::SpinButton)
            .accessibility_value_range(range)
            .invalid(!state.is_valid())
            .disabled(state.disabled)
            .accessibility_read_only(state.read_only)
            .required(state.required)
            .app_region_no_drag()
    }
    /// Create the unstyled input part. Use [`Self::input_with`] to supply an existing element.
    pub fn input(self, state: &NumberFieldState) -> Element {
        self.input_with(state, crate::text_input(""))
    }

    /// Decorate the application-owned increment button.
    ///
    /// Steppers stay out of the Tab sequence because the input already answers arrow keys, which
    /// matches native desktop spin buttons.
    pub fn increment_with(self, state: &NumberFieldState, increment: Element) -> Element {
        stepper(increment, self.increment_id(), state.disabled)
    }
    /// Create the unstyled increment part. Use [`Self::increment_with`] to supply an existing element.
    pub fn increment(self, state: &NumberFieldState) -> Element {
        self.increment_with(state, crate::button())
    }

    /// Decorate the application-owned decrement button.
    pub fn decrement_with(self, state: &NumberFieldState, decrement: Element) -> Element {
        stepper(decrement, self.decrement_id(), state.disabled)
    }
    /// Create the unstyled decrement part. Use [`Self::decrement_with`] to supply an existing element.
    pub fn decrement(self, state: &NumberFieldState) -> Element {
        self.decrement_with(state, crate::button())
    }

    /// Decorate the caller-owned group that wraps the decrement, input, and increment parts.
    ///
    /// Base UI's Group keeps the three controls one addressable unit; QuickGUI supplies the stable
    /// identity and the Group role and adds no layout, so the application still chooses the row,
    /// the order, and the spacing.
    pub fn group_with(self, group: Element) -> Element {
        group
            .id(self.group_id())
            .accessibility_role(AccessibilityRole::Group)
    }
    /// Create the unstyled group part. Use [`Self::group_with`] to supply an existing element.
    pub fn group(self) -> Element {
        self.group_with(crate::div())
    }

    /// Decorate the caller-owned area a pointer drag scrubs the value over.
    ///
    /// Attach a [`crate::ViewContext::pointer_listener`] registered for [`Self::scrub_area_id`] and
    /// forward the event to [`NumberFieldState::apply_scrub`]. QuickGUI supplies the identity, the
    /// axis-appropriate resize cursor, drag exclusion, and text-selection suppression; the area is
    /// hidden from assistive technology because the input already carries the spin-button
    /// semantics.
    pub fn scrub_area_with(self, state: &NumberFieldState, scrub_area: Element) -> Element {
        let scrub_area = scrub_area
            .id(self.scrub_area_id())
            .accessibility_hidden(true)
            .app_region_no_drag()
            .user_select_none();
        if state.disabled || state.read_only {
            return scrub_area.cursor_default();
        }
        match state.scrub_direction {
            NumberFieldScrubDirection::Vertical => scrub_area.cursor_ns_resize(),
            NumberFieldScrubDirection::Horizontal | NumberFieldScrubDirection::Both => {
                scrub_area.cursor_ew_resize()
            }
        }
    }
    /// Create the unstyled scrub area part. Use [`Self::scrub_area_with`] to supply an existing element.
    pub fn scrub_area(self, state: &NumberFieldState) -> Element {
        self.scrub_area_with(state, crate::div())
    }

    /// Decorate the caller-owned cursor a scrub area shows while it is being dragged.
    ///
    /// Mount it only while [`NumberFieldState::is_scrubbing`] is true and place it from
    /// [`NumberFieldState::scrub_position`]; QuickGUI supplies the identity and keeps the
    /// decoration out of the accessible name and out of hit testing.
    pub fn scrub_area_cursor_with(self, cursor: Element) -> Element {
        cursor
            .id(self.scrub_area_cursor_id())
            .accessibility_hidden(true)
            .app_region_no_drag()
            .user_select_none()
    }
    /// Create the unstyled scrub area cursor part. Use [`Self::scrub_area_cursor_with`] to supply an existing element.
    pub fn scrub_area_cursor(self) -> Element {
        self.scrub_area_cursor_with(crate::div())
    }
}

fn stepper(element: Element, id: ElementId, disabled: bool) -> Element {
    element
        .id(id)
        .accessibility_role(AccessibilityRole::Button)
        .clickable()
        .tab_index(-1)
        .cursor_default()
        .app_region_no_drag()
        .user_select_none()
        .disabled(disabled)
}

/// Create an unstyled controlled number-field input.
///
/// This shorthand is equivalent to
/// `NumberField::new(id).input_with(state, text_input(state.text().clone()))`.
pub fn number_field(id: impl Into<ElementId>, state: &NumberFieldState) -> Element {
    NumberField::new(id).input_with(state, text_input(state.text().clone()))
}

/// Create an unstyled number-field root.
pub fn number_field_root(id: impl Into<ElementId>) -> Element {
    NumberField::new(id).root_with(div())
}

fn derived_number_field_id(scope: ElementId, tag: u64) -> ElementId {
    let mut hash = scope.as_u64().rotate_left(11) ^ tag;
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    if hash == 0 || hash == u64::MAX || hash == scope.as_u64() {
        hash ^= tag.rotate_left(17);
    }
    ElementId::new(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, IntoElement, Key, TestAppContext, View, ViewContext, button, text};

    #[test]
    fn parsing_and_formatting_follow_caller_supplied_separators() {
        let plain = NumberFieldFormat::default();
        assert_eq!(plain.parse("42"), Some(42.0));
        assert_eq!(plain.parse("  -3.5 "), Some(-3.5));
        assert_eq!(plain.parse("+7"), Some(7.0));
        assert_eq!(plain.parse(""), None);
        assert_eq!(plain.parse("."), None);
        assert_eq!(plain.parse("1.2.3"), None);
        assert_eq!(plain.parse("12a"), None);
        assert_eq!(plain.parse("1e3"), None);
        assert_eq!(plain.parse("１２３"), None);
        assert_eq!(
            plain.parse(&"9".repeat(MAX_NUMBER_FIELD_TEXT_BYTES + 1)),
            None
        );
        assert_eq!(plain.format(42.0).as_ref(), "42");
        assert_eq!(plain.format(f64::NAN).as_ref(), "");

        let unsigned = NumberFieldFormat::default().sign(false);
        assert_eq!(unsigned.parse("-3"), None);
        assert_eq!(unsigned.parse("3"), Some(3.0));

        let scientific = NumberFieldFormat::default().exponent(true);
        assert_eq!(scientific.parse("1.5e3"), Some(1_500.0));
        assert_eq!(scientific.parse("1.5e-3"), Some(0.0015));
        assert_eq!(scientific.parse("e3"), None);

        let european = NumberFieldFormat::default()
            .decimal_separator(',')
            .group_separator(Some('.'));
        assert_eq!(european.parse("1.234,56"), Some(1_234.56));
        assert_eq!(european.parse("1234,56"), Some(1_234.56));
        assert_eq!(european.parse(".5"), None);
        assert_eq!(european.format(1_234.5).as_ref(), "1.234,5");
        assert_eq!(european.format(-9_876_543.0).as_ref(), "-9.876.543");
        assert_eq!(european.format(12.0).as_ref(), "12");

        // A digit or sign is never accepted as a separator.
        assert_eq!(
            NumberFieldFormat::default()
                .decimal_separator('5')
                .decimal(),
            '.'
        );
        assert_eq!(
            NumberFieldFormat::default()
                .group_separator(Some('-'))
                .group(),
            None
        );

        let fixed = NumberFieldFormat::default().precision(2);
        assert_eq!(fixed.format(4.56789).as_ref(), "4.57");
        assert_eq!(fixed.format(2.0).as_ref(), "2.00");
        assert_eq!(
            NumberFieldFormat::default()
                .precision(200)
                .precision_digits(),
            Some(MAX_NUMBER_FIELD_PRECISION)
        );
    }

    #[test]
    fn state_edits_commit_clamp_and_step_within_bounds() {
        let mut state = NumberFieldState::new(5.0).range(0.0, 10.0).step(2.0);
        assert_eq!(state.text().as_ref(), "5");
        assert_eq!(state.value(), Some(5.0));
        assert!(state.is_valid());

        assert!(state.set_text("7"));
        assert!(!state.set_text("7"));
        assert_eq!(state.value(), Some(7.0));
        assert!(state.is_valid());
        assert!(!state.commit());

        // Typing keeps arbitrary text; only committing clamps and reformats it.
        assert!(state.set_text("400"));
        assert_eq!(state.text().as_ref(), "400");
        assert!(!state.is_valid());
        assert!(state.commit());
        assert_eq!(state.text().as_ref(), "10");
        assert_eq!(state.value(), Some(10.0));

        assert!(state.set_text("abc"));
        assert_eq!(state.value(), None);
        assert_eq!(state.committed_value(), Some(10.0));
        assert!(!state.is_valid());
        assert!(state.commit());
        assert_eq!(state.value(), Some(10.0));
        assert_eq!(state.text().as_ref(), "10");

        assert!(state.set_text(""));
        assert!(state.is_valid());
        assert!(state.commit());
        assert_eq!(state.value(), None);
        assert_eq!(state.text().as_ref(), "");

        // Stepping an empty field starts from zero clamped into range.
        assert!(state.step_by(1.0));
        assert_eq!(state.value(), Some(2.0));
        assert!(state.increment());
        assert_eq!(state.value(), Some(4.0));
        assert!(state.decrement());
        assert_eq!(state.value(), Some(2.0));
        assert!(state.step_by(-100.0));
        assert_eq!(state.value(), Some(0.0));
        assert!(!state.step_by(-1.0));
        assert!(!state.step_by(f64::NAN));

        assert!(state.wheel(1.0, true));
        assert_eq!(state.value(), Some(2.0));
        assert!(!state.wheel(1.0, false));
        assert!(!state.wheel(0.0, true));
        assert!(state.wheel(-40.0, true));
        assert_eq!(state.value(), Some(0.0));

        let long = "1".repeat(MAX_NUMBER_FIELD_TEXT_BYTES + 12);
        assert!(state.set_text(long));
        assert_eq!(state.text().len(), MAX_NUMBER_FIELD_TEXT_BYTES);

        let mut disabled = NumberFieldState::new(1.0).disabled(true);
        assert!(!disabled.set_text("9"));
        assert!(!disabled.increment());
        assert!(!disabled.commit());

        let inverted = NumberFieldState::new(0.0).range(10.0, -10.0);
        assert_eq!((inverted.minimum(), inverted.maximum()), (-10.0, 10.0));
        let unbounded = NumberFieldState::new(0.0).range(f64::NAN, 4.0);
        assert_eq!(unbounded.minimum(), f64::NEG_INFINITY);
        assert!(NumberFieldState::empty().value().is_none());
        assert!(NumberFieldState::new(f64::NAN).value().is_none());
        assert_eq!(
            NumberFieldState::new(1.5).precision(2).text().as_ref(),
            "1.50"
        );
        assert_eq!(NumberFieldState::new(1.0).step(-4.0).step_value(), 1.0);
    }

    #[test]
    fn press_and_hold_uses_exact_deadlines_and_stops_on_release() {
        let start = Instant::now();
        let mut state = NumberFieldState::new(0.0).range(0.0, 1_000.0).step(1.0);
        assert_eq!(state.repeat_deadline(), None);
        assert!(!state.repeat(start));

        assert!(state.press_step(true, start));
        assert_eq!(state.value(), Some(1.0));
        assert_eq!(
            state.repeat_deadline(),
            Some(start + NUMBER_FIELD_REPEAT_DELAY)
        );
        assert!(state.is_stepping());

        // Nothing happens before the exact deadline.
        assert!(!state.repeat(start + NUMBER_FIELD_REPEAT_DELAY - Duration::from_millis(1)));
        assert_eq!(state.value(), Some(1.0));

        let first = start + NUMBER_FIELD_REPEAT_DELAY;
        assert!(state.repeat(first));
        assert_eq!(state.value(), Some(2.0));
        assert_eq!(
            state.repeat_deadline(),
            Some(first + NUMBER_FIELD_REPEAT_INTERVAL)
        );

        // One late wakeup applies exactly the steps that came due, not an unbounded burst.
        let late = first + NUMBER_FIELD_REPEAT_INTERVAL * 4;
        assert!(state.repeat(late));
        assert_eq!(state.value(), Some(6.0));
        assert_eq!(
            state.repeat_deadline(),
            Some(first + NUMBER_FIELD_REPEAT_INTERVAL * 5)
        );

        assert!(state.release_step());
        assert!(!state.release_step());
        assert_eq!(state.repeat_deadline(), None);
        assert!(!state.is_stepping());
        assert!(!state.repeat(late + Duration::from_secs(10)));
        assert_eq!(state.value(), Some(6.0));

        let mut down = NumberFieldState::new(5.0).range(0.0, 10.0);
        assert!(down.press_step(false, start));
        assert_eq!(down.value(), Some(4.0));
        assert!(down.repeat(start + NUMBER_FIELD_REPEAT_DELAY));
        assert_eq!(down.value(), Some(3.0));

        let mut disabled = NumberFieldState::new(1.0).disabled(true);
        assert!(!disabled.press_step(true, start));
        assert_eq!(disabled.repeat_deadline(), None);
    }

    #[test]
    fn parts_add_exact_semantics_without_appearance() {
        let state = NumberFieldState::new(5.0).range(0.0, 10.0).step(2.0);
        let field = NumberField::new("quantity");
        let root = field.root_with(div().bg(Color::rgb8(1, 2, 3)));
        assert_eq!(root.explicit_id, Some("quantity".into()));
        assert_eq!(root.accessibility.role, AccessibilityRole::Group);
        assert_eq!(root.visual.background, Some(Color::rgb8(1, 2, 3)));

        let input = field.input_with(&state, text_input(state.text().clone()).w(80.0));
        assert_eq!(input.explicit_id, Some(field.input_id()));
        assert_eq!(input.accessibility.role, AccessibilityRole::SpinButton);
        assert_eq!(
            input.accessibility.value_range.as_deref(),
            Some(&AccessibilityValueRange::new(5.0, 0.0, 10.0).step(2.0))
        );
        assert!(!input.accessibility.invalid);

        let mut invalid = state.clone();
        assert!(invalid.set_text("99"));
        let invalid_input = field.input_with(&invalid, text_input(invalid.text().clone()));
        assert!(invalid_input.accessibility.invalid);

        let unbounded = NumberFieldState::new(3.0);
        let unbounded_input = field.input_with(&unbounded, text_input(unbounded.text().clone()));
        let range = unbounded_input
            .accessibility
            .value_range
            .as_deref()
            .expect("value range");
        assert_eq!(range.value, Some(3.0));
        assert_eq!(range.min, None);
        assert_eq!(range.max, None);

        let increment = field.increment_with(&state, div().child("+"));
        assert_eq!(increment.explicit_id, Some(field.increment_id()));
        assert_eq!(increment.accessibility.role, AccessibilityRole::Button);
        assert!(increment.clickable);
        assert_eq!(increment.tab_index, -1);
        let decrement = field.decrement_with(&state, div().child("-"));
        assert_eq!(decrement.explicit_id, Some(field.decrement_id()));

        let disabled = NumberFieldState::new(1.0).disabled(true);
        assert!(
            field
                .increment_with(&disabled, div())
                .accessibility
                .disabled
        );

        let ids = [
            field.root_id(),
            field.input_id(),
            field.increment_id(),
            field.decrement_id(),
        ];
        for (index, id) in ids.iter().enumerate() {
            assert!(!ids[..index].contains(id));
        }
    }

    struct NumberFieldView {
        quantity: NumberFieldState,
    }

    impl Default for NumberFieldView {
        fn default() -> Self {
            Self {
                quantity: NumberFieldState::new(5.0).range(0.0, 10.0).step(2.0),
            }
        }
    }

    impl View for NumberFieldView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let field = NumberField::new("quantity");
            let edit = cx.input_listener(field.input_id(), |view: &mut Self, value, cx| {
                if view.quantity.set_text(value) {
                    cx.invalidate();
                }
            });
            // The input is marked invalid while its text is out of range, and QuickGUI blocks
            // Return submission for an invalid control, so commit from an ordinary key listener.
            let commit = cx.key_down_listener(field.input_id(), |view: &mut Self, event, cx| {
                if event.key == Key::Enter && view.quantity.commit() {
                    cx.invalidate();
                }
            });
            let up = cx.listener(field.increment_id(), |view: &mut Self, cx| {
                if view.quantity.increment() {
                    cx.invalidate();
                }
            });
            let down = cx.listener(field.decrement_id(), |view: &mut Self, cx| {
                if view.quantity.decrement() {
                    cx.invalidate();
                }
            });

            field.root_with(
                div()
                    .child(
                        field.input_with(
                            &self.quantity,
                            text_input(self.quantity.text().clone())
                                .w(120.0)
                                .on_input(edit)
                                .on_key_down(commit),
                        ),
                    )
                    .child(
                        field
                            .increment_with(&self.quantity, button().child(text("+")).on_click(up)),
                    )
                    .child(
                        field.decrement_with(
                            &self.quantity,
                            button().child(text("-")).on_click(down),
                        ),
                    ),
            )
        }
    }

    #[test]
    fn controlled_editing_and_accessibility_paths_stay_deterministic() {
        let (mut cx, view) = TestAppContext::new(NumberFieldView::default()).unwrap();
        let window = view.window_handle();
        let field = NumberField::new("quantity");

        cx.click(window, field.increment_id()).unwrap();
        assert_eq!(
            cx.read(view, |view| view.quantity.value()).unwrap(),
            Some(7.0)
        );
        cx.click(window, field.decrement_id()).unwrap();
        cx.click(window, field.decrement_id()).unwrap();
        assert_eq!(
            cx.read(view, |view| view.quantity.value()).unwrap(),
            Some(3.0)
        );

        cx.focus(window, field.input_id()).unwrap();
        cx.simulate_input(window, "9").unwrap();
        assert_eq!(
            cx.read(view, |view| view.quantity.text().to_string())
                .unwrap(),
            "39"
        );
        cx.simulate_keystrokes(window, "enter").unwrap();
        assert_eq!(
            cx.read(view, |view| view.quantity.value()).unwrap(),
            Some(10.0)
        );

        let update = cx.accessibility_update(window).unwrap();
        let node = |id: ElementId| {
            update
                .nodes
                .iter()
                .find_map(|(node_id, node)| (node_id.0 == id.as_u64()).then_some(node))
                .expect("number field accessibility node")
        };
        let input = node(field.input_id());
        assert_eq!(input.role(), accesskit::Role::SpinButton);
        assert_eq!(input.numeric_value(), Some(10.0));
        assert_eq!(input.min_numeric_value(), Some(0.0));
        assert_eq!(input.max_numeric_value(), Some(10.0));
        assert_eq!(input.numeric_value_step(), Some(2.0));
        assert_eq!(node(field.increment_id()).role(), accesskit::Role::Button);

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }

    #[test]
    fn shorthands_are_semantic_unstyled_parts() {
        let state = NumberFieldState::new(2.0);
        let input = number_field("count", &state);
        assert_eq!(input.accessibility.role, AccessibilityRole::SpinButton);
        let root = number_field_root("count");
        assert_eq!(root.accessibility.role, AccessibilityRole::Group);
        assert!(root.children.is_empty());
    }

    fn scrub_event(phase: PointerPhase, dx: f32, dy: f32, modifiers: Modifiers) -> PointerEvent {
        PointerEvent {
            phase,
            position: crate::Point::new(100.0 + dx, 100.0 + dy),
            origin: crate::Point::new(100.0, 100.0),
            local_position: crate::Point::new(dx, dy),
            local_origin: crate::Point::ZERO,
            delta: crate::Vector::new(dx, dy),
            button: crate::MouseButton::Left,
            modifiers,
            size: crate::Size::new(40.0, 20.0),
        }
    }

    #[test]
    fn modifier_step_sizes_default_to_ten_and_a_tenth_of_the_step() {
        let state = NumberFieldState::new(0.0).step(2.0);
        assert_eq!(state.step_amount(NumberFieldStepSize::Normal), 2.0);
        assert_eq!(state.step_amount(NumberFieldStepSize::Large), 20.0);
        assert_eq!(state.step_amount(NumberFieldStepSize::Small), 0.2);

        let declared = state.small_step(0.5).large_step(50.0);
        assert_eq!(declared.small_step_value(), 0.5);
        assert_eq!(declared.large_step_value(), 50.0);
        // Non-finite or non-positive overrides fall back to the derived defaults.
        let restored = declared.small_step(f64::NAN).large_step(-1.0);
        assert_eq!(restored.small_step_value(), 0.2);
        assert_eq!(restored.large_step_value(), 20.0);

        assert_eq!(
            NumberFieldStepSize::from_modifiers(Modifiers::empty()),
            NumberFieldStepSize::Normal
        );
        assert_eq!(
            NumberFieldStepSize::from_modifiers(Modifiers::ALT),
            NumberFieldStepSize::Small
        );
        assert_eq!(
            NumberFieldStepSize::from_modifiers(Modifiers::SHIFT),
            NumberFieldStepSize::Large
        );
        // A coarse gesture wins when both modifiers are held.
        assert_eq!(
            NumberFieldStepSize::from_modifiers(Modifiers::SHIFT | Modifiers::ALT),
            NumberFieldStepSize::Large
        );

        let mut field = NumberFieldState::new(10.0).step(2.0).precision(1);
        assert!(field.step_with_modifiers(1.0, Modifiers::SHIFT));
        assert_eq!(field.value(), Some(30.0));
        assert!(field.step_with_modifiers(-1.0, Modifiers::ALT));
        assert_eq!(field.value(), Some(29.8));
        assert!(field.step_with_modifiers(1.0, Modifiers::empty()));
        assert_eq!(field.value(), Some(31.8));
    }

    #[test]
    fn snap_on_step_lands_on_the_grid_and_wheel_scrubbing_can_be_refused() {
        let mut loose = NumberFieldState::new(3.0).step(5.0);
        assert!(loose.increment());
        assert_eq!(loose.value(), Some(8.0));

        let mut snapped = NumberFieldState::new(3.0).step(5.0).snap_on_step(true);
        assert!(snapped.snaps_on_step());
        assert!(snapped.increment());
        assert_eq!(snapped.value(), Some(10.0));
        assert!(snapped.decrement());
        assert_eq!(snapped.value(), Some(5.0));

        // The grid is measured from the minimum when the field declares one.
        let mut offset = NumberFieldState::new(3.0)
            .range(1.0, 100.0)
            .step(5.0)
            .snap_on_step(true);
        assert!(offset.increment());
        assert_eq!(offset.value(), Some(6.0));

        let mut wheeled = NumberFieldState::new(0.0).step(1.0);
        assert!(wheeled.allows_wheel_scrub());
        assert!(wheeled.wheel(1.0, true));
        assert_eq!(wheeled.value(), Some(1.0));
        assert!(wheeled.wheel_with_modifiers(1.0, true, Modifiers::SHIFT));
        assert_eq!(wheeled.value(), Some(11.0));
        assert!(!wheeled.wheel(1.0, false));

        let mut refused = NumberFieldState::new(0.0).allow_wheel_scrub(false);
        assert!(!refused.allows_wheel_scrub());
        assert!(!refused.wheel(1.0, true));
        assert!(!refused.wheel_with_modifiers(-1.0, true, Modifiers::SHIFT));
        assert_eq!(refused.value(), Some(0.0));
    }

    #[test]
    fn a_read_only_field_refuses_every_change_without_leaving_the_tab_sequence() {
        let mut field = NumberFieldState::new(4.0)
            .step(1.0)
            .read_only(true)
            .required(true);
        assert!(field.is_read_only());
        assert!(field.is_required());
        assert!(!field.is_disabled());
        assert!(!field.increment());
        assert!(!field.set_text("9"));
        assert!(!field.commit());
        assert!(!field.wheel(1.0, true));
        assert!(!field.press_step(true, Instant::now()));
        assert!(!field.apply_scrub(&scrub_event(
            PointerPhase::Down,
            0.0,
            0.0,
            Modifiers::empty()
        )));
        assert_eq!(field.value(), Some(4.0));

        let number_field = NumberField::new("quantity");
        let input = number_field.input_with(&field, text_input(field.text().clone()));
        assert!(input.accessibility.read_only);
        assert!(input.accessibility.required);
        assert!(!input.accessibility.disabled);
        // A read-only scrub area shows no drag affordance.
        assert_eq!(
            number_field.scrub_area_with(&field, div()).cursor_style,
            Some(crate::CursorStyle::Arrow)
        );

        let state = field.state();
        assert!(state.read_only);
        assert!(state.required);
        assert!(!state.scrubbing);
        assert!(!state.stepping);
        assert!(state.valid);
    }

    #[test]
    fn a_captured_scrub_accumulates_pixels_into_whole_steps_and_retains_nothing_after_release() {
        let mut field = NumberFieldState::new(0.0)
            .step(1.0)
            .scrub_sensitivity(10.0)
            .range(-100.0, 100.0);
        assert_eq!(field.scrub_sensitivity_value(), 10.0);
        assert_eq!(
            field.scrub_direction_value(),
            NumberFieldScrubDirection::Horizontal
        );

        assert!(field.apply_scrub(&scrub_event(
            PointerPhase::Down,
            0.0,
            0.0,
            Modifiers::empty()
        )));
        assert!(field.is_scrubbing());
        assert_eq!(
            field.scrub_position(),
            Some(crate::Point::new(100.0, 100.0))
        );
        assert_eq!(field.value(), Some(0.0));

        // Less than one sensitivity of travel accumulates instead of stepping.
        field.apply_scrub(&scrub_event(
            PointerPhase::Move,
            6.0,
            0.0,
            Modifiers::empty(),
        ));
        assert_eq!(field.value(), Some(0.0));
        // The retained remainder makes the next short move cross the threshold exactly once.
        field.apply_scrub(&scrub_event(
            PointerPhase::Move,
            6.0,
            0.0,
            Modifiers::empty(),
        ));
        assert_eq!(field.value(), Some(1.0));
        // A long drag converts every whole step it travelled.
        field.apply_scrub(&scrub_event(
            PointerPhase::Move,
            35.0,
            0.0,
            Modifiers::empty(),
        ));
        assert_eq!(field.value(), Some(4.0));
        // Dragging back the other way reverses it, carrying the retained remainder with it.
        field.apply_scrub(&scrub_event(
            PointerPhase::Move,
            -40.0,
            0.0,
            Modifiers::empty(),
        ));
        assert_eq!(field.value(), Some(1.0));
        // The held modifier selects the large step.
        field.apply_scrub(&scrub_event(
            PointerPhase::Move,
            30.0,
            0.0,
            Modifiers::SHIFT,
        ));
        assert_eq!(field.value(), Some(21.0));

        assert!(field.apply_scrub(&scrub_event(PointerPhase::Up, 0.0, 0.0, Modifiers::empty())));
        assert!(!field.is_scrubbing());
        assert_eq!(field.scrub_position(), None);
        assert!(!field.end_scrub());
        // A move without a session is inert rather than resuming the gesture.
        assert!(!field.apply_scrub(&scrub_event(
            PointerPhase::Move,
            100.0,
            0.0,
            Modifiers::empty()
        )));
        assert_eq!(field.value(), Some(21.0));

        // A vertical area increases upward.
        let mut vertical = NumberFieldState::new(0.0)
            .step(1.0)
            .scrub_sensitivity(5.0)
            .scrub_direction(NumberFieldScrubDirection::Vertical);
        vertical.apply_scrub(&scrub_event(
            PointerPhase::Down,
            0.0,
            0.0,
            Modifiers::empty(),
        ));
        vertical.apply_scrub(&scrub_event(
            PointerPhase::Move,
            0.0,
            -10.0,
            Modifiers::empty(),
        ));
        assert_eq!(vertical.value(), Some(2.0));
        vertical.apply_scrub(&scrub_event(
            PointerPhase::Cancel,
            0.0,
            0.0,
            Modifiers::empty(),
        ));
        assert!(!vertical.is_scrubbing());

        // Sensitivity is bounded and a non-finite value restores the default.
        assert_eq!(
            NumberFieldState::new(0.0)
                .scrub_sensitivity(1.0e9)
                .scrub_sensitivity_value(),
            MAX_NUMBER_FIELD_SCRUB_SENSITIVITY
        );
        assert_eq!(
            NumberFieldState::new(0.0)
                .scrub_sensitivity(f32::NAN)
                .scrub_sensitivity_value(),
            DEFAULT_NUMBER_FIELD_SCRUB_SENSITIVITY
        );
    }

    #[test]
    fn base_ui_number_field_parts_have_distinct_identities_and_no_appearance() {
        let field = NumberFieldState::new(4.0).scrub_direction(NumberFieldScrubDirection::Vertical);
        let number_field = NumberField::new("quantity");
        let ids = [
            number_field.input_id(),
            number_field.increment_id(),
            number_field.decrement_id(),
            number_field.group_id(),
            number_field.scrub_area_id(),
            number_field.scrub_area_cursor_id(),
        ];
        for (index, id) in ids.iter().enumerate() {
            assert_ne!(*id, number_field.root_id());
            assert!(!ids[..index].contains(id));
        }

        let group = number_field.group_with(div().bg(crate::Color::rgb8(1, 2, 3)));
        assert_eq!(group.explicit_id, Some(number_field.group_id()));
        assert_eq!(group.accessibility.role, AccessibilityRole::Group);
        assert_eq!(group.visual.background, Some(crate::Color::rgb8(1, 2, 3)));

        let scrub = number_field.scrub_area_with(&field, div());
        assert_eq!(scrub.explicit_id, Some(number_field.scrub_area_id()));
        assert!(scrub.accessibility.hidden);
        assert_eq!(scrub.cursor_style, Some(crate::CursorStyle::ResizeUpDown));
        assert_eq!(scrub.visual.background, None);
        assert_eq!(
            number_field
                .scrub_area_with(
                    &field.scrub_direction(NumberFieldScrubDirection::Horizontal),
                    div()
                )
                .cursor_style,
            Some(crate::CursorStyle::ResizeLeftRight)
        );

        let cursor = number_field.scrub_area_cursor_with(div());
        assert_eq!(
            cursor.explicit_id,
            Some(number_field.scrub_area_cursor_id())
        );
        assert!(cursor.accessibility.hidden);
        assert_eq!(cursor.visual.background, None);
    }

    struct ScrubFieldView {
        amount: NumberFieldState,
        locked: NumberFieldState,
    }

    impl Default for ScrubFieldView {
        fn default() -> Self {
            Self {
                amount: NumberFieldState::new(20.0)
                    .range(0.0, 100.0)
                    .step(1.0)
                    .scrub_sensitivity(4.0),
                locked: NumberFieldState::new(7.0).read_only(true).required(true),
            }
        }
    }

    impl View for ScrubFieldView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let amount = NumberField::new("amount");
            let locked = NumberField::new("locked");
            let scrub =
                cx.pointer_listener(amount.scrub_area_id(), |view: &mut Self, event, cx| {
                    if view.amount.apply_scrub(event) {
                        cx.invalidate();
                    }
                });
            let mut scrub_area =
                amount.scrub_area_with(&self.amount, div().w(40.0).h(20.0).on_pointer(scrub));
            if self.amount.is_scrubbing() {
                scrub_area = scrub_area.child(amount.scrub_area_cursor_with(div().size(8.0, 8.0)));
            }
            div()
                .child(
                    amount.root_with(div()).child(
                        amount
                            .group_with(div().flex_row())
                            .child(amount.decrement_with(&self.amount, button().child(text("-"))))
                            .child(amount.input_with(
                                &self.amount,
                                text_input(self.amount.text().clone()).w(80.0),
                            ))
                            .child(amount.increment_with(&self.amount, button().child(text("+"))))
                            .child(scrub_area),
                    ),
                )
                .child(
                    locked.root_with(div()).child(
                        locked.input_with(
                            &self.locked,
                            text_input(self.locked.text().clone())
                                .w(80.0)
                                .accessibility_label("Locked amount"),
                        ),
                    ),
                )
        }
    }

    #[test]
    fn scrub_and_read_only_parts_mount_and_project_without_idle_work() {
        let (mut cx, view) = TestAppContext::new(ScrubFieldView::default()).unwrap();
        let window = view.window_handle();
        let amount = NumberField::new("amount");
        let locked = NumberField::new("locked");

        assert!(cx.contains_element(window, amount.group_id()).unwrap());
        assert!(cx.contains_element(window, amount.scrub_area_id()).unwrap());
        // The scrub cursor is mounted only while a gesture is active.
        assert!(
            !cx.contains_element(window, amount.scrub_area_cursor_id())
                .unwrap()
        );

        let update = cx.accessibility_update(window).unwrap();
        let node = |id: ElementId| {
            update
                .nodes
                .iter()
                .find_map(|(node_id, node)| (node_id.0 == id.as_u64()).then_some(node))
                .expect("number field accessibility node")
        };
        let group = node(amount.group_id());
        assert_eq!(group.role(), accesskit::Role::Group);

        let editable = node(amount.input_id());
        assert!(!editable.is_read_only());
        assert!(!editable.is_required());

        let read_only = node(locked.input_id());
        assert_eq!(read_only.role(), accesskit::Role::SpinButton);
        assert!(read_only.is_read_only());
        assert!(read_only.is_required());
        assert!(!read_only.is_disabled());
        assert_eq!(read_only.label(), Some("Locked amount"));

        // A read-only field stays focusable, unlike a disabled one.
        cx.focus(window, locked.input_id()).unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some(locked.input_id()));

        cx.update(view, |view, cx| {
            let started = view.amount.apply_scrub(&scrub_event(
                PointerPhase::Down,
                0.0,
                0.0,
                Modifiers::empty(),
            ));
            let moved = view.amount.apply_scrub(&scrub_event(
                PointerPhase::Move,
                12.0,
                0.0,
                Modifiers::empty(),
            ));
            assert!(started && moved);
            cx.invalidate();
        })
        .unwrap();
        assert_eq!(
            cx.read(view, |view| view.amount.value()).unwrap(),
            Some(23.0)
        );
        assert!(
            cx.contains_element(window, amount.scrub_area_cursor_id())
                .unwrap()
        );

        cx.update(view, |view, cx| {
            view.amount.end_scrub();
            cx.invalidate();
        })
        .unwrap();
        assert!(
            !cx.contains_element(window, amount.scrub_area_cursor_id())
                .unwrap()
        );

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }
}
