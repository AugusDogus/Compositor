use std::sync::Arc;

use crate::{
    AccessibilityRole, AccessibilityValueRange, Element, ElementId, FocusHandle, Key, KeyBinding,
    Modifiers, StateAccessor, ViewContext, div,
};

/// Smallest year one civil date, date field, or calendar accepts.
pub const MIN_CIVIL_YEAR: i32 = 1;
/// Largest year one civil date, date field, or calendar accepts.
///
/// Four digits keep every segment a fixed width, keep the retained state a plain copyable value,
/// and keep typed entry bounded without a locale or era database.
pub const MAX_CIVIL_YEAR: i32 = 9999;

/// Maximum UTF-8 bytes retained by one segment placeholder.
pub const MAX_DATE_FIELD_PLACEHOLDER_BYTES: usize = 16;

const DATE_FIELD_KEY_CONTEXT: &str = "DateField";
const TIME_FIELD_KEY_CONTEXT: &str = "TimeField";
const DATE_FIELD_SEGMENT_ID_TAG: u64 = 0x3f19_ba57_c208_71ed;
const TIME_FIELD_SEGMENT_ID_TAG: u64 = 0x9c40_1e83_57da_2b6f;

/// Step the focused date or time segment one unit toward its maximum, wrapping at the end.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DateFieldIncrement;
/// Step the focused date or time segment one unit toward its minimum, wrapping at the start.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DateFieldDecrement;
/// Move editing focus to the next segment in the field's declared order.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DateFieldNextSegment;
/// Move editing focus to the previous segment in the field's declared order.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DateFieldPreviousSegment;
/// Clear the focused segment and its pending typed digits.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DateFieldClearSegment;
/// Set the focused segment to its smallest legal value.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DateFieldSegmentMinimum;
/// Set the focused segment to its largest legal value.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DateFieldSegmentMaximum;

/// Contextual bindings used by [`DateFieldSegment::key_with`].
///
/// Tab and Shift-Tab already move between segments because every segment is an ordinary focusable
/// control; the arrow bindings add the desktop convention of moving inside one composite field.
pub fn date_field_key_bindings() -> [KeyBinding; 8] {
    segment_key_bindings(DATE_FIELD_KEY_CONTEXT)
}

/// Contextual bindings used by [`TimeFieldSegment::key_with`].
///
/// These bind the same typed actions as [`date_field_key_bindings`] in the time field's own key
/// context, so one application can install both without ambiguity.
pub fn time_field_key_bindings() -> [KeyBinding; 8] {
    segment_key_bindings(TIME_FIELD_KEY_CONTEXT)
}

fn segment_key_bindings(context: &str) -> [KeyBinding; 8] {
    [
        KeyBinding::new("up", DateFieldIncrement, Some(context)),
        KeyBinding::new("down", DateFieldDecrement, Some(context)),
        KeyBinding::new("right", DateFieldNextSegment, Some(context)),
        KeyBinding::new("left", DateFieldPreviousSegment, Some(context)),
        KeyBinding::new("backspace", DateFieldClearSegment, Some(context)),
        KeyBinding::new("delete", DateFieldClearSegment, Some(context)),
        KeyBinding::new("home", DateFieldSegmentMinimum, Some(context)),
        KeyBinding::new("end", DateFieldSegmentMaximum, Some(context)),
    ]
}

/// A proleptic Gregorian calendar date with no time zone, clock, or locale.
///
/// The fields are plain data so an application can destructure them. Construction through
/// [`CivilDate::new`] validates month, day, leap years, and the retained year bounds; a value
/// assembled by hand can be checked with [`CivilDate::is_valid`].
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CivilDate {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

impl CivilDate {
    /// Create a validated date, or `None` when the components are not a real calendar day.
    pub const fn new(year: i32, month: u8, day: u8) -> Option<Self> {
        let date = Self { year, month, day };
        if date.is_valid() { Some(date) } else { None }
    }

    /// Whether this value is a real day inside [`MIN_CIVIL_YEAR`]`..=`[`MAX_CIVIL_YEAR`].
    pub const fn is_valid(self) -> bool {
        self.year >= MIN_CIVIL_YEAR
            && self.year <= MAX_CIVIL_YEAR
            && self.month >= 1
            && self.month <= 12
            && self.day >= 1
            && self.day <= Self::days_in_month(self.year, self.month)
    }

    /// Proleptic Gregorian leap-year rule.
    pub const fn is_leap_year(year: i32) -> bool {
        year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
    }

    /// Days in one month, honoring leap years. An out-of-range month reports 31.
    pub const fn days_in_month(year: i32, month: u8) -> u8 {
        match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if Self::is_leap_year(year) {
                    29
                } else {
                    28
                }
            }
            _ => 31,
        }
    }

    /// Clamp the components into the retained bounds, shortening the day for short months.
    pub const fn clamped(self) -> Self {
        let year = if self.year < MIN_CIVIL_YEAR {
            MIN_CIVIL_YEAR
        } else if self.year > MAX_CIVIL_YEAR {
            MAX_CIVIL_YEAR
        } else {
            self.year
        };
        let month = if self.month < 1 {
            1
        } else if self.month > 12 {
            12
        } else {
            self.month
        };
        let last = Self::days_in_month(year, month);
        let day = if self.day < 1 {
            1
        } else if self.day > last {
            last
        } else {
            self.day
        };
        Self { year, month, day }
    }

    /// Days since 1970-01-01 for a valid date, using Howard Hinnant's `days_from_civil`.
    pub const fn epoch_day(self) -> i64 {
        let year = self.year as i64 - if self.month <= 2 { 1 } else { 0 };
        let era = if year >= 0 { year } else { year - 399 } / 400;
        let year_of_era = year - era * 400;
        let month = self.month as i64;
        let day = self.day as i64;
        let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        era * 146_097 + day_of_era - 719_468
    }

    /// The date `days` after 1970-01-01, or `None` outside the retained year bounds.
    ///
    /// This is Howard Hinnant's `civil_from_days`, the exact inverse of [`Self::epoch_day`].
    pub const fn from_epoch_day(days: i64) -> Option<Self> {
        let days = days + 719_468;
        let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
        let day_of_era = days - era * 146_097;
        let year_of_era =
            (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let year = year_of_era + era * 400;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let shifted_month = (5 * day_of_year + 2) / 153;
        let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u8;
        let month = (shifted_month + if shifted_month < 10 { 3 } else { -9 }) as u8;
        let year = year + if month <= 2 { 1 } else { 0 };
        if year < MIN_CIVIL_YEAR as i64 || year > MAX_CIVIL_YEAR as i64 {
            return None;
        }
        Self::new(year as i32, month, day)
    }

    /// Day of the week, where Monday is `0` and Sunday is `6`.
    pub const fn weekday(self) -> u8 {
        let day = self.epoch_day() + 3;
        (day.rem_euclid(7)) as u8
    }
}

/// A wall-clock time of day with no date, time zone, or leap seconds.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CivilTime {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl CivilTime {
    /// Create a validated 24-hour time, or `None` when a component is out of range.
    pub const fn new(hour: u8, minute: u8, second: u8) -> Option<Self> {
        let time = Self {
            hour,
            minute,
            second,
        };
        if time.is_valid() { Some(time) } else { None }
    }

    pub const fn is_valid(self) -> bool {
        self.hour <= 23 && self.minute <= 59 && self.second <= 59
    }

    /// Clamp every component into its legal range.
    pub const fn clamped(self) -> Self {
        Self {
            hour: if self.hour > 23 { 23 } else { self.hour },
            minute: if self.minute > 59 { 59 } else { self.minute },
            second: if self.second > 59 { 59 } else { self.second },
        }
    }

    /// The 12-hour half this time falls in.
    pub const fn period(self) -> CivilPeriod {
        if self.hour < 12 {
            CivilPeriod::Am
        } else {
            CivilPeriod::Pm
        }
    }

    /// The hour as it is displayed on a 12-hour clock, in `1..=12`.
    pub const fn hour12(self) -> u8 {
        match self.hour % 12 {
            0 => 12,
            hour => hour,
        }
    }
}

/// The half of a 12-hour clock a time falls in.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CivilPeriod {
    Am,
    Pm,
}

impl CivilPeriod {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Am => "AM",
            Self::Pm => "PM",
        }
    }
}

/// One editable part of a date field.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DateSegment {
    Year,
    Month,
    Day,
}

impl DateSegment {
    const fn index(self) -> usize {
        match self {
            Self::Year => 0,
            Self::Month => 1,
            Self::Day => 2,
        }
    }

    const fn digits(self) -> u8 {
        match self {
            Self::Year => 4,
            Self::Month | Self::Day => 2,
        }
    }

    /// The framework's default accessible name for this segment.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Year => "Year",
            Self::Month => "Month",
            Self::Day => "Day",
        }
    }
}

/// One editable part of a time field.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TimeSegment {
    Hour,
    Minute,
    Second,
    Period,
}

impl TimeSegment {
    const fn index(self) -> usize {
        match self {
            Self::Hour => 0,
            Self::Minute => 1,
            Self::Second => 2,
            Self::Period => 3,
        }
    }

    const fn digits(self) -> u8 {
        2
    }

    /// The framework's default accessible name for this segment.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Hour => "Hour",
            Self::Minute => "Minute",
            Self::Second => "Second",
            Self::Period => "AM or PM",
        }
    }
}

/// Segment order declared by one date field.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DateFieldOrder {
    /// ISO-style `2026-09-03`.
    #[default]
    YearMonthDay,
    /// `03/09/2026`.
    DayMonthYear,
    /// `09/03/2026`.
    MonthDayYear,
}

impl DateFieldOrder {
    /// The segments in the order the application lays them out.
    pub const fn segments(self) -> [DateSegment; 3] {
        match self {
            Self::YearMonthDay => [DateSegment::Year, DateSegment::Month, DateSegment::Day],
            Self::DayMonthYear => [DateSegment::Day, DateSegment::Month, DateSegment::Year],
            Self::MonthDayYear => [DateSegment::Month, DateSegment::Day, DateSegment::Year],
        }
    }
}

/// Pending typed digits for the segment that owns editing focus.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct SegmentBuffer {
    value: u32,
    len: u8,
}

impl SegmentBuffer {
    const fn clear(&mut self) {
        self.value = 0;
        self.len = 0;
    }
}

fn bounded_placeholder(text: impl Into<Arc<str>>, fallback: &'static str) -> Arc<str> {
    let text = text.into();
    if text.len() <= MAX_DATE_FIELD_PLACEHOLDER_BYTES {
        text
    } else {
        Arc::from(fallback)
    }
}

fn wrapped_step(value: i32, delta: i32, minimum: i32, maximum: i32) -> i32 {
    if maximum <= minimum {
        return minimum;
    }
    let span = maximum - minimum + 1;
    minimum + (value - minimum + delta).rem_euclid(span)
}

/// Controlled segmented editing state for one date field.
///
/// The application owns every visible declaration: separators, widths, colors, the placeholder
/// look, and the focused-segment highlight. QuickGUI owns the calendar contract — leap years, month
/// lengths, wrapping steps, typed-digit entry with automatic advance, bounded validation — plus
/// which segment is being edited and the native spin-button semantics of each one.
///
/// The state retains one bounded placeholder string per segment and no task, timer, observer, or
/// idle scheduler source.
#[derive(Clone, Debug, PartialEq)]
pub struct DateFieldState {
    year: Option<i32>,
    month: Option<u8>,
    day: Option<u8>,
    order: DateFieldOrder,
    focused: DateSegment,
    minimum: Option<CivilDate>,
    maximum: Option<CivilDate>,
    placeholders: [Arc<str>; 3],
    buffer: SegmentBuffer,
    disabled: bool,
}

impl Default for DateFieldState {
    fn default() -> Self {
        Self::new()
    }
}

impl DateFieldState {
    /// An empty field whose first segment owns editing focus.
    pub fn new() -> Self {
        Self {
            year: None,
            month: None,
            day: None,
            order: DateFieldOrder::default(),
            focused: DateFieldOrder::default().segments()[0],
            minimum: None,
            maximum: None,
            placeholders: [Arc::from("YYYY"), Arc::from("MM"), Arc::from("DD")],
            buffer: SegmentBuffer::default(),
            disabled: false,
        }
    }

    /// A field holding one complete date. An invalid date is clamped before it is retained.
    pub fn from_date(date: CivilDate) -> Self {
        let date = date.clamped();
        let mut state = Self::new();
        state.year = Some(date.year);
        state.month = Some(date.month);
        state.day = Some(date.day);
        state
    }

    /// Declare the segment order the application lays out.
    #[must_use]
    pub fn order(mut self, order: DateFieldOrder) -> Self {
        self.order = order;
        self.focused = order.segments()[0];
        self
    }

    /// Reject dates before this day. The bound is reported through validity, never by rewriting
    /// what the user typed.
    #[must_use]
    pub fn minimum(mut self, minimum: CivilDate) -> Self {
        self.minimum = Some(minimum.clamped());
        self
    }

    /// Reject dates after this day.
    #[must_use]
    pub fn maximum(mut self, maximum: CivilDate) -> Self {
        self.maximum = Some(maximum.clamped());
        self
    }

    /// Replace one segment's placeholder. Longer text than
    /// [`MAX_DATE_FIELD_PLACEHOLDER_BYTES`] keeps the default.
    #[must_use]
    pub fn placeholder(mut self, segment: DateSegment, text: impl Into<Arc<str>>) -> Self {
        let fallback = match segment {
            DateSegment::Year => "YYYY",
            DateSegment::Month => "MM",
            DateSegment::Day => "DD",
        };
        self.placeholders[segment.index()] = bounded_placeholder(text, fallback);
        self
    }

    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub const fn segment_order(&self) -> DateFieldOrder {
        self.order
    }

    pub const fn minimum_date(&self) -> Option<CivilDate> {
        self.minimum
    }

    pub const fn maximum_date(&self) -> Option<CivilDate> {
        self.maximum
    }

    pub const fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// The complete date, or `None` while any segment is empty or the combination is not real.
    pub fn value(&self) -> Option<CivilDate> {
        let (year, month, day) = (self.year?, self.month?, self.day?);
        CivilDate::new(year, month, day)
    }

    /// Replace the value. `None` clears every segment.
    pub fn set_value(&mut self, value: Option<CivilDate>) -> bool {
        let (year, month, day) = match value {
            Some(date) => {
                let date = date.clamped();
                (Some(date.year), Some(date.month), Some(date.day))
            }
            None => (None, None, None),
        };
        if (self.year, self.month, self.day) == (year, month, day) {
            return false;
        }
        self.year = year;
        self.month = month;
        self.day = day;
        self.buffer.clear();
        true
    }

    /// The raw numeric content of one segment.
    pub const fn segment_value(&self, segment: DateSegment) -> Option<i32> {
        match segment {
            DateSegment::Year => self.year,
            DateSegment::Month => match self.month {
                Some(month) => Some(month as i32),
                None => None,
            },
            DateSegment::Day => match self.day {
                Some(day) => Some(day as i32),
                None => None,
            },
        }
    }

    pub const fn is_filled(&self, segment: DateSegment) -> bool {
        self.segment_value(segment).is_some()
    }

    /// Whether every segment holds a value.
    pub const fn is_complete(&self) -> bool {
        self.year.is_some() && self.month.is_some() && self.day.is_some()
    }

    /// Whether the field is empty or holds a real date inside its declared bounds.
    ///
    /// A partly typed date is invalid: that is what a form needs to reject, and it matches the
    /// desktop and web behavior of a segmented date control.
    pub fn is_valid(&self) -> bool {
        if self.year.is_none() && self.month.is_none() && self.day.is_none() {
            return true;
        }
        let Some(value) = self.value() else {
            return false;
        };
        if self.minimum.is_some_and(|minimum| value < minimum) {
            return false;
        }
        self.maximum.is_none_or(|maximum| value <= maximum)
    }

    pub fn placeholder_text(&self, segment: DateSegment) -> &Arc<str> {
        &self.placeholders[segment.index()]
    }

    /// The text one segment displays: zero-padded digits, or its placeholder while empty.
    pub fn segment_text(&self, segment: DateSegment) -> String {
        match self.segment_value(segment) {
            Some(value) => format!(
                "{value:0width$}",
                width = usize::from(segment.digits()).min(4)
            ),
            None => self.placeholders[segment.index()].to_string(),
        }
    }

    pub const fn focused_segment(&self) -> DateSegment {
        self.focused
    }

    /// Move editing focus to one segment, discarding pending typed digits.
    pub fn focus_segment(&mut self, segment: DateSegment) -> bool {
        if self.focused == segment {
            return false;
        }
        self.focused = segment;
        self.buffer.clear();
        true
    }

    /// Move to the next segment in the declared order. The final segment reports `false` so Tab
    /// can leave the field instead of trapping focus.
    pub fn focus_next(&mut self) -> bool {
        let segments = self.order.segments();
        let position = segments.iter().position(|item| *item == self.focused);
        match position {
            Some(position) if position + 1 < segments.len() => {
                self.focus_segment(segments[position + 1])
            }
            _ => false,
        }
    }

    /// Move to the previous segment in the declared order.
    pub fn focus_previous(&mut self) -> bool {
        let segments = self.order.segments();
        match segments.iter().position(|item| *item == self.focused) {
            Some(position) if position > 0 => self.focus_segment(segments[position - 1]),
            _ => false,
        }
    }

    /// The inclusive numeric range one segment accepts right now.
    ///
    /// The day's maximum honors the retained month and year, so February in a leap year offers 29.
    pub fn segment_bounds(&self, segment: DateSegment) -> (i32, i32) {
        match segment {
            DateSegment::Year => (MIN_CIVIL_YEAR, MAX_CIVIL_YEAR),
            DateSegment::Month => (1, 12),
            DateSegment::Day => (
                1,
                i32::from(CivilDate::days_in_month(
                    self.year.unwrap_or(2000),
                    self.month.unwrap_or(1),
                )),
            ),
        }
    }

    /// Step one segment, wrapping at its bounds. An empty segment takes its minimum when stepped
    /// forward and its maximum when stepped backward.
    pub fn step_segment(&mut self, segment: DateSegment, delta: i32) -> bool {
        if self.disabled || delta == 0 {
            return false;
        }
        let (minimum, maximum) = self.segment_bounds(segment);
        let next = match self.segment_value(segment) {
            Some(current) => wrapped_step(current, delta, minimum, maximum),
            None if delta > 0 => minimum,
            None => maximum,
        };
        self.buffer.clear();
        self.write_segment(segment, next)
    }

    /// Step the focused segment one unit toward its maximum.
    pub fn increment(&mut self) -> bool {
        self.step_segment(self.focused, 1)
    }

    /// Step the focused segment one unit toward its minimum.
    pub fn decrement(&mut self) -> bool {
        self.step_segment(self.focused, -1)
    }

    /// Set the focused segment to its smallest legal value.
    pub fn to_segment_minimum(&mut self) -> bool {
        if self.disabled {
            return false;
        }
        let (minimum, _) = self.segment_bounds(self.focused);
        self.buffer.clear();
        self.write_segment(self.focused, minimum)
    }

    /// Set the focused segment to its largest legal value.
    pub fn to_segment_maximum(&mut self) -> bool {
        if self.disabled {
            return false;
        }
        let (_, maximum) = self.segment_bounds(self.focused);
        self.buffer.clear();
        self.write_segment(self.focused, maximum)
    }

    /// Clear the focused segment and any pending typed digits.
    pub fn clear_segment(&mut self) -> bool {
        if self.disabled {
            return false;
        }
        let had_digits = self.buffer.len != 0;
        self.buffer.clear();
        let cleared = match self.focused {
            DateSegment::Year => self.year.take().is_some(),
            DateSegment::Month => self.month.take().is_some(),
            DateSegment::Day => self.day.take().is_some(),
        };
        cleared || had_digits
    }

    /// Clear every segment.
    pub fn clear(&mut self) -> bool {
        if self.disabled {
            return false;
        }
        self.buffer.clear();
        self.set_value(None) || self.year.is_some()
    }

    /// Type one ASCII digit into the focused segment.
    ///
    /// The segment advances as soon as no further digit could fit: a month of `4`, a day of `9`,
    /// or a complete four-digit year all move editing to the next segment immediately.
    pub fn type_digit(&mut self, digit: u8) -> bool {
        if self.disabled || digit > 9 {
            return false;
        }
        let segment = self.focused;
        let (minimum, maximum) = self.typing_bounds(segment);
        let digits = segment.digits();
        let mut candidate = self.buffer.value * 10 + u32::from(digit);
        let mut length = self.buffer.len + 1;
        if candidate as i32 > maximum {
            candidate = u32::from(digit);
            length = 1;
        }
        self.buffer.value = candidate;
        self.buffer.len = length;
        let mut changed = false;
        if candidate as i32 >= minimum {
            changed |= self.write_segment(segment, candidate as i32);
        }
        if length >= digits || candidate as i32 * 10 > maximum {
            let advanced = self.focus_next();
            if !advanced {
                self.buffer.clear();
            }
            changed |= advanced;
        }
        changed || length == 1
    }

    /// Type one printable character. Only ASCII digits change a date field.
    pub fn type_character(&mut self, character: char) -> bool {
        character
            .to_digit(10)
            .is_some_and(|digit| self.type_digit(digit as u8))
    }

    /// The range typed digits may reach before the retained value is clamped.
    ///
    /// The day accepts `31` in every month so a two-digit day can always be typed; the retained
    /// value is then shortened to the month's real length. Arrow steps and the projected
    /// spin-button range keep using [`Self::segment_bounds`], which is month-aware.
    fn typing_bounds(&self, segment: DateSegment) -> (i32, i32) {
        match segment {
            DateSegment::Day => (1, 31),
            other => self.segment_bounds(other),
        }
    }

    fn write_segment(&mut self, segment: DateSegment, value: i32) -> bool {
        let before = (self.year, self.month, self.day);
        match segment {
            DateSegment::Year => self.year = Some(value.clamp(0, MAX_CIVIL_YEAR)),
            DateSegment::Month => self.month = Some(value.clamp(1, 12) as u8),
            DateSegment::Day => self.day = Some(value.clamp(1, 31) as u8),
        }
        self.clamp_day();
        before != (self.year, self.month, self.day)
    }

    fn clamp_day(&mut self) {
        if let Some(day) = self.day {
            let last = CivilDate::days_in_month(self.year.unwrap_or(2000), self.month.unwrap_or(1));
            if day > last {
                self.day = Some(last);
            }
        }
    }
}

/// Controlled segmented editing state for one time field.
///
/// Hours are always retained on a 24-hour clock. A 12-hour field projects the same value through
/// an hour segment of `1..=12` plus an AM/PM segment, so switching presentation never rewrites the
/// application's value.
#[derive(Clone, Debug, PartialEq)]
pub struct TimeFieldState {
    hour: Option<u8>,
    minute: Option<u8>,
    second: Option<u8>,
    hour12: bool,
    seconds: bool,
    focused: TimeSegment,
    minimum: Option<CivilTime>,
    maximum: Option<CivilTime>,
    placeholders: [Arc<str>; 4],
    buffer: SegmentBuffer,
    disabled: bool,
}

impl Default for TimeFieldState {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeFieldState {
    /// An empty 24-hour field with hour and minute segments.
    pub fn new() -> Self {
        Self {
            hour: None,
            minute: None,
            second: None,
            hour12: false,
            seconds: false,
            focused: TimeSegment::Hour,
            minimum: None,
            maximum: None,
            placeholders: [
                Arc::from("HH"),
                Arc::from("MM"),
                Arc::from("SS"),
                Arc::from("AM"),
            ],
            buffer: SegmentBuffer::default(),
            disabled: false,
        }
    }

    /// A field holding one time. Out-of-range components are clamped before they are retained.
    pub fn from_time(time: CivilTime) -> Self {
        let time = time.clamped();
        let mut state = Self::new();
        state.hour = Some(time.hour);
        state.minute = Some(time.minute);
        state.second = Some(time.second);
        state
    }

    /// Present the hour on a 12-hour clock with an AM/PM segment.
    #[must_use]
    pub const fn hour12(mut self, hour12: bool) -> Self {
        self.hour12 = hour12;
        self
    }

    /// Show a seconds segment.
    #[must_use]
    pub const fn seconds(mut self, seconds: bool) -> Self {
        self.seconds = seconds;
        self
    }

    #[must_use]
    pub fn minimum(mut self, minimum: CivilTime) -> Self {
        self.minimum = Some(minimum.clamped());
        self
    }

    #[must_use]
    pub fn maximum(mut self, maximum: CivilTime) -> Self {
        self.maximum = Some(maximum.clamped());
        self
    }

    /// Replace one segment's placeholder. Longer text than
    /// [`MAX_DATE_FIELD_PLACEHOLDER_BYTES`] keeps the default.
    #[must_use]
    pub fn placeholder(mut self, segment: TimeSegment, text: impl Into<Arc<str>>) -> Self {
        let fallback = match segment {
            TimeSegment::Hour => "HH",
            TimeSegment::Minute => "MM",
            TimeSegment::Second => "SS",
            TimeSegment::Period => "AM",
        };
        self.placeholders[segment.index()] = bounded_placeholder(text, fallback);
        self
    }

    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub const fn is_hour12(&self) -> bool {
        self.hour12
    }

    pub const fn shows_seconds(&self) -> bool {
        self.seconds
    }

    pub const fn minimum_time(&self) -> Option<CivilTime> {
        self.minimum
    }

    pub const fn maximum_time(&self) -> Option<CivilTime> {
        self.maximum
    }

    pub const fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// The segments this field presents, in layout order.
    pub const fn segments(&self) -> TimeSegments {
        let mut items = [
            TimeSegment::Hour,
            TimeSegment::Minute,
            TimeSegment::Hour,
            TimeSegment::Hour,
        ];
        let mut len = 2;
        if self.seconds {
            items[len] = TimeSegment::Second;
            len += 1;
        }
        if self.hour12 {
            items[len] = TimeSegment::Period;
            len += 1;
        }
        TimeSegments {
            items,
            len: len as u8,
            index: 0,
        }
    }

    /// Whether one segment participates in this field's presentation.
    pub const fn has_segment(&self, segment: TimeSegment) -> bool {
        match segment {
            TimeSegment::Hour | TimeSegment::Minute => true,
            TimeSegment::Second => self.seconds,
            TimeSegment::Period => self.hour12,
        }
    }

    /// The complete time, or `None` while a presented segment is empty.
    pub fn value(&self) -> Option<CivilTime> {
        let hour = self.hour?;
        let minute = self.minute?;
        let second = if self.seconds { self.second? } else { 0 };
        CivilTime::new(hour, minute, second)
    }

    pub fn set_value(&mut self, value: Option<CivilTime>) -> bool {
        let (hour, minute, second) = match value {
            Some(time) => {
                let time = time.clamped();
                (Some(time.hour), Some(time.minute), Some(time.second))
            }
            None => (None, None, None),
        };
        if (self.hour, self.minute, self.second) == (hour, minute, second) {
            return false;
        }
        self.hour = hour;
        self.minute = minute;
        self.second = second;
        self.buffer.clear();
        true
    }

    /// The 12-hour half currently retained, or `None` while the hour is empty.
    pub fn period(&self) -> Option<CivilPeriod> {
        self.hour.map(|hour| {
            if hour < 12 {
                CivilPeriod::Am
            } else {
                CivilPeriod::Pm
            }
        })
    }

    /// Move the retained hour into one half of the clock.
    pub fn set_period(&mut self, period: CivilPeriod) -> bool {
        let hour = self.hour.unwrap_or(0);
        let next = match period {
            CivilPeriod::Am => hour % 12,
            CivilPeriod::Pm => hour % 12 + 12,
        };
        if self.hour == Some(next) {
            return false;
        }
        self.hour = Some(next);
        true
    }

    /// The raw numeric content of one segment, using the presented hour clock.
    pub fn segment_value(&self, segment: TimeSegment) -> Option<i32> {
        match segment {
            TimeSegment::Hour => self.hour.map(|hour| {
                if self.hour12 {
                    i32::from(match hour % 12 {
                        0 => 12,
                        hour => hour,
                    })
                } else {
                    i32::from(hour)
                }
            }),
            TimeSegment::Minute => self.minute.map(i32::from),
            TimeSegment::Second => self.second.map(i32::from),
            TimeSegment::Period => self
                .period()
                .map(|period| i32::from(period == CivilPeriod::Pm)),
        }
    }

    pub fn is_filled(&self, segment: TimeSegment) -> bool {
        self.segment_value(segment).is_some()
    }

    pub fn is_complete(&self) -> bool {
        self.segments().all(|segment| self.is_filled(segment))
    }

    /// Whether the field is empty or holds a time inside its declared bounds.
    pub fn is_valid(&self) -> bool {
        if self.hour.is_none() && self.minute.is_none() && self.second.is_none() {
            return true;
        }
        let Some(value) = self.value() else {
            return false;
        };
        if self.minimum.is_some_and(|minimum| value < minimum) {
            return false;
        }
        self.maximum.is_none_or(|maximum| value <= maximum)
    }

    pub fn placeholder_text(&self, segment: TimeSegment) -> &Arc<str> {
        &self.placeholders[segment.index()]
    }

    /// The text one segment displays: zero-padded digits, `AM`/`PM`, or its placeholder.
    pub fn segment_text(&self, segment: TimeSegment) -> String {
        if segment == TimeSegment::Period {
            return match self.period() {
                Some(period) => period.label().to_string(),
                None => self.placeholders[segment.index()].to_string(),
            };
        }
        match self.segment_value(segment) {
            Some(value) => format!("{value:02}"),
            None => self.placeholders[segment.index()].to_string(),
        }
    }

    pub const fn focused_segment(&self) -> TimeSegment {
        self.focused
    }

    pub fn focus_segment(&mut self, segment: TimeSegment) -> bool {
        if self.focused == segment || !self.has_segment(segment) {
            return false;
        }
        self.focused = segment;
        self.buffer.clear();
        true
    }

    /// Move to the next presented segment. The final segment reports `false`.
    pub fn focus_next(&mut self) -> bool {
        let segments = self.segments();
        let mut seen = false;
        for segment in segments {
            if seen {
                return self.focus_segment(segment);
            }
            seen = segment == self.focused;
        }
        false
    }

    /// Move to the previous presented segment.
    pub fn focus_previous(&mut self) -> bool {
        let mut previous = None;
        for segment in self.segments() {
            if segment == self.focused {
                return previous.is_some_and(|previous| self.focus_segment(previous));
            }
            previous = Some(segment);
        }
        false
    }

    /// The inclusive numeric range one segment accepts.
    pub const fn segment_bounds(&self, segment: TimeSegment) -> (i32, i32) {
        match segment {
            TimeSegment::Hour if self.hour12 => (1, 12),
            TimeSegment::Hour => (0, 23),
            TimeSegment::Minute | TimeSegment::Second => (0, 59),
            TimeSegment::Period => (0, 1),
        }
    }

    /// Step one segment, wrapping at its bounds.
    pub fn step_segment(&mut self, segment: TimeSegment, delta: i32) -> bool {
        if self.disabled || delta == 0 || !self.has_segment(segment) {
            return false;
        }
        self.buffer.clear();
        if segment == TimeSegment::Period {
            let period = match self.period() {
                Some(CivilPeriod::Am) => CivilPeriod::Pm,
                Some(CivilPeriod::Pm) => CivilPeriod::Am,
                None => CivilPeriod::Am,
            };
            return self.set_period(period);
        }
        let (minimum, maximum) = self.segment_bounds(segment);
        let next = match self.segment_value(segment) {
            Some(current) => wrapped_step(current, delta, minimum, maximum),
            None if delta > 0 => minimum,
            None => maximum,
        };
        self.write_segment(segment, next)
    }

    pub fn increment(&mut self) -> bool {
        self.step_segment(self.focused, 1)
    }

    pub fn decrement(&mut self) -> bool {
        self.step_segment(self.focused, -1)
    }

    pub fn to_segment_minimum(&mut self) -> bool {
        if self.disabled {
            return false;
        }
        self.buffer.clear();
        if self.focused == TimeSegment::Period {
            return self.set_period(CivilPeriod::Am);
        }
        let (minimum, _) = self.segment_bounds(self.focused);
        self.write_segment(self.focused, minimum)
    }

    pub fn to_segment_maximum(&mut self) -> bool {
        if self.disabled {
            return false;
        }
        self.buffer.clear();
        if self.focused == TimeSegment::Period {
            return self.set_period(CivilPeriod::Pm);
        }
        let (_, maximum) = self.segment_bounds(self.focused);
        self.write_segment(self.focused, maximum)
    }

    /// Clear the focused segment and any pending typed digits.
    ///
    /// Clearing the AM/PM segment clears the hour it describes, because a 12-hour clock cannot
    /// retain an hour without a half.
    pub fn clear_segment(&mut self) -> bool {
        if self.disabled {
            return false;
        }
        let had_digits = self.buffer.len != 0;
        self.buffer.clear();
        let cleared = match self.focused {
            TimeSegment::Hour | TimeSegment::Period => self.hour.take().is_some(),
            TimeSegment::Minute => self.minute.take().is_some(),
            TimeSegment::Second => self.second.take().is_some(),
        };
        cleared || had_digits
    }

    pub fn clear(&mut self) -> bool {
        if self.disabled {
            return false;
        }
        self.buffer.clear();
        self.set_value(None)
    }

    /// Type one ASCII digit into the focused segment.
    pub fn type_digit(&mut self, digit: u8) -> bool {
        if self.disabled || digit > 9 || self.focused == TimeSegment::Period {
            return false;
        }
        let segment = self.focused;
        let (minimum, maximum) = self.segment_bounds(segment);
        let mut candidate = self.buffer.value * 10 + u32::from(digit);
        let mut length = self.buffer.len + 1;
        if candidate as i32 > maximum {
            candidate = u32::from(digit);
            length = 1;
        }
        self.buffer.value = candidate;
        self.buffer.len = length;
        let mut changed = false;
        if candidate as i32 >= minimum {
            changed |= self.write_segment(segment, candidate as i32);
        }
        if length >= segment.digits() || candidate as i32 * 10 > maximum {
            let advanced = self.focus_next();
            if !advanced {
                self.buffer.clear();
            }
            changed |= advanced;
        }
        changed || length == 1
    }

    /// Type one printable character.
    ///
    /// Digits edit numeric segments; `a` and `p` set the AM/PM segment, matching the desktop
    /// convention for a 12-hour clock.
    pub fn type_character(&mut self, character: char) -> bool {
        if self.focused == TimeSegment::Period {
            return match character.to_ascii_lowercase() {
                'a' => self.set_period(CivilPeriod::Am),
                'p' => self.set_period(CivilPeriod::Pm),
                _ => false,
            };
        }
        character
            .to_digit(10)
            .is_some_and(|digit| self.type_digit(digit as u8))
    }

    fn write_segment(&mut self, segment: TimeSegment, value: i32) -> bool {
        let before = (self.hour, self.minute, self.second);
        match segment {
            TimeSegment::Hour => {
                let hour = if self.hour12 {
                    let period = self.period().unwrap_or(CivilPeriod::Am);
                    let base = (value.clamp(1, 12) % 12) as u8;
                    match period {
                        CivilPeriod::Am => base,
                        CivilPeriod::Pm => base + 12,
                    }
                } else {
                    value.clamp(0, 23) as u8
                };
                self.hour = Some(hour);
            }
            TimeSegment::Minute => self.minute = Some(value.clamp(0, 59) as u8),
            TimeSegment::Second => self.second = Some(value.clamp(0, 59) as u8),
            TimeSegment::Period => {
                return self.set_period(if value == 0 {
                    CivilPeriod::Am
                } else {
                    CivilPeriod::Pm
                });
            }
        }
        before != (self.hour, self.minute, self.second)
    }
}

/// A copyable iterator over the segments one time field presents.
#[derive(Clone, Copy, Debug)]
pub struct TimeSegments {
    items: [TimeSegment; 4],
    len: u8,
    index: u8,
}

impl Iterator for TimeSegments {
    type Item = TimeSegment;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.len {
            return None;
        }
        let segment = self.items[self.index as usize];
        self.index += 1;
        Some(segment)
    }
}

/// A copyable declaration for one unstyled date field.
///
/// The descriptor retains no allocation, task, timer, observer, or idle scheduler source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "a DateField descriptor has no effect until one of its parts is mounted"]
pub struct DateField {
    root_id: ElementId,
}

impl DateField {
    pub fn new(root_id: impl Into<ElementId>) -> Self {
        Self {
            root_id: root_id.into(),
        }
    }

    pub const fn root_id(self) -> ElementId {
        self.root_id
    }

    pub fn segment_id(self, segment: DateSegment) -> ElementId {
        derived_segment_id(
            self.root_id,
            DATE_FIELD_SEGMENT_ID_TAG,
            segment.index() as u64,
        )
    }

    /// Describe one segment so its part and typed actions can be attached.
    pub const fn segment(self, segment: DateSegment) -> DateFieldSegment {
        DateFieldSegment {
            field: self,
            segment,
        }
    }

    /// Decorate an application-owned root without adding layout or appearance.
    ///
    /// The root is the group that owns the field's accessible name, its disabled state, its
    /// validity, and the active-descendant relationship to the segment being edited.
    pub fn root_with(self, state: &DateFieldState, root: Element) -> Element {
        let disabled = state.disabled || root.accessibility.disabled;
        root.id(self.root_id)
            .accessibility_role(AccessibilityRole::Group)
            .accessibility_active_descendant(self.segment_id(state.focused))
            .invalid(!state.is_valid())
            .disabled(disabled)
            .cursor_default()
            .app_region_no_drag()
            .user_select_none()
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(self, state: &DateFieldState) -> Element {
        self.root_with(state, crate::div())
    }
}

/// A copyable declaration for one date-field segment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "a DateFieldSegment descriptor has no effect until its part is mounted"]
pub struct DateFieldSegment {
    field: DateField,
    segment: DateSegment,
}

impl DateFieldSegment {
    pub const fn segment(self) -> DateSegment {
        self.segment
    }

    pub fn segment_id(self) -> ElementId {
        self.field.segment_id(self.segment)
    }

    /// Decorate an application-owned segment without adding layout or appearance.
    ///
    /// The segment becomes a focusable spin button carrying its current numeric value, its live
    /// bounds, and the framework's default accessible name. Call `.accessibility_label(...)` after
    /// this decorator to replace that name with localized product text.
    pub fn segment_with(self, state: &DateFieldState, element: Element) -> Element {
        let disabled = state.disabled || element.accessibility.disabled;
        let (minimum, maximum) = state.segment_bounds(self.segment);
        let mut element = element
            .id(self.segment_id())
            .accessibility_role(AccessibilityRole::SpinButton)
            .accessibility_label(self.segment.label())
            .accessibility_value(state.segment_text(self.segment))
            .focusable()
            .tab_index(0)
            .key_context(DATE_FIELD_KEY_CONTEXT)
            .selected(state.focused == self.segment)
            .disabled(disabled)
            .cursor_default()
            .app_region_no_drag()
            .user_select_none();
        if let Some(value) = state.segment_value(self.segment) {
            element = element.accessibility_value_range(AccessibilityValueRange::new(
                f64::from(value),
                f64::from(minimum),
                f64::from(maximum),
            ));
        } else {
            element = element.accessibility_value_range(AccessibilityValueRange::indeterminate(
                f64::from(minimum),
                f64::from(maximum),
            ));
        }
        element
    }

    /// Attach QuickGUI's typed segment actions and digit entry.
    ///
    /// Install [`date_field_key_bindings`] once on the application keymap. Focusing the segment
    /// also makes it the edited segment, so typed digits always reach the segment the user sees.
    pub fn key_with<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        element: Element,
        access: fn(&mut V) -> &mut DateFieldState,
    ) -> Element {
        self.key_with_accessor(cx, element, StateAccessor::from(access))
    }
    /// Create the unstyled key part. Use [`Self::key_with`] to supply an existing element.
    pub fn key<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut DateFieldState,
    ) -> Element {
        self.key_with(cx, crate::div(), access)
    }

    /// Attach the typed segment actions against a per-instance state accessor.
    ///
    /// A host that renders many declared date fields through one view passes an accessor that
    /// captures which [`DateFieldState`] this segment belongs to.
    pub fn key_with_accessor<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        element: Element,
        access_source: StateAccessor<V, DateFieldState>,
    ) -> Element {
        let id = self.segment_id();
        let segment = self.segment;
        let access = access_source.clone();
        let increment = cx.action_listener(id, move |view, _: &DateFieldIncrement, cx| {
            let state = access.get(view);
            state.focus_segment(segment);
            if state.increment() {
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let decrement = cx.action_listener(id, move |view, _: &DateFieldDecrement, cx| {
            let state = access.get(view);
            state.focus_segment(segment);
            if state.decrement() {
                cx.invalidate();
            }
        });
        let field = self.field;
        let access = access_source.clone();
        let next = cx.action_listener(id, move |view, _: &DateFieldNextSegment, cx| {
            let state = access.get(view);
            state.focus_segment(segment);
            if state.focus_next() {
                let focused = state.focused_segment();
                cx.focus(FocusHandle::new(field.segment_id(focused)));
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let previous = cx.action_listener(id, move |view, _: &DateFieldPreviousSegment, cx| {
            let state = access.get(view);
            state.focus_segment(segment);
            if state.focus_previous() {
                let focused = state.focused_segment();
                cx.focus(FocusHandle::new(field.segment_id(focused)));
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let clear = cx.action_listener(id, move |view, _: &DateFieldClearSegment, cx| {
            let state = access.get(view);
            state.focus_segment(segment);
            if state.clear_segment() {
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let minimum = cx.action_listener(id, move |view, _: &DateFieldSegmentMinimum, cx| {
            let state = access.get(view);
            state.focus_segment(segment);
            if state.to_segment_minimum() {
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let maximum = cx.action_listener(id, move |view, _: &DateFieldSegmentMaximum, cx| {
            let state = access.get(view);
            state.focus_segment(segment);
            if state.to_segment_maximum() {
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let digits = cx.key_down_listener(id, move |view, event, cx| {
            if event
                .modifiers
                .intersects(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER)
            {
                return;
            }
            let key = event.key_char.as_ref().unwrap_or(&event.key);
            let Key::Character(value) = key else {
                return;
            };
            let Some(character) = value.chars().next() else {
                return;
            };
            let state = access.get(view);
            state.focus_segment(segment);
            if state.type_character(character) {
                let focused = state.focused_segment();
                if focused != segment {
                    cx.focus(FocusHandle::new(field.segment_id(focused)));
                }
                cx.invalidate();
                cx.prevent_default();
                cx.stop_propagation();
            }
        });
        let access = access_source.clone();
        let focus = cx.listener(id, move |view, cx| {
            if access.get(view).focus_segment(segment) {
                cx.invalidate();
            }
        });

        element
            .on_action(increment)
            .on_action(decrement)
            .on_action(next)
            .on_action(previous)
            .on_action(clear)
            .on_action(minimum)
            .on_action(maximum)
            .on_key_down(digits)
            .on_click(focus)
    }
}

/// A copyable declaration for one unstyled time field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "a TimeField descriptor has no effect until one of its parts is mounted"]
pub struct TimeField {
    root_id: ElementId,
}

impl TimeField {
    pub fn new(root_id: impl Into<ElementId>) -> Self {
        Self {
            root_id: root_id.into(),
        }
    }

    pub const fn root_id(self) -> ElementId {
        self.root_id
    }

    pub fn segment_id(self, segment: TimeSegment) -> ElementId {
        derived_segment_id(
            self.root_id,
            TIME_FIELD_SEGMENT_ID_TAG,
            segment.index() as u64,
        )
    }

    pub const fn segment(self, segment: TimeSegment) -> TimeFieldSegment {
        TimeFieldSegment {
            field: self,
            segment,
        }
    }

    /// Decorate an application-owned root without adding layout or appearance.
    pub fn root_with(self, state: &TimeFieldState, root: Element) -> Element {
        let disabled = state.disabled || root.accessibility.disabled;
        root.id(self.root_id)
            .accessibility_role(AccessibilityRole::Group)
            .accessibility_active_descendant(self.segment_id(state.focused))
            .invalid(!state.is_valid())
            .disabled(disabled)
            .cursor_default()
            .app_region_no_drag()
            .user_select_none()
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(self, state: &TimeFieldState) -> Element {
        self.root_with(state, crate::div())
    }
}

/// A copyable declaration for one time-field segment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "a TimeFieldSegment descriptor has no effect until its part is mounted"]
pub struct TimeFieldSegment {
    field: TimeField,
    segment: TimeSegment,
}

impl TimeFieldSegment {
    pub const fn segment(self) -> TimeSegment {
        self.segment
    }

    pub fn segment_id(self) -> ElementId {
        self.field.segment_id(self.segment)
    }

    /// Decorate an application-owned segment without adding layout or appearance.
    ///
    /// The AM/PM segment is a spin button with two states rather than a numeric range, so it
    /// exposes its text through the accessible value instead of a value range.
    pub fn segment_with(self, state: &TimeFieldState, element: Element) -> Element {
        let disabled = state.disabled || element.accessibility.disabled;
        let (minimum, maximum) = state.segment_bounds(self.segment);
        let mut element = element
            .id(self.segment_id())
            .accessibility_role(AccessibilityRole::SpinButton)
            .accessibility_label(self.segment.label())
            .accessibility_value(state.segment_text(self.segment))
            .focusable()
            .tab_index(0)
            .key_context(TIME_FIELD_KEY_CONTEXT)
            .selected(state.focused == self.segment)
            .disabled(disabled)
            .cursor_default()
            .app_region_no_drag()
            .user_select_none();
        if self.segment != TimeSegment::Period {
            element = match state.segment_value(self.segment) {
                Some(value) => element.accessibility_value_range(AccessibilityValueRange::new(
                    f64::from(value),
                    f64::from(minimum),
                    f64::from(maximum),
                )),
                None => element.accessibility_value_range(AccessibilityValueRange::indeterminate(
                    f64::from(minimum),
                    f64::from(maximum),
                )),
            };
        }
        element
    }

    /// Attach QuickGUI's typed segment actions and typed entry.
    ///
    /// Install [`time_field_key_bindings`] once on the application keymap.
    pub fn key_with<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        element: Element,
        access: fn(&mut V) -> &mut TimeFieldState,
    ) -> Element {
        self.key_with_accessor(cx, element, StateAccessor::from(access))
    }
    /// Create the unstyled key part. Use [`Self::key_with`] to supply an existing element.
    pub fn key<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut TimeFieldState,
    ) -> Element {
        self.key_with(cx, crate::div(), access)
    }

    /// Attach the typed segment actions against a per-instance state accessor.
    ///
    /// A host that renders many declared time fields through one view passes an accessor that
    /// captures which [`TimeFieldState`] this segment belongs to.
    pub fn key_with_accessor<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        element: Element,
        access_source: StateAccessor<V, TimeFieldState>,
    ) -> Element {
        let id = self.segment_id();
        let segment = self.segment;
        let access = access_source.clone();
        let increment = cx.action_listener(id, move |view, _: &DateFieldIncrement, cx| {
            let state = access.get(view);
            state.focus_segment(segment);
            if state.increment() {
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let decrement = cx.action_listener(id, move |view, _: &DateFieldDecrement, cx| {
            let state = access.get(view);
            state.focus_segment(segment);
            if state.decrement() {
                cx.invalidate();
            }
        });
        let field = self.field;
        let access = access_source.clone();
        let next = cx.action_listener(id, move |view, _: &DateFieldNextSegment, cx| {
            let state = access.get(view);
            state.focus_segment(segment);
            if state.focus_next() {
                let focused = state.focused_segment();
                cx.focus(FocusHandle::new(field.segment_id(focused)));
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let previous = cx.action_listener(id, move |view, _: &DateFieldPreviousSegment, cx| {
            let state = access.get(view);
            state.focus_segment(segment);
            if state.focus_previous() {
                let focused = state.focused_segment();
                cx.focus(FocusHandle::new(field.segment_id(focused)));
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let clear = cx.action_listener(id, move |view, _: &DateFieldClearSegment, cx| {
            let state = access.get(view);
            state.focus_segment(segment);
            if state.clear_segment() {
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let minimum = cx.action_listener(id, move |view, _: &DateFieldSegmentMinimum, cx| {
            let state = access.get(view);
            state.focus_segment(segment);
            if state.to_segment_minimum() {
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let maximum = cx.action_listener(id, move |view, _: &DateFieldSegmentMaximum, cx| {
            let state = access.get(view);
            state.focus_segment(segment);
            if state.to_segment_maximum() {
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let typed = cx.key_down_listener(id, move |view, event, cx| {
            if event
                .modifiers
                .intersects(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER)
            {
                return;
            }
            let key = event.key_char.as_ref().unwrap_or(&event.key);
            let Key::Character(value) = key else {
                return;
            };
            let Some(character) = value.chars().next() else {
                return;
            };
            let state = access.get(view);
            state.focus_segment(segment);
            if state.type_character(character) {
                let focused = state.focused_segment();
                if focused != segment {
                    cx.focus(FocusHandle::new(field.segment_id(focused)));
                }
                cx.invalidate();
                cx.prevent_default();
                cx.stop_propagation();
            }
        });
        let access = access_source.clone();
        let focus = cx.listener(id, move |view, cx| {
            if access.get(view).focus_segment(segment) {
                cx.invalidate();
            }
        });

        element
            .on_action(increment)
            .on_action(decrement)
            .on_action(next)
            .on_action(previous)
            .on_action(clear)
            .on_action(minimum)
            .on_action(maximum)
            .on_key_down(typed)
            .on_click(focus)
    }
}

/// Create an unstyled empty date-field root.
///
/// This shorthand is equivalent to `DateField::new(id).root_with(state, div())`.
pub fn date_field(id: impl Into<ElementId>, state: &DateFieldState) -> Element {
    DateField::new(id).root_with(state, div())
}

/// Create an unstyled empty time-field root.
///
/// This shorthand is equivalent to `TimeField::new(id).root_with(state, div())`.
pub fn time_field(id: impl Into<ElementId>, state: &TimeFieldState) -> Element {
    TimeField::new(id).root_with(state, div())
}

fn derived_segment_id(parent: ElementId, tag: u64, segment: u64) -> ElementId {
    let mut hash = parent.as_u64() ^ tag ^ segment.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    if hash == parent.as_u64() || hash == u64::MAX {
        hash ^= tag.rotate_left(19);
    }
    ElementId::new(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Application, Color, IntoElement, View, WindowOptions, text};

    #[test]
    fn civil_dates_validate_leap_years_month_lengths_and_weekdays() {
        assert!(CivilDate::is_leap_year(2024));
        assert!(!CivilDate::is_leap_year(1900));
        assert!(CivilDate::is_leap_year(2000));
        assert_eq!(CivilDate::days_in_month(2024, 2), 29);
        assert_eq!(CivilDate::days_in_month(2023, 2), 28);
        assert_eq!(CivilDate::days_in_month(2023, 4), 30);
        assert!(CivilDate::new(2024, 2, 29).is_some());
        assert!(CivilDate::new(2023, 2, 29).is_none());
        assert!(CivilDate::new(2023, 13, 1).is_none());
        assert!(CivilDate::new(0, 1, 1).is_none());
        assert!(CivilDate::new(MAX_CIVIL_YEAR + 1, 1, 1).is_none());
        assert_eq!(
            CivilDate {
                year: 2023,
                month: 2,
                day: 31,
            }
            .clamped(),
            CivilDate {
                year: 2023,
                month: 2,
                day: 28,
            }
        );

        assert_eq!(
            CivilDate {
                year: 1970,
                month: 1,
                day: 1,
            }
            .epoch_day(),
            0
        );
        assert_eq!(
            CivilDate {
                year: 2026,
                month: 9,
                day: 3,
            }
            .epoch_day(),
            20_699
        );
        assert_eq!(
            CivilDate {
                year: 2026,
                month: 9,
                day: 3,
            }
            .weekday(),
            3
        );
        assert_eq!(
            CivilDate {
                year: 2000,
                month: 3,
                day: 1,
            }
            .weekday(),
            2
        );

        assert!(CivilTime::new(23, 59, 59).is_some());
        assert!(CivilTime::new(24, 0, 0).is_none());
        assert_eq!(
            CivilTime {
                hour: 30,
                minute: 90,
                second: 90,
            }
            .clamped(),
            CivilTime {
                hour: 23,
                minute: 59,
                second: 59,
            }
        );
        assert_eq!(CivilTime::new(0, 5, 0).unwrap().hour12(), 12);
        assert_eq!(CivilTime::new(13, 5, 0).unwrap().hour12(), 1);
        assert_eq!(CivilTime::new(13, 5, 0).unwrap().period(), CivilPeriod::Pm);
    }

    #[test]
    fn typed_digits_advance_segments_and_arrows_wrap_inside_bounds() {
        let mut state = DateFieldState::new().order(DateFieldOrder::MonthDayYear);
        assert_eq!(state.focused_segment(), DateSegment::Month);
        assert_eq!(state.segment_text(DateSegment::Month), "MM");

        assert!(state.type_digit(4));
        assert_eq!(state.segment_value(DateSegment::Month), Some(4));
        assert_eq!(state.focused_segment(), DateSegment::Day, "4 cannot grow");
        assert_eq!(state.segment_text(DateSegment::Month), "04");

        assert!(state.type_digit(3));
        assert_eq!(
            state.focused_segment(),
            DateSegment::Day,
            "30 fits in April"
        );
        assert!(state.type_digit(1));
        assert_eq!(
            state.segment_value(DateSegment::Day),
            Some(30),
            "April clamps 31 to 30"
        );
        assert_eq!(state.focused_segment(), DateSegment::Year);

        for digit in [2, 0, 2, 4] {
            assert!(state.type_digit(digit));
        }
        assert_eq!(
            state.focused_segment(),
            DateSegment::Year,
            "no segment left"
        );
        assert_eq!(
            state.value(),
            Some(CivilDate {
                year: 2024,
                month: 4,
                day: 30,
            })
        );
        assert!(state.is_complete());
        assert!(state.is_valid());

        state.focus_segment(DateSegment::Month);
        assert!(state.step_segment(DateSegment::Month, 8));
        assert_eq!(state.segment_value(DateSegment::Month), Some(12));
        assert!(state.increment());
        assert_eq!(state.segment_value(DateSegment::Month), Some(1));
        assert!(state.decrement());
        assert_eq!(state.segment_value(DateSegment::Month), Some(12));

        state.focus_segment(DateSegment::Day);
        assert!(state.to_segment_maximum());
        assert_eq!(state.segment_value(DateSegment::Day), Some(31));
        assert!(state.step_segment(DateSegment::Month, -10));
        assert_eq!(state.segment_value(DateSegment::Month), Some(2));
        assert_eq!(state.segment_value(DateSegment::Day), Some(29));
        assert_eq!(state.segment_bounds(DateSegment::Day), (1, 29));

        state.focus_segment(DateSegment::Day);
        assert!(state.clear_segment());
        assert!(!state.is_filled(DateSegment::Day));
        assert_eq!(state.segment_text(DateSegment::Day), "DD");
        assert!(state.value().is_none());
        assert!(!state.is_valid(), "a partly filled date is invalid");

        assert!(state.increment());
        assert_eq!(state.segment_value(DateSegment::Day), Some(1));
        assert!(state.clear_segment());
        assert!(state.decrement());
        assert_eq!(state.segment_value(DateSegment::Day), Some(29));
    }

    #[test]
    fn declared_bounds_report_validity_without_rewriting_typed_values() {
        let mut state = DateFieldState::from_date(CivilDate::new(2026, 6, 15).unwrap())
            .minimum(CivilDate::new(2026, 1, 1).unwrap())
            .maximum(CivilDate::new(2026, 12, 31).unwrap());
        assert!(state.is_valid());

        state.focus_segment(DateSegment::Year);
        assert!(state.to_segment_minimum());
        assert_eq!(state.segment_value(DateSegment::Year), Some(MIN_CIVIL_YEAR));
        assert!(!state.is_valid(), "before the declared minimum");
        assert_eq!(
            state.value(),
            Some(CivilDate {
                year: 1,
                month: 6,
                day: 15,
            }),
            "the typed value is retained, not clamped into range"
        );

        assert!(state.set_value(Some(CivilDate::new(2026, 3, 4).unwrap())));
        assert!(state.is_valid());
        assert!(state.clear());
        assert!(state.is_valid(), "an empty field is not an invalid field");
        assert!(state.value().is_none());

        let mut disabled =
            DateFieldState::from_date(CivilDate::new(2026, 6, 15).unwrap()).disabled(true);
        assert!(!disabled.increment());
        assert!(!disabled.type_digit(9));
        assert!(!disabled.clear_segment());
    }

    #[test]
    fn time_fields_project_a_twelve_hour_clock_over_one_retained_value() {
        let mut state = TimeFieldState::from_time(CivilTime::new(13, 45, 30).unwrap())
            .hour12(true)
            .seconds(true);
        assert_eq!(
            state.segments().collect::<Vec<_>>(),
            vec![
                TimeSegment::Hour,
                TimeSegment::Minute,
                TimeSegment::Second,
                TimeSegment::Period,
            ]
        );
        assert_eq!(state.segment_value(TimeSegment::Hour), Some(1));
        assert_eq!(state.segment_text(TimeSegment::Hour), "01");
        assert_eq!(state.segment_text(TimeSegment::Period), "PM");
        assert_eq!(state.value(), CivilTime::new(13, 45, 30));

        state.focus_segment(TimeSegment::Period);
        assert!(state.increment());
        assert_eq!(state.value(), CivilTime::new(1, 45, 30));
        assert_eq!(state.segment_text(TimeSegment::Period), "AM");
        assert!(state.type_character('p'));
        assert_eq!(state.value(), CivilTime::new(13, 45, 30));
        assert!(!state.type_character('7'), "the period takes no digits");

        state.focus_segment(TimeSegment::Hour);
        assert!(state.type_digit(1));
        assert_eq!(
            state.focused_segment(),
            TimeSegment::Hour,
            "11 or 12 remain"
        );
        assert!(state.type_digit(2));
        assert_eq!(state.value(), CivilTime::new(12, 45, 30));
        assert_eq!(state.focused_segment(), TimeSegment::Minute);

        assert!(state.step_segment(TimeSegment::Minute, 15));
        assert_eq!(state.value(), CivilTime::new(12, 0, 30));
        assert!(state.decrement());
        assert_eq!(state.value(), CivilTime::new(12, 59, 30));

        let mut plain = TimeFieldState::new().minimum(CivilTime::new(9, 0, 0).unwrap());
        assert_eq!(
            plain.segments().collect::<Vec<_>>(),
            vec![TimeSegment::Hour, TimeSegment::Minute]
        );
        assert!(plain.is_valid(), "an empty time field is valid");
        assert!(plain.type_digit(2));
        assert!(plain.type_digit(3));
        assert_eq!(plain.segment_value(TimeSegment::Hour), Some(23));
        assert_eq!(plain.focused_segment(), TimeSegment::Minute);
        assert!(!plain.is_valid(), "the minute is still empty");
        assert!(plain.type_digit(0));
        assert!(plain.type_digit(5));
        assert_eq!(plain.value(), CivilTime::new(23, 5, 0));
        assert!(plain.is_valid());
        assert!(!plain.focus_next(), "the last segment does not trap Tab");
        assert!(plain.focus_previous());
        assert_eq!(plain.focused_segment(), TimeSegment::Hour);
    }

    #[test]
    fn placeholders_are_bounded_and_default_when_oversized() {
        let long = "P".repeat(MAX_DATE_FIELD_PLACEHOLDER_BYTES + 1);
        let state = DateFieldState::new()
            .placeholder(DateSegment::Day, long.clone())
            .placeholder(DateSegment::Month, "Mon");
        assert_eq!(&**state.placeholder_text(DateSegment::Day), "DD");
        assert_eq!(&**state.placeholder_text(DateSegment::Month), "Mon");

        let time = TimeFieldState::new()
            .placeholder(TimeSegment::Hour, long)
            .placeholder(TimeSegment::Minute, "min");
        assert_eq!(&**time.placeholder_text(TimeSegment::Hour), "HH");
        assert_eq!(&**time.placeholder_text(TimeSegment::Minute), "min");
    }

    #[test]
    fn parts_add_exact_semantics_without_layout_or_appearance() {
        let state = DateFieldState::from_date(CivilDate::new(2026, 2, 3).unwrap());
        let field = DateField::new("due");
        let root = field.root_with(
            &state,
            div()
                .w(240.0)
                .bg(Color::rgb8(1, 2, 3))
                .child("Application-owned separator"),
        );
        assert_eq!(root.accessibility.role, AccessibilityRole::Group);
        assert_eq!(root.visual.background, Some(Color::rgb8(1, 2, 3)));
        assert_eq!(root.children.len(), 1);
        assert!(!root.accessibility.invalid);
        assert_eq!(
            root.accessibility.relations.active_descendant(),
            Some(field.segment_id(DateSegment::Year))
        );

        let month = field
            .segment(DateSegment::Month)
            .segment_with(&state, div().child(text("02")));
        assert_eq!(month.accessibility.role, AccessibilityRole::SpinButton);
        assert_eq!(month.accessibility.label.as_deref(), Some("Month"));
        assert_eq!(month.accessibility.value.as_deref(), Some("02"));
        assert!(month.focusable);
        let range = month
            .accessibility
            .value_range
            .as_deref()
            .expect("a filled segment projects its numeric value");
        assert_eq!(range.value, Some(2.0));
        assert_eq!(range.min, Some(1.0));
        assert_eq!(range.max, Some(12.0));
        assert_eq!(month.visual.background, None);
        assert_eq!(month.children.len(), 1);

        let empty = DateFieldState::new();
        let year = DateField::new("due")
            .segment(DateSegment::Year)
            .segment_with(&empty, div());
        let range = year
            .accessibility
            .value_range
            .as_deref()
            .expect("an empty segment still projects its bounds");
        assert_eq!(range.value, None);
        assert_eq!(range.min, Some(f64::from(MIN_CIVIL_YEAR)));
        assert_eq!(range.max, Some(f64::from(MAX_CIVIL_YEAR)));

        let mut partial = DateFieldState::new();
        assert!(partial.type_digit(2));
        let invalid_root = DateField::new("due").root_with(&partial, div());
        assert!(invalid_root.accessibility.invalid);

        let time_state = TimeFieldState::from_time(CivilTime::new(9, 5, 0).unwrap()).hour12(true);
        let time = TimeField::new("start");
        let period = time
            .segment(TimeSegment::Period)
            .segment_with(&time_state, div());
        assert_eq!(period.accessibility.role, AccessibilityRole::SpinButton);
        assert_eq!(period.accessibility.value.as_deref(), Some("AM"));
        assert!(
            period.accessibility.value_range.is_none(),
            "AM/PM is not a numeric range"
        );

        assert_eq!(
            date_field("due", &state).accessibility.role,
            AccessibilityRole::Group
        );
        assert!(date_field("due", &state).children.is_empty());
        assert_eq!(
            time_field("start", &time_state).accessibility.role,
            AccessibilityRole::Group
        );
    }

    #[test]
    fn segment_bindings_are_contextual_and_complete() {
        for (bindings, context) in [
            (date_field_key_bindings(), DATE_FIELD_KEY_CONTEXT),
            (time_field_key_bindings(), TIME_FIELD_KEY_CONTEXT),
        ] {
            assert_eq!(bindings.len(), 8);
            assert!(bindings.iter().all(|binding| {
                binding.context_predicate().is_some_and(|predicate| {
                    predicate
                        .depth_of(&[crate::KeyContext::parse(context).unwrap()])
                        .is_some()
                })
            }));
        }
    }

    struct FieldsView {
        due: DateFieldState,
        start: TimeFieldState,
    }

    impl FieldsView {
        fn due(view: &mut Self) -> &mut DateFieldState {
            &mut view.due
        }

        fn start(view: &mut Self) -> &mut TimeFieldState {
            &mut view.start
        }
    }

    impl View for FieldsView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let date = DateField::new("due");
            let mut date_root = date.root_with(&self.due, div()).accessibility_label("Due");
            for segment in self.due.segment_order().segments() {
                let part = date.segment(segment);
                let element =
                    part.segment_with(&self.due, div().child(text(self.due.segment_text(segment))));
                date_root = date_root.child(part.key_with(cx, element, Self::due));
            }

            let time = TimeField::new("start");
            let mut time_root = time
                .root_with(&self.start, div())
                .accessibility_label("Start");
            for segment in self.start.segments() {
                let part = time.segment(segment);
                let element = part.segment_with(
                    &self.start,
                    div().child(text(self.start.segment_text(segment))),
                );
                time_root = time_root.child(part.key_with(cx, element, Self::start));
            }

            div().child(date_root).child(time_root)
        }
    }

    #[test]
    fn segments_use_existing_focus_keyboard_and_idle_paths() {
        let (mut cx, view) = Application::new()
            .bind_keys(date_field_key_bindings())
            .bind_keys(time_field_key_bindings())
            .into_test_context(
                WindowOptions::default(),
                FieldsView {
                    due: DateFieldState::new(),
                    start: TimeFieldState::new().hour12(true),
                },
            )
            .unwrap();
        let window = view.window_handle();
        let date = DateField::new("due");
        let time = TimeField::new("start");

        cx.simulate_keystrokes(window, "tab").unwrap();
        assert_eq!(
            cx.focused(window).unwrap(),
            Some(date.segment_id(DateSegment::Year))
        );

        cx.simulate_keystrokes(window, "2 0 2 6").unwrap();
        assert_eq!(
            cx.read(view, |view| view.due.segment_value(DateSegment::Year))
                .unwrap(),
            Some(2026)
        );
        assert_eq!(
            cx.read(view, |view| view.due.focused_segment()).unwrap(),
            DateSegment::Month,
            "a complete year advances to the next segment"
        );
        assert_eq!(
            cx.focused(window).unwrap(),
            Some(date.segment_id(DateSegment::Month)),
            "native focus follows the edited segment"
        );

        cx.simulate_keystrokes(window, "2").unwrap();
        cx.simulate_keystrokes(window, "3 1").unwrap();
        assert_eq!(
            cx.read(view, |view| view.due.value()).unwrap(),
            CivilDate::new(2026, 2, 28),
            "February shortens the typed day"
        );

        cx.simulate_keystrokes(window, "up").unwrap();
        assert_eq!(
            cx.read(view, |view| view.due.value()).unwrap(),
            CivilDate::new(2026, 2, 1),
            "the day wraps inside its own month"
        );
        cx.simulate_keystrokes(window, "left").unwrap();
        assert_eq!(
            cx.focused(window).unwrap(),
            Some(date.segment_id(DateSegment::Month))
        );
        cx.simulate_keystrokes(window, "backspace").unwrap();
        assert!(cx.read(view, |view| view.due.value()).unwrap().is_none());
        assert!(!cx.read(view, |view| view.due.is_valid()).unwrap());

        cx.click(window, date.segment_id(DateSegment::Day)).unwrap();
        assert_eq!(
            cx.read(view, |view| view.due.focused_segment()).unwrap(),
            DateSegment::Day
        );

        cx.click(window, time.segment_id(TimeSegment::Hour))
            .unwrap();
        cx.simulate_keystrokes(window, "9").unwrap();
        assert_eq!(
            cx.read(view, |view| view.start.segment_value(TimeSegment::Hour))
                .unwrap(),
            Some(9)
        );
        cx.simulate_keystrokes(window, "1 5").unwrap();
        assert_eq!(
            cx.read(view, |view| view.start.value()).unwrap(),
            CivilTime::new(9, 15, 0)
        );
        assert_eq!(
            cx.focused(window).unwrap(),
            Some(time.segment_id(TimeSegment::Period))
        );
        cx.simulate_keystrokes(window, "down").unwrap();
        assert_eq!(
            cx.read(view, |view| view.start.value()).unwrap(),
            CivilTime::new(21, 15, 0)
        );

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }
}
