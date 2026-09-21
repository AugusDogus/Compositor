use super::*;

/// Maximum actions one callback may target at another retained window.
pub const MAX_TARGETED_ACTIONS_PER_EVENT: usize = 256;
/// Maximum cross-window actions retained across one application effect cycle.
pub const MAX_PENDING_TARGETED_ACTIONS: usize = 1_024;
/// Maximum native popup menus one event callback may request.
pub const MAX_NATIVE_POPUP_MENUS_PER_EVENT: usize = 4;

/// Framework-level input and window events, expressed in logical pixels.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The native window asked to close. Call [`EventContext::prevent_close`] to keep it open.
    CloseRequested,
    PointerMoved(Point),
    PointerLeft,
    MouseButton {
        button: MouseButton,
        pressed: bool,
    },
    Click(crate::ElementId),
    /// A secondary-click gesture targeted an element with a context-menu listener.
    ContextMenu(ContextMenuEvent),
    /// A top-level surface was dismissed by Escape or an outside pointer press.
    Dismiss(crate::ElementId),
    /// A coalesced scroll delta. Multiple platform wheel events may become one event.
    Scroll(Vector),
    /// Force-sensitive pointer pressure targeted at the current logical pointer position.
    MousePressure(MousePressureEvent),
    /// A native pinch-to-zoom gesture.
    Pinch(PinchEvent),
    /// A native two-finger rotation gesture.
    Rotation(RotationEvent),
    /// A native smart-magnify gesture, normally a two-finger double tap on macOS.
    SmartMagnify(SmartMagnifyEvent),
    /// One raw direct-touch contact in window coordinates.
    ///
    /// Indirect devices such as macOS trackpads are exposed through scroll and gesture events
    /// because their contacts have no corresponding position in the window.
    Touch(TouchEvent),
    KeyDown {
        /// Normalized command identity, independent from Caps Lock and text composition.
        key: Key,
        /// Normalized printable character this press could produce before IME composition.
        key_char: Option<Key>,
        modifiers: Modifiers,
        repeat: bool,
    },
    KeyUp {
        key: Key,
        key_char: Option<Key>,
        modifiers: Modifiers,
    },
    /// Committed text from the platform input method.
    TextInput(String),
    ModifiersChanged(Modifiers),
    /// The focused element changed within the window.
    FocusChanged(Option<ElementId>),
    /// The native window itself gained or lost focus.
    Focused(bool),
    /// The effective native light/dark appearance changed while following the system.
    AppearanceChanged(WindowAppearance),
    /// The native window moved in logical desktop coordinates.
    Moved {
        logical_position: Point,
        scale_factor: f32,
    },
    /// Files from another application are currently hovering this window.
    FilesHovered(DroppedFiles),
    /// A native file drag left the window without being dropped.
    FilesHoverCancelled,
    /// Files from another application were dropped into this window.
    FilesDropped(DroppedFiles),
    /// A drag promoted from this window to the native platform has ended.
    ExternalDragEnded(ExternalDragEndEvent),
    Resized {
        logical_size: Size,
        scale_factor: f32,
    },
    /// The native window was minimized (`true`) or restored from the dock/taskbar (`false`).
    Minimized(bool),
    /// The native window entered (`true`) or left (`false`) the platform's maximized/zoomed state.
    Maximized(bool),
    /// The native window entered (`true`) or left (`false`) fullscreen.
    FullscreenChanged(bool),
    /// Delivered exactly once, after this window's first frame reached the screen.
    ///
    /// A window created with `WindowOptions::show(false)` can safely be shown here, which is the
    /// flicker-free "ready to show" moment.
    FirstPresented,
    /// The compositor started (`true`) or stopped (`false`) hiding this window's contents.
    OcclusionChanged(bool),
    /// The effective native stacking level changed.
    WindowLevelChanged(WindowLevel),
    /// The window manager resized this window. Call
    /// [`EventContext::constrain_resize`](crate::EventContext::constrain_resize) to request a
    /// narrower inner size before the next frame is laid out.
    WillResize {
        proposed_size: Size,
    },
    /// The window manager moved this window. Call
    /// [`EventContext::constrain_move`](crate::EventContext::constrain_move) to request a
    /// different logical desktop position.
    WillMove {
        proposed_position: Point,
    },
}

/// Maximum controlled text fields exposed by one form submission.
pub const MAX_FORM_FIELDS: usize = 256;
/// Maximum invalid controls retained by one validation report.
pub const MAX_VALIDATION_ISSUES: usize = 256;
/// Maximum UTF-8 bytes retained for one declarative validation message.
pub const MAX_VALIDATION_MESSAGE_BYTES: usize = 4 * 1024;
/// Maximum programmatic form submissions queued by one event callback.
pub const MAX_FORM_SUBMISSIONS_PER_EVENT: usize = 64;

/// One controlled text value captured for a valid form submission.
///
/// Values stay shared with the retained text input, so submitting a large text area does not copy
/// its contents. Fields are emitted in document order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FormField {
    id: ElementId,
    value: Arc<str>,
}

impl FormField {
    pub(crate) fn new(id: ElementId, value: Arc<str>) -> Self {
        Self { id, value }
    }

    pub const fn id(&self) -> ElementId {
        self.id
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

/// Data delivered after every enabled control in a form is valid.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FormSubmitEvent {
    form: ElementId,
    trigger: Option<ElementId>,
    fields: Arc<[FormField]>,
    truncated: bool,
}

impl FormSubmitEvent {
    pub(crate) fn new(
        form: ElementId,
        trigger: Option<ElementId>,
        fields: Vec<FormField>,
        truncated: bool,
    ) -> Self {
        debug_assert!(fields.len() <= MAX_FORM_FIELDS);
        Self {
            form,
            trigger,
            fields: Arc::from(fields),
            truncated,
        }
    }

    pub const fn form(&self) -> ElementId {
        self.form
    }

    /// The input, submit button, or custom action that initiated submission.
    pub const fn trigger(&self) -> Option<ElementId> {
        self.trigger
    }

    pub fn fields(&self) -> &[FormField] {
        &self.fields
    }

    pub fn value(&self, id: impl Into<ElementId>) -> Option<&str> {
        let id = id.into();
        self.fields
            .iter()
            .find(|field| field.id == id)
            .map(FormField::value)
    }

    /// Whether fields beyond [`MAX_FORM_FIELDS`] were omitted.
    pub const fn is_truncated(&self) -> bool {
        self.truncated
    }
}

/// One enabled control that prevented a form submission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidationIssue {
    id: ElementId,
    message: Option<Arc<str>>,
    message_truncated: bool,
}

impl ValidationIssue {
    pub(crate) fn new(id: ElementId, message: Option<Arc<str>>, message_truncated: bool) -> Self {
        Self {
            id,
            message,
            message_truncated,
        }
    }

    pub const fn id(&self) -> ElementId {
        self.id
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// Whether the original message exceeded [`MAX_VALIDATION_MESSAGE_BYTES`].
    pub const fn is_message_truncated(&self) -> bool {
        self.message_truncated
    }
}

/// Bounded, document-ordered details for a form that could not be submitted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidationReport {
    form: ElementId,
    trigger: Option<ElementId>,
    issues: Arc<[ValidationIssue]>,
    truncated: bool,
}

impl ValidationReport {
    pub(crate) fn new(
        form: ElementId,
        trigger: Option<ElementId>,
        issues: Vec<ValidationIssue>,
        truncated: bool,
    ) -> Self {
        debug_assert!(issues.len() <= MAX_VALIDATION_ISSUES);
        Self {
            form,
            trigger,
            issues: Arc::from(issues),
            truncated,
        }
    }

    pub const fn form(&self) -> ElementId {
        self.form
    }

    pub const fn trigger(&self) -> Option<ElementId> {
        self.trigger
    }

    pub fn issues(&self) -> &[ValidationIssue] {
        &self.issues
    }

    pub fn first(&self) -> Option<&ValidationIssue> {
        self.issues.first()
    }

    /// Whether invalid controls beyond [`MAX_VALIDATION_ISSUES`] were omitted.
    pub const fn is_truncated(&self) -> bool {
        self.truncated
    }
}

/// Maximum path count retained for one native file drag.
pub const MAX_DROPPED_FILES: usize = 4_096;

/// Maximum file count retained for one outbound native drag.
pub const MAX_EXTERNAL_DRAG_FILES: usize = 4_096;
/// Maximum encoded bytes retained for one outbound native-drag path.
pub const MAX_EXTERNAL_DRAG_PATH_BYTES: usize = 16 * 1024;
/// Maximum encoded path bytes retained across one outbound native drag.
pub const MAX_EXTERNAL_DRAG_TOTAL_PATH_BYTES: usize = 8 * 1024 * 1024;
/// Maximum UTF-8 storage retained by one inbound or outbound native text drag.
pub const MAX_EXTERNAL_DRAG_TEXT_BYTES: usize = 1024 * 1024;
/// Maximum UTF-8 storage retained by one inbound or outbound native URL drag.
pub const MAX_EXTERNAL_DRAG_URL_BYTES: usize = 16 * 1024;

/// A bounded, shared collection of paths delivered by a native file drag.
///
/// Winit reports one path at a time. QuickGUI retains the paths observed for the current native
/// drag and delivers one collection when the platform submits it. The same value can be accepted
/// by a typed [`crate::DropListener`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DroppedFiles {
    paths: Arc<[PathBuf]>,
    truncated: bool,
}

impl DroppedFiles {
    pub fn new(paths: impl IntoIterator<Item = PathBuf>) -> Self {
        let mut paths = paths.into_iter();
        let retained = paths.by_ref().take(MAX_DROPPED_FILES).collect::<Vec<_>>();
        Self {
            paths: Arc::from(retained),
            truncated: paths.next().is_some(),
        }
    }

    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// Whether paths beyond [`MAX_DROPPED_FILES`] were discarded.
    pub fn is_truncated(&self) -> bool {
        self.truncated
    }

    pub(crate) fn from_retained(paths: Vec<PathBuf>, truncated: bool) -> Self {
        debug_assert!(paths.len() <= MAX_DROPPED_FILES);
        Self {
            paths: Arc::from(paths),
            truncated,
        }
    }
}

/// Files offered to the platform when an internal drag leaves its QuickGUI window.
///
/// Directory metadata is supplied by the application so starting a native drag never performs a
/// synchronous filesystem query. Construction examines at most
/// [`MAX_EXTERNAL_DRAG_FILES`] plus one entry and retains bounded path storage.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FileDragPaths {
    entries: Arc<[(PathBuf, bool)]>,
    truncated: bool,
}

impl FileDragPaths {
    /// Build a bounded outbound file payload from `(path, is_directory)` pairs.
    pub fn new(entries: impl IntoIterator<Item = (PathBuf, bool)>) -> Self {
        let mut entries = entries.into_iter();
        let mut retained = Vec::with_capacity(entries.size_hint().0.min(MAX_EXTERNAL_DRAG_FILES));
        let mut retained_bytes = 0_usize;
        let mut truncated = false;

        for (path, is_directory) in entries.by_ref().take(MAX_EXTERNAL_DRAG_FILES) {
            let path_bytes = path.as_os_str().as_encoded_bytes().len();
            let next_bytes = retained_bytes.saturating_add(path_bytes);
            if path_bytes == 0
                || path_bytes > MAX_EXTERNAL_DRAG_PATH_BYTES
                || next_bytes > MAX_EXTERNAL_DRAG_TOTAL_PATH_BYTES
            {
                truncated = true;
                continue;
            }
            retained_bytes = next_bytes;
            retained.push((path, is_directory));
        }
        truncated |= entries.next().is_some();

        Self {
            entries: Arc::from(retained),
            truncated,
        }
    }

    /// Build a bounded payload containing only files.
    pub fn files(paths: impl IntoIterator<Item = PathBuf>) -> Self {
        Self::new(paths.into_iter().map(|path| (path, false)))
    }

    /// Build a bounded payload containing only directories.
    pub fn directories(paths: impl IntoIterator<Item = PathBuf>) -> Self {
        Self::new(paths.into_iter().map(|path| (path, true)))
    }

    /// The retained paths paired with whether each one is a directory.
    pub fn entries(&self) -> &[(PathBuf, bool)] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether an entry was omitted by a count or byte bound.
    pub fn is_truncated(&self) -> bool {
        self.truncated
    }
}

/// Bounded plain text exchanged with a native drag destination.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExternalDragText {
    pub(super) text: Arc<str>,
    truncated: bool,
}

impl ExternalDragText {
    pub fn new(text: impl Into<Arc<str>>) -> Self {
        let text = text.into();
        if text.len() <= MAX_EXTERNAL_DRAG_TEXT_BYTES {
            return Self {
                text,
                truncated: false,
            };
        }

        let mut end = MAX_EXTERNAL_DRAG_TEXT_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        Self {
            text: Arc::from(&text[..end]),
            truncated: true,
        }
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Whether the UTF-8 suffix beyond [`MAX_EXTERNAL_DRAG_TEXT_BYTES`] was discarded.
    pub fn is_truncated(&self) -> bool {
        self.truncated
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn from_bounded(text: String, truncated: bool) -> Self {
        debug_assert!(text.len() <= MAX_EXTERNAL_DRAG_TEXT_BYTES);
        Self {
            text: Arc::from(text),
            truncated,
        }
    }
}

impl From<&str> for ExternalDragText {
    fn from(text: &str) -> Self {
        Self::new(text)
    }
}

impl From<String> for ExternalDragText {
    fn from(text: String) -> Self {
        Self::new(text)
    }
}

impl From<Arc<str>> for ExternalDragText {
    fn from(text: Arc<str>) -> Self {
        Self::new(text)
    }
}

/// A bounded absolute URL exchanged with a native drag destination.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalDragUrl(pub(super) Arc<str>);

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ExternalDragUrlError {
    #[error("external drag URL is empty")]
    Empty,
    #[error(
        "external drag URL is {bytes} bytes; the maximum is {MAX_EXTERNAL_DRAG_URL_BYTES} bytes"
    )]
    TooLarge { bytes: usize },
    #[error("external drag URL must have an absolute RFC 3986 scheme")]
    InvalidScheme,
    #[error("external drag URL contains whitespace or control characters")]
    InvalidCharacter,
}

impl ExternalDragUrl {
    pub fn new(url: impl Into<Arc<str>>) -> Result<Self, ExternalDragUrlError> {
        let url = url.into();
        if url.is_empty() {
            return Err(ExternalDragUrlError::Empty);
        }
        if url.len() > MAX_EXTERNAL_DRAG_URL_BYTES {
            return Err(ExternalDragUrlError::TooLarge { bytes: url.len() });
        }
        if url
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
        {
            return Err(ExternalDragUrlError::InvalidCharacter);
        }
        let Some((scheme, remainder)) = url.split_once(':') else {
            return Err(ExternalDragUrlError::InvalidScheme);
        };
        let mut characters = scheme.chars();
        if remainder.is_empty()
            || !characters
                .next()
                .is_some_and(|character| character.is_ascii_alphabetic())
            || !characters.all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
            })
        {
            return Err(ExternalDragUrlError::InvalidScheme);
        }
        Ok(Self(url))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Plain text delivered by an inbound native drag.
///
/// This is the same exact typed value as [`ExternalDragText`], allowing one drop listener to
/// accept a drag originating in QuickGUI or another application.
pub type DroppedText = ExternalDragText;

/// An absolute URL delivered by an inbound native drag.
///
/// This is the same exact typed value as [`ExternalDragUrl`].
pub type DroppedUrl = ExternalDragUrl;

/// Data offered when an internal drag is promoted to a native platform drag.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExternalDragPayload {
    /// Existing on-disk files or directories.
    Files(FileDragPaths),
    /// Plain UTF-8 text.
    Text(ExternalDragText),
    /// One absolute URL.
    Url(ExternalDragUrl),
}

/// The operation chosen by the native drag destination.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalDragOperation {
    Cancelled,
    Copied,
    Moved,
    Linked,
    Deleted,
    Other,
}

/// Completion metadata for [`Event::ExternalDragEnded`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExternalDragEndEvent {
    /// The stable element that originated the promoted drag.
    pub source: ElementId,
    pub operation: ExternalDragOperation,
}

/// Input geometry supplied when an internal drag crosses the movement threshold.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DragStartEvent {
    /// Position where the primary button was pressed.
    pub origin: Point,
    /// Position that crossed the drag threshold.
    pub position: Point,
    pub modifiers: Modifiers,
}

/// Identifies whether a typed drop originated in this window, another QuickGUI window, or another
/// application.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DragOrigin {
    /// The payload never left this QuickGUI window's retained drag path.
    Internal(ElementId),
    /// A process-local typed payload crossed between independently retained QuickGUI windows.
    CrossWindow {
        window: WindowHandle,
        source: ElementId,
    },
    /// A payload originated outside this QuickGUI application.
    External,
}

/// Geometry supplied to a typed drop listener.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DropEvent {
    pub position: Point,
    pub modifiers: Modifiers,
    pub origin: DragOrigin,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum MouseButton {
    #[default]
    Left,
    Right,
    Middle,
    Back,
    Forward,
    Other(u16),
}

/// One of the two ordered stages used for targeted desktop mouse dispatch.
///
/// Capture listeners run from the root toward the hit-tested target. Bubble listeners then run
/// from that target back toward the root. Stopping propagation ends the remainder of both stages
/// without changing the framework's default focus, selection, drag, or click behavior.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum DispatchPhase {
    #[default]
    Bubble,
    Capture,
}

impl DispatchPhase {
    pub const fn bubble(self) -> bool {
        matches!(self, Self::Bubble)
    }

    pub const fn capture(self) -> bool {
        matches!(self, Self::Capture)
    }
}

/// A desktop mouse-button press targeted through the retained element tree.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MouseDownEvent {
    pub button: MouseButton,
    pub position: Point,
    pub modifiers: Modifiers,
    /// Native multi-click count. The first press is `1`.
    pub click_count: usize,
    /// Whether this press activated an otherwise unfocused native window.
    pub first_mouse: bool,
}

impl MouseDownEvent {
    pub const fn is_focusing(self) -> bool {
        matches!(self.button, MouseButton::Left)
    }
}

/// A desktop mouse-button release targeted through the retained element tree.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MouseUpEvent {
    pub button: MouseButton,
    pub position: Point,
    pub modifiers: Modifiers,
    /// Count shared with the matching [`MouseDownEvent`].
    pub click_count: usize,
}

/// Mouse motion targeted through the retained element tree.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MouseMoveEvent {
    pub position: Point,
    pub pressed_button: Option<MouseButton>,
    pub modifiers: Modifiers,
}

impl MouseMoveEvent {
    pub const fn dragging(self) -> bool {
        self.pressed_button.is_some()
    }

    pub fn dragging_button(self, button: MouseButton) -> bool {
        matches!(self.pressed_button, Some(pressed) if pressed == button)
    }
}

/// Mouse motion delivered when the pointer leaves the native window.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MouseExitEvent {
    pub position: Point,
    pub pressed_button: Option<MouseButton>,
    pub modifiers: Modifiers,
}

/// The stage of a captured pointer interaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PointerPhase {
    Down,
    Move,
    Up,
    /// The window lost focus before the pressed button was released.
    Cancel,
}

/// A pointer event delivered to an element that owns pointer capture.
///
/// Positions and deltas use logical pixels. [`Self::position`] and [`Self::origin`] are relative
/// to the native window; [`Self::local_position`] and [`Self::local_origin`] are relative to the
/// captured element. Once an element receives [`PointerPhase::Down`], it continues to receive move
/// events and the terminal up or cancel event even when the pointer is outside its bounds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerEvent {
    pub phase: PointerPhase,
    /// Current pointer position in logical window coordinates.
    pub position: Point,
    /// Position at which this capture started.
    pub origin: Point,
    /// Current pointer position relative to the captured element's top-left corner.
    ///
    /// The value can be outside the element while pointer capture is active.
    pub local_position: Point,
    /// Capture origin relative to the captured element's current top-left corner.
    pub local_origin: Point,
    /// Motion since the preceding captured event.
    pub delta: Vector,
    pub button: MouseButton,
    pub modifiers: Modifiers,
    /// The captured element's laid-out size in logical pixels.
    ///
    /// Pointer arithmetic for a slider, splitter, or custom drag surface needs the element's own
    /// extent, and the layout that produced it lives in the core. Delivering it with the event
    /// keeps that geometry exact for a hosted binding instead of forcing it to re-derive a size
    /// the core already knows. It is `Size::ZERO` before the event is localized to an element.
    pub size: Size,
}

impl PointerEvent {
    pub(crate) fn localize(mut self, bounds: Rect) -> Self {
        self.local_position = Point::new(self.position.x - bounds.x, self.position.y - bounds.y);
        self.local_origin = Point::new(self.origin.x - bounds.x, self.origin.y - bounds.y);
        self.size = Size::new(bounds.width, bounds.height);
        self
    }
}

/// Maximum absolute platform pixel delta retained from one scroll-wheel event.
///
/// Real trackpad deltas are many orders of magnitude smaller. The bound keeps malformed native
/// input finite before application zoom or pan arithmetic sees it.
pub const MAX_SCROLL_PIXELS_PER_EVENT: f32 = 1_048_576.0;

/// Maximum absolute platform line delta retained from one scroll-wheel event.
pub const MAX_SCROLL_LINES_PER_EVENT: f32 = 4_096.0;

/// Native scroll-wheel movement before conversion into an application-selected line height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScrollDelta {
    /// Exact logical-pixel movement from a trackpad or precise wheel.
    Pixels(Vector),
    /// Device line units from a discrete wheel.
    Lines(Vector),
}

impl Default for ScrollDelta {
    fn default() -> Self {
        Self::Lines(Vector::ZERO)
    }
}

impl ScrollDelta {
    /// Whether this delta came from a precise pixel-scrolling device.
    pub fn precise(&self) -> bool {
        matches!(self, Self::Pixels(_))
    }

    /// Convert this delta to finite logical pixels using `line_height` for discrete wheels.
    pub fn pixel_delta(&self, line_height: f32) -> Vector {
        match *self {
            Self::Pixels(delta) => bounded_scroll_vector(delta, MAX_SCROLL_PIXELS_PER_EVENT),
            Self::Lines(delta) => {
                let line_height = if line_height.is_finite() {
                    line_height.clamp(0.0, MAX_SCROLL_PIXELS_PER_EVENT)
                } else {
                    0.0
                };
                bounded_scroll_vector(
                    Vector::new(delta.x * line_height, delta.y * line_height),
                    MAX_SCROLL_PIXELS_PER_EVENT,
                )
            }
        }
    }

    pub(crate) fn bounded(self) -> Self {
        match self {
            Self::Pixels(delta) => {
                Self::Pixels(bounded_scroll_vector(delta, MAX_SCROLL_PIXELS_PER_EVENT))
            }
            Self::Lines(delta) => {
                Self::Lines(bounded_scroll_vector(delta, MAX_SCROLL_LINES_PER_EVENT))
            }
        }
    }
}

/// A scroll-wheel event delivered through the topmost element's ancestor path.
///
/// Call [`EventContext::prevent_default`] to suppress retained scrolling and
/// [`EventContext::stop_propagation`] to keep the event from reaching a listening ancestor.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollWheelEvent {
    pub position: Point,
    pub delta: ScrollDelta,
    pub phase: GesturePhase,
    pub modifiers: Modifiers,
}

impl ScrollWheelEvent {
    pub(crate) fn bounded(mut self) -> Self {
        self.delta = self.delta.bounded();
        self
    }
}

fn bounded_scroll_vector(delta: Vector, limit: f32) -> Vector {
    let component = |value: f32| {
        if value.is_finite() {
            value.clamp(-limit, limit)
        } else {
            0.0
        }
    };
    Vector::new(component(delta.x), component(delta.y))
}

/// Maximum absolute magnification retained from one native pinch event.
///
/// Native deltas are normally small fractions. Bounding malformed platform input keeps
/// application zoom arithmetic finite without changing ordinary gestures.
pub const MAX_PINCH_DELTA_PER_EVENT: f32 = 8.0;

/// Maximum absolute rotation retained from one native gesture event, in degrees.
pub const MAX_ROTATION_DEGREES_PER_EVENT: f32 = 360.0;

/// The lifecycle phase of a native continuous gesture.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum GesturePhase {
    Started,
    #[default]
    Moved,
    Ended,
    Cancelled,
}

/// Maximum simultaneously captured touch contacts retained by one window.
pub const MAX_ACTIVE_TOUCHES_PER_WINDOW: usize = 32;

/// Maximum absolute logical coordinate accepted from a native touch sample.
pub const MAX_TOUCH_COORDINATE: f32 = 16_777_216.0;

/// Opaque identity for one touch from start through end or cancellation.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TouchId(pub u64);

/// The lifecycle phase of one raw touch contact.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum TouchPhase {
    Started,
    #[default]
    Moved,
    Ended,
    Cancelled,
}

/// One fixed-size direct-touch sample in logical top-left window coordinates.
///
/// QuickGUI hit-tests a contact only at [`TouchPhase::Started`] and captures the nearest listening
/// element for the rest of that contact. This is distinct from mouse pointer capture and supports
/// multiple simultaneous touch IDs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TouchEvent {
    pub id: TouchId,
    pub phase: TouchPhase,
    pub position: Point,
    /// Normalized pressure in `0.0..=1.0` when reported by the platform.
    pub force: Option<f32>,
}

impl TouchEvent {
    pub(crate) fn bounded(mut self) -> Self {
        let coordinate = |value: f32| {
            if value.is_finite() {
                value.clamp(-MAX_TOUCH_COORDINATE, MAX_TOUCH_COORDINATE)
            } else {
                0.0
            }
        };
        self.position = Point::new(coordinate(self.position.x), coordinate(self.position.y));
        self.force = self.force.map(|force| {
            if force.is_finite() {
                force.clamp(0.0, 1.0)
            } else {
                0.0
            }
        });
        self
    }
}

/// The click level reported by a force-sensitive pointing device.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum PressureStage {
    #[default]
    Zero,
    Normal,
    Force,
    /// A future platform stage that QuickGUI does not assign a semantic name yet.
    Other(i64),
}

/// Force-sensitive pointer input, currently produced by macOS Force Touch trackpads.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MousePressureEvent {
    /// Logical top-left window coordinate of the pressure event.
    pub position: Point,
    /// Pressure within the current stage, normalized to `0.0..=1.0`.
    pub pressure: f32,
    pub stage: PressureStage,
    pub modifiers: Modifiers,
}

/// A native pinch-to-zoom event targeted at the pointer's logical window position.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PinchEvent {
    pub position: Point,
    /// Positive values magnify and negative values shrink. `0.1` represents a 10% increment.
    pub delta: f32,
    pub phase: GesturePhase,
    pub modifiers: Modifiers,
}

/// A native two-finger rotation event.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RotationEvent {
    pub position: Point,
    /// Incremental rotation in degrees. Positive values rotate counterclockwise.
    pub delta: f32,
    pub phase: GesturePhase,
    pub modifiers: Modifiers,
}

/// A native smart-magnify request, normally a two-finger double tap on macOS.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SmartMagnifyEvent {
    pub position: Point,
    pub modifiers: Modifiers,
}

/// Web-style `contextmenu` event delivered on secondary-button press.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContextMenuEvent {
    pub target: ElementId,
    pub position: Point,
    pub modifiers: Modifiers,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Key {
    Character(String),
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    PageUp,
    PageDown,
    Home,
    End,
    Enter,
    Escape,
    Space,
    Tab,
    Backspace,
    Delete,
    Insert,
    Function(u8),
    Other,
}

impl Key {
    /// Whether pressing this key reveals focus even when it moves nothing.
    ///
    /// Tab and the arrows are the keys a keyboard user presses to find where focus is, so a press
    /// paints the focus styles of the element that already has focus, like CSS `:focus-visible`.
    pub(crate) fn reveals_focus(&self) -> bool {
        matches!(
            self,
            Key::Tab | Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight
        )
    }
}

/// One normalized key press delivered through the focused element path.
///
/// Key listeners run after keymap actions have had a chance to consume the keystroke and before
/// QuickGUI applies text-editing, focus-traversal, activation, or dismissal defaults.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyDownEvent {
    pub key: Key,
    /// Printable character the physical press could produce before IME composition.
    pub key_char: Option<Key>,
    /// Composed UTF-8 text reported by the platform for this press.
    ///
    /// This can differ from `key` for input methods and synthesized Unicode input. It is absent
    /// for navigation keys and may contain more than one Unicode scalar value.
    pub text: Option<String>,
    pub modifiers: Modifiers,
    pub repeat: bool,
}

/// One normalized key release delivered through the focused element path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyUpEvent {
    pub key: Key,
    /// Printable character the physical key could produce before IME composition.
    pub key_char: Option<Key>,
    pub modifiers: Modifiers,
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
    pub struct Modifiers: u8 {
        const SHIFT = 1 << 0;
        const CONTROL = 1 << 1;
        const ALT = 1 << 2;
        const SUPER = 1 << 3;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A captured pointer event carries the element's own laid-out extent.
    ///
    /// Slider and splitter arithmetic needs the size layout already decided; deriving it again
    /// from a declared width would drift the moment a flexible track resolves differently.
    #[test]
    fn localizing_a_pointer_event_carries_the_captured_element_geometry() {
        let event = PointerEvent {
            phase: PointerPhase::Move,
            position: Point::new(140.0, 60.0),
            origin: Point::new(100.0, 50.0),
            local_position: Point::ZERO,
            local_origin: Point::ZERO,
            delta: Vector::new(40.0, 10.0),
            button: MouseButton::Left,
            modifiers: Modifiers::empty(),
            size: Size::ZERO,
        };
        assert_eq!(event.size, Size::ZERO);

        let localized = event.localize(Rect::new(100.0, 40.0, 200.0, 20.0));
        assert_eq!(localized.local_position, Point::new(40.0, 20.0));
        assert_eq!(localized.local_origin, Point::new(0.0, 10.0));
        assert_eq!(localized.size, Size::new(200.0, 20.0));
    }
}
