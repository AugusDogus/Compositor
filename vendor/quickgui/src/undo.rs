//! Application-wide undo and redo with bounded, named, groupable entries.
//!
//! A text input owns its own bounded history, so QuickGUI routes Undo and Redo like AppKit does:
//! while a text input has keyboard focus the input's history handles both commands and the typed
//! [`Undo`]/[`Redo`] actions never fire. Everywhere else the application's [`UndoManager`] handles
//! them. See `docs/text-and-forms.md` for the complete policy.
//!
//! The manager retains only closures the application supplied plus bounded action names. It holds
//! no timer, task, or observer and performs no work while nothing is undone.

use std::{collections::VecDeque, fmt, rc::Rc, sync::Arc};

use crate::{EventContext, Global, KeyBinding};

/// Maximum undoable entries retained on each of the undo and redo stacks.
pub const MAX_UNDO_ENTRIES: usize = 256;

/// Maximum entries collected into one [`UndoManager::begin_group`] group.
pub const MAX_UNDO_GROUP_ENTRIES: usize = 1_024;

/// Maximum UTF-8 bytes retained for one action name.
pub const MAX_UNDO_ACTION_NAME_BYTES: usize = 256;

/// Undo the last registered change.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Undo;

/// Redo the last undone change.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Redo;

/// The conventional application bindings for [`Undo`] and [`Redo`].
///
/// A focused text input consumes these first through the framework's editor shortcut path, so the
/// actions reach application listeners only when no text input has focus.
pub fn undo_key_bindings() -> [KeyBinding; 3] {
    [
        KeyBinding::new("cmd-z", Undo, None),
        KeyBinding::new("cmd-shift-z", Redo, None),
        KeyBinding::new("ctrl-y", Redo, None),
    ]
}

type UndoCallback<C> = Rc<dyn Fn(&mut C)>;

/// One reversible application change.
///
/// Both callbacks are `Fn` rather than `FnOnce` so an entry survives repeated undo and redo cycles
/// without being rebuilt. Keep them cheap and free of blocking work: they run on the main thread
/// inside the event that requested the change.
pub struct UndoEntry<C = EventContext> {
    name: Arc<str>,
    undo: UndoCallback<C>,
    redo: UndoCallback<C>,
}

impl<C: 'static> UndoEntry<C> {
    /// Create one named entry.
    ///
    /// The name appears in `Undo <name>` and `Redo <name>` menu titles and is truncated at a UTF-8
    /// boundary to [`MAX_UNDO_ACTION_NAME_BYTES`].
    pub fn new(
        name: impl Into<Arc<str>>,
        undo: impl Fn(&mut C) + 'static,
        redo: impl Fn(&mut C) + 'static,
    ) -> Self {
        Self {
            name: bounded_name(name.into()),
            undo: Rc::new(undo),
            redo: Rc::new(redo),
        }
    }

    /// The bounded action name.
    pub fn name(&self) -> &Arc<str> {
        &self.name
    }
}

impl<C> Clone for UndoEntry<C> {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            undo: self.undo.clone(),
            redo: self.redo.clone(),
        }
    }
}

impl<C> fmt::Debug for UndoEntry<C> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UndoEntry")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// A value-based reversible change over one cloneable application value.
///
/// This is the ergonomic path when a change is "the value was `before`, now it is `after`": the
/// change retains both snapshots and [`Self::into_entry`] turns them into an [`UndoEntry`] with a
/// single accessor closure.
#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use = "an UndoableChange has no effect until it is registered"]
pub struct UndoableChange<T> {
    name: Arc<str>,
    before: T,
    after: T,
}

impl<T: Clone + 'static> UndoableChange<T> {
    /// Retain one named before/after pair.
    pub fn new(name: impl Into<Arc<str>>, before: T, after: T) -> Self {
        Self {
            name: bounded_name(name.into()),
            before,
            after,
        }
    }

    /// The bounded action name.
    pub fn name(&self) -> &Arc<str> {
        &self.name
    }

    /// The value before the change.
    pub const fn before(&self) -> &T {
        &self.before
    }

    /// The value after the change.
    pub const fn after(&self) -> &T {
        &self.after
    }

    /// Turn the pair into a reversible entry driven by one accessor.
    pub fn into_entry<C: 'static>(self, apply: impl Fn(&mut C, &T) + 'static) -> UndoEntry<C> {
        let apply = Rc::new(apply);
        let undo_apply = apply.clone();
        let before = self.before;
        let after = self.after;
        UndoEntry::new(
            self.name,
            move |context: &mut C| undo_apply(context, &before),
            move |context: &mut C| apply(context, &after),
        )
    }
}

#[derive(Debug)]
struct OpenGroup<C> {
    name: Arc<str>,
    entries: Vec<UndoEntry<C>>,
    truncated: bool,
}

/// Bounded application-wide undo and redo history.
///
/// Store one per document, or install it as a [`Global`] when the application has a single
/// undoable surface.
pub struct UndoManager<C = EventContext> {
    undo: VecDeque<UndoEntry<C>>,
    redo: VecDeque<UndoEntry<C>>,
    group: Option<OpenGroup<C>>,
}

impl<C: 'static> Default for UndoManager<C> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C: 'static> Global for UndoManager<C> {}

impl<C> fmt::Debug for UndoManager<C> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UndoManager")
            .field("undo", &self.undo.len())
            .field("redo", &self.redo.len())
            .field("grouping", &self.group.is_some())
            .finish()
    }
}

impl<C: 'static> UndoManager<C> {
    /// Create an empty history.
    pub fn new() -> Self {
        Self {
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            group: None,
        }
    }

    /// Record one reversible change.
    ///
    /// Registering clears the redo stack, exactly like a new edit in a text input. While a group is
    /// open the entry joins the group instead of the undo stack.
    pub fn register(&mut self, entry: UndoEntry<C>) {
        if let Some(group) = &mut self.group {
            if group.entries.len() == MAX_UNDO_GROUP_ENTRIES {
                group.truncated = true;
                return;
            }
            group.entries.push(entry);
            return;
        }
        push_bounded(&mut self.undo, entry);
        self.redo.clear();
    }

    /// Start collecting registered changes into one named entry.
    ///
    /// Nested calls are rejected so a mismatched [`Self::end_group`] can never merge unrelated
    /// edits. Returns whether the group was opened.
    pub fn begin_group(&mut self, name: impl Into<Arc<str>>) -> bool {
        if self.group.is_some() {
            return false;
        }
        self.group = Some(OpenGroup {
            name: bounded_name(name.into()),
            entries: Vec::new(),
            truncated: false,
        });
        true
    }

    /// Close the open group and push it as one undoable entry.
    ///
    /// An empty group is discarded. Returns whether an entry was pushed.
    pub fn end_group(&mut self) -> bool {
        let Some(group) = self.group.take() else {
            return false;
        };
        if group.entries.is_empty() {
            return false;
        }
        let undo_entries = group.entries.clone();
        let redo_entries = group.entries;
        let entry = UndoEntry {
            name: group.name,
            undo: Rc::new(move |context: &mut C| {
                for entry in undo_entries.iter().rev() {
                    (entry.undo)(context);
                }
            }),
            redo: Rc::new(move |context: &mut C| {
                for entry in &redo_entries {
                    (entry.redo)(context);
                }
            }),
        };
        push_bounded(&mut self.undo, entry);
        self.redo.clear();
        true
    }

    /// Discard an open group without recording it.
    pub fn cancel_group(&mut self) -> bool {
        self.group.take().is_some()
    }

    /// Whether a group is currently collecting changes.
    pub const fn is_grouping(&self) -> bool {
        self.group.is_some()
    }

    /// Whether an undoable entry exists.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether a redoable entry exists.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The name of the next undoable entry, for an `Undo <name>` menu title.
    pub fn undo_action_name(&self) -> Option<&Arc<str>> {
        self.undo.back().map(UndoEntry::name)
    }

    /// The name of the next redoable entry, for a `Redo <name>` menu title.
    pub fn redo_action_name(&self) -> Option<&Arc<str>> {
        self.redo.back().map(UndoEntry::name)
    }

    /// Retained undoable entries.
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    /// Retained redoable entries.
    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }

    /// Drop every retained entry and any open group.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.group = None;
    }

    /// Undo the most recent change.
    pub fn undo(&mut self, context: &mut C) -> bool {
        let Some(entry) = self.undo.pop_back() else {
            return false;
        };
        (entry.undo)(context);
        push_bounded(&mut self.redo, entry);
        true
    }

    /// Redo the most recently undone change.
    pub fn redo(&mut self, context: &mut C) -> bool {
        let Some(entry) = self.redo.pop_back() else {
            return false;
        };
        (entry.redo)(context);
        push_bounded(&mut self.undo, entry);
        true
    }

    /// Undo only when a focused text input did not already claim the command.
    ///
    /// Pass the value of the framework's focus check so the routing policy stays in one place.
    pub fn undo_unless_handled(&mut self, handled_by_text_input: bool, context: &mut C) -> bool {
        !handled_by_text_input && self.undo(context)
    }

    /// Redo only when a focused text input did not already claim the command.
    pub fn redo_unless_handled(&mut self, handled_by_text_input: bool, context: &mut C) -> bool {
        !handled_by_text_input && self.redo(context)
    }
}

fn push_bounded<C>(stack: &mut VecDeque<UndoEntry<C>>, entry: UndoEntry<C>) {
    while stack.len() >= MAX_UNDO_ENTRIES {
        stack.pop_front();
    }
    stack.push_back(entry);
}

fn bounded_name(name: Arc<str>) -> Arc<str> {
    if name.len() <= MAX_UNDO_ACTION_NAME_BYTES {
        return name;
    }
    let mut end = MAX_UNDO_ACTION_NAME_BYTES;
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    Arc::from(&name[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Document {
        text: String,
        applied: usize,
    }

    fn set_text(name: &str, before: &str, after: &str) -> UndoEntry<Document> {
        UndoableChange::new(name, before.to_owned(), after.to_owned()).into_entry(
            |document: &mut Document, value: &String| {
                document.text = value.clone();
                document.applied += 1;
            },
        )
    }

    #[test]
    fn undo_and_redo_replay_named_changes() {
        let mut manager = UndoManager::<Document>::new();
        let mut document = Document::default();
        assert!(!manager.can_undo());
        assert!(manager.undo_action_name().is_none());

        document.text = "one".to_owned();
        manager.register(set_text("Typing", "", "one"));
        document.text = "one two".to_owned();
        manager.register(set_text("Paste", "one", "one two"));

        assert_eq!(manager.undo_action_name().map(Arc::as_ref), Some("Paste"));
        assert!(manager.undo(&mut document));
        assert_eq!(document.text, "one");
        assert_eq!(manager.redo_action_name().map(Arc::as_ref), Some("Paste"));
        assert_eq!(manager.undo_action_name().map(Arc::as_ref), Some("Typing"));

        assert!(manager.redo(&mut document));
        assert_eq!(document.text, "one two");
        assert!(manager.undo(&mut document));
        assert!(manager.undo(&mut document));
        assert_eq!(document.text, "");
        assert!(!manager.undo(&mut document));
    }

    #[test]
    fn registering_a_change_clears_the_redo_stack() {
        let mut manager = UndoManager::<Document>::new();
        let mut document = Document::default();
        manager.register(set_text("Typing", "", "a"));
        assert!(manager.undo(&mut document));
        assert!(manager.can_redo());

        manager.register(set_text("Typing", "", "b"));
        assert!(!manager.can_redo());
        assert!(manager.redo_action_name().is_none());
    }

    #[test]
    fn groups_undo_and_redo_as_one_named_entry_in_the_right_order() {
        let order = Rc::new(RefCell::new(Vec::new()));
        let mut manager = UndoManager::<Document>::new();
        let mut document = Document::default();

        assert!(manager.begin_group("Format Table"));
        assert!(!manager.begin_group("Nested"));
        assert!(manager.is_grouping());
        for step in 0..3_usize {
            let order = order.clone();
            let undo_order = order.clone();
            manager.register(UndoEntry::new(
                "step",
                move |_: &mut Document| undo_order.borrow_mut().push(format!("undo {step}")),
                move |_: &mut Document| order.borrow_mut().push(format!("redo {step}")),
            ));
        }
        assert_eq!(manager.undo_len(), 0);
        assert!(manager.end_group());
        assert_eq!(manager.undo_len(), 1);
        assert_eq!(
            manager.undo_action_name().map(Arc::as_ref),
            Some("Format Table")
        );

        assert!(manager.undo(&mut document));
        assert_eq!(
            order.borrow().as_slice(),
            ["undo 2", "undo 1", "undo 0"].map(str::to_owned)
        );
        order.borrow_mut().clear();
        assert!(manager.redo(&mut document));
        assert_eq!(
            order.borrow().as_slice(),
            ["redo 0", "redo 1", "redo 2"].map(str::to_owned)
        );
    }

    #[test]
    fn empty_and_cancelled_groups_record_nothing() {
        let mut manager = UndoManager::<Document>::new();
        assert!(manager.begin_group("Empty"));
        assert!(!manager.end_group());
        assert_eq!(manager.undo_len(), 0);

        assert!(manager.begin_group("Cancelled"));
        manager.register(set_text("step", "", "x"));
        assert!(manager.cancel_group());
        assert_eq!(manager.undo_len(), 0);
        assert!(!manager.cancel_group());
        assert!(!manager.end_group());
    }

    #[test]
    fn history_and_names_stay_bounded() {
        let mut manager = UndoManager::<Document>::new();
        for index in 0..(MAX_UNDO_ENTRIES + 64) {
            manager.register(set_text("Typing", "", &index.to_string()));
        }
        assert_eq!(manager.undo_len(), MAX_UNDO_ENTRIES);

        let long = "é".repeat(MAX_UNDO_ACTION_NAME_BYTES);
        manager.register(set_text(&long, "", ""));
        let name = manager.undo_action_name().unwrap();
        assert!(name.len() <= MAX_UNDO_ACTION_NAME_BYTES);
        assert!(std::str::from_utf8(name.as_bytes()).is_ok());

        manager.begin_group("Wide");
        for index in 0..(MAX_UNDO_GROUP_ENTRIES + 8) {
            manager.register(set_text("step", "", &index.to_string()));
        }
        assert!(manager.end_group());
        assert_eq!(manager.undo_action_name().map(Arc::as_ref), Some("Wide"));

        manager.clear();
        assert!(!manager.can_undo());
        assert!(!manager.can_redo());
        assert!(!manager.is_grouping());
    }

    #[test]
    fn routing_defers_to_a_focused_text_input() {
        let mut manager = UndoManager::<Document>::new();
        let mut document = Document::default();
        manager.register(set_text("Typing", "", "a"));

        assert!(!manager.undo_unless_handled(true, &mut document));
        assert_eq!(manager.undo_len(), 1);
        assert!(manager.undo_unless_handled(false, &mut document));
        assert_eq!(manager.undo_len(), 0);

        assert!(!manager.redo_unless_handled(true, &mut document));
        assert!(manager.redo_unless_handled(false, &mut document));
        assert_eq!(document.applied, 2);
    }

    #[test]
    fn undo_key_bindings_cover_both_platform_conventions() {
        let bindings = undo_key_bindings();
        assert_eq!(bindings.len(), 3);
        assert!(
            bindings
                .iter()
                .any(|binding| binding.action().downcast_ref::<Undo>().is_some())
        );
        assert_eq!(
            bindings
                .iter()
                .filter(|binding| binding.action().downcast_ref::<Redo>().is_some())
                .count(),
            2
        );
    }
}
