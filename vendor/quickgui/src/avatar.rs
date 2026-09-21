use std::sync::Arc;
use web_time::{Duration, Instant};

use crate::{AccessibilityRole, Element, ElementId, EventContext, StateAccessor, ViewContext, div};

/// Longest fallback delay one avatar may declare.
///
/// The delay is an exact one-shot deadline, never a repeating timer, so the bound only keeps a
/// mistyped configuration from parking a window's next wake-up arbitrarily far in the future.
pub const MAX_AVATAR_FALLBACK_DELAY: Duration = Duration::from_secs(10);

const AVATAR_IMAGE_ID_TAG: u64 = 0x3d1c_77a6_5b90_e284;
const AVATAR_FALLBACK_ID_TAG: u64 = 0x9ab4_0e51_c236_71df;

/// Load state of the image behind one [`Avatar`].
///
/// The values match Base UI's `Avatar.Root` loading status so a hosted renderer can forward them
/// unchanged.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AvatarLoadingStatus {
    /// No image source is declared, so the fallback is the whole avatar.
    #[default]
    Idle,
    /// A declared source is being fetched or decoded.
    Loading,
    /// A declared source decoded successfully and the image part may mount.
    Loaded,
    /// A declared source failed; the fallback takes over immediately.
    Error,
}

impl AvatarLoadingStatus {
    pub const fn is_loaded(self) -> bool {
        matches!(self, Self::Loaded)
    }

    pub const fn is_error(self) -> bool {
        matches!(self, Self::Error)
    }
}

/// Controlled, allocation-free load state for one avatar.
///
/// The application owns which source it loads and reports the result; QuickGUI owns the exact
/// fallback deadline that keeps a brief load from flashing initials on screen. Nothing here polls:
/// [`Self::next_deadline`] reports the single instant the owner should ask for one repaint, and
/// [`Self::poll`] applies it. A settled avatar reports no deadline at all.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AvatarState {
    status: AvatarLoadingStatus,
    delay: Duration,
    fallback_at: Option<Instant>,
}

impl Default for AvatarState {
    fn default() -> Self {
        Self::new()
    }
}

impl AvatarState {
    /// Declare an avatar with no source yet, whose fallback is visible immediately.
    pub const fn new() -> Self {
        Self {
            status: AvatarLoadingStatus::Idle,
            delay: Duration::ZERO,
            fallback_at: None,
        }
    }

    /// Declare an avatar whose source is already loading at `now`.
    pub fn loading(now: Instant) -> Self {
        let mut state = Self::new();
        state.set_loading_status(AvatarLoadingStatus::Loading, now);
        state
    }

    /// Hold the fallback back for `delay` after loading starts.
    ///
    /// This is Base UI's `Avatar.Fallback delay`. A zero delay shows the fallback immediately;
    /// anything longer than [`MAX_AVATAR_FALLBACK_DELAY`] is clamped to it, and a non-finite or
    /// unrepresentable value falls back to zero.
    #[must_use]
    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay.min(MAX_AVATAR_FALLBACK_DELAY);
        self
    }

    pub const fn loading_status(&self) -> AvatarLoadingStatus {
        self.status
    }

    pub const fn fallback_delay(&self) -> Duration {
        self.delay
    }

    /// Replace the retained load status, returning whether it changed.
    ///
    /// Entering [`AvatarLoadingStatus::Loading`] or [`AvatarLoadingStatus::Idle`] arms the exact
    /// fallback deadline; [`AvatarLoadingStatus::Error`] shows the fallback at once, and
    /// [`AvatarLoadingStatus::Loaded`] disarms it.
    pub fn set_loading_status(&mut self, status: AvatarLoadingStatus, now: Instant) -> bool {
        if self.status == status {
            return false;
        }
        self.status = status;
        self.fallback_at = match status {
            AvatarLoadingStatus::Loading | AvatarLoadingStatus::Idle => {
                (!self.delay.is_zero()).then(|| now + self.delay)
            }
            AvatarLoadingStatus::Loaded | AvatarLoadingStatus::Error => None,
        };
        true
    }

    /// The single instant a repaint is needed, or `None` while the avatar is settled.
    pub const fn next_deadline(&self) -> Option<Instant> {
        self.fallback_at
    }

    /// Retire an elapsed fallback deadline, returning whether the visible state changed.
    pub fn poll(&mut self, now: Instant) -> bool {
        match self.fallback_at {
            Some(deadline) if deadline <= now => {
                self.fallback_at = None;
                true
            }
            _ => false,
        }
    }

    /// Whether the image part should be mounted.
    pub const fn shows_image(&self) -> bool {
        self.status.is_loaded()
    }

    /// Whether the fallback part should be mounted.
    ///
    /// A loaded image hides the fallback; an unloaded one shows it only once the declared delay
    /// has elapsed, which is why a fast cache hit never flashes initials.
    pub const fn shows_fallback(&self) -> bool {
        !self.status.is_loaded() && self.fallback_at.is_none()
    }
}

/// A copyable declaration for one controlled, unstyled avatar.
///
/// The application owns the shape, size, colors, initials, icon, and image source. QuickGUI
/// supplies stable part identities, the Image role and accessible name on the root, and
/// accessibility-hidden image and fallback parts so an avatar is announced exactly once.
///
/// The descriptor retains no allocation beyond its accessible name, and no task, timer, observer,
/// or idle scheduler source.
#[derive(Clone, Debug, PartialEq)]
#[must_use = "an Avatar descriptor has no effect until one of its parts is mounted"]
pub struct Avatar {
    root_id: ElementId,
    label: Arc<str>,
}

impl Avatar {
    /// Declare an avatar with the accessible name assistive technology announces.
    pub fn new(root_id: impl Into<ElementId>, label: impl Into<Arc<str>>) -> Self {
        Self {
            root_id: root_id.into(),
            label: label.into(),
        }
    }

    pub fn root_id(&self) -> ElementId {
        self.root_id
    }

    pub fn label(&self) -> &Arc<str> {
        &self.label
    }

    pub fn image_id(&self) -> ElementId {
        derived_avatar_id(self.root_id, AVATAR_IMAGE_ID_TAG)
    }

    pub fn fallback_id(&self) -> ElementId {
        derived_avatar_id(self.root_id, AVATAR_FALLBACK_ID_TAG)
    }

    /// Decorate an application-owned root without adding layout or appearance.
    ///
    /// The root carries the whole avatar's Image role and accessible name, so swapping between the
    /// image and the fallback never changes what is announced.
    pub fn root_with(&self, root: Element) -> Element {
        root.id(self.root_id)
            .accessibility_role(AccessibilityRole::Image)
            .accessibility_label(Arc::clone(&self.label))
            .user_select_none()
            .app_region_no_drag()
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(&self) -> Element {
        self.root_with(crate::div())
    }

    /// Decorate the application-owned image without adding appearance.
    ///
    /// Pass a [`crate::img`] built from the caller's own source. Mount it only while
    /// [`AvatarState::shows_image`] is true; the part is hidden from assistive technology because
    /// the root already names the avatar.
    pub fn image_with(&self, image: Element) -> Element {
        image
            .id(self.image_id())
            .accessibility_hidden(true)
            .user_select_none()
    }
    /// Create the unstyled image part. Use [`Self::image_with`] to supply an existing element.
    pub fn image(&self) -> Element {
        self.image_with(crate::div())
    }

    /// Decorate the application-owned fallback without adding appearance.
    ///
    /// Mount it only while [`AvatarState::shows_fallback`] is true.
    pub fn fallback_with(&self, fallback: Element) -> Element {
        fallback
            .id(self.fallback_id())
            .accessibility_hidden(true)
            .user_select_none()
    }
    /// Create the unstyled fallback part. Use [`Self::fallback_with`] to supply an existing element.
    pub fn fallback(&self) -> Element {
        self.fallback_with(crate::div())
    }

    /// Apply a load status reported by the application and notify the owner exactly once.
    ///
    /// QuickGUI decodes images on a worker pool without a per-element completion event, so the
    /// application reports the status it already knows — from its own `cx.spawn` load, an
    /// [`crate::ImageResource`] loader, or a hosted renderer — and this is the `onLoadingStatusChange`
    /// counterpart: the retained status changes, the exact fallback deadline is re-armed, and
    /// `on_loading_status_change` runs only for a real transition.
    ///
    /// Returns whether the retained state changed.
    pub fn apply_loading_status<V: 'static>(
        view: &mut V,
        cx: &mut EventContext,
        access: &StateAccessor<V, AvatarState>,
        status: AvatarLoadingStatus,
        now: Instant,
        on_loading_status_change: impl FnOnce(&mut V, AvatarLoadingStatus, &mut EventContext),
    ) -> bool {
        if !access.get(view).set_loading_status(status, now) {
            return false;
        }
        on_loading_status_change(view, status, cx);
        true
    }

    /// Ask for the one repaint an armed fallback deadline needs.
    ///
    /// Call this while rendering. A settled avatar requests nothing, so the window stays asleep.
    pub fn schedule<V: 'static>(cx: &mut ViewContext<'_, V>, state: &AvatarState) {
        if let Some(deadline) = state.next_deadline() {
            cx.request_repaint_at(deadline);
        }
    }
}

/// Create an unstyled avatar root.
///
/// This shorthand is equivalent to `Avatar::new(id, label).root_with(div())`.
pub fn avatar(id: impl Into<ElementId>, label: impl Into<Arc<str>>) -> Element {
    Avatar::new(id, label).root_with(div())
}

fn derived_avatar_id(scope: ElementId, tag: u64) -> ElementId {
    let mut hash = scope.as_u64().rotate_left(19) ^ tag;
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    if hash == 0 || hash == u64::MAX || hash == scope.as_u64() {
        hash ^= tag.rotate_left(23);
    }
    ElementId::new(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, IntoElement, TestAppContext, View, text};

    #[test]
    fn fallback_delay_is_one_exact_deadline() {
        let start = Instant::now();
        let mut state = AvatarState::new().delay(Duration::from_millis(200));
        assert_eq!(state.loading_status(), AvatarLoadingStatus::Idle);
        assert!(state.shows_fallback());
        assert!(!state.shows_image());
        assert_eq!(state.next_deadline(), None);

        assert!(state.set_loading_status(AvatarLoadingStatus::Loading, start));
        assert!(!state.set_loading_status(AvatarLoadingStatus::Loading, start));
        assert_eq!(
            state.next_deadline(),
            Some(start + Duration::from_millis(200))
        );
        assert!(!state.shows_fallback());
        assert!(!state.shows_image());

        assert!(!state.poll(start + Duration::from_millis(199)));
        assert!(state.poll(start + Duration::from_millis(200)));
        assert!(state.shows_fallback());
        assert_eq!(state.next_deadline(), None);
        assert!(!state.poll(start + Duration::from_secs(5)));

        assert!(state.set_loading_status(AvatarLoadingStatus::Loaded, start));
        assert!(state.shows_image());
        assert!(!state.shows_fallback());
        assert_eq!(state.next_deadline(), None);

        assert!(state.set_loading_status(AvatarLoadingStatus::Error, start));
        assert!(state.loading_status().is_error());
        assert!(state.shows_fallback());
        assert_eq!(state.next_deadline(), None);

        // Without a declared delay the fallback is visible from the first frame.
        let immediate = AvatarState::loading(start);
        assert_eq!(immediate.next_deadline(), None);
        assert!(immediate.shows_fallback());
        assert!(!immediate.shows_image());

        let clamped = AvatarState::new().delay(Duration::from_secs(600));
        assert_eq!(clamped.fallback_delay(), MAX_AVATAR_FALLBACK_DELAY);
        assert_eq!(AvatarState::default(), AvatarState::new());
    }

    #[test]
    fn parts_add_exact_semantics_without_appearance() {
        let avatar = Avatar::new("member", "Ada Lovelace");
        let root = avatar.root_with(div().size(32.0, 32.0).bg(Color::rgb8(9, 9, 9)));
        assert_eq!(root.explicit_id, Some("member".into()));
        assert_eq!(root.accessibility.role, AccessibilityRole::Image);
        assert_eq!(
            root.accessibility.label.as_deref(),
            Some("Ada Lovelace"),
            "the root owns the whole avatar's accessible name"
        );
        assert_eq!(root.visual.background, Some(Color::rgb8(9, 9, 9)));
        assert!(!root.focusable);

        let image = avatar.image_with(div());
        assert_eq!(image.explicit_id, Some(avatar.image_id()));
        assert!(image.accessibility.hidden);
        assert_eq!(image.visual.background, None);

        let fallback = avatar.fallback_with(div().child(text("AL")));
        assert_eq!(fallback.explicit_id, Some(avatar.fallback_id()));
        assert!(fallback.accessibility.hidden);

        let ids = [avatar.root_id(), avatar.image_id(), avatar.fallback_id()];
        for (index, id) in ids.iter().enumerate() {
            assert_ne!(*id, ElementId::new(0));
            assert_ne!(*id, ElementId::new(u64::MAX));
            assert!(!ids[..index].contains(id));
        }
        assert_eq!(avatar.image_id(), Avatar::new("member", "other").image_id());

        let shorthand = super::avatar("solo", "Solo");
        assert_eq!(shorthand.accessibility.role, AccessibilityRole::Image);
        assert!(shorthand.children.is_empty());
    }

    struct AvatarView {
        state: AvatarState,
        changes: Vec<AvatarLoadingStatus>,
    }

    impl AvatarView {
        fn access() -> StateAccessor<Self, AvatarState> {
            StateAccessor::new(|view: &mut Self| &mut view.state)
        }
    }

    impl View for AvatarView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            // Retiring an elapsed deadline is the only work a settled avatar ever does.
            let _ = self.state.poll(Instant::now());
            Avatar::schedule(cx, &self.state);
            let avatar = Avatar::new("member", "Ada Lovelace");
            let load = cx.listener("load", {
                let access = Self::access();
                move |view, cx| {
                    Avatar::apply_loading_status(
                        view,
                        cx,
                        &access,
                        AvatarLoadingStatus::Loaded,
                        Instant::now(),
                        |view, status, cx| {
                            view.changes.push(status);
                            cx.invalidate();
                        },
                    );
                }
            });
            let mut root = avatar.root_with(div().size(32.0, 32.0));
            if self.state.shows_image() {
                root = root.child(avatar.image_with(div().size_full()));
            }
            if self.state.shows_fallback() {
                root = root.child(avatar.fallback_with(div().child(text("AL"))));
            }
            div()
                .child(root)
                .child(crate::button().id("load").child("Load").on_click(load))
        }
    }

    #[test]
    fn controlled_status_projects_one_named_image_and_sleeps() {
        let (mut cx, view) = TestAppContext::new(AvatarView {
            state: AvatarState::new().delay(Duration::from_millis(150)),
            changes: Vec::new(),
        })
        .unwrap();
        let window = view.window_handle();
        let avatar = Avatar::new("member", "Ada Lovelace");

        assert!(cx.contains_element(window, avatar.fallback_id()).unwrap());
        assert!(!cx.contains_element(window, avatar.image_id()).unwrap());

        let update = cx.accessibility_update(window).unwrap();
        let root = update
            .nodes
            .iter()
            .find_map(|(id, node)| (id.0 == avatar.root_id().as_u64()).then_some(node))
            .expect("avatar accessibility node");
        assert_eq!(root.role(), accesskit::Role::Image);
        assert_eq!(root.label(), Some("Ada Lovelace"));

        cx.click(window, "load").unwrap();
        assert_eq!(
            cx.read(view, |view| view.changes.clone()).unwrap(),
            vec![AvatarLoadingStatus::Loaded]
        );
        assert!(cx.contains_element(window, avatar.image_id()).unwrap());
        assert!(!cx.contains_element(window, avatar.fallback_id()).unwrap());

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }
}
