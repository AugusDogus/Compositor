use web_time::Duration;

use crate::{
    AccessibilityPopover, AccessibilityRole, AsyncViewContext, Element, ElementId, EventContext,
    FocusHandle, StateAccessor, Task,
};

const DIALOG_ROOT_ID_TAG: u64 = 0x6405_1ba9_158d_f8fd;
const DIALOG_BACKDROP_ID_TAG: u64 = 0xfbf2_90af_5f5b_982d;
const DIALOG_POPOVER_ID_TAG: u64 = 0x21e9_d20f_8574_4dd8;
const DIALOG_TITLE_ID_TAG: u64 = 0xb50c_ee8e_b0c0_8ed7;
const DIALOG_DESCRIPTION_ID_TAG: u64 = 0x86a8_0ac0_5627_538e;
const DIALOG_CLOSE_ID_TAG: u64 = 0xe5d1_e6af_b19b_c827;
const DIALOG_VIEWPORT_ID_TAG: u64 = 0x3a70_c5e9_2d18_b6f4;

/// Longest open or close transition one [`DialogState`] may hold a dialog mounted for.
pub const MAX_DIALOG_TRANSITION: Duration = Duration::from_secs(10);

/// Native accessibility behavior for one in-window modal surface.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DialogKind {
    /// A modal task or editing surface which may close from its backdrop by default.
    #[default]
    Dialog,
    /// A consequential confirmation surface whose backdrop does not dismiss it by default.
    AlertDialog,
}

impl DialogKind {
    const fn accessibility_role(self) -> AccessibilityRole {
        match self {
            Self::Dialog => AccessibilityRole::Dialog,
            Self::AlertDialog => AccessibilityRole::AlertDialog,
        }
    }

    const fn dismiss_on_backdrop(self) -> bool {
        matches!(self, Self::Dialog)
    }
}

/// Copyable declaration for one controlled, unstyled in-window dialog.
///
/// The application owns the `open` value, all visual declarations, and the listener that changes
/// that value. QuickGUI supplies stable part identities, a viewport overlay, nested topmost focus
/// containment, independent Escape/backdrop dismissal, focus restoration, app-drag exclusion,
/// native-view occlusion through the overlay plane, and exact dialog accessibility semantics.
/// Mount [`Self::root_with`] only while [`Self::is_open`] is true.
///
/// The descriptor retains no task, timer, observer, registry entry, or idle scheduler source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Dialog {
    id: ElementId,
    open: bool,
    kind: DialogKind,
    initial_focus: Option<FocusHandle>,
    restore_focus: Option<FocusHandle>,
    dismiss_on_escape: bool,
    dismiss_on_backdrop: bool,
}

impl Dialog {
    /// Declare an ordinary modal dialog.
    pub fn new(id: impl Into<ElementId>, open: bool) -> Self {
        Self::with_kind(id, open, DialogKind::Dialog)
    }

    /// Declare a consequential alert dialog.
    ///
    /// Escape remains enabled, while a backdrop press is blocked without dismissing by default.
    pub fn alert(id: impl Into<ElementId>, open: bool) -> Self {
        Self::with_kind(id, open, DialogKind::AlertDialog)
    }

    pub fn with_kind(id: impl Into<ElementId>, open: bool, kind: DialogKind) -> Self {
        Self {
            id: id.into(),
            open,
            kind,
            initial_focus: None,
            restore_focus: None,
            dismiss_on_escape: true,
            dismiss_on_backdrop: kind.dismiss_on_backdrop(),
        }
    }

    pub const fn is_open(self) -> bool {
        self.open
    }

    pub const fn kind(self) -> DialogKind {
        self.kind
    }

    /// Prefer one mounted descendant when this dialog opens.
    ///
    /// Without an explicit target, the focus trap chooses its first enabled Tab stop and falls
    /// back to the popover root when no interactive descendant exists.
    pub fn initial_focus(mut self, focus: impl Into<ElementId>) -> Self {
        self.initial_focus = Some(FocusHandle::new(focus));
        self
    }

    /// Restore focus to one stable control after every dismissal path.
    pub fn restore_focus_to(mut self, focus: impl Into<ElementId>) -> Self {
        self.restore_focus = Some(FocusHandle::new(focus));
        self
    }

    pub const fn dismiss_on_escape(mut self, dismiss: bool) -> Self {
        self.dismiss_on_escape = dismiss;
        self
    }

    pub const fn dismiss_on_backdrop(mut self, dismiss: bool) -> Self {
        self.dismiss_on_backdrop = dismiss;
        self
    }

    pub fn root_id(self) -> ElementId {
        derived_dialog_id(self.id, DIALOG_ROOT_ID_TAG)
    }

    pub fn backdrop_id(self) -> ElementId {
        derived_dialog_id(self.id, DIALOG_BACKDROP_ID_TAG)
    }

    pub fn popover_id(self) -> ElementId {
        derived_dialog_id(self.id, DIALOG_POPOVER_ID_TAG)
    }

    pub fn title_id(self) -> ElementId {
        derived_dialog_id(self.id, DIALOG_TITLE_ID_TAG)
    }

    pub fn description_id(self) -> ElementId {
        derived_dialog_id(self.id, DIALOG_DESCRIPTION_ID_TAG)
    }

    pub fn close_id(self) -> ElementId {
        derived_dialog_id(self.id, DIALOG_CLOSE_ID_TAG)
    }

    /// Stable identity of the scrolling viewport between the backdrop and the popup.
    pub fn viewport_id(self) -> ElementId {
        derived_dialog_id(self.id, DIALOG_VIEWPORT_ID_TAG)
    }

    pub fn popover_focus(self) -> FocusHandle {
        FocusHandle::new(self.popover_id())
    }

    /// Request the declared initial focus in the same event that mounts the dialog.
    pub fn focus_initial(self, cx: &mut EventContext) {
        cx.focus(self.initial_focus.unwrap_or_else(|| self.popover_focus()));
    }

    /// Restore the declared focus after an explicit close action.
    ///
    /// Escape and backdrop dismissal already use the same retained target automatically.
    pub fn focus_restore(self, cx: &mut EventContext) {
        if let Some(focus) = self.restore_focus {
            cx.focus(focus);
        }
    }

    /// Decorate an application-owned trigger without adding appearance.
    pub fn trigger_with(self, id: impl Into<ElementId>, trigger: Element) -> Element {
        let trigger = trigger
            .id(id)
            .focusable()
            .accessibility_role(AccessibilityRole::Button)
            .accessibility_has_popover(AccessibilityPopover::Dialog)
            .accessibility_expanded(self.open)
            .app_region_no_drag()
            .user_select_none();
        if self.open {
            trigger.accessibility_controls(self.popover_id())
        } else {
            trigger
        }
    }
    /// Create the unstyled trigger part. Use [`Self::trigger_with`] to supply an existing element.
    pub fn trigger(self, id: impl Into<ElementId>) -> Element {
        self.trigger_with(id, crate::button())
    }

    /// Decorate the full-window portal and active modal focus boundary.
    pub fn root_with(self, root: Element) -> Element {
        root.id(self.root_id())
            .overlay()
            .inset_0()
            .size_full()
            .focus_trap()
            .restore_previous_focus()
            .app_region_no_drag()
            .cursor_default()
    }
    /// Create the unstyled root part. Use [`Self::root_with`] to supply an existing element.
    pub fn root(self) -> Element {
        self.root_with(crate::div())
    }

    /// Decorate the caller-owned visual backdrop.
    pub fn backdrop_with(self, backdrop: Element) -> Element {
        backdrop
            .id(self.backdrop_id())
            .absolute()
            .inset_0()
            .size_full()
            .app_region_no_drag()
            .cursor_default()
    }
    /// Create the unstyled backdrop part. Use [`Self::backdrop_with`] to supply an existing element.
    pub fn backdrop(self) -> Element {
        self.backdrop_with(crate::div())
    }

    /// Decorate the caller-owned modal popover.
    ///
    /// The popover is a negative-Tab-index focus fallback. Enabled descendant Tab stops are chosen
    /// first when the enclosing trap mounts. Title and description relationships project only
    /// when the corresponding parts are mounted, so incomplete compositions never emit dangling
    /// native node references.
    pub fn popup_with(self, popover: Element) -> Element {
        let mut popover = popover
            .id(self.popover_id())
            .track_focus(self.popover_focus())
            .tab_index(-1)
            .accessibility_role(self.kind.accessibility_role())
            .accessibility_modal(true)
            .accessibility_labelled_by(self.title_id())
            .accessibility_described_by(self.description_id())
            .app_region_no_drag()
            .cursor_default();
        if self.dismiss_on_escape {
            popover = popover.dismiss_on_escape();
        }
        if self.dismiss_on_backdrop {
            popover = popover.dismiss_on_pointer_outside();
        }
        if let Some(focus) = self.restore_focus {
            popover = popover.restore_focus_to(focus);
        }
        popover
    }
    /// Create the unstyled popup part. Use [`Self::popup_with`] to supply an existing element.
    pub fn popup(self) -> Element {
        self.popup_with(crate::div())
    }

    /// Decorate the caller-owned scrolling viewport the popup sits inside, Base UI's Viewport.
    ///
    /// A dialog taller than the window must scroll as one surface rather than clipping its own
    /// content, and the scroll must live outside the popup so the popup keeps its own padding and
    /// shadow. QuickGUI supplies the stable identity and the scroll container; size, alignment, and
    /// padding stay application-owned.
    pub fn viewport_with(self, viewport: Element) -> Element {
        viewport
            .id(self.viewport_id())
            .overflow_y_scroll()
            .app_region_no_drag()
    }
    /// Create the unstyled viewport part. Use [`Self::viewport_with`] to supply an existing element.
    pub fn viewport(self) -> Element {
        self.viewport_with(crate::div())
    }

    /// Assign the stable visible label target used by the popover.
    pub fn title_with(self, title: Element) -> Element {
        title.id(self.title_id())
    }
    /// Create the unstyled title part. Use [`Self::title_with`] to supply an existing element.
    pub fn title(self) -> Element {
        self.title_with(crate::div())
    }

    /// Assign the stable visible description target used by the popover.
    pub fn description_with(self, description: Element) -> Element {
        description.id(self.description_id())
    }
    /// Create the unstyled description part. Use [`Self::description_with`] to supply an existing element.
    pub fn description(self) -> Element {
        self.description_with(crate::div())
    }

    /// Decorate a caller-owned close control with button behavior and no visual defaults.
    pub fn close_with(self, label: impl Into<std::sync::Arc<str>>, close: Element) -> Element {
        close
            .id(self.close_id())
            .clickable()
            .accessibility_role(AccessibilityRole::Button)
            .accessibility_label(label)
            .app_region_no_drag()
            .user_select_none()
    }
    /// Create the unstyled close part. Use [`Self::close_with`] to supply an existing element.
    pub fn close(self, label: impl Into<std::sync::Arc<str>>) -> Element {
        self.close_with(label, crate::button())
    }
}

/// Controlled open state that keeps a dialog mounted for its own transition.
///
/// Base UI reports `onOpenChangeComplete` once an open or close transition has finished, so an
/// application can unmount content only after its exit animation has played. QuickGUI owns no
/// dialog animation — motion is application presentation — so this state owns the one thing the
/// framework can own exactly: the deadline. Declare the duration your own transition takes, drive
/// open and close through [`Self::set_open`], and mount the dialog while [`Self::is_mounted`] is
/// true.
///
/// Every deadline is an exact one-shot task. A zero duration completes in the same controlled
/// update with no task at all, and a settled dialog owns no timer, observer, or idle scheduler
/// source.
#[derive(Debug)]
pub struct DialogState {
    open: bool,
    mounted: bool,
    enter: Duration,
    exit: Duration,
    pending: Option<bool>,
    completed: Option<bool>,
    generation: u64,
    task: Option<Task<()>>,
}

impl Default for DialogState {
    fn default() -> Self {
        Self::new()
    }
}

impl DialogState {
    /// Create a closed dialog whose transitions complete immediately.
    pub const fn new() -> Self {
        Self {
            open: false,
            mounted: false,
            enter: Duration::ZERO,
            exit: Duration::ZERO,
            pending: None,
            completed: None,
            generation: 0,
            task: None,
        }
    }

    /// Declare how long the application's own opening transition takes.
    pub fn enter_duration(mut self, duration: Duration) -> Self {
        self.enter = duration.min(MAX_DIALOG_TRANSITION);
        self
    }

    /// Declare how long the application's own closing transition takes.
    ///
    /// The dialog stays mounted for exactly this long after it closes, which is what lets an exit
    /// animation play instead of the content vanishing.
    pub fn exit_duration(mut self, duration: Duration) -> Self {
        self.exit = duration.min(MAX_DIALOG_TRANSITION);
        self
    }

    /// Whether the dialog is open.
    pub const fn is_open(&self) -> bool {
        self.open
    }

    /// Whether the dialog should still be declared in the tree.
    ///
    /// This stays true through a closing transition, so mount [`Dialog::root_with`] against it
    /// rather than against [`Self::is_open`].
    pub const fn is_mounted(&self) -> bool {
        self.mounted
    }

    /// Whether a transition deadline is outstanding.
    pub const fn is_transitioning(&self) -> bool {
        self.pending.is_some()
    }

    /// The direction of the last transition that ran to completion.
    ///
    /// This is Base UI's `onOpenChangeComplete` value: `Some(true)` after an open finished,
    /// `Some(false)` after a close finished, and `None` before either has.
    pub const fn open_change_complete(&self) -> Option<bool> {
        self.completed
    }

    /// Change the open state and schedule the completion of the matching transition.
    ///
    /// This is the `fn`-pointer entry point for a view that owns one dialog per field; a host that
    /// renders many declared dialogs through one view uses [`Self::set_open_with`]. Returns whether
    /// the open state changed.
    pub fn set_open<V: 'static>(
        &mut self,
        open: bool,
        access: fn(&mut V) -> &mut Self,
        cx: &mut EventContext,
    ) -> bool {
        self.set_open_with(open, StateAccessor::from(access), cx)
    }

    /// Change the open state through a per-instance accessor.
    pub fn set_open_with<V: 'static>(
        &mut self,
        open: bool,
        access: StateAccessor<V, Self>,
        cx: &mut EventContext,
    ) -> bool {
        let changed = self.open != open;
        self.cancel_pending();
        self.open = open;
        if open {
            self.mounted = true;
        }
        let duration = if open { self.enter } else { self.exit };
        if duration.is_zero() {
            self.finish(open);
            return changed;
        }
        self.pending = Some(open);
        let generation = self.generation;
        let spawned = cx.spawn::<V, _, _, _>(move |task_cx: AsyncViewContext<V>| async move {
            if task_cx.sleep(duration).await.is_err() {
                return;
            }
            let _ = task_cx
                .update(move |view, cx| {
                    let state = access.get(view);
                    if state.generation != generation || state.pending != Some(open) {
                        return;
                    }
                    state.pending = None;
                    state.task = None;
                    state.finish(open);
                    cx.invalidate();
                })
                .await;
        });
        match spawned {
            Ok(task) => self.task = Some(task),
            Err(_) => {
                // A window that cannot own another foreground task still gets correct state; only
                // the exit transition is skipped.
                self.pending = None;
                self.finish(open);
            }
        }
        changed
    }

    /// Drop any outstanding transition deadline without changing the open state.
    pub fn cancel_pending(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.pending = None;
        if let Some(task) = self.task.take() {
            task.cancel();
        }
    }

    fn finish(&mut self, open: bool) {
        self.mounted = open;
        self.completed = Some(open);
    }
}

fn derived_dialog_id(parent: ElementId, tag: u64) -> ElementId {
    let mut hash = parent.as_u64() ^ tag;
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    if hash == 0 || hash == parent.as_u64() || hash == u64::MAX {
        hash ^= tag.rotate_left(17);
    }
    ElementId::new(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, IntoElement, TestAppContext, View, ViewContext, button, div, text};

    #[test]
    fn parts_add_exact_behavior_without_appearance() {
        let dialog = Dialog::new("settings", true).restore_focus_to("open-settings");
        let trigger = dialog.trigger_with("open-settings", div());
        assert_eq!(trigger.accessibility.role, AccessibilityRole::Button);
        assert_eq!(
            trigger.accessibility.has_popover,
            Some(AccessibilityPopover::Dialog)
        );
        assert_eq!(trigger.accessibility.expanded, Some(true));
        assert_eq!(
            trigger.accessibility.relations.controls(),
            Some(dialog.popover_id())
        );

        let root = dialog.root_with(div());
        assert!(root.focus_trap);
        assert!(root.restore_previous_focus);
        assert!(root.portal);
        assert!(root.blocks_pointer);
        assert_eq!(root.visual.background, None);
        assert_eq!(root.visual.border_color, None);

        let popover = dialog.popup_with(div());
        assert_eq!(popover.accessibility.role, AccessibilityRole::Dialog);
        assert!(popover.accessibility.modal);
        assert_eq!(
            popover.accessibility.relations.labelled_by(),
            Some(dialog.title_id())
        );
        assert_eq!(
            popover.accessibility.relations.described_by(),
            Some(dialog.description_id())
        );
        assert!(popover.dismiss_policy.on_escape());
        assert!(popover.dismiss_policy.on_pointer_outside());
        assert_eq!(popover.visual.background, None);
        assert_eq!(popover.visual.border_color, None);

        let alert = Dialog::alert("delete", true).popup_with(div());
        assert_eq!(alert.accessibility.role, AccessibilityRole::AlertDialog);
        assert!(alert.dismiss_policy.on_escape());
        assert!(!alert.dismiss_policy.on_pointer_outside());
    }

    #[test]
    fn derived_part_ids_are_stable_distinct_and_never_reuse_the_base() {
        let dialog = Dialog::new(0_u64, true);
        let ids = [
            dialog.root_id(),
            dialog.backdrop_id(),
            dialog.popover_id(),
            dialog.title_id(),
            dialog.description_id(),
            dialog.close_id(),
        ];
        for (index, id) in ids.iter().enumerate() {
            assert_ne!(*id, ElementId::new(0));
            assert_ne!(*id, ElementId::new(u64::MAX));
            assert!(!ids[..index].contains(id));
        }
        assert_eq!(dialog.popover_id(), Dialog::new(0_u64, false).popover_id());
    }

    #[derive(Default)]
    struct DialogView {
        open: bool,
        closed: usize,
    }

    impl View for DialogView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let dialog = Dialog::new("test-dialog", self.open)
                .initial_focus("dialog-first")
                .restore_focus_to("dialog-trigger");
            let open = cx.listener("dialog-trigger", move |view, cx| {
                view.open = true;
                dialog.focus_initial(cx);
                cx.invalidate();
            });
            let close = cx.listener(dialog.close_id(), move |view, cx| {
                view.open = false;
                view.closed += 1;
                dialog.focus_restore(cx);
                cx.invalidate();
            });
            let dismiss = cx.dismiss_listener(dialog.popover_id(), move |view, cx| {
                view.open = false;
                view.closed += 1;
                cx.invalidate();
            });

            let mut root = div()
                .size_full()
                .relative()
                .child(
                    dialog
                        .trigger_with("dialog-trigger", button().child("Open"))
                        .on_click(open),
                )
                .child(button().id("outside").child("Outside"));
            if self.open {
                let popover = dialog
                    .popup_with(
                        div()
                            .w(240.0)
                            .h(160.0)
                            .bg(Color::BLACK)
                            .child(dialog.title_with(text("Settings")))
                            .child(dialog.description_with(text("Change settings")))
                            .child(button().id("dialog-first").child("First"))
                            .child(button().id("dialog-second").child("Second"))
                            .child(dialog.close_with("Close settings", div()).on_click(close)),
                    )
                    .on_dismiss(dismiss);
                root = root.child(
                    dialog
                        .root_with(div().flex_row().items_center().justify_center())
                        .child(dialog.backdrop_with(div()))
                        .child(popover),
                );
            }
            root
        }
    }

    #[test]
    fn controlled_dialog_traps_tabs_restores_focus_and_sleeps() {
        let (mut cx, view) = TestAppContext::new(DialogView::default()).unwrap();
        let window = view.window_handle();
        cx.click(window, "dialog-trigger").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some("dialog-first".into()));

        cx.simulate_keystrokes(window, "tab").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some("dialog-second".into()));
        cx.simulate_keystrokes(window, "tab").unwrap();
        assert_eq!(
            cx.focused(window).unwrap(),
            Some(Dialog::new("test-dialog", true).close_id())
        );
        cx.simulate_keystrokes(window, "tab").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some("dialog-first".into()));
        assert!(cx.focus(window, "outside").is_err());

        cx.simulate_keystrokes(window, "escape").unwrap();
        assert!(!cx.read(view, |view| view.open).unwrap());
        assert_eq!(cx.read(view, |view| view.closed).unwrap(), 1);
        assert_eq!(cx.focused(window).unwrap(), Some("dialog-trigger".into()));

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }

    #[test]
    fn the_viewport_part_scrolls_the_dialog_without_adding_appearance() {
        let dialog = Dialog::new("confirm", true);
        let viewport = dialog.viewport_with(crate::div().bg(crate::Color::rgb8(1, 2, 3)));
        assert_eq!(viewport.explicit_id, Some(dialog.viewport_id()));
        assert_eq!(
            viewport.visual.background,
            Some(crate::Color::rgb8(1, 2, 3))
        );
        assert_eq!(viewport.visual.border_color, None);
        assert!(!viewport.focusable);
        assert!(!viewport.accessibility.modal);

        for other in [
            dialog.root_id(),
            dialog.backdrop_id(),
            dialog.popover_id(),
            dialog.title_id(),
            dialog.description_id(),
            dialog.close_id(),
        ] {
            assert_ne!(dialog.viewport_id(), other);
        }
        assert_eq!(
            dialog.viewport_id(),
            Dialog::new("confirm", false).viewport_id()
        );
    }

    #[derive(Default)]
    struct TransitionView {
        dialog: DialogState,
    }

    impl crate::View for TransitionView {
        fn render(&mut self, cx: &mut crate::ViewContext<'_, Self>) -> impl crate::IntoElement {
            let dialog = Dialog::new("confirm", self.dialog.is_open());
            let open = cx.listener("open", |view: &mut Self, cx| {
                view.dialog
                    .set_open(true, |view: &mut Self| &mut view.dialog, cx);
                cx.invalidate();
            });
            let close = cx.listener(dialog.close_id(), |view: &mut Self, cx| {
                view.dialog
                    .set_open(false, |view: &mut Self| &mut view.dialog, cx);
                cx.invalidate();
            });
            let mut root = crate::div()
                .size_full()
                .child(crate::button().id("open").child("Open").on_click(open));
            if self.dialog.is_mounted() {
                root = root.child(
                    dialog.root_with(crate::div()).child(
                        dialog.viewport_with(crate::div()).child(
                            dialog
                                .popup_with(crate::div())
                                .child(dialog.title_with(crate::text("Confirm")))
                                .child(dialog.close_with("Close", crate::div().on_click(close))),
                        ),
                    ),
                );
            }
            root
        }
    }

    #[test]
    fn a_dialog_stays_mounted_for_its_exit_transition_and_reports_completion() {
        let (mut cx, view) = crate::TestAppContext::new(TransitionView {
            dialog: DialogState::new()
                .enter_duration(Duration::from_millis(80))
                .exit_duration(Duration::from_millis(120)),
        })
        .unwrap();
        let window = view.window_handle();
        let dialog = Dialog::new("confirm", true);
        assert_eq!(
            cx.read(view, |view| view.dialog.open_change_complete())
                .unwrap(),
            None
        );

        cx.click(window, "open").unwrap();
        assert!(cx.read(view, |view| view.dialog.is_open()).unwrap());
        assert!(cx.read(view, |view| view.dialog.is_mounted()).unwrap());
        assert!(
            cx.read(view, |view| view.dialog.is_transitioning())
                .unwrap()
        );
        assert!(cx.contains_element(window, dialog.popover_id()).unwrap());
        assert!(cx.contains_element(window, dialog.viewport_id()).unwrap());

        cx.advance_time(Duration::from_millis(79)).unwrap();
        assert!(
            cx.read(view, |view| view.dialog.is_transitioning())
                .unwrap()
        );
        cx.advance_time(Duration::from_millis(1)).unwrap();
        assert!(
            !cx.read(view, |view| view.dialog.is_transitioning())
                .unwrap()
        );
        assert_eq!(
            cx.read(view, |view| view.dialog.open_change_complete())
                .unwrap(),
            Some(true)
        );

        // Closing keeps the dialog mounted for exactly the declared exit duration.
        cx.click(window, dialog.close_id()).unwrap();
        assert!(!cx.read(view, |view| view.dialog.is_open()).unwrap());
        assert!(cx.read(view, |view| view.dialog.is_mounted()).unwrap());
        assert!(cx.contains_element(window, dialog.popover_id()).unwrap());
        cx.advance_time(Duration::from_millis(119)).unwrap();
        assert!(cx.read(view, |view| view.dialog.is_mounted()).unwrap());
        cx.advance_time(Duration::from_millis(1)).unwrap();
        assert!(!cx.read(view, |view| view.dialog.is_mounted()).unwrap());
        assert_eq!(
            cx.read(view, |view| view.dialog.open_change_complete())
                .unwrap(),
            Some(false)
        );
        assert!(!cx.contains_element(window, dialog.popover_id()).unwrap());

        // A settled dialog owns no timer, task, or idle frame.
        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }

    #[test]
    fn zero_duration_transitions_complete_in_the_same_update_and_are_bounded() {
        let state = DialogState::new()
            .enter_duration(Duration::from_secs(3_600))
            .exit_duration(Duration::from_secs(3_600));
        assert_eq!(state.enter, MAX_DIALOG_TRANSITION);
        assert_eq!(state.exit, MAX_DIALOG_TRANSITION);

        let immediate = DialogState::new();
        assert!(!immediate.is_open());
        assert!(!immediate.is_mounted());
        assert!(!immediate.is_transitioning());
        assert_eq!(immediate.open_change_complete(), None);
    }
}
