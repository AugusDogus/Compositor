use crate::{
    AccessibilityRole, CivilDate, Element, ElementId, FocusHandle, KeyBinding, MAX_CIVIL_YEAR,
    MIN_CIVIL_YEAR, StateAccessor, ViewContext, div,
};

/// Maximum week rows one month grid mounts.
///
/// Every Gregorian month fits in six weeks whatever weekday it starts on, so a calendar's mounted
/// row count is a constant rather than a function of application data.
pub const MAX_CALENDAR_WEEKS: usize = 6;

/// Days in one calendar week row.
pub const CALENDAR_WEEK_DAYS: usize = 7;

const CALENDAR_KEY_CONTEXT: &str = "Calendar";
const CALENDAR_DAY_ID_TAG: u64 = 0x8ba4_31d7_e59c_0264;
const CALENDAR_WEEK_ID_TAG: u64 = 0x51f0_9c73_2ad8_e416;

/// Move the roving day focus one day earlier.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CalendarPreviousDay;
/// Move the roving day focus one day later.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CalendarNextDay;
/// Move the roving day focus one week earlier.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CalendarPreviousWeek;
/// Move the roving day focus one week later.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CalendarNextWeek;
/// Move the roving day focus to the first day of its week row.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CalendarWeekStart;
/// Move the roving day focus to the last day of its week row.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CalendarWeekEnd;
/// Show the previous month.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CalendarPreviousMonth;
/// Show the next month.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CalendarNextMonth;
/// Show the same month one year earlier.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CalendarPreviousYear;
/// Show the same month one year later.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CalendarNextYear;
/// Select the focused day.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CalendarSelect;

/// Contextual bindings used by [`Calendar::key_with`].
pub fn calendar_key_bindings() -> [KeyBinding; 12] {
    [
        KeyBinding::new("left", CalendarPreviousDay, Some(CALENDAR_KEY_CONTEXT)),
        KeyBinding::new("right", CalendarNextDay, Some(CALENDAR_KEY_CONTEXT)),
        KeyBinding::new("up", CalendarPreviousWeek, Some(CALENDAR_KEY_CONTEXT)),
        KeyBinding::new("down", CalendarNextWeek, Some(CALENDAR_KEY_CONTEXT)),
        KeyBinding::new("home", CalendarWeekStart, Some(CALENDAR_KEY_CONTEXT)),
        KeyBinding::new("end", CalendarWeekEnd, Some(CALENDAR_KEY_CONTEXT)),
        KeyBinding::new("pageup", CalendarPreviousMonth, Some(CALENDAR_KEY_CONTEXT)),
        KeyBinding::new("pagedown", CalendarNextMonth, Some(CALENDAR_KEY_CONTEXT)),
        KeyBinding::new(
            "shift-pageup",
            CalendarPreviousYear,
            Some(CALENDAR_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "shift-pagedown",
            CalendarNextYear,
            Some(CALENDAR_KEY_CONTEXT),
        ),
        KeyBinding::new("enter", CalendarSelect, Some(CALENDAR_KEY_CONTEXT)),
        KeyBinding::new("space", CalendarSelect, Some(CALENDAR_KEY_CONTEXT)),
    ]
}

/// The weekday a calendar's week rows start on.
///
/// QuickGUI has no locale database, so the application declares this the way it declares a date
/// field's segment order.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CalendarWeekday {
    #[default]
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl CalendarWeekday {
    /// The zero-based index used by [`CivilDate::weekday`], where Monday is `0`.
    pub const fn index(self) -> u8 {
        match self {
            Self::Monday => 0,
            Self::Tuesday => 1,
            Self::Wednesday => 2,
            Self::Thursday => 3,
            Self::Friday => 4,
            Self::Saturday => 5,
            Self::Sunday => 6,
        }
    }

    /// The weekday at one zero-based index, wrapping every seven days.
    pub const fn from_index(index: u8) -> Self {
        match index % 7 {
            0 => Self::Monday,
            1 => Self::Tuesday,
            2 => Self::Wednesday,
            3 => Self::Thursday,
            4 => Self::Friday,
            5 => Self::Saturday,
            _ => Self::Sunday,
        }
    }
}

/// Controlled month-grid state with roving day focus.
///
/// The application owns every cell, weekday header, month caption, and color. QuickGUI owns the
/// calendar arithmetic, which day holds the grid's single Tab stop, which month is shown, and the
/// native grid semantics. The state is a plain copyable value with no allocation, task, timer,
/// observer, or idle scheduler source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CalendarState {
    year: i32,
    month: u8,
    focused: CivilDate,
    selected: Option<CivilDate>,
    minimum: Option<CivilDate>,
    maximum: Option<CivilDate>,
    first_weekday: CalendarWeekday,
    disabled: bool,
}

impl CalendarState {
    /// A calendar showing `focused`'s month with the roving focus on that day.
    pub fn new(focused: CivilDate) -> Self {
        let focused = focused.clamped();
        Self {
            year: focused.year,
            month: focused.month,
            focused,
            selected: None,
            minimum: None,
            maximum: None,
            first_weekday: CalendarWeekday::Monday,
            disabled: false,
        }
    }

    /// A calendar showing and focusing one selected day.
    pub fn selected(selected: CivilDate) -> Self {
        let mut state = Self::new(selected);
        state.selected = Some(state.focused);
        state
    }

    #[must_use]
    pub const fn first_weekday(mut self, weekday: CalendarWeekday) -> Self {
        self.first_weekday = weekday;
        self
    }

    /// Refuse to select days before this one. Focus still moves across the boundary so the user can
    /// see why a day is unavailable.
    #[must_use]
    pub fn minimum(mut self, minimum: CivilDate) -> Self {
        self.minimum = Some(minimum.clamped());
        self
    }

    /// Refuse to select days after this one.
    #[must_use]
    pub fn maximum(mut self, maximum: CivilDate) -> Self {
        self.maximum = Some(maximum.clamped());
        self
    }

    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub const fn is_disabled(self) -> bool {
        self.disabled
    }

    pub const fn week_start(self) -> CalendarWeekday {
        self.first_weekday
    }

    /// The year and month the grid shows.
    pub const fn displayed_month(self) -> (i32, u8) {
        (self.year, self.month)
    }

    pub const fn focused_day(self) -> CivilDate {
        self.focused
    }

    pub const fn selected_day(self) -> Option<CivilDate> {
        self.selected
    }

    pub const fn minimum_day(self) -> Option<CivilDate> {
        self.minimum
    }

    pub const fn maximum_day(self) -> Option<CivilDate> {
        self.maximum
    }

    /// Whether one day belongs to the month the grid shows.
    pub const fn is_in_displayed_month(self, day: CivilDate) -> bool {
        day.year == self.year && day.month == self.month
    }

    /// Whether one day may be selected, honoring the declared bounds.
    pub fn is_selectable(self, day: CivilDate) -> bool {
        if self.disabled || !day.is_valid() {
            return false;
        }
        if self.minimum.is_some_and(|minimum| day < minimum) {
            return false;
        }
        self.maximum.is_none_or(|maximum| day <= maximum)
    }

    /// The number of week rows the displayed month needs, never more than
    /// [`MAX_CALENDAR_WEEKS`].
    pub fn week_count(self) -> usize {
        let lead = self.lead_days();
        let days = usize::from(CivilDate::days_in_month(self.year, self.month));
        (lead + days)
            .div_ceil(CALENDAR_WEEK_DAYS)
            .min(MAX_CALENDAR_WEEKS)
    }

    /// One week row as seven consecutive days, or `None` past [`Self::week_count`].
    ///
    /// Leading and trailing cells belong to the adjacent months; use
    /// [`Self::is_in_displayed_month`] to paint them differently.
    pub fn week(self, index: usize) -> Option<[CivilDate; CALENDAR_WEEK_DAYS]> {
        if index >= self.week_count() {
            return None;
        }
        let start = self.grid_start()?;
        let mut days = [start; CALENDAR_WEEK_DAYS];
        for (column, day) in days.iter_mut().enumerate() {
            let offset = (index * CALENDAR_WEEK_DAYS + column) as i64;
            *day = CivilDate::from_epoch_day(start.epoch_day() + offset)?;
        }
        Some(days)
    }

    /// The zero-based row and column of one day inside the current grid.
    pub fn position_of(self, day: CivilDate) -> Option<(usize, usize)> {
        let start = self.grid_start()?;
        let offset = day.epoch_day() - start.epoch_day();
        if offset < 0 {
            return None;
        }
        let offset = offset as usize;
        let row = offset / CALENDAR_WEEK_DAYS;
        (row < self.week_count()).then_some((row, offset % CALENDAR_WEEK_DAYS))
    }

    /// Show one month without moving the roving focus out of it.
    pub fn show_month(&mut self, year: i32, month: u8) -> bool {
        if self.disabled
            || !(MIN_CIVIL_YEAR..=MAX_CIVIL_YEAR).contains(&year)
            || !(1..=12).contains(&month)
        {
            return false;
        }
        if (self.year, self.month) == (year, month) {
            return false;
        }
        self.year = year;
        self.month = month;
        let day = self.focused.day.min(CivilDate::days_in_month(year, month));
        self.focused = CivilDate { year, month, day };
        true
    }

    /// Move the roving day focus, following it into an adjacent month when it crosses a boundary.
    pub fn move_focus_days(&mut self, days: i64) -> bool {
        if self.disabled || days == 0 {
            return false;
        }
        let Some(next) = CivilDate::from_epoch_day(self.focused.epoch_day() + days) else {
            return false;
        };
        self.focus_day(next)
    }

    /// Move the roving day focus to a specific day, following it into another month.
    pub fn focus_day(&mut self, day: CivilDate) -> bool {
        if self.disabled || !day.is_valid() || self.focused == day {
            return false;
        }
        self.focused = day;
        self.year = day.year;
        self.month = day.month;
        true
    }

    /// Move the roving focus to the first or last day of its own week row.
    pub fn focus_week_edge(&mut self, last: bool) -> bool {
        if self.disabled {
            return false;
        }
        let weekday = i64::from(self.focused.weekday());
        let start = i64::from(self.first_weekday.index());
        let offset = (weekday - start).rem_euclid(CALENDAR_WEEK_DAYS as i64);
        let delta = if last {
            CALENDAR_WEEK_DAYS as i64 - 1 - offset
        } else {
            -offset
        };
        self.move_focus_days(delta)
    }

    /// Show the month `months` away, keeping the focused day of the month where it fits.
    pub fn move_focus_months(&mut self, months: i32) -> bool {
        if self.disabled || months == 0 {
            return false;
        }
        let total = self.year as i64 * 12 + i64::from(self.month) - 1 + i64::from(months);
        let year = total.div_euclid(12);
        let month = total.rem_euclid(12) as u8 + 1;
        if !(i64::from(MIN_CIVIL_YEAR)..=i64::from(MAX_CIVIL_YEAR)).contains(&year) {
            return false;
        }
        self.show_month(year as i32, month)
    }

    /// Select the focused day, if the declared bounds allow it.
    pub fn select_focused(&mut self) -> bool {
        self.select(self.focused)
    }

    /// Select one day, if the declared bounds allow it.
    pub fn select(&mut self, day: CivilDate) -> bool {
        if !self.is_selectable(day) {
            return false;
        }
        let changed = self.selected != Some(day) || self.focused != day;
        self.selected = Some(day);
        self.focused = day;
        self.year = day.year;
        self.month = day.month;
        changed
    }

    /// Clear the selection without moving focus or changing the displayed month.
    pub fn clear_selection(&mut self) -> bool {
        self.selected.take().is_some()
    }

    fn lead_days(self) -> usize {
        let first = CivilDate {
            year: self.year,
            month: self.month,
            day: 1,
        };
        let weekday = i32::from(first.weekday());
        let start = i32::from(self.first_weekday.index());
        (weekday - start).rem_euclid(CALENDAR_WEEK_DAYS as i32) as usize
    }

    fn grid_start(self) -> Option<CivilDate> {
        let first = CivilDate {
            year: self.year,
            month: self.month,
            day: 1,
        };
        CivilDate::from_epoch_day(first.epoch_day() - self.lead_days() as i64)
    }
}

/// A copyable declaration for one unstyled month grid.
///
/// The descriptor retains no allocation, task, timer, observer, or idle scheduler source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "a Calendar descriptor has no effect until one of its parts is mounted"]
pub struct Calendar {
    root_id: ElementId,
}

impl Calendar {
    pub fn new(root_id: impl Into<ElementId>) -> Self {
        Self {
            root_id: root_id.into(),
        }
    }

    pub const fn root_id(self) -> ElementId {
        self.root_id
    }

    pub fn week_id(self, index: usize) -> ElementId {
        derived_calendar_id(self.root_id, CALENDAR_WEEK_ID_TAG, index as u64)
    }

    pub fn day_id(self, day: CivilDate) -> ElementId {
        derived_calendar_id(self.root_id, CALENDAR_DAY_ID_TAG, day.epoch_day() as u64)
    }

    /// Decorate an application-owned month grid without adding layout or appearance.
    pub fn grid_with(self, state: CalendarState, grid: Element) -> Element {
        let disabled = state.disabled || grid.accessibility.disabled;
        let grid = grid
            .id(self.root_id)
            .accessibility_role(AccessibilityRole::Grid)
            .accessibility_row_count(state.week_count())
            .accessibility_column_count(CALENDAR_WEEK_DAYS)
            .cursor_default()
            .app_region_no_drag()
            .user_select_none()
            .disabled(disabled);
        grid.accessibility_active_descendant(self.day_id(state.focused))
    }
    /// Create the unstyled grid part. Use [`Self::grid_with`] to supply an existing element.
    pub fn grid(self, state: CalendarState) -> Element {
        self.grid_with(state, crate::div())
    }

    /// Decorate an application-owned week row without adding layout or appearance.
    pub fn week_with(self, index: usize, week: Element) -> Element {
        week.id(self.week_id(index))
            .accessibility_role(AccessibilityRole::Row)
            .accessibility_row_index(index)
            .app_region_no_drag()
    }
    /// Create the unstyled week part. Use [`Self::week_with`] to supply an existing element.
    pub fn week(self, index: usize) -> Element {
        self.week_with(index, crate::div())
    }

    /// Decorate an application-owned day cell without adding layout or appearance.
    ///
    /// Exactly one day carries the grid's Tab stop; the rest are reached with the arrow keys.
    /// QuickGUI sets the cell's accessible value to the ISO date; call `.accessibility_label(...)`
    /// afterwards to replace it with localized product text.
    pub fn day_with(self, state: CalendarState, day: CivilDate, cell: Element) -> Element {
        let focused = state.focused == day;
        let selectable = state.is_selectable(day);
        let disabled = !selectable || cell.accessibility.disabled;
        let mut cell = cell
            .id(self.day_id(day))
            .accessibility_role(AccessibilityRole::GridCell)
            .accessibility_value(format!("{:04}-{:02}-{:02}", day.year, day.month, day.day))
            .selected(state.selected == Some(day))
            .disabled(disabled)
            .clickable()
            .tab_index(if focused { 0 } else { -1 })
            .key_context(CALENDAR_KEY_CONTEXT)
            .cursor_default()
            .app_region_no_drag()
            .user_select_none();
        if let Some((row, column)) = state.position_of(day) {
            cell = cell
                .accessibility_row_index(row)
                .accessibility_column_index(column);
        }
        cell
    }
    /// Create the unstyled day part. Use [`Self::day_with`] to supply an existing element.
    pub fn day(self, state: CalendarState, day: CivilDate) -> Element {
        self.day_with(state, day, crate::button())
    }

    /// Attach QuickGUI's typed calendar actions and click selection to one day cell.
    ///
    /// Install [`calendar_key_bindings`] once on the application keymap.
    pub fn key_with<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        day: CivilDate,
        cell: Element,
        access: fn(&mut V) -> &mut CalendarState,
    ) -> Element {
        self.key_with_accessor(cx, day, cell, StateAccessor::from(access))
    }
    /// Create the unstyled key part. Use [`Self::key_with`] to supply an existing element.
    pub fn key<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        day: CivilDate,
        access: fn(&mut V) -> &mut CalendarState,
    ) -> Element {
        self.key_with(cx, day, crate::div(), access)
    }

    /// Attach the typed calendar actions against a per-instance state accessor.
    ///
    /// A host that renders many declared calendars through one view passes an accessor that
    /// captures which [`CalendarState`] this day cell belongs to.
    pub fn key_with_accessor<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        day: CivilDate,
        cell: Element,
        access_source: StateAccessor<V, CalendarState>,
    ) -> Element {
        let id = self.day_id(day);
        let calendar = self;
        let access = access_source.clone();
        let previous_day = cx.action_listener(id, move |view, _: &CalendarPreviousDay, cx| {
            move_calendar_focus(view, cx, &access, calendar, day, |state| {
                state.move_focus_days(-1)
            });
        });
        let access = access_source.clone();
        let next_day = cx.action_listener(id, move |view, _: &CalendarNextDay, cx| {
            move_calendar_focus(view, cx, &access, calendar, day, |state| {
                state.move_focus_days(1)
            });
        });
        let access = access_source.clone();
        let previous_week = cx.action_listener(id, move |view, _: &CalendarPreviousWeek, cx| {
            move_calendar_focus(view, cx, &access, calendar, day, |state| {
                state.move_focus_days(-(CALENDAR_WEEK_DAYS as i64))
            });
        });
        let access = access_source.clone();
        let next_week = cx.action_listener(id, move |view, _: &CalendarNextWeek, cx| {
            move_calendar_focus(view, cx, &access, calendar, day, |state| {
                state.move_focus_days(CALENDAR_WEEK_DAYS as i64)
            });
        });
        let access = access_source.clone();
        let week_start = cx.action_listener(id, move |view, _: &CalendarWeekStart, cx| {
            move_calendar_focus(view, cx, &access, calendar, day, |state| {
                state.focus_week_edge(false)
            });
        });
        let access = access_source.clone();
        let week_end = cx.action_listener(id, move |view, _: &CalendarWeekEnd, cx| {
            move_calendar_focus(view, cx, &access, calendar, day, |state| {
                state.focus_week_edge(true)
            });
        });
        let access = access_source.clone();
        let previous_month = cx.action_listener(id, move |view, _: &CalendarPreviousMonth, cx| {
            move_calendar_focus(view, cx, &access, calendar, day, |state| {
                state.move_focus_months(-1)
            });
        });
        let access = access_source.clone();
        let next_month = cx.action_listener(id, move |view, _: &CalendarNextMonth, cx| {
            move_calendar_focus(view, cx, &access, calendar, day, |state| {
                state.move_focus_months(1)
            });
        });
        let access = access_source.clone();
        let previous_year = cx.action_listener(id, move |view, _: &CalendarPreviousYear, cx| {
            move_calendar_focus(view, cx, &access, calendar, day, |state| {
                state.move_focus_months(-12)
            });
        });
        let access = access_source.clone();
        let next_year = cx.action_listener(id, move |view, _: &CalendarNextYear, cx| {
            move_calendar_focus(view, cx, &access, calendar, day, |state| {
                state.move_focus_months(12)
            });
        });
        let access = access_source.clone();
        let select = cx.action_listener(id, move |view, _: &CalendarSelect, cx| {
            let state = access.get(view);
            state.focus_day(day);
            if state.select_focused() {
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let clicked = cx.listener(id, move |view, cx| {
            let state = access.get(view);
            state.focus_day(day);
            if state.select(day) {
                cx.invalidate();
            }
        });

        cell.on_action(previous_day)
            .on_action(next_day)
            .on_action(previous_week)
            .on_action(next_week)
            .on_action(week_start)
            .on_action(week_end)
            .on_action(previous_month)
            .on_action(next_month)
            .on_action(previous_year)
            .on_action(next_year)
            .on_action(select)
            .on_click(clicked)
    }
}

fn move_calendar_focus<V: 'static>(
    view: &mut V,
    cx: &mut crate::EventContext,
    access: &StateAccessor<V, CalendarState>,
    calendar: Calendar,
    day: CivilDate,
    mutate: impl FnOnce(&mut CalendarState) -> bool,
) {
    let state = access.get(view);
    state.focus_day(day);
    if mutate(state) {
        let focused = state.focused_day();
        cx.focus(FocusHandle::new(calendar.day_id(focused)));
        cx.invalidate();
    }
}

/// Create an unstyled semantic month-grid root.
///
/// This shorthand is equivalent to `Calendar::new(id).grid_with(state, div())`.
pub fn calendar(id: impl Into<ElementId>, state: CalendarState) -> Element {
    Calendar::new(id).grid_with(state, div())
}

fn derived_calendar_id(parent: ElementId, tag: u64, value: u64) -> ElementId {
    let mut hash = parent.as_u64() ^ tag ^ value.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    if hash == parent.as_u64() || hash == u64::MAX {
        hash ^= tag.rotate_left(29);
    }
    ElementId::new(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Application, Color, IntoElement, View, WindowOptions, text};

    fn september_2026() -> CivilDate {
        CivilDate::new(2026, 9, 3).expect("a real day")
    }

    #[test]
    fn month_grids_are_six_weeks_at_most_and_start_on_the_declared_weekday() {
        let state = CalendarState::new(september_2026());
        assert_eq!(state.displayed_month(), (2026, 9));
        assert_eq!(state.focused_day(), september_2026());

        // 2026-09-01 is a Tuesday, so a Monday-first grid leads with one August day.
        let first_week = state.week(0).expect("the first week row");
        assert_eq!(first_week[0], CivilDate::new(2026, 8, 31).unwrap());
        assert_eq!(first_week[1], CivilDate::new(2026, 9, 1).unwrap());
        assert!(!state.is_in_displayed_month(first_week[0]));
        assert!(state.is_in_displayed_month(first_week[1]));
        assert_eq!(state.week_count(), 5);
        assert!(state.week(state.week_count()).is_none());
        assert_eq!(state.position_of(september_2026()), Some((0, 3)));

        let sunday_first =
            CalendarState::new(september_2026()).first_weekday(CalendarWeekday::Sunday);
        let first_week = sunday_first.week(0).expect("the first week row");
        assert_eq!(first_week[0], CivilDate::new(2026, 8, 30).unwrap());
        assert_eq!(first_week[2], CivilDate::new(2026, 9, 1).unwrap());

        // Every month fits inside the mounted bound, including a 31-day month that starts late.
        for year in [2024, 2026] {
            for month in 1..=12 {
                let state = CalendarState::new(CivilDate::new(year, month, 1).unwrap());
                assert!(state.week_count() <= MAX_CALENDAR_WEEKS);
                let last = CivilDate::new(year, month, CivilDate::days_in_month(year, month))
                    .expect("a real last day");
                assert!(state.position_of(last).is_some());
            }
        }
        let long = CalendarState::new(CivilDate::new(2026, 8, 1).unwrap());
        assert_eq!(long.week_count(), 6, "August 2026 needs six Monday rows");
    }

    #[test]
    fn roving_focus_crosses_months_and_selection_honors_declared_bounds() {
        let mut state = CalendarState::selected(september_2026())
            .minimum(CivilDate::new(2026, 9, 1).unwrap())
            .maximum(CivilDate::new(2026, 9, 30).unwrap());
        assert_eq!(state.selected_day(), Some(september_2026()));

        assert!(state.move_focus_days(-1));
        assert_eq!(state.focused_day(), CivilDate::new(2026, 9, 2).unwrap());
        assert!(state.move_focus_days(-(CALENDAR_WEEK_DAYS as i64)));
        assert_eq!(
            state.focused_day(),
            CivilDate::new(2026, 8, 26).unwrap(),
            "a week step follows focus into the previous month"
        );
        assert_eq!(state.displayed_month(), (2026, 8));

        assert!(state.focus_week_edge(false));
        assert_eq!(state.focused_day(), CivilDate::new(2026, 8, 24).unwrap());
        assert!(state.focus_week_edge(true));
        assert_eq!(state.focused_day(), CivilDate::new(2026, 8, 30).unwrap());
        assert!(!state.focus_week_edge(true), "already at the week's end");

        assert!(state.move_focus_months(1));
        assert_eq!(state.displayed_month(), (2026, 9));
        assert_eq!(
            state.focused_day(),
            CivilDate::new(2026, 9, 30).unwrap(),
            "a short month shortens the retained day"
        );
        assert!(state.move_focus_months(-12));
        assert_eq!(state.displayed_month(), (2025, 9));

        // Selection refuses days outside the declared range; focus still moves there.
        assert!(!state.select_focused());
        assert_eq!(state.selected_day(), Some(september_2026()));
        assert!(state.focus_day(CivilDate::new(2026, 9, 20).unwrap()));
        assert!(state.select_focused());
        assert_eq!(
            state.selected_day(),
            Some(CivilDate::new(2026, 9, 20).unwrap())
        );
        assert!(!state.is_selectable(CivilDate::new(2026, 10, 1).unwrap()));
        assert!(!state.select(CivilDate::new(2026, 10, 1).unwrap()));
        assert!(state.clear_selection());
        assert!(!state.clear_selection());

        let mut disabled = CalendarState::new(september_2026()).disabled(true);
        assert!(!disabled.move_focus_days(1));
        assert!(!disabled.move_focus_months(1));
        assert!(!disabled.select_focused());
    }

    #[test]
    fn parts_add_exact_grid_semantics_without_layout_or_appearance() {
        let state = CalendarState::selected(september_2026());
        let calendar_bar = Calendar::new("calendar");

        let grid = calendar_bar.grid_with(
            state,
            div().w(280.0).bg(Color::rgb8(1, 2, 3)).child("Caption"),
        );
        assert_eq!(grid.accessibility.role, AccessibilityRole::Grid);
        assert_eq!(grid.visual.background, Some(Color::rgb8(1, 2, 3)));
        assert_eq!(grid.children.len(), 1);
        assert_eq!(
            grid.accessibility.relations.active_descendant(),
            Some(calendar_bar.day_id(september_2026()))
        );

        let week = calendar_bar.week_with(1, div().flex_row());
        assert_eq!(week.accessibility.role, AccessibilityRole::Row);

        let focused = calendar_bar.day_with(state, september_2026(), div().child(text("3")));
        assert_eq!(focused.accessibility.role, AccessibilityRole::GridCell);
        assert_eq!(focused.accessibility.value.as_deref(), Some("2026-09-03"));
        assert!(focused.accessibility.selected);
        assert_eq!(focused.tab_index, 0, "the focused day owns the Tab stop");
        assert!(focused.clickable);
        assert_eq!(focused.visual.background, None);
        assert_eq!(focused.children.len(), 1);

        let other = calendar_bar.day_with(state, CivilDate::new(2026, 9, 4).unwrap(), div());
        assert_eq!(other.tab_index, -1);
        assert!(!other.accessibility.selected);

        let bounded =
            CalendarState::new(september_2026()).maximum(CivilDate::new(2026, 9, 3).unwrap());
        let unavailable =
            Calendar::new("calendar").day_with(bounded, CivilDate::new(2026, 9, 4).unwrap(), div());
        assert!(
            unavailable.accessibility.disabled,
            "a day outside the declared range is not selectable"
        );

        assert_eq!(
            calendar("calendar", state).accessibility.role,
            AccessibilityRole::Grid
        );
    }

    #[test]
    fn calendar_bindings_are_contextual_and_complete() {
        let bindings = calendar_key_bindings();
        assert_eq!(bindings.len(), 12);
        assert!(bindings.iter().all(|binding| {
            binding.context_predicate().is_some_and(|predicate| {
                predicate
                    .depth_of(&[crate::KeyContext::parse(CALENDAR_KEY_CONTEXT).unwrap()])
                    .is_some()
            })
        }));
    }

    struct CalendarView {
        calendar: CalendarState,
    }

    impl CalendarView {
        fn calendar(view: &mut Self) -> &mut CalendarState {
            &mut view.calendar
        }
    }

    impl View for CalendarView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let grid = Calendar::new("calendar");
            let mut root = grid
                .grid_with(self.calendar, div().flex_col())
                .accessibility_label("Choose a day");
            for index in 0..self.calendar.week_count() {
                let days = self.calendar.week(index).expect("a mounted week row");
                let mut row = grid.week_with(index, div().flex_row());
                for day in days {
                    let cell = grid.day_with(
                        self.calendar,
                        day,
                        div().w(28.0).h(24.0).child(text(day.day.to_string())),
                    );
                    row = row.child(grid.key_with(cx, day, cell, Self::calendar));
                }
                root = root.child(row);
            }
            root
        }
    }

    #[test]
    fn calendar_uses_existing_focus_keyboard_click_and_idle_paths() {
        let (mut cx, view) = Application::new()
            .bind_keys(calendar_key_bindings())
            .into_test_context(
                WindowOptions::default(),
                CalendarView {
                    calendar: CalendarState::new(september_2026()),
                },
            )
            .unwrap();
        let window = view.window_handle();
        let grid = Calendar::new("calendar");

        cx.simulate_keystrokes(window, "tab").unwrap();
        assert_eq!(
            cx.focused(window).unwrap(),
            Some(grid.day_id(september_2026()))
        );

        cx.simulate_keystrokes(window, "right").unwrap();
        assert_eq!(
            cx.read(view, |view| view.calendar.focused_day()).unwrap(),
            CivilDate::new(2026, 9, 4).unwrap()
        );
        assert_eq!(
            cx.focused(window).unwrap(),
            Some(grid.day_id(CivilDate::new(2026, 9, 4).unwrap()))
        );

        cx.simulate_keystrokes(window, "down").unwrap();
        assert_eq!(
            cx.read(view, |view| view.calendar.focused_day()).unwrap(),
            CivilDate::new(2026, 9, 11).unwrap()
        );
        cx.simulate_keystrokes(window, "home").unwrap();
        assert_eq!(
            cx.read(view, |view| view.calendar.focused_day()).unwrap(),
            CivilDate::new(2026, 9, 7).unwrap()
        );
        cx.simulate_keystrokes(window, "enter").unwrap();
        assert_eq!(
            cx.read(view, |view| view.calendar.selected_day()).unwrap(),
            Some(CivilDate::new(2026, 9, 7).unwrap())
        );

        cx.simulate_keystrokes(window, "pageup").unwrap();
        assert_eq!(
            cx.read(view, |view| view.calendar.displayed_month())
                .unwrap(),
            (2026, 8)
        );

        let clicked = CivilDate::new(2026, 8, 12).unwrap();
        cx.click(window, grid.day_id(clicked)).unwrap();
        assert_eq!(
            cx.read(view, |view| view.calendar.selected_day()).unwrap(),
            Some(clicked)
        );

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }
}
