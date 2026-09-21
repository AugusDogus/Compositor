#[cfg(not(target_os = "macos"))]
use std::hash::{DefaultHasher, Hash, Hasher};
use std::{collections::HashSet, fmt, sync::Arc};

use thiserror::Error;
use winit::{event_loop::ActiveEventLoop, monitor::MonitorHandle};

use crate::{Point, Rect, Size};

/// Maximum active displays retained in one application snapshot.
pub const MAX_DISPLAYS: usize = 64;
/// Maximum UTF-8 bytes retained for one operating-system display name.
pub const MAX_DISPLAY_NAME_BYTES: usize = 4 * 1024;
/// Maximum granular [`DisplayEvent`] values produced by one snapshot diff.
///
/// One reconfiguration can at most remove every previous display and add every new one, so the
/// bound is twice [`MAX_DISPLAYS`].
pub const MAX_DISPLAY_EVENTS: usize = MAX_DISPLAYS * 2;
/// Largest reported bit depth retained by [`Display::color_depth`].
pub const MAX_DISPLAY_COLOR_DEPTH: u8 = 64;

const MAX_DISPLAY_LOGICAL_COORDINATE: f32 = 16_777_216.0;
const MAX_DISPLAY_LOGICAL_DIMENSION: f32 = 1_048_576.0;
const MIN_DISPLAY_SCALE_FACTOR: f32 = 0.25;
const MAX_DISPLAY_SCALE_FACTOR: f32 = 16.0;

/// Process-level display identity.
///
/// On macOS this is the current Core Graphics display identifier. It is suitable for selecting a
/// display during the current application run. Persist [`Display::uuid`] when a stable physical
/// display identity is required across launches.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DisplayId(u64);

impl DisplayId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Stable physical display identity when the platform exposes one.
///
/// QuickGUI currently supplies this on macOS. The byte representation avoids retaining native
/// Core Foundation objects and does not require a UUID allocation for reads.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DisplayUuid([u8; 16]);

impl DisplayUuid {
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    pub const fn into_bytes(self) -> [u8; 16] {
        self.0
    }
}

impl fmt::Debug for DisplayUuid {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl fmt::Display for DisplayUuid {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = self.0;
        write!(
            formatter,
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            bytes[0],
            bytes[1],
            bytes[2],
            bytes[3],
            bytes[4],
            bytes[5],
            bytes[6],
            bytes[7],
            bytes[8],
            bytes[9],
            bytes[10],
            bytes[11],
            bytes[12],
            bytes[13],
            bytes[14],
            bytes[15],
        )
    }
}

/// One immutable display description in global logical desktop coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct Display {
    id: DisplayId,
    uuid: Option<DisplayUuid>,
    name: Arc<str>,
    bounds: Rect,
    visible_bounds: Rect,
    scale_factor: f32,
    refresh_rate_millihertz: Option<u32>,
    rotation_degrees: u16,
    is_internal: bool,
    color_depth: Option<u8>,
    primary: bool,
}

impl Display {
    /// Build a validated display value, primarily for deterministic platform adapters and tests.
    pub fn new(
        id: DisplayId,
        name: impl Into<Arc<str>>,
        bounds: Rect,
        visible_bounds: Rect,
        scale_factor: f32,
    ) -> Result<Self, DisplayError> {
        let name = name.into();
        if name.len() > MAX_DISPLAY_NAME_BYTES || name.contains('\0') {
            return Err(DisplayError::InvalidName);
        }
        if !valid_display_bounds(bounds)
            || !valid_display_bounds(visible_bounds)
            || bounds.intersection(visible_bounds) != Some(visible_bounds)
        {
            return Err(DisplayError::InvalidBounds);
        }
        if !scale_factor.is_finite()
            || !(MIN_DISPLAY_SCALE_FACTOR..=MAX_DISPLAY_SCALE_FACTOR).contains(&scale_factor)
        {
            return Err(DisplayError::InvalidScaleFactor);
        }
        Ok(Self {
            id,
            uuid: None,
            name,
            bounds,
            visible_bounds,
            scale_factor,
            refresh_rate_millihertz: None,
            rotation_degrees: 0,
            is_internal: false,
            color_depth: None,
            primary: false,
        })
    }

    pub const fn id(&self) -> DisplayId {
        self.id
    }

    pub const fn uuid(&self) -> Option<DisplayUuid> {
        self.uuid
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Complete display rectangle in global logical desktop coordinates.
    pub const fn bounds(&self) -> Rect {
        self.bounds
    }

    /// Work area excluding persistent native chrome such as the Dock, menu bar, or taskbar.
    pub const fn visible_bounds(&self) -> Rect {
        self.visible_bounds
    }

    pub const fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    pub const fn refresh_rate_millihertz(&self) -> Option<u32> {
        self.refresh_rate_millihertz
    }

    pub const fn is_primary(&self) -> bool {
        self.primary
    }

    /// Clockwise desktop rotation reported by the operating system: `0`, `90`, `180`, or `270`.
    ///
    /// Platforms that do not report a rotation return `0`.
    pub const fn rotation_degrees(&self) -> u16 {
        self.rotation_degrees
    }

    /// Whether the operating system reports this display as the machine's built-in panel.
    pub const fn is_internal(&self) -> bool {
        self.is_internal
    }

    /// Bits per pixel reported by the operating system, when it exposes one.
    pub const fn color_depth(&self) -> Option<u8> {
        self.color_depth
    }

    pub fn with_uuid(mut self, uuid: DisplayUuid) -> Self {
        self.uuid = Some(uuid);
        self
    }

    pub fn with_refresh_rate_millihertz(mut self, refresh_rate: u32) -> Self {
        self.refresh_rate_millihertz = (refresh_rate > 0).then_some(refresh_rate);
        self
    }

    /// Record a clockwise rotation, normalized to the nearest quarter turn.
    pub fn with_rotation_degrees(mut self, degrees: u16) -> Self {
        self.rotation_degrees = normalize_rotation_degrees(degrees);
        self
    }

    /// Record whether the display is the machine's built-in panel.
    pub fn with_internal(mut self, is_internal: bool) -> Self {
        self.is_internal = is_internal;
        self
    }

    /// Record a reported bit depth. Zero and values above [`MAX_DISPLAY_COLOR_DEPTH`] clear it.
    pub fn with_color_depth(mut self, bits_per_pixel: u8) -> Self {
        self.color_depth = (bits_per_pixel > 0 && bits_per_pixel <= MAX_DISPLAY_COLOR_DEPTH)
            .then_some(bits_per_pixel);
        self
    }

    /// Center and, if necessary, shrink a window rectangle into this display's visible work area.
    pub fn centered_bounds(&self, size: Size) -> Rect {
        let area = self.visible_bounds;
        let width = sane_window_dimension(size.width, area.width);
        let height = sane_window_dimension(size.height, area.height);
        Rect::new(
            area.x + (area.width - width) * 0.5,
            area.y + (area.height - height) * 0.5,
            width,
            height,
        )
    }

    /// Keep a window rectangle fully inside this display's visible work area.
    ///
    /// Oversized or non-finite dimensions are replaced by the available dimension. This helper is
    /// intentionally pure and does not move a native window by itself.
    pub fn constrain_bounds(&self, bounds: Rect) -> Rect {
        let area = self.visible_bounds;
        let width = sane_window_dimension(bounds.width, area.width);
        let height = sane_window_dimension(bounds.height, area.height);
        let x = if bounds.x.is_finite() {
            bounds.x.clamp(area.x, area.right() - width)
        } else {
            area.x
        };
        let y = if bounds.y.is_finite() {
            bounds.y.clamp(area.y, area.bottom() - height)
        } else {
            area.y
        };
        Rect::new(x, y, width, height)
    }
}

/// A bounded, cheaply cloned snapshot of all active displays.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Displays {
    displays: Arc<[Display]>,
    primary: Option<DisplayId>,
}

impl Displays {
    /// Build a deterministic display snapshot.
    pub fn new(
        mut displays: Vec<Display>,
        primary: Option<DisplayId>,
    ) -> Result<Self, DisplayError> {
        if displays.len() > MAX_DISPLAYS {
            return Err(DisplayError::TooManyDisplays);
        }
        let mut ids = HashSet::with_capacity(displays.len());
        if displays.iter().any(|display| !ids.insert(display.id)) {
            return Err(DisplayError::DuplicateId);
        }
        if primary.is_some_and(|primary| !ids.contains(&primary)) {
            return Err(DisplayError::UnknownPrimary);
        }
        for display in &mut displays {
            display.primary = Some(display.id) == primary;
        }
        displays.sort_unstable_by_key(|display| display.id);
        Ok(Self {
            displays: displays.into(),
            primary,
        })
    }

    pub fn all(&self) -> &[Display] {
        &self.displays
    }

    pub fn primary(&self) -> Option<&Display> {
        self.primary.and_then(|id| self.find(id))
    }

    pub const fn primary_id(&self) -> Option<DisplayId> {
        self.primary
    }

    pub fn find(&self, id: DisplayId) -> Option<&Display> {
        self.displays
            .binary_search_by_key(&id, |display| display.id)
            .ok()
            .map(|index| &self.displays[index])
    }

    pub fn is_empty(&self) -> bool {
        self.displays.is_empty()
    }

    pub fn len(&self) -> usize {
        self.displays.len()
    }

    /// Compute the granular changes that turn `self` into `next`.
    ///
    /// Both snapshots are sorted by [`DisplayId`], so the diff is one linear merge and its output
    /// is deterministic: events are emitted in ascending identifier order and the result never
    /// exceeds [`MAX_DISPLAY_EVENTS`]. A display present in both snapshots produces
    /// [`DisplayEvent::MetricsChanged`] only when any observable field differs, including the
    /// primary flag.
    pub fn diff(&self, next: &Self) -> Vec<DisplayEvent> {
        let previous = self.all();
        let current = next.all();
        let mut events = Vec::new();
        let (mut left, mut right) = (0, 0);
        while left < previous.len() || right < current.len() {
            match (previous.get(left), current.get(right)) {
                (Some(old), Some(new)) if old.id == new.id => {
                    if old != new {
                        events.push(DisplayEvent::MetricsChanged(new.clone()));
                    }
                    left += 1;
                    right += 1;
                }
                (Some(old), Some(new)) if old.id < new.id => {
                    events.push(DisplayEvent::Removed(old.id));
                    left += 1;
                }
                (Some(_), Some(new)) => {
                    events.push(DisplayEvent::Added(new.clone()));
                    right += 1;
                }
                (Some(old), None) => {
                    events.push(DisplayEvent::Removed(old.id));
                    left += 1;
                }
                (None, Some(new)) => {
                    events.push(DisplayEvent::Added(new.clone()));
                    right += 1;
                }
                (None, None) => break,
            }
        }
        debug_assert!(events.len() <= MAX_DISPLAY_EVENTS);
        events
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn test_default() -> Self {
        let id = DisplayId::new(1);
        Self::new(
            vec![
                Display::new(
                    id,
                    "Test display",
                    Rect::new(0.0, 0.0, 1_440.0, 900.0),
                    Rect::new(0.0, 24.0, 1_440.0, 876.0),
                    2.0,
                )
                .expect("the built-in test display is valid"),
            ],
            Some(id),
        )
        .expect("the built-in test display snapshot is valid")
    }
}

/// One granular change between two consecutive display snapshots.
///
/// Delivered by `App::on_display_event`. The coarse snapshot observed through
/// [`crate::EventContext::displays`] keeps working unchanged; these events only describe what
/// moved between two snapshots so an application can react without re-scanning every display.
#[derive(Clone, Debug, PartialEq)]
pub enum DisplayEvent {
    /// A display the previous snapshot did not contain.
    Added(Display),
    /// A display the new snapshot no longer contains.
    Removed(DisplayId),
    /// A retained display whose bounds, work area, scale, refresh rate, rotation, depth, name, or
    /// primary flag changed.
    MetricsChanged(Display),
}

impl DisplayEvent {
    /// Identifier of the display this event describes.
    pub fn display_id(&self) -> DisplayId {
        match self {
            Self::Added(display) | Self::MetricsChanged(display) => display.id,
            Self::Removed(id) => *id,
        }
    }

    /// The new display description, absent for [`Self::Removed`].
    pub fn display(&self) -> Option<&Display> {
        match self {
            Self::Added(display) | Self::MetricsChanged(display) => Some(display),
            Self::Removed(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum DisplayError {
    #[error("a display name is too large or contains a null byte")]
    InvalidName,
    #[error("display bounds must be finite, positive, and the visible bounds must be contained")]
    InvalidBounds,
    #[error("a display scale factor must be finite and between 0.25 and 16")]
    InvalidScaleFactor,
    #[error("a display snapshot cannot retain more than {MAX_DISPLAYS} displays")]
    TooManyDisplays,
    #[error("display identifiers must be unique within one snapshot")]
    DuplicateId,
    #[error("the primary display identifier must occur in the snapshot")]
    UnknownPrimary,
}

pub(crate) fn native_displays(event_loop: &ActiveEventLoop) -> Displays {
    let primary_monitor = event_loop.primary_monitor();
    let mut monitors = Vec::with_capacity(4);
    if let Some(primary) = primary_monitor.clone() {
        monitors.push(primary);
    }
    for monitor in event_loop.available_monitors() {
        if monitors.len() == MAX_DISPLAYS {
            break;
        }
        if !monitors.contains(&monitor) {
            monitors.push(monitor);
        }
    }

    let primary = primary_monitor.as_ref().map(native_display_id);
    let mut ids = HashSet::with_capacity(monitors.len());
    let mut displays = Vec::with_capacity(monitors.len());
    for monitor in monitors {
        let id = native_display_id(&monitor);
        if !ids.insert(id) {
            tracing::warn!(?id, "ignoring a display identifier collision");
            continue;
        }
        let Some(display) = display_from_monitor(&monitor, id) else {
            ids.remove(&id);
            continue;
        };
        displays.push(display);
    }
    let primary = primary
        .filter(|primary| ids.contains(primary))
        .or_else(|| displays.first().map(Display::id));
    Displays::new(displays, primary).unwrap_or_default()
}

/// Resolve one short-lived native handle only when a new window needs monitor targeting.
pub(crate) fn native_monitor(event_loop: &ActiveEventLoop, id: DisplayId) -> Option<MonitorHandle> {
    if let Some(primary) = event_loop.primary_monitor()
        && native_display_id(&primary) == id
    {
        return Some(primary);
    }
    event_loop
        .available_monitors()
        .take(MAX_DISPLAYS)
        .find(|monitor| native_display_id(monitor) == id)
}

pub(crate) fn native_display_id(monitor: &MonitorHandle) -> DisplayId {
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::MonitorHandleExtMacOS;
        DisplayId::new(u64::from(monitor.native_id()))
    }
    #[cfg(target_os = "windows")]
    {
        use winit::platform::windows::MonitorHandleExtWindows;

        let mut hasher = DefaultHasher::new();
        monitor.native_id().hash(&mut hasher);
        DisplayId::new(hasher.finish().max(1))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let mut hasher = DefaultHasher::new();
        monitor.name().hash(&mut hasher);
        let position = monitor.position();
        position.x.hash(&mut hasher);
        position.y.hash(&mut hasher);
        let size = monitor.size();
        size.width.hash(&mut hasher);
        size.height.hash(&mut hasher);
        monitor.scale_factor().to_bits().hash(&mut hasher);
        monitor.refresh_rate_millihertz().hash(&mut hasher);
        DisplayId::new(hasher.finish().max(1))
    }
}

fn display_from_monitor(monitor: &MonitorHandle, id: DisplayId) -> Option<Display> {
    let scale_factor = monitor.scale_factor() as f32;
    if !scale_factor.is_finite()
        || !(MIN_DISPLAY_SCALE_FACTOR..=MAX_DISPLAY_SCALE_FACTOR).contains(&scale_factor)
    {
        return None;
    }
    let position = monitor.position();
    let size = monitor.size();
    let bounds = Rect::new(
        position.x as f32 / scale_factor,
        position.y as f32 / scale_factor,
        size.width as f32 / scale_factor,
        size.height as f32 / scale_factor,
    );
    if !valid_display_bounds(bounds) {
        return None;
    }

    #[cfg(target_os = "macos")]
    let metadata = macos_display_metadata(monitor, id, bounds);
    #[cfg(not(target_os = "macos"))]
    let metadata = NativeDisplayMetadata {
        name: bounded_native_name(monitor.name(), id),
        visible_bounds: bounds,
        uuid: None,
        rotation_degrees: 0,
        is_internal: false,
        color_depth: None,
    };

    let mut display = Display::new(
        id,
        metadata.name,
        bounds,
        metadata.visible_bounds,
        scale_factor,
    )
    .ok()?;
    display.uuid = metadata.uuid;
    display.rotation_degrees = metadata.rotation_degrees;
    display.is_internal = metadata.is_internal;
    display.color_depth = metadata.color_depth;
    display.refresh_rate_millihertz = monitor
        .refresh_rate_millihertz()
        .filter(|refresh_rate| *refresh_rate > 0);
    Some(display)
}

/// Operating-system display facts QuickGUI copies out before releasing every native handle.
struct NativeDisplayMetadata {
    name: Arc<str>,
    visible_bounds: Rect,
    uuid: Option<DisplayUuid>,
    rotation_degrees: u16,
    is_internal: bool,
    color_depth: Option<u8>,
}

fn bounded_native_name(name: Option<String>, id: DisplayId) -> Arc<str> {
    let Some(mut name) = name.filter(|name| !name.contains('\0')) else {
        return Arc::from(format!("Display {}", id.get()));
    };
    if name.len() > MAX_DISPLAY_NAME_BYTES {
        let mut end = MAX_DISPLAY_NAME_BYTES;
        while !name.is_char_boundary(end) {
            end -= 1;
        }
        name.truncate(end);
    }
    Arc::from(name)
}

#[cfg(target_os = "macos")]
fn macos_display_metadata(
    monitor: &MonitorHandle,
    id: DisplayId,
    bounds: Rect,
) -> NativeDisplayMetadata {
    use objc2_app_kit::{NSBitsPerPixelFromDepth, NSScreen};
    use objc2_foundation::NSUTF8StringEncoding;
    use winit::platform::macos::MonitorHandleExtMacOS;

    let mut name = None;
    let mut visible_bounds = bounds;
    let mut color_depth = None;
    if let Some(screen) = monitor
        .ns_screen()
        .and_then(|screen| unsafe { (screen as *const NSScreen).as_ref() })
    {
        let bits = unsafe { NSBitsPerPixelFromDepth(screen.depth()) };
        color_depth = u8::try_from(bits)
            .ok()
            .filter(|bits| *bits > 0 && *bits <= MAX_DISPLAY_COLOR_DEPTH);
        let native_name = unsafe { screen.localizedName() };
        if native_name.lengthOfBytesUsingEncoding(NSUTF8StringEncoding) <= MAX_DISPLAY_NAME_BYTES {
            name = Some(native_name.to_string());
        }
        let frame = screen.frame();
        let visible = screen.visibleFrame();
        let left_inset = (visible.origin.x - frame.origin.x) as f32;
        let top_inset =
            (frame.origin.y + frame.size.height - visible.origin.y - visible.size.height) as f32;
        let proposed = Rect::new(
            bounds.x + left_inset,
            bounds.y + top_inset,
            visible.size.width as f32,
            visible.size.height as f32,
        );
        if let Some(intersection) = bounds.intersection(proposed)
            && valid_display_bounds(intersection)
        {
            visible_bounds = intersection;
        }
    }
    let (rotation_degrees, is_internal) = macos_display_orientation(id);
    NativeDisplayMetadata {
        name: bounded_native_name(name.or_else(|| monitor.name()), id),
        visible_bounds,
        uuid: macos_display_uuid(id),
        rotation_degrees,
        is_internal,
        color_depth,
    }
}

/// Read the Core Graphics rotation and built-in flag without retaining a native handle.
#[cfg(target_os = "macos")]
fn macos_display_orientation(id: DisplayId) -> (u16, bool) {
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn CGDisplayRotation(display: u32) -> f64;
        fn CGDisplayIsBuiltin(display: u32) -> i32;
    }

    let Ok(id) = u32::try_from(id.get()) else {
        return (0, false);
    };
    let rotation = unsafe { CGDisplayRotation(id) };
    // Core Graphics reports a counter-clockwise angle; QuickGUI publishes the clockwise turn.
    let rotation = if rotation.is_finite() {
        let clockwise = (360.0 - rotation).rem_euclid(360.0);
        normalize_rotation_degrees(clockwise.round() as u16)
    } else {
        0
    };
    (rotation, unsafe { CGDisplayIsBuiltin(id) } != 0)
}

#[cfg(target_os = "macos")]
fn macos_display_uuid(id: DisplayId) -> Option<DisplayUuid> {
    use core_foundation::{
        base::TCFType,
        uuid::{CFUUID, CFUUIDGetUUIDBytes, CFUUIDRef},
    };

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn CGDisplayCreateUUIDFromDisplayID(display: u32) -> CFUUIDRef;
    }

    let id = u32::try_from(id.get()).ok()?;
    let uuid = unsafe { CGDisplayCreateUUIDFromDisplayID(id) };
    if uuid.is_null() {
        return None;
    }
    let uuid = unsafe { CFUUID::wrap_under_create_rule(uuid) };
    let bytes = unsafe { CFUUIDGetUUIDBytes(uuid.as_concrete_TypeRef()) };
    Some(DisplayUuid::from_bytes([
        bytes.byte0,
        bytes.byte1,
        bytes.byte2,
        bytes.byte3,
        bytes.byte4,
        bytes.byte5,
        bytes.byte6,
        bytes.byte7,
        bytes.byte8,
        bytes.byte9,
        bytes.byte10,
        bytes.byte11,
        bytes.byte12,
        bytes.byte13,
        bytes.byte14,
        bytes.byte15,
    ]))
}

fn valid_display_bounds(bounds: Rect) -> bool {
    bounds.x.is_finite()
        && bounds.y.is_finite()
        && bounds.width.is_finite()
        && bounds.height.is_finite()
        && bounds.x.abs() <= MAX_DISPLAY_LOGICAL_COORDINATE
        && bounds.y.abs() <= MAX_DISPLAY_LOGICAL_COORDINATE
        && bounds.width > 0.0
        && bounds.height > 0.0
        && bounds.width <= MAX_DISPLAY_LOGICAL_DIMENSION
        && bounds.height <= MAX_DISPLAY_LOGICAL_DIMENSION
}

fn normalize_rotation_degrees(degrees: u16) -> u16 {
    let degrees = degrees % 360;
    match degrees {
        0..=44 | 315..=359 => 0,
        45..=134 => 90,
        135..=224 => 180,
        _ => 270,
    }
}

/// Convert a physical status-item rectangle into QuickGUI's global logical desktop coordinates.
///
/// The tray backend reports physical pixels with a top-left origin. The correct scale factor is
/// the one of the display that ends up containing the converted rectangle, so each candidate
/// display is tested with its own factor before falling back to the primary display.
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
pub(crate) fn logical_rect_from_physical(
    displays: &Displays,
    position: (f64, f64),
    size: (u32, u32),
) -> Option<Rect> {
    let convert = |scale: f32| -> Option<Rect> {
        let scale = f64::from(scale);
        if !(scale.is_finite() && scale > 0.0) {
            return None;
        }
        let rect = Rect::new(
            (position.0 / scale) as f32,
            (position.1 / scale) as f32,
            (f64::from(size.0) / scale) as f32,
            (f64::from(size.1) / scale) as f32,
        );
        (rect.x.is_finite() && rect.y.is_finite() && rect.width >= 0.0 && rect.height >= 0.0)
            .then_some(rect)
    };
    for display in displays.all() {
        if let Some(rect) = convert(display.scale_factor)
            && display.bounds.contains(Point::new(
                rect.x + rect.width * 0.5,
                rect.y + rect.height * 0.5,
            ))
        {
            return Some(rect);
        }
    }
    convert(displays.primary().map_or(1.0, Display::scale_factor))
}

fn sane_window_dimension(requested: f32, available: f32) -> f32 {
    if requested.is_finite() && requested > 0.0 {
        requested.min(available)
    } else {
        available
    }
}

pub(crate) fn display_for_rect(displays: &Displays, bounds: Rect) -> Option<DisplayId> {
    let center = Point::new(
        bounds.x + bounds.width * 0.5,
        bounds.y + bounds.height * 0.5,
    );
    displays
        .all()
        .iter()
        .find(|display| display.bounds.contains(center))
        .or_else(|| {
            displays
                .all()
                .iter()
                .max_by(|left, right| {
                    intersection_area(left.bounds, bounds)
                        .total_cmp(&intersection_area(right.bounds, bounds))
                })
                .filter(|display| intersection_area(display.bounds, bounds) > 0.0)
        })
        .map(Display::id)
        .or_else(|| displays.primary_id())
}

fn intersection_area(left: Rect, right: Rect) -> f32 {
    left.intersection(right)
        .map_or(0.0, |bounds| bounds.width * bounds.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(id: u64, x: f32) -> Display {
        Display::new(
            DisplayId::new(id),
            format!("Display {id}"),
            Rect::new(x, 0.0, 1_000.0, 800.0),
            Rect::new(x, 24.0, 1_000.0, 776.0),
            2.0,
        )
        .unwrap()
    }

    #[test]
    fn snapshots_are_sorted_and_mark_exactly_one_primary() {
        let displays = Displays::new(
            vec![display(2, 1_000.0), display(1, 0.0)],
            Some(DisplayId::new(1)),
        )
        .unwrap();
        assert_eq!(displays.all()[0].id(), DisplayId::new(1));
        assert!(displays.all()[0].is_primary());
        assert!(!displays.all()[1].is_primary());
        assert_eq!(displays.primary().map(Display::id), Some(DisplayId::new(1)));
    }

    #[test]
    fn centered_and_constrained_bounds_stay_in_the_work_area() {
        let display = display(1, 0.0);
        assert_eq!(
            display.centered_bounds(Size::new(400.0, 300.0)),
            Rect::new(300.0, 262.0, 400.0, 300.0)
        );
        assert_eq!(
            display.constrain_bounds(Rect::new(-400.0, 900.0, 1_400.0, 900.0)),
            display.visible_bounds()
        );
    }

    #[test]
    fn display_selection_prefers_the_window_center_then_intersection() {
        let displays = Displays::new(
            vec![display(1, 0.0), display(2, 1_000.0)],
            Some(DisplayId::new(1)),
        )
        .unwrap();
        assert_eq!(
            display_for_rect(&displays, Rect::new(900.0, 100.0, 400.0, 400.0)),
            Some(DisplayId::new(2))
        );
        assert_eq!(
            display_for_rect(&displays, Rect::new(4_000.0, 100.0, 400.0, 400.0)),
            Some(DisplayId::new(1))
        );
    }

    #[test]
    fn snapshot_diffs_report_additions_removals_and_metric_changes_in_id_order() {
        let previous = Displays::new(
            vec![display(1, 0.0), display(2, 1_000.0), display(4, 3_000.0)],
            Some(DisplayId::new(1)),
        )
        .unwrap();
        let moved = display(2, 2_000.0);
        let next = Displays::new(
            vec![display(1, 0.0), moved.clone(), display(3, 4_000.0)],
            Some(DisplayId::new(1)),
        )
        .unwrap();

        let events = previous.diff(&next);
        assert_eq!(
            events,
            vec![
                DisplayEvent::MetricsChanged(next.find(DisplayId::new(2)).unwrap().clone()),
                DisplayEvent::Added(next.find(DisplayId::new(3)).unwrap().clone()),
                DisplayEvent::Removed(DisplayId::new(4)),
            ]
        );
        assert_eq!(events[0].display_id(), DisplayId::new(2));
        assert_eq!(events[2].display(), None);
        assert!(previous.diff(&previous).is_empty());
        assert!(events.len() <= MAX_DISPLAY_EVENTS);
    }

    #[test]
    fn snapshot_diffs_report_a_changed_primary_and_stay_bounded() {
        let first = Displays::new(
            vec![display(1, 0.0), display(2, 1_000.0)],
            Some(DisplayId::new(1)),
        )
        .unwrap();
        let second = Displays::new(
            vec![display(1, 0.0), display(2, 1_000.0)],
            Some(DisplayId::new(2)),
        )
        .unwrap();
        assert_eq!(
            first.diff(&second),
            vec![
                DisplayEvent::MetricsChanged(second.find(DisplayId::new(1)).unwrap().clone()),
                DisplayEvent::MetricsChanged(second.find(DisplayId::new(2)).unwrap().clone()),
            ]
        );

        let mut many = Vec::new();
        for id in 0..MAX_DISPLAYS {
            many.push(display(id as u64 + 1, id as f32 * 1_000.0));
        }
        let full = Displays::new(many, None).unwrap();
        assert_eq!(Displays::default().diff(&full).len(), MAX_DISPLAYS);
        assert_eq!(full.diff(&Displays::default()).len(), MAX_DISPLAYS);
    }

    #[test]
    fn optional_display_metadata_is_normalized() {
        let display = display(1, 0.0);
        assert_eq!(display.rotation_degrees(), 0);
        assert!(!display.is_internal());
        assert_eq!(display.color_depth(), None);

        let rotated = display
            .clone()
            .with_rotation_degrees(450)
            .with_internal(true)
            .with_color_depth(32);
        assert_eq!(rotated.rotation_degrees(), 90);
        assert!(rotated.is_internal());
        assert_eq!(rotated.color_depth(), Some(32));
        assert_eq!(
            display
                .clone()
                .with_rotation_degrees(271)
                .rotation_degrees(),
            270
        );
        assert_eq!(display.clone().with_color_depth(0).color_depth(), None);
        assert_eq!(
            display
                .clone()
                .with_color_depth(MAX_DISPLAY_COLOR_DEPTH + 1)
                .color_depth(),
            None
        );
        assert_ne!(display, rotated, "metadata participates in snapshot diffs");
    }

    #[test]
    fn physical_rectangles_convert_with_the_containing_display_scale() {
        let displays = Displays::new(
            vec![display(1, 0.0), display(2, 1_000.0)],
            Some(DisplayId::new(1)),
        )
        .unwrap();
        assert_eq!(
            logical_rect_from_physical(&displays, (2_400.0, 0.0), (44, 48)),
            Some(Rect::new(1_200.0, 0.0, 22.0, 24.0))
        );
        // A rectangle outside every display still converts with the primary scale factor.
        assert_eq!(
            logical_rect_from_physical(&displays, (40_000.0, 0.0), (44, 48)),
            Some(Rect::new(20_000.0, 0.0, 22.0, 24.0))
        );
        assert_eq!(
            logical_rect_from_physical(&Displays::default(), (10.0, 20.0), (4, 6)),
            Some(Rect::new(10.0, 20.0, 4.0, 6.0))
        );
    }

    #[test]
    fn invalid_or_unbounded_snapshots_are_rejected() {
        let duplicate = Displays::new(vec![display(1, 0.0), display(1, 1_000.0)], None);
        assert_eq!(duplicate, Err(DisplayError::DuplicateId));
        let mut too_many = Vec::new();
        for id in 0..=MAX_DISPLAYS {
            too_many.push(display(id as u64 + 1, id as f32 * 1_000.0));
        }
        assert_eq!(
            Displays::new(too_many, None),
            Err(DisplayError::TooManyDisplays)
        );
    }
}
