use std::sync::Arc;
use web_time::{Duration, Instant};

use crate::{
    AccessibilityLive, AccessibilityRole, Element, ElementId, Key, PointerEvent, PointerPhase,
    StateAccessor, ViewContext, div,
};

/// Maximum toasts retained by one queue.
///
/// Pushing past the bound drops the oldest toast, so a burst of application events can never grow
/// the retained tree without limit.
pub const MAX_TOASTS: usize = 8;

/// Maximum UTF-8 bytes retained by one toast title, description, or action label.
pub const MAX_TOAST_TEXT_BYTES: usize = 512;

/// Default auto-dismiss duration a provider gives a toast that declares none.
pub const DEFAULT_TOAST_TIMEOUT: Duration = Duration::from_secs(5);

/// Default number of toasts shown before the rest are flagged limited.
pub const DEFAULT_TOAST_LIMIT: usize = 3;

/// Maximum swipe distance one toast may require before it dismisses, in logical pixels.
pub const MAX_TOAST_SWIPE_THRESHOLD: f32 = 512.0;

/// Default swipe distance one toast requires before it dismisses, in logical pixels.
pub const DEFAULT_TOAST_SWIPE_THRESHOLD: f32 = 48.0;

/// Longest auto-dismiss duration retained by one toast.
pub const MAX_TOAST_DURATION: Duration = Duration::from_secs(60);

const TOAST_ROOT_ID_TAG: u64 = 0x51c8_ae37_60b2_49df;
const TOAST_TITLE_ID_TAG: u64 = 0xd94b_3f16_c827_a05e;
const TOAST_DESCRIPTION_ID_TAG: u64 = 0x0e73_82ba_195c_6df4;
const TOAST_ACTION_ID_TAG: u64 = 0xa620_5d9e_71f3_8c17;
const TOAST_CLOSE_ID_TAG: u64 = 0x3fb1_c74d_2e96_50a8;
const TOAST_POSITIONER_ID_TAG: u64 = 0x8e02_46b5_da91_c73f;
const TOAST_CONTENT_ID_TAG: u64 = 0x1c5d_930a_47e8_b264;
const TOAST_PORTAL_ID_TAG: u64 = 0x72a9_f4e1_0c36_58bd;

/// Urgency of one toast, which selects its live-region politeness and role.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ToastKind {
    #[default]
    Info,
    Success,
    Warning,
    Error,
    /// Work that has not finished yet, Base UI's `loading` type.
    ///
    /// A loading toast is polite and, when queued through [`ToastManager::promise`], persistent
    /// until the application resolves it.
    Loading,
}

impl ToastKind {
    /// Whether this kind interrupts the current screen-reader utterance.
    pub const fn is_assertive(self) -> bool {
        matches!(self, Self::Warning | Self::Error)
    }

    const fn live(self) -> AccessibilityLive {
        if self.is_assertive() {
            AccessibilityLive::Assertive
        } else {
            AccessibilityLive::Polite
        }
    }

    const fn role(self) -> AccessibilityRole {
        if self.is_assertive() {
            AccessibilityRole::Alert
        } else {
            AccessibilityRole::Status
        }
    }
}

/// The direction a toast is swiped to dismiss it, Base UI's `swipeDirection`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ToastSwipeDirection {
    #[default]
    Right,
    Left,
    Up,
    Down,
}

impl ToastSwipeDirection {
    /// Project one pointer motion onto this direction, in logical pixels.
    ///
    /// Movement away from the direction is negative, so a gesture that starts the wrong way has to
    /// come back before it counts.
    const fn project(self, dx: f32, dy: f32) -> f32 {
        match self {
            Self::Right => dx,
            Self::Left => -dx,
            Self::Up => -dy,
            Self::Down => dy,
        }
    }

    /// Whether this direction runs along the horizontal axis.
    pub const fn is_horizontal(self) -> bool {
        matches!(self, Self::Right | Self::Left)
    }
}

/// What one captured swipe event did to a toast.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ToastSwipeChange {
    /// Whether the swipe movement or the queue changed, so the application redraws.
    pub changed: bool,
    /// Whether the released swipe passed the threshold and dismissed the toast.
    pub dismissed: bool,
}

/// Stable identity of one queued toast.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ToastId(u64);

impl ToastId {
    pub const fn value(self) -> u64 {
        self.0
    }
}

/// One caller-declared toast.
///
/// Text is shared and bounded. The application owns every visual declaration; QuickGUI retains
/// only the strings it announces and the exact duration after which the toast disappears.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Toast {
    title: Arc<str>,
    description: Option<Arc<str>>,
    action: Option<Arc<str>>,
    kind: ToastKind,
    duration: Option<Duration>,
}

impl Toast {
    /// Create a toast with an auto-dismiss duration of five seconds.
    pub fn new(title: impl Into<Arc<str>>) -> Self {
        Self {
            title: bounded_text(title.into()),
            description: None,
            action: None,
            kind: ToastKind::Info,
            duration: Some(Duration::from_secs(5)),
        }
    }

    #[must_use]
    pub fn description(mut self, description: impl Into<Arc<str>>) -> Self {
        let description = bounded_text(description.into());
        self.description = (!description.is_empty()).then_some(description);
        self
    }

    /// Label one caller-owned action control, such as *Undo*.
    #[must_use]
    pub fn action(mut self, action: impl Into<Arc<str>>) -> Self {
        let action = bounded_text(action.into());
        self.action = (!action.is_empty()).then_some(action);
        self
    }

    #[must_use]
    pub const fn kind(mut self, kind: ToastKind) -> Self {
        self.kind = kind;
        self
    }

    /// Replace the auto-dismiss duration, clamped to [`MAX_TOAST_DURATION`].
    #[must_use]
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration.min(MAX_TOAST_DURATION));
        self
    }

    /// Keep this toast until it is dismissed explicitly.
    #[must_use]
    pub const fn persistent(mut self) -> Self {
        self.duration = None;
        self
    }

    pub fn title(&self) -> &Arc<str> {
        &self.title
    }

    pub fn description_text(&self) -> Option<&Arc<str>> {
        self.description.as_ref()
    }

    pub fn action_label(&self) -> Option<&Arc<str>> {
        self.action.as_ref()
    }

    pub const fn toast_kind(&self) -> ToastKind {
        self.kind
    }

    pub const fn duration_value(&self) -> Option<Duration> {
        self.duration
    }
}

/// One queued toast with its exact remaining lifetime.
#[derive(Clone, Debug, PartialEq)]
pub struct ToastEntry {
    id: ToastId,
    toast: Toast,
    deadline: Option<Instant>,
    remaining: Option<Duration>,
    paused: bool,
    limited: bool,
    swiping: bool,
    swipe: f32,
}

impl ToastEntry {
    pub const fn id(&self) -> ToastId {
        self.id
    }

    pub const fn toast(&self) -> &Toast {
        &self.toast
    }

    /// The exact instant this toast disappears, or `None` while paused or persistent.
    pub const fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// Whether this toast's countdown is currently paused by hover or focus.
    pub const fn is_paused(&self) -> bool {
        self.paused
    }

    /// Whether this toast sits past the provider's visible limit, Base UI's `data-limited`.
    ///
    /// A limited toast stays queued, keeps announcing, and keeps counting down; it is only
    /// presentation that the application collapses behind the newer ones.
    pub const fn is_limited(&self) -> bool {
        self.limited
    }

    /// Whether a captured swipe is currently moving this toast.
    pub const fn is_swiping(&self) -> bool {
        self.swiping
    }

    /// How far this toast has been swiped along the declared direction, in logical pixels.
    ///
    /// This is Base UI's swipe movement: the application translates the toast by it while the
    /// gesture is in flight and animates it back when the gesture is released short.
    pub const fn swipe_movement(&self) -> f32 {
        self.swipe
    }
}

/// A bounded in-window toast queue with exact one-shot auto-dismissal.
///
/// The manager owns no timer. It reports the single next deadline through [`Self::next_deadline`]
/// so the application sleeps exactly once with [`crate::AsyncViewContext::sleep_until`] and then
/// calls [`Self::expire`]. An empty or fully paused queue reports no deadline at all, so a settled
/// window keeps zero idle sources.
#[derive(Clone, Debug, PartialEq)]
pub struct ToastManager {
    entries: Vec<ToastEntry>,
    next_id: u64,
    timeout: Duration,
    limit: usize,
    expanded: bool,
    swipe_direction: ToastSwipeDirection,
    swipe_threshold: f32,
}

impl Default for ToastManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ToastManager {
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_id: 1,
            timeout: DEFAULT_TOAST_TIMEOUT,
            limit: DEFAULT_TOAST_LIMIT,
            expanded: false,
            swipe_direction: ToastSwipeDirection::Right,
            swipe_threshold: DEFAULT_TOAST_SWIPE_THRESHOLD,
        }
    }

    /// Replace the auto-dismiss duration a queued toast inherits, Base UI's Provider `timeout`.
    ///
    /// A toast that declares its own [`Toast::duration`] or [`Toast::persistent`] keeps it. The
    /// value is clamped to [`MAX_TOAST_DURATION`].
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout.min(MAX_TOAST_DURATION);
        self
    }

    /// Replace how many toasts stay unlimited, Base UI's Provider `limit`.
    ///
    /// The newest `limit` toasts are ordinary; every older one is flagged
    /// [`ToastEntry::is_limited`] so the application can collapse it behind the stack. Limited
    /// toasts still announce and still count down. A limit of zero is treated as one.
    #[must_use]
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = limit.clamp(1, MAX_TOASTS);
        self.apply_limit();
        self
    }

    /// Choose the direction a swipe dismisses a toast, Base UI's `swipeDirection`.
    #[must_use]
    pub const fn swipe_direction(mut self, direction: ToastSwipeDirection) -> Self {
        self.swipe_direction = direction;
        self
    }

    /// Set how far a swipe must travel before it dismisses, in logical pixels.
    #[must_use]
    pub fn swipe_threshold(mut self, threshold: f32) -> Self {
        self.swipe_threshold = if threshold.is_finite() && threshold > 0.0 {
            threshold.min(MAX_TOAST_SWIPE_THRESHOLD)
        } else {
            DEFAULT_TOAST_SWIPE_THRESHOLD
        };
        self
    }

    /// The auto-dismiss duration a queued toast inherits.
    pub const fn timeout_value(&self) -> Duration {
        self.timeout
    }

    /// How many toasts stay unlimited.
    pub const fn limit_value(&self) -> usize {
        self.limit
    }

    /// The direction a swipe dismisses a toast.
    pub const fn swipe_direction_value(&self) -> ToastSwipeDirection {
        self.swipe_direction
    }

    /// How far a swipe must travel before it dismisses.
    pub const fn swipe_threshold_value(&self) -> f32 {
        self.swipe_threshold
    }

    /// Whether the viewport is showing its stack expanded, Base UI's `expanded` state.
    pub const fn is_expanded(&self) -> bool {
        self.expanded
    }

    /// Expand or collapse the stack, returning whether it changed.
    ///
    /// A viewport normally expands while it is hovered or holds focus. QuickGUI keeps this an
    /// explicit application decision so it never fights a product's own motion.
    pub fn set_expanded(&mut self, expanded: bool) -> bool {
        std::mem::replace(&mut self.expanded, expanded) != expanded
    }

    pub fn entries(&self) -> &[ToastEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entry(&self, id: ToastId) -> Option<&ToastEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// Queue one toast, returning its stable identity.
    ///
    /// Reaching [`MAX_TOASTS`] drops the oldest toast rather than growing the queue.
    pub fn push(&mut self, toast: Toast, now: Instant) -> ToastId {
        if self.entries.len() == MAX_TOASTS {
            self.entries.remove(0);
        }
        let id = ToastId(self.next_id);
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let deadline = toast.duration.map(|duration| now + duration);
        self.entries.push(ToastEntry {
            id,
            toast,
            deadline,
            remaining: None,
            paused: false,
            limited: false,
            swiping: false,
            swipe: 0.0,
        });
        self.apply_limit();
        id
    }

    /// Queue one toast, Base UI's manager `add`.
    ///
    /// This is [`Self::push`] under Base UI's name; both stay supported.
    pub fn add(&mut self, toast: Toast, now: Instant) -> ToastId {
        self.push(toast, now)
    }

    /// Replace one queued toast's content and re-arm its countdown, Base UI's manager `update`.
    ///
    /// The identity, its position in the stack, and any swipe in flight are preserved, so a
    /// progress message can become a result without the toast jumping or re-announcing as new.
    /// Returns whether the toast was queued.
    pub fn update(&mut self, id: ToastId, toast: Toast, now: Instant) -> bool {
        let timeout = self.timeout;
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) else {
            return false;
        };
        let duration = toast.duration;
        entry.toast = toast;
        if entry.paused {
            entry.remaining = duration;
            entry.deadline = None;
        } else {
            entry.remaining = None;
            entry.deadline = duration.map(|declared| now + declared.min(timeout.max(declared)));
        }
        true
    }

    /// Remove one toast, Base UI's manager `close`.
    ///
    /// This is [`Self::dismiss`] under Base UI's name; both stay supported.
    pub fn close(&mut self, id: ToastId) -> bool {
        self.dismiss(id)
    }

    /// Remove every toast, Base UI's manager `close` with no identity.
    ///
    /// This is [`Self::clear`] under Base UI's name; both stay supported.
    pub fn close_all(&mut self) -> bool {
        self.clear()
    }

    /// Queue a persistent loading toast for work the application has just started.
    ///
    /// This is the first half of Base UI's `promise` helper: QuickGUI owns no future, so the
    /// application resolves the same identity with [`Self::resolve`] from whichever foreground
    /// task it already spawned. Nothing here schedules a timer.
    pub fn promise(&mut self, loading: impl Into<Arc<str>>, now: Instant) -> ToastId {
        self.push(
            Toast::new(loading).kind(ToastKind::Loading).persistent(),
            now,
        )
    }

    /// Resolve a toast queued by [`Self::promise`] into its success or error result.
    ///
    /// This is [`Self::update`] named for the promise flow; the identity and stack position are
    /// preserved and the countdown restarts from `now`.
    pub fn resolve(&mut self, id: ToastId, resolved: Toast, now: Instant) -> bool {
        self.update(id, resolved, now)
    }

    /// Remove one toast, returning whether it was queued.
    pub fn dismiss(&mut self, id: ToastId) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.id != id);
        let changed = self.entries.len() != before;
        if changed {
            self.apply_limit();
        }
        changed
    }

    /// Remove every toast, returning whether the queue changed.
    pub fn clear(&mut self) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        self.entries.clear();
        true
    }

    /// Pause one toast's countdown on hover or focus, returning whether it changed.
    pub fn pause(&mut self, id: ToastId, now: Instant) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) else {
            return false;
        };
        if entry.paused {
            return false;
        }
        entry.paused = true;
        if let Some(deadline) = entry.deadline.take() {
            entry.remaining = Some(deadline.saturating_duration_since(now));
        }
        true
    }

    /// Resume one paused toast's countdown, returning whether it changed.
    pub fn resume(&mut self, id: ToastId, now: Instant) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) else {
            return false;
        };
        if !entry.paused {
            return false;
        }
        entry.paused = false;
        if let Some(remaining) = entry.remaining.take() {
            entry.deadline = Some(now + remaining);
        }
        true
    }

    /// The single next auto-dismiss instant across the queue.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.entries.iter().filter_map(|entry| entry.deadline).min()
    }

    /// Remove every toast whose exact deadline has passed, returning whether the queue changed.
    pub fn expire(&mut self, now: Instant) -> bool {
        let before = self.entries.len();
        self.entries
            .retain(|entry| entry.deadline.is_none_or(|deadline| deadline > now));
        let changed = self.entries.len() != before;
        if changed {
            self.apply_limit();
        }
        changed
    }

    /// Apply one captured pointer event from a toast root's swipe gesture.
    ///
    /// This is Base UI's swipe-to-dismiss: motion is projected onto the declared
    /// [`Self::swipe_direction`] and exposed through [`ToastEntry::swipe_movement`] so the
    /// application translates the toast itself. Releasing past [`Self::swipe_threshold`] dismisses
    /// it; releasing short resets the movement to zero and leaves the toast queued.
    ///
    /// The gesture is pure pointer capture and schedules nothing. Motion away from the direction
    /// is clamped at zero, so a toast can never be dragged the wrong way.
    pub fn apply_swipe(&mut self, id: ToastId, event: &PointerEvent) -> ToastSwipeChange {
        let direction = self.swipe_direction;
        let threshold = self.swipe_threshold;
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) else {
            return ToastSwipeChange::default();
        };
        match event.phase {
            PointerPhase::Down => {
                let changed = !entry.swiping || entry.swipe != 0.0;
                entry.swiping = true;
                entry.swipe = 0.0;
                ToastSwipeChange {
                    changed,
                    dismissed: false,
                }
            }
            PointerPhase::Move => {
                if !entry.swiping {
                    return ToastSwipeChange::default();
                }
                let travel = direction.project(event.delta.x, event.delta.y);
                if !travel.is_finite() {
                    return ToastSwipeChange::default();
                }
                let next = (entry.swipe + travel).max(0.0);
                let changed = next != entry.swipe;
                entry.swipe = next;
                ToastSwipeChange {
                    changed,
                    dismissed: false,
                }
            }
            PointerPhase::Up => {
                if !entry.swiping {
                    return ToastSwipeChange::default();
                }
                entry.swiping = false;
                if entry.swipe >= threshold {
                    self.dismiss(id);
                    return ToastSwipeChange {
                        changed: true,
                        dismissed: true,
                    };
                }
                let changed = entry.swipe != 0.0;
                entry.swipe = 0.0;
                ToastSwipeChange {
                    changed,
                    dismissed: false,
                }
            }
            PointerPhase::Cancel => {
                let changed = entry.swiping || entry.swipe != 0.0;
                entry.swiping = false;
                entry.swipe = 0.0;
                ToastSwipeChange {
                    changed,
                    dismissed: false,
                }
            }
        }
    }

    /// Flag every toast past the visible limit, newest first.
    fn apply_limit(&mut self) {
        let len = self.entries.len();
        let limit = self.limit.clamp(1, MAX_TOASTS);
        for (index, entry) in self.entries.iter_mut().enumerate() {
            // Entries are ordered oldest first, so the newest `limit` of them stay unlimited.
            entry.limited = len.saturating_sub(index) > limit;
        }
    }
}

/// A controlled, unstyled toast-viewport descriptor.
///
/// The application owns the viewport's placement, stacking, spacing, colors, icons, and motion.
/// QuickGUI supplies stable identities, live-region announcements chosen by toast kind, exact
/// title and description relationships, and focused Escape dismissal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "a ToastViewport descriptor has no effect until its parts are mounted"]
pub struct ToastViewport {
    root_id: ElementId,
}

impl ToastViewport {
    pub fn new(root_id: impl Into<ElementId>) -> Self {
        Self {
            root_id: root_id.into(),
        }
    }

    pub const fn viewport_id(self) -> ElementId {
        self.root_id
    }

    /// Decorate the application-owned viewport without adding layout or appearance.
    ///
    /// The viewport itself is an ordinary group; each toast is its own live region, so an
    /// unchanged queue announces nothing.
    pub fn viewport_with(self, viewport: Element) -> Element {
        viewport
            .id(self.root_id)
            .accessibility_role(AccessibilityRole::Group)
            .app_region_no_drag()
    }
    /// Create the unstyled viewport part. Use [`Self::viewport_with`] to supply an existing element.
    pub fn viewport(self) -> Element {
        self.viewport_with(crate::div())
    }

    /// Stable identity of the portal the viewport is mounted inside.
    pub fn portal_id(self) -> ElementId {
        derived_toast_id(self.root_id, TOAST_PORTAL_ID_TAG, 0)
    }

    /// Decorate a caller-owned window-level portal for the toast layer.
    ///
    /// Base UI renders the viewport into a portal so toasts escape the layout that raised them.
    /// QuickGUI's overlay plane does the same, and this part deliberately does **not** block
    /// pointer input: a full-window toast layer that swallowed clicks would break every control
    /// underneath it. Placement, size, and stacking direction stay application-owned.
    pub fn portal_with(self, portal: Element) -> Element {
        let mut portal = portal
            .id(self.portal_id())
            .overlay()
            .app_region_no_drag()
            .cursor_default();
        // `overlay()` opts into pointer blocking for popovers and dialogs. A toast layer must let
        // everything it floats over stay clickable; only the toasts themselves take the pointer.
        portal.blocks_pointer = false;
        portal
    }
    /// Create the unstyled portal part. Use [`Self::portal_with`] to supply an existing element.
    pub fn portal(self) -> Element {
        self.portal_with(crate::div())
    }

    /// Describe the parts of one queued toast.
    ///
    /// `index` counts from the newest toast, which is the order an application stacks them in.
    pub fn toast(self, entry: &ToastEntry) -> ToastParts {
        ToastParts {
            viewport: self,
            id: entry.id,
            kind: entry.toast.kind,
            has_description: entry.toast.description.is_some(),
            index: 0,
            limited: entry.limited,
            expanded: false,
            swiping: entry.swiping,
            swipe: entry.swipe,
        }
    }

    /// Describe the parts of every queued toast, newest first.
    ///
    /// Each descriptor carries its own stack index, the provider's limited flag, and the manager's
    /// expanded state, which is everything Base UI publishes as `data-index`, `data-limited`, and
    /// `data-expanded`.
    pub fn toasts(self, manager: &ToastManager) -> impl Iterator<Item = ToastParts> + '_ {
        let expanded = manager.is_expanded();
        manager
            .entries()
            .iter()
            .rev()
            .enumerate()
            .map(move |(index, entry)| ToastParts {
                viewport: self,
                id: entry.id,
                kind: entry.toast.kind,
                has_description: entry.toast.description.is_some(),
                index,
                limited: entry.limited,
                expanded,
                swiping: entry.swiping,
                swipe: entry.swipe,
            })
    }
}

/// A copyable declaration for the parts of one queued toast.
#[derive(Clone, Copy, Debug, PartialEq)]
#[must_use = "a ToastParts descriptor has no effect until its parts are mounted"]
pub struct ToastParts {
    viewport: ToastViewport,
    id: ToastId,
    kind: ToastKind,
    has_description: bool,
    index: usize,
    limited: bool,
    expanded: bool,
    swiping: bool,
    swipe: f32,
}

impl ToastParts {
    pub const fn id(self) -> ToastId {
        self.id
    }

    pub const fn kind(self) -> ToastKind {
        self.kind
    }

    pub fn root_id(self) -> ElementId {
        derived_toast_id(self.viewport.root_id, TOAST_ROOT_ID_TAG, self.id.0)
    }

    pub fn title_id(self) -> ElementId {
        derived_toast_id(self.viewport.root_id, TOAST_TITLE_ID_TAG, self.id.0)
    }

    pub fn description_id(self) -> ElementId {
        derived_toast_id(self.viewport.root_id, TOAST_DESCRIPTION_ID_TAG, self.id.0)
    }

    pub fn action_id(self) -> ElementId {
        derived_toast_id(self.viewport.root_id, TOAST_ACTION_ID_TAG, self.id.0)
    }

    pub fn close_id(self) -> ElementId {
        derived_toast_id(self.viewport.root_id, TOAST_CLOSE_ID_TAG, self.id.0)
    }

    pub fn positioner_id(self) -> ElementId {
        derived_toast_id(self.viewport.root_id, TOAST_POSITIONER_ID_TAG, self.id.0)
    }

    pub fn content_id(self) -> ElementId {
        derived_toast_id(self.viewport.root_id, TOAST_CONTENT_ID_TAG, self.id.0)
    }

    /// This toast's position in the stack, counting from the newest, Base UI's `data-index`.
    pub const fn index(self) -> usize {
        self.index
    }

    /// Whether this toast sits past the provider's visible limit, Base UI's `data-limited`.
    pub const fn is_limited(self) -> bool {
        self.limited
    }

    /// Whether the viewport is showing its stack expanded, Base UI's `data-expanded`.
    pub const fn is_expanded(self) -> bool {
        self.expanded
    }

    /// Whether a captured swipe is currently moving this toast.
    pub const fn is_swiping(self) -> bool {
        self.swiping
    }

    /// How far this toast has been swiped, in logical pixels.
    pub const fn swipe_movement(self) -> f32 {
        self.swipe
    }

    /// The stacking offset of this toast, given the pitch the application lays its stack out at.
    ///
    /// Base UI derives `--toast-offset` from the measured heights of the newer toasts. QuickGUI
    /// never measures on the application's behalf, so the caller supplies the pitch it wants — a
    /// small peek while the stack is collapsed, a full row height plus gap while it is expanded —
    /// and this returns the offset for this toast's [`Self::index`].
    pub fn offset(self, pitch: f32) -> f32 {
        if !pitch.is_finite() {
            return 0.0;
        }
        self.index as f32 * pitch
    }

    /// Decorate the application-owned toast root without adding layout or appearance.
    ///
    /// Informational toasts project a polite Status region; warnings and errors project an
    /// assertive Alert region. The root is focusable so keyboard users can reach the toast's
    /// action and close controls, and so Escape can dismiss the toast that has focus.
    pub fn root_with(self, root: Element) -> Element {
        let root = root
            .id(self.root_id())
            .accessibility_role(self.kind.role())
            .accessibility_live(self.kind.live())
            .accessibility_labelled_by(self.title_id())
            .focusable()
            .app_region_no_drag();
        if self.has_description {
            root.accessibility_described_by(self.description_id())
        } else {
            root
        }
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(self) -> Element {
        self.root_with(crate::div())
    }

    /// Decorate the caller-owned wrapper that places one toast inside the stack.
    ///
    /// Base UI separates the Positioner, which owns where the toast sits, from the Root, which
    /// owns what it announces. QuickGUI supplies the stable identity and drag exclusion; the
    /// stacking transform, spacing, and motion stay application-owned — derive them from
    /// [`Self::index`], [`Self::offset`], and [`Self::is_expanded`].
    pub fn positioner_with(self, positioner: Element) -> Element {
        positioner.id(self.positioner_id()).app_region_no_drag()
    }
    /// Create the unstyled positioner part. Use [`Self::positioner_with`] to supply an existing element.
    pub fn positioner(self) -> Element {
        self.positioner_with(crate::div())
    }

    /// Decorate the caller-owned content wrapper inside the toast root.
    ///
    /// The root already carries the live region and the title and description relationships, so
    /// the content is an ordinary container with a stable identity.
    pub fn content_with(self, content: Element) -> Element {
        content.id(self.content_id())
    }
    /// Create the unstyled content part. Use [`Self::content_with`] to supply an existing element.
    pub fn content(self) -> Element {
        self.content_with(crate::div())
    }

    /// Decorate the application-owned title, which names the toast.
    pub fn title_with(self, title: Element) -> Element {
        title.id(self.title_id())
    }
    /// Create the unstyled title part. Use [`Self::title_with`] to supply an existing element.
    pub fn title(self) -> Element {
        self.title_with(crate::div())
    }

    /// Decorate the application-owned description, which describes the toast.
    pub fn description_with(self, description: Element) -> Element {
        description.id(self.description_id())
    }
    /// Create the unstyled description part. Use [`Self::description_with`] to supply an existing element.
    pub fn description(self) -> Element {
        self.description_with(crate::div())
    }

    /// Decorate the application-owned action control.
    pub fn action_with(self, action: Element) -> Element {
        action
            .id(self.action_id())
            .accessibility_role(AccessibilityRole::Button)
            .clickable()
            .cursor_default()
            .app_region_no_drag()
            .user_select_none()
    }
    /// Create the unstyled action part. Use [`Self::action_with`] to supply an existing element.
    pub fn action(self) -> Element {
        self.action_with(crate::button())
    }

    /// Decorate the application-owned close control.
    pub fn close_with(self, close: Element) -> Element {
        close
            .id(self.close_id())
            .accessibility_role(AccessibilityRole::Button)
            .clickable()
            .cursor_default()
            .app_region_no_drag()
            .user_select_none()
    }
    /// Create the unstyled close part. Use [`Self::close_with`] to supply an existing element.
    pub fn close(self) -> Element {
        self.close_with(crate::button())
    }

    /// Attach focused Escape dismissal to this toast's root.
    ///
    /// Escape is handled only while focus is inside this toast, so it never competes with a
    /// dialog, popover, or the application's own Escape handling.
    pub fn key_with<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        root: Element,
        access: fn(&mut V) -> &mut ToastManager,
    ) -> Element {
        self.key_with_accessor(cx, root, StateAccessor::from(access))
    }
    /// Create the unstyled key part. Use [`Self::key_with`] to supply an existing element.
    pub fn key<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        access: fn(&mut V) -> &mut ToastManager,
    ) -> Element {
        self.key_with(cx, crate::div(), access)
    }

    /// Attach focused Escape dismissal against a per-instance [`ToastManager`] accessor.
    ///
    /// A host that owns one manager per declared viewport passes an accessor that captures which
    /// viewport this toast belongs to.
    pub fn key_with_accessor<V: 'static>(
        self,
        cx: &mut ViewContext<'_, V>,
        root: Element,
        access: StateAccessor<V, ToastManager>,
    ) -> Element {
        let id = self.id;
        let dismiss = cx.key_down_listener(self.root_id(), move |view, event, cx| {
            if event.key != Key::Escape || event.repeat {
                return;
            }
            if access.get(view).dismiss(id) {
                cx.stop_propagation();
                cx.prevent_default();
                cx.invalidate();
            }
        });
        root.on_key_down(dismiss)
    }
}

fn bounded_text(text: Arc<str>) -> Arc<str> {
    if text.len() <= MAX_TOAST_TEXT_BYTES {
        return text;
    }
    let mut end = MAX_TOAST_TEXT_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    Arc::from(&text[..end])
}

/// Create an unstyled toast-viewport root.
///
/// This shorthand is equivalent to `ToastViewport::new(id).viewport_with(div())`.
pub fn toast_viewport(id: impl Into<ElementId>) -> Element {
    ToastViewport::new(id).viewport_with(div())
}

fn derived_toast_id(scope: ElementId, tag: u64, toast: u64) -> ElementId {
    let mut hash = scope
        .as_u64()
        .rotate_left(5)
        .wrapping_add(toast.rotate_right(17))
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
    use crate::{Color, IntoElement, TestAppContext, View, button, text};

    #[test]
    fn queue_is_bounded_and_countdowns_are_exact() {
        let start = Instant::now();
        let mut manager = ToastManager::new();
        assert!(manager.is_empty());
        assert_eq!(manager.next_deadline(), None);
        assert!(!manager.expire(start));

        let saved = manager.push(Toast::new("Saved").duration(Duration::from_secs(4)), start);
        let failed = manager.push(
            Toast::new("Upload failed")
                .description("The network connection dropped.")
                .action("Retry")
                .kind(ToastKind::Error)
                .duration(Duration::from_secs(8)),
            start,
        );
        assert_eq!(manager.len(), 2);
        assert_ne!(saved, failed);
        assert_eq!(
            manager.next_deadline(),
            Some(start + Duration::from_secs(4))
        );

        assert!(!manager.expire(start + Duration::from_secs(3)));
        assert!(manager.expire(start + Duration::from_secs(5)));
        assert_eq!(manager.len(), 1);
        assert!(manager.entry(saved).is_none());
        assert_eq!(
            manager.next_deadline(),
            Some(start + Duration::from_secs(8))
        );

        // Hover or focus pauses the countdown; nothing expires while paused.
        assert!(manager.pause(failed, start + Duration::from_secs(5)));
        assert!(!manager.pause(failed, start + Duration::from_secs(5)));
        assert_eq!(manager.next_deadline(), None);
        assert!(manager.entry(failed).expect("paused toast").is_paused());
        assert!(!manager.expire(start + Duration::from_secs(60)));
        assert!(manager.resume(failed, start + Duration::from_secs(60)));
        assert_eq!(
            manager.next_deadline(),
            Some(start + Duration::from_secs(63))
        );
        assert!(!manager.resume(failed, start));
        assert!(manager.dismiss(failed));

        let persistent = manager.push(Toast::new("Recording").persistent(), start);
        assert_eq!(
            manager.entry(persistent).expect("persistent").deadline(),
            None
        );
        assert!(manager.pause(persistent, start));
        assert!(manager.entry(persistent).expect("persistent").is_paused());
        assert!(!manager.expire(start + Duration::from_secs(3_600)));
        assert!(manager.resume(persistent, start + Duration::from_secs(3_600)));
        assert!(manager.entry(persistent).is_some());

        assert!(manager.dismiss(persistent));
        assert!(!manager.dismiss(persistent));
        assert!(!manager.clear());
        manager.push(Toast::new("Another"), start);
        assert!(manager.clear());

        for index in 0..MAX_TOASTS + 3 {
            manager.push(Toast::new(format!("Toast {index}")), start);
        }
        assert_eq!(manager.len(), MAX_TOASTS);
        assert_eq!(
            manager.entries()[0].toast().title().as_ref(),
            format!("Toast {}", 3).as_str()
        );

        let clamped = Toast::new("Long").duration(Duration::from_secs(600));
        assert_eq!(clamped.duration_value(), Some(MAX_TOAST_DURATION));
        let long = "x".repeat(MAX_TOAST_TEXT_BYTES + 40);
        let bounded = Toast::new(long.clone()).description(long).action("");
        assert_eq!(bounded.title().len(), MAX_TOAST_TEXT_BYTES);
        assert_eq!(
            bounded.description_text().map(|text| text.len()),
            Some(MAX_TOAST_TEXT_BYTES)
        );
        assert_eq!(bounded.action_label(), None);
    }

    #[test]
    fn updating_a_paused_toast_keeps_its_new_countdown_paused() {
        let start = Instant::now();
        let mut manager = ToastManager::new();
        let id = manager.push(Toast::new("Uploading").persistent(), start);
        assert!(manager.pause(id, start));
        assert!(manager.update(
            id,
            Toast::new("Uploaded").duration(Duration::from_secs(4)),
            start + Duration::from_secs(1),
        ));
        assert!(manager.entry(id).expect("updated toast").is_paused());
        assert_eq!(manager.next_deadline(), None);
        assert!(!manager.expire(start + Duration::from_secs(60)));

        assert!(manager.resume(id, start + Duration::from_secs(60)));
        assert_eq!(
            manager.next_deadline(),
            Some(start + Duration::from_secs(64))
        );
    }

    #[test]
    fn kinds_select_live_politeness_and_role() {
        assert!(!ToastKind::Info.is_assertive());
        assert!(!ToastKind::Success.is_assertive());
        assert!(ToastKind::Warning.is_assertive());
        assert!(ToastKind::Error.is_assertive());

        let start = Instant::now();
        let mut manager = ToastManager::new();
        manager.push(Toast::new("Saved").kind(ToastKind::Success), start);
        manager.push(
            Toast::new("Upload failed")
                .description("Retry when back online.")
                .kind(ToastKind::Error),
            start,
        );
        let viewport = ToastViewport::new("toasts");
        let polite = viewport.toast(&manager.entries()[0]);
        let assertive = viewport.toast(&manager.entries()[1]);

        let polite_root = polite.root_with(div().bg(Color::rgb8(1, 2, 3)));
        assert_eq!(polite_root.accessibility.role, AccessibilityRole::Status);
        assert_eq!(
            polite_root.accessibility.live,
            Some(AccessibilityLive::Polite)
        );
        assert_eq!(
            polite_root.accessibility.relations.labelled_by(),
            Some(polite.title_id())
        );
        assert_eq!(polite_root.accessibility.relations.described_by(), None);
        assert!(polite_root.focusable);
        assert_eq!(polite_root.visual.background, Some(Color::rgb8(1, 2, 3)));

        let assertive_root = assertive.root_with(div());
        assert_eq!(assertive_root.accessibility.role, AccessibilityRole::Alert);
        assert_eq!(
            assertive_root.accessibility.live,
            Some(AccessibilityLive::Assertive)
        );
        assert_eq!(
            assertive_root.accessibility.relations.described_by(),
            Some(assertive.description_id())
        );

        let viewport_element = viewport.viewport_with(div().gap_2());
        assert_eq!(viewport_element.explicit_id, Some("toasts".into()));
        assert_eq!(
            viewport_element.accessibility.role,
            AccessibilityRole::Group
        );
        assert!(viewport_element.accessibility.live.is_none());

        let title = polite.title_with(text("Saved"));
        assert_eq!(title.explicit_id, Some(polite.title_id()));
        let description = assertive.description_with(text("Retry when back online."));
        assert_eq!(description.explicit_id, Some(assertive.description_id()));
        let action = assertive.action_with(div().child("Retry"));
        assert_eq!(action.explicit_id, Some(assertive.action_id()));
        assert!(action.clickable);
        let close = assertive.close_with(div().child("×"));
        assert_eq!(close.explicit_id, Some(assertive.close_id()));
        assert!(close.clickable);

        let ids = [
            viewport.viewport_id(),
            polite.root_id(),
            polite.title_id(),
            polite.description_id(),
            polite.action_id(),
            polite.close_id(),
            assertive.root_id(),
            assertive.title_id(),
        ];
        for (index, id) in ids.iter().enumerate() {
            assert!(!ids[..index].contains(id));
        }
    }

    #[derive(Default)]
    struct ToastView {
        toasts: ToastManager,
        undone: bool,
    }

    impl ToastView {
        fn toasts(view: &mut Self) -> &mut ToastManager {
            &mut view.toasts
        }
    }

    impl View for ToastView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let now = Instant::now();
            let publish = cx.listener("publish", move |view: &mut Self, cx| {
                view.toasts.push(
                    Toast::new("Item deleted")
                        .description("The item moved to the trash.")
                        .action("Undo")
                        .kind(ToastKind::Warning),
                    now,
                );
                cx.invalidate();
            });

            let viewport = ToastViewport::new("toasts");
            let mut surface = viewport.viewport_with(div());
            for entry in self.toasts.entries() {
                let parts = viewport.toast(entry);
                let id = entry.id();
                let undo = cx.listener(parts.action_id(), move |view: &mut Self, cx| {
                    view.undone = true;
                    view.toasts.dismiss(id);
                    cx.invalidate();
                });
                let close = cx.listener(parts.close_id(), move |view: &mut Self, cx| {
                    view.toasts.dismiss(id);
                    cx.invalidate();
                });
                let mut root = div().child(parts.title_with(text(entry.toast().title().clone())));
                if let Some(description) = entry.toast().description_text() {
                    root = root.child(parts.description_with(text(description.clone())));
                }
                if let Some(label) = entry.toast().action_label() {
                    root = root
                        .child(parts.action_with(div().child(text(label.clone())).on_click(undo)));
                }
                root = root.child(parts.close_with(div().child(text("Close")).on_click(close)));
                surface = surface.child(parts.key_with(cx, parts.root_with(root), Self::toasts));
            }

            div()
                .child(button().id("publish").child("Delete").on_click(publish))
                .child(surface)
        }
    }

    #[test]
    fn live_regions_escape_and_idle_paths_are_deterministic() {
        let (mut cx, view) = TestAppContext::new(ToastView::default()).unwrap();
        let window = view.window_handle();
        let viewport = ToastViewport::new("toasts");

        cx.click(window, "publish").unwrap();
        assert_eq!(cx.read(view, |view| view.toasts.len()).unwrap(), 1);
        let first = cx.read(view, |view| view.toasts.entries()[0].id()).unwrap();
        let parts = ToastParts {
            viewport,
            id: first,
            kind: ToastKind::Warning,
            has_description: true,
            index: 0,
            limited: false,
            expanded: false,
            swiping: false,
            swipe: 0.0,
        };

        let update = cx.accessibility_update(window).unwrap();
        let node = |id: ElementId| {
            update
                .nodes
                .iter()
                .find_map(|(node_id, node)| (node_id.0 == id.as_u64()).then_some(node))
                .expect("toast accessibility node")
        };
        let root = node(parts.root_id());
        assert_eq!(root.role(), accesskit::Role::Alert);
        assert_eq!(root.live(), Some(accesskit::Live::Assertive));
        assert_eq!(
            root.labelled_by(),
            &[accesskit::NodeId(parts.title_id().as_u64())]
        );
        assert_eq!(
            root.described_by(),
            &[accesskit::NodeId(parts.description_id().as_u64())]
        );

        cx.click(window, parts.action_id()).unwrap();
        assert!(cx.read(view, |view| view.undone).unwrap());
        assert!(cx.read(view, |view| view.toasts.is_empty()).unwrap());

        cx.click(window, "publish").unwrap();
        let second = cx.read(view, |view| view.toasts.entries()[0].id()).unwrap();
        let second_parts = ToastParts {
            viewport,
            id: second,
            kind: ToastKind::Warning,
            has_description: true,
            index: 0,
            limited: false,
            expanded: false,
            swiping: false,
            swipe: 0.0,
        };
        cx.focus(window, second_parts.root_id()).unwrap();
        cx.simulate_keystrokes(window, "escape").unwrap();
        assert!(cx.read(view, |view| view.toasts.is_empty()).unwrap());

        cx.click(window, "publish").unwrap();
        let third = cx.read(view, |view| view.toasts.entries()[0].id()).unwrap();
        let third_parts = ToastParts {
            viewport,
            id: third,
            kind: ToastKind::Warning,
            has_description: true,
            index: 0,
            limited: false,
            expanded: false,
            swiping: false,
            swipe: 0.0,
        };
        cx.click(window, third_parts.close_id()).unwrap();
        assert!(cx.read(view, |view| view.toasts.is_empty()).unwrap());

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }

    #[test]
    fn shorthand_is_a_semantic_unstyled_root() {
        let element = toast_viewport("toasts");
        assert_eq!(element.accessibility.role, AccessibilityRole::Group);
        assert!(element.children.is_empty());
        assert_eq!(element.visual.background, None);
    }

    fn swipe_event(phase: PointerPhase, dx: f32, dy: f32) -> PointerEvent {
        PointerEvent {
            phase,
            position: crate::Point::new(200.0 + dx, 100.0 + dy),
            origin: crate::Point::new(200.0, 100.0),
            local_position: crate::Point::new(dx, dy),
            local_origin: crate::Point::ZERO,
            delta: crate::Vector::new(dx, dy),
            button: crate::MouseButton::Left,
            modifiers: crate::Modifiers::empty(),
            size: crate::Size::new(280.0, 64.0),
        }
    }

    #[test]
    fn provider_timing_and_limits_are_bounded_and_only_flag_presentation() {
        let manager = ToastManager::new();
        assert_eq!(manager.timeout_value(), DEFAULT_TOAST_TIMEOUT);
        assert_eq!(manager.limit_value(), DEFAULT_TOAST_LIMIT);
        assert_eq!(
            manager.swipe_threshold_value(),
            DEFAULT_TOAST_SWIPE_THRESHOLD
        );
        assert_eq!(manager.swipe_direction_value(), ToastSwipeDirection::Right);
        assert!(!manager.is_expanded());

        let bounded = ToastManager::new()
            .timeout(Duration::from_secs(3_600))
            .limit(999)
            .swipe_threshold(f32::NAN);
        assert_eq!(bounded.timeout_value(), MAX_TOAST_DURATION);
        assert_eq!(bounded.limit_value(), MAX_TOASTS);
        assert_eq!(
            bounded.swipe_threshold_value(),
            DEFAULT_TOAST_SWIPE_THRESHOLD
        );
        assert_eq!(ToastManager::new().limit(0).limit_value(), 1);
        assert_eq!(
            ToastManager::new()
                .swipe_threshold(1.0e9)
                .swipe_threshold_value(),
            MAX_TOAST_SWIPE_THRESHOLD
        );

        let now = Instant::now();
        let mut manager = ToastManager::new().limit(2);
        let first = manager.add(Toast::new("One"), now);
        let second = manager.add(Toast::new("Two"), now);
        let third = manager.add(Toast::new("Three"), now);
        assert!(manager.entry(first).unwrap().is_limited());
        assert!(!manager.entry(second).unwrap().is_limited());
        assert!(!manager.entry(third).unwrap().is_limited());

        // Limiting is presentation only: a limited toast still counts down.
        assert_eq!(
            manager.entry(first).unwrap().deadline(),
            Some(now + Duration::from_secs(5))
        );

        // Removing a newer toast promotes the older one back into the visible set.
        assert!(manager.close(third));
        assert!(!manager.entry(first).unwrap().is_limited());
        assert!(manager.close_all());
        assert!(manager.is_empty());

        let mut expandable = ToastManager::new();
        assert!(expandable.set_expanded(true));
        assert!(!expandable.set_expanded(true));
        assert!(expandable.is_expanded());
        assert!(expandable.set_expanded(false));
    }

    #[test]
    fn promise_toasts_keep_their_identity_across_a_resolution() {
        let now = Instant::now();
        let mut manager = ToastManager::new();
        let id = manager.promise("Uploading…", now);
        let entry = manager.entry(id).expect("queued loading toast");
        assert_eq!(entry.toast().toast_kind(), ToastKind::Loading);
        assert_eq!(entry.deadline(), None);
        assert!(!ToastKind::Loading.is_assertive());

        let later = now + Duration::from_secs(2);
        assert!(
            manager.resolve(
                id,
                Toast::new("Upload complete")
                    .kind(ToastKind::Success)
                    .duration(Duration::from_secs(4)),
                later
            )
        );
        let entry = manager.entry(id).expect("resolved toast");
        // The identity, and therefore the mounted element and its stack position, are unchanged.
        assert_eq!(entry.id(), id);
        assert_eq!(entry.toast().title().as_ref(), "Upload complete");
        assert_eq!(entry.toast().toast_kind(), ToastKind::Success);
        assert_eq!(entry.deadline(), Some(later + Duration::from_secs(4)));
        assert_eq!(manager.len(), 1);

        // Updating a toast that is not queued reports it rather than resurrecting it.
        assert!(!manager.update(ToastId(999), Toast::new("Gone"), later));

        // A resolved failure re-arms exactly the same way.
        assert!(manager.update(
            id,
            Toast::new("Upload failed").kind(ToastKind::Error),
            later
        ));
        assert!(
            manager
                .entry(id)
                .unwrap()
                .toast()
                .toast_kind()
                .is_assertive()
        );
        assert!(manager.expire(later + Duration::from_secs(6)));
        assert!(manager.is_empty());
        assert_eq!(manager.next_deadline(), None);
    }

    #[test]
    fn a_captured_swipe_dismisses_past_the_threshold_and_springs_back_short_of_it() {
        let now = Instant::now();
        let mut manager = ToastManager::new().swipe_threshold(40.0);
        let id = manager.add(Toast::new("Saved"), now);

        // Pressing enters the swiping state, which the application already restyles from.
        assert_eq!(
            manager.apply_swipe(id, &swipe_event(PointerPhase::Down, 0.0, 0.0)),
            ToastSwipeChange {
                changed: true,
                dismissed: false,
            }
        );
        assert!(manager.entry(id).unwrap().is_swiping());

        let moved = manager.apply_swipe(id, &swipe_event(PointerPhase::Move, 12.0, 3.0));
        assert!(moved.changed);
        assert!(!moved.dismissed);
        assert_eq!(manager.entry(id).unwrap().swipe_movement(), 12.0);

        // Motion away from the declared direction is clamped rather than inverting the toast.
        manager.apply_swipe(id, &swipe_event(PointerPhase::Move, -40.0, 0.0));
        assert_eq!(manager.entry(id).unwrap().swipe_movement(), 0.0);

        // Releasing short of the threshold springs back and keeps the toast queued.
        manager.apply_swipe(id, &swipe_event(PointerPhase::Move, 20.0, 0.0));
        let released = manager.apply_swipe(id, &swipe_event(PointerPhase::Up, 0.0, 0.0));
        assert!(released.changed);
        assert!(!released.dismissed);
        assert_eq!(manager.entry(id).unwrap().swipe_movement(), 0.0);
        assert!(!manager.entry(id).unwrap().is_swiping());
        assert_eq!(manager.len(), 1);

        // Releasing past it dismisses.
        manager.apply_swipe(id, &swipe_event(PointerPhase::Down, 0.0, 0.0));
        manager.apply_swipe(id, &swipe_event(PointerPhase::Move, 45.0, 0.0));
        let dismissed = manager.apply_swipe(id, &swipe_event(PointerPhase::Up, 0.0, 0.0));
        assert!(dismissed.dismissed);
        assert!(manager.is_empty());
        assert_eq!(
            manager.apply_swipe(id, &swipe_event(PointerPhase::Move, 10.0, 0.0)),
            ToastSwipeChange::default()
        );

        // A cancelled gesture resets without dismissing, on either axis.
        let mut upward = ToastManager::new()
            .swipe_direction(ToastSwipeDirection::Up)
            .swipe_threshold(30.0);
        let id = upward.add(Toast::new("Saved"), now);
        upward.apply_swipe(id, &swipe_event(PointerPhase::Down, 0.0, 0.0));
        upward.apply_swipe(id, &swipe_event(PointerPhase::Move, 0.0, -20.0));
        assert_eq!(upward.entry(id).unwrap().swipe_movement(), 20.0);
        let cancelled = upward.apply_swipe(id, &swipe_event(PointerPhase::Cancel, 0.0, 0.0));
        assert!(cancelled.changed);
        assert!(!cancelled.dismissed);
        assert_eq!(upward.entry(id).unwrap().swipe_movement(), 0.0);
        assert_eq!(upward.len(), 1);
        assert!(ToastSwipeDirection::Right.is_horizontal());
        assert!(!ToastSwipeDirection::Up.is_horizontal());
    }

    #[test]
    fn stack_descriptors_carry_index_limit_and_expansion_without_appearance() {
        let now = Instant::now();
        let mut manager = ToastManager::new().limit(2);
        manager.add(Toast::new("One"), now);
        manager.add(Toast::new("Two"), now);
        manager.add(Toast::new("Three").description("Third"), now);
        manager.set_expanded(true);

        let viewport = ToastViewport::new("toasts");
        let stack = viewport.toasts(&manager).collect::<Vec<_>>();
        assert_eq!(stack.len(), 3);
        // Newest first, which is the order an application stacks them in.
        assert_eq!(stack[0].index(), 0);
        assert_eq!(stack[2].index(), 2);
        assert!(!stack[0].is_limited());
        assert!(stack[2].is_limited());
        assert!(stack.iter().all(|toast| toast.is_expanded()));
        assert_eq!(stack[0].offset(12.0), 0.0);
        assert_eq!(stack[2].offset(12.0), 24.0);
        assert_eq!(stack[2].offset(f32::NAN), 0.0);

        let toast = stack[0];
        let ids = [
            toast.root_id(),
            toast.positioner_id(),
            toast.content_id(),
            toast.title_id(),
            toast.description_id(),
            toast.action_id(),
            toast.close_id(),
            viewport.portal_id(),
        ];
        for (index, id) in ids.iter().enumerate() {
            assert_ne!(*id, viewport.viewport_id());
            assert!(!ids[..index].contains(id));
        }

        let positioner = toast.positioner_with(div().bg(Color::rgb8(1, 2, 3)));
        assert_eq!(positioner.explicit_id, Some(toast.positioner_id()));
        assert_eq!(positioner.visual.background, Some(Color::rgb8(1, 2, 3)));
        assert!(!positioner.accessibility.hidden);
        let content = toast.content_with(div());
        assert_eq!(content.explicit_id, Some(toast.content_id()));

        // The toast layer floats over the window without swallowing its clicks.
        let portal = viewport.portal_with(div());
        assert_eq!(portal.explicit_id, Some(viewport.portal_id()));
        assert!(portal.portal);
        assert!(!portal.blocks_pointer);
        assert_eq!(portal.visual.background, None);

        // A single entry still describes itself, defaulting to the top of the stack.
        let single = viewport.toast(manager.entries().last().expect("newest toast"));
        assert_eq!(single.index(), 0);
        assert!(!single.is_expanded());
        assert_eq!(single.root_id(), toast.root_id());
    }

    struct ToastStackView {
        toasts: ToastManager,
    }

    impl Default for ToastStackView {
        fn default() -> Self {
            Self {
                toasts: ToastManager::new().limit(2).swipe_threshold(40.0),
            }
        }
    }

    impl View for ToastStackView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let now = Instant::now();
            let publish = cx.listener("publish", move |view: &mut Self, cx| {
                let index = view.toasts.len() + 1;
                view.toasts.add(Toast::new(format!("Toast {index}")), now);
                cx.invalidate();
            });
            let expand = cx.hover_listener(
                ToastViewport::new("toasts").viewport_id(),
                |view: &mut Self, hovered, cx| {
                    if view.toasts.set_expanded(*hovered) {
                        cx.invalidate();
                    }
                },
            );

            let viewport = ToastViewport::new("toasts");
            let mut stack = viewport
                .viewport_with(div().on_hover(expand))
                .relative()
                .w(280.0)
                .h(200.0);
            for parts in viewport.toasts(&self.toasts).collect::<Vec<_>>() {
                let id = parts.id();
                let swipe =
                    cx.pointer_listener(parts.root_id(), move |view: &mut Self, event, cx| {
                        if view.toasts.apply_swipe(id, event).changed {
                            cx.invalidate();
                        }
                    });
                let pitch = if parts.is_expanded() { 68.0 } else { 12.0 };
                let root = parts
                    .root_with(div().w(260.0).h(60.0).on_pointer(swipe))
                    .child(parts.content_with(div().child(parts.title_with(text("Toast")))));
                stack = stack.child(
                    parts.positioner_with(
                        div()
                            .absolute()
                            .left(parts.swipe_movement())
                            .top(parts.offset(pitch))
                            .opacity(if parts.is_limited() { 0.4 } else { 1.0 })
                            .child(root),
                    ),
                );
            }

            div()
                .child(button().id("publish").child("Publish").on_click(publish))
                .child(viewport.portal_with(div()).child(stack))
        }
    }

    #[test]
    fn a_stacked_viewport_mounts_every_part_and_swipes_without_idle_work() {
        let (mut cx, view) = TestAppContext::new(ToastStackView::default()).unwrap();
        let window = view.window_handle();
        let viewport = ToastViewport::new("toasts");

        assert!(cx.contains_element(window, viewport.portal_id()).unwrap());
        cx.click(window, "publish").unwrap();
        cx.click(window, "publish").unwrap();
        cx.click(window, "publish").unwrap();
        assert_eq!(cx.read(view, |view| view.toasts.len()).unwrap(), 3);

        let stack = cx
            .read(view, |view| {
                viewport
                    .toasts(&view.toasts)
                    .map(|parts| (parts.id(), parts.index(), parts.is_limited()))
                    .collect::<Vec<_>>()
            })
            .unwrap();
        assert_eq!(stack.len(), 3);
        assert_eq!(stack[0].1, 0);
        assert!(!stack[0].2);
        assert!(stack[2].2);
        for (id, _, _) in &stack {
            let parts = cx
                .read(view, |view| {
                    viewport
                        .toasts(&view.toasts)
                        .find(|parts| parts.id() == *id)
                        .expect("declared toast")
                })
                .unwrap();
            assert!(cx.contains_element(window, parts.positioner_id()).unwrap());
            assert!(cx.contains_element(window, parts.content_id()).unwrap());
            assert!(cx.contains_element(window, parts.root_id()).unwrap());
        }

        // Collapsed and expanded stacks lay out at different pitches.
        let newest = stack[0].0;
        let older = stack[1].0;
        let collapsed = cx
            .read(view, |view| {
                viewport
                    .toasts(&view.toasts)
                    .find(|parts| parts.id() == older)
                    .expect("older toast")
                    .offset(12.0)
            })
            .unwrap();
        assert_eq!(collapsed, 12.0);
        cx.update(view, |view, cx| {
            view.toasts.set_expanded(true);
            cx.invalidate();
        })
        .unwrap();
        assert!(cx.read(view, |view| view.toasts.is_expanded()).unwrap());

        // A released swipe past the threshold removes exactly the toast it moved.
        cx.update(view, |view, cx| {
            view.toasts
                .apply_swipe(newest, &swipe_event(PointerPhase::Down, 0.0, 0.0));
            view.toasts
                .apply_swipe(newest, &swipe_event(PointerPhase::Move, 50.0, 0.0));
            cx.invalidate();
        })
        .unwrap();
        assert_eq!(
            cx.read(view, |view| view
                .toasts
                .entry(newest)
                .unwrap()
                .swipe_movement())
                .unwrap(),
            50.0
        );
        cx.update(view, |view, cx| {
            assert!(
                view.toasts
                    .apply_swipe(newest, &swipe_event(PointerPhase::Up, 0.0, 0.0))
                    .dismissed
            );
            cx.invalidate();
        })
        .unwrap();
        assert_eq!(cx.read(view, |view| view.toasts.len()).unwrap(), 2);
        // Dropping the newest toast promotes the limited one back into the visible set.
        assert!(
            cx.read(view, |view| view
                .toasts
                .entries()
                .iter()
                .all(|entry| !entry.is_limited()))
                .unwrap()
        );

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }
}
