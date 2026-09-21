use std::{fmt, sync::Arc};

use thiserror::Error;

use crate::{
    AccessibilityRole, Element, ElementId, EventContext, FocusHandle, IntoElement, KeyBinding,
    ListOffset, ListState, StateAccessor, ViewContext, div,
};

/// Maximum nodes retained by one tree state.
pub const MAX_TREE_NODES: usize = 1_000_000;
/// Maximum hierarchical depth accepted by one tree.
pub const MAX_TREE_DEPTH: usize = 256;
/// Maximum UTF-8 bytes retained for one tree node label.
pub const MAX_TREE_LABEL_BYTES: usize = 64 * 1024;
/// Maximum UTF-8 label bytes retained by one complete tree.
pub const MAX_TREE_TEXT_BYTES: usize = 16 * 1024 * 1024;

/// Default accessible name of the placeholder row shown while children load.
pub const DEFAULT_TREE_LOADING_LABEL: &str = "Loading…";

const TREE_KEY_CONTEXT: &str = "Tree";
const TREE_ROW_ID_TAG: u64 = 0x7d5d_5f29_58ab_f1f7;
const TREE_DISCLOSURE_ID_TAG: u64 = 0xb55a_8c0a_f31f_00e9;
const TREE_LOADING_ID_TAG: u64 = 0x2c8f_47b1_e05d_9a34;
const TREE_VISIBLE_UNSET: u32 = u32::MAX;
/// Marks one visible row as the loading placeholder of the entry it wraps.
///
/// Node indices are bounded by [`MAX_TREE_NODES`], so the top bit of the compact 32-bit visible
/// index is free and the placeholder costs no extra vector.
const TREE_PLACEHOLDER_FLAG: u32 = 0x8000_0000;

/// Move to the previous visible enabled tree item.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TreePrevious;
/// Move to the next visible enabled tree item.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TreeNext;
/// Collapse an expanded branch, otherwise move to its parent.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TreeCollapseOrParent;
/// Expand a collapsed branch, otherwise move to its first visible child.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TreeExpandOrChild;
/// Move to the first visible enabled tree item.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TreeFirst;
/// Move to the final visible enabled tree item.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TreeLast;
/// Move one visible page upward.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TreePageUp;
/// Move one visible page downward.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TreePageDown;
/// Toggle the expansion state of the selected branch.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TreeToggle;
/// Activate the selected item.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TreeActivate;

/// Dispatched by the tree when expanding a branch whose children are not loaded yet.
///
/// The tree owns no loader: it shows a bounded placeholder row and asks the application once.
/// Answer with [`TreeState::set_children`], which validates and splices atomically.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TreeLoadChildren {
    pub node: ElementId,
}

/// Contextual bindings used by [`TreeState::element`].
pub fn tree_key_bindings() -> [KeyBinding; 10] {
    [
        KeyBinding::new("up", TreePrevious, Some(TREE_KEY_CONTEXT)),
        KeyBinding::new("down", TreeNext, Some(TREE_KEY_CONTEXT)),
        KeyBinding::new("left", TreeCollapseOrParent, Some(TREE_KEY_CONTEXT)),
        KeyBinding::new("right", TreeExpandOrChild, Some(TREE_KEY_CONTEXT)),
        KeyBinding::new("platform-up", TreeFirst, Some(TREE_KEY_CONTEXT)),
        KeyBinding::new("platform-down", TreeLast, Some(TREE_KEY_CONTEXT)),
        KeyBinding::new("pageup", TreePageUp, Some(TREE_KEY_CONTEXT)),
        KeyBinding::new("pagedown", TreePageDown, Some(TREE_KEY_CONTEXT)),
        KeyBinding::new("space", TreeToggle, Some(TREE_KEY_CONTEXT)),
        KeyBinding::new("enter", TreeActivate, Some(TREE_KEY_CONTEXT)),
    ]
}

/// Application value and hierarchy retained by a [`TreeState`].
#[derive(Clone, Debug)]
pub struct TreeNode<T> {
    id: ElementId,
    label: Arc<str>,
    value: T,
    children: Vec<TreeNode<T>>,
    disabled: bool,
    pending: bool,
}

impl<T> TreeNode<T> {
    pub fn new(id: impl Into<ElementId>, label: impl Into<Arc<str>>, value: T) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            value,
            children: Vec::new(),
            disabled: false,
            pending: false,
        }
    }

    /// Declare a branch whose children have not been loaded yet.
    ///
    /// A pending node expands like any other branch. The first expansion mounts one placeholder
    /// row and dispatches [`TreeLoadChildren`]; supplying children through
    /// [`TreeState::set_children`] clears the pending flag. A node that already carries children
    /// is an ordinary branch and ignores this flag.
    pub fn pending(mut self, pending: bool) -> Self {
        self.pending = pending;
        self
    }

    pub const fn is_pending(&self) -> bool {
        self.pending
    }

    pub fn child(mut self, child: TreeNode<T>) -> Self {
        self.children.push(child);
        self
    }

    pub fn children(mut self, children: impl IntoIterator<Item = TreeNode<T>>) -> Self {
        self.children.extend(children);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub const fn id(&self) -> ElementId {
        self.id
    }

    pub fn label(&self) -> &Arc<str> {
        &self.label
    }

    pub fn value(&self) -> &T {
        &self.value
    }

    pub fn child_nodes(&self) -> &[TreeNode<T>] {
        &self.children
    }

    pub const fn is_disabled(&self) -> bool {
        self.disabled
    }
}

/// A tree source exceeded a hard identity, depth, or retained-text boundary.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum TreeError {
    #[error("a tree supports at most {limit} nodes")]
    TooManyNodes { limit: usize },
    #[error("tree depth {depth} exceeds the limit of {limit}")]
    TooDeep { depth: usize, limit: usize },
    #[error("tree node {id:?} has a duplicate stable ID")]
    DuplicateId { id: ElementId },
    #[error("tree node {id:?} has {bytes} label bytes; the limit is {limit}")]
    LabelTooLong {
        id: ElementId,
        bytes: usize,
        limit: usize,
    },
    #[error("tree labels retain {bytes} bytes; the total limit is {limit}")]
    TextBudgetExceeded { bytes: usize, limit: usize },
}

/// Structural geometry retained by one virtualized tree.
///
/// Indentation, disclosure size, padding, colors, typography, borders, radii, opacity, and motion
/// all belong to the caller-owned row returned from [`TreeState::element`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TreeLayout {
    pub row_height: f32,
}

impl TreeLayout {
    pub fn new(row_height: f32) -> Self {
        Self {
            row_height: finite_clamped(row_height, 20.0, 256.0, 30.0),
        }
    }

    pub fn row_height(mut self, height: f32) -> Self {
        self.row_height = finite_clamped(height, 20.0, 256.0, 30.0);
        self
    }

    fn sanitized(mut self) -> Self {
        self.row_height = finite_clamped(self.row_height, 20.0, 256.0, 30.0);
        self
    }
}

impl Default for TreeLayout {
    fn default() -> Self {
        Self::new(30.0)
    }
}

struct TreeEntry<T> {
    id: ElementId,
    label: Arc<str>,
    value: T,
    parent: Option<u32>,
    level: u32,
    position_in_set: u32,
    size_of_set: u32,
    subtree_end: u32,
    child_count: u32,
    disabled: bool,
    pending: bool,
}

struct TreeArena<T> {
    entries: Vec<TreeEntry<T>>,
    id_index: Vec<(u64, u32)>,
    root_count: usize,
}

/// A borrowed visible tree row passed to application rendering.
pub struct TreeRow<'a, T> {
    entry: &'a TreeEntry<T>,
    expanded: bool,
    selected: bool,
    loading: Option<&'a Arc<str>>,
}

impl<T> Clone for TreeRow<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

/// A row is a bundle of borrows, so it is copyable whatever the application value is.
impl<T> Copy for TreeRow<'_, T> {}

impl<T> fmt::Debug for TreeRow<'_, T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TreeRow")
            .field("id", &self.entry.id)
            .field("label", &self.entry.label)
            .field("level", &self.entry.level)
            .field("expanded", &self.expanded)
            .field("selected", &self.selected)
            .field("disabled", &self.entry.disabled)
            .field("loading", &self.loading.is_some())
            .finish_non_exhaustive()
    }
}

impl<'a, T> TreeRow<'a, T> {
    /// The stable node ID of this row, or of the branch a loading placeholder belongs to.
    pub const fn id(self) -> ElementId {
        self.entry.id
    }

    /// The node's accessible label, or the loading label for a placeholder row.
    pub fn label(self) -> &'a Arc<str> {
        match self.loading {
            Some(label) => label,
            None => &self.entry.label,
        }
    }

    /// The application value of this node, or of the branch a loading placeholder belongs to.
    pub fn value(self) -> &'a T {
        &self.entry.value
    }

    pub const fn level(self) -> usize {
        match self.loading {
            Some(_) => self.entry.level as usize + 1,
            None => self.entry.level as usize,
        }
    }

    pub const fn position_in_set(self) -> usize {
        match self.loading {
            Some(_) => 0,
            None => self.entry.position_in_set as usize,
        }
    }

    pub const fn size_of_set(self) -> usize {
        match self.loading {
            Some(_) => 1,
            None => self.entry.size_of_set as usize,
        }
    }

    pub const fn has_children(self) -> bool {
        self.loading.is_none() && (self.entry.child_count != 0 || self.entry.pending)
    }

    /// Whether this row is the placeholder mounted while a branch's children load.
    pub const fn is_loading(self) -> bool {
        self.loading.is_some()
    }

    /// Whether this branch's children have not been supplied yet.
    pub const fn is_pending(self) -> bool {
        self.loading.is_none() && self.entry.pending
    }

    pub const fn is_expanded(self) -> bool {
        self.loading.is_none() && self.expanded
    }

    pub const fn is_selected(self) -> bool {
        self.loading.is_none() && self.selected
    }

    /// A loading placeholder is never selectable, so it always reports as disabled.
    pub const fn is_disabled(self) -> bool {
        self.loading.is_some() || self.entry.disabled
    }
}

/// Retained hierarchy, expansion, selection, and virtual-scroll state for a tree view.
///
/// Source nodes are flattened once into a bounded preorder arena. Expansion rebuilds only a lean
/// visible-index vector; ordinary frames and clean idle periods perform no hierarchy walk, timer,
/// or polling. Rows are mounted through the existing sparse [`ListState`] path.
pub struct TreeState<T> {
    entries: Vec<TreeEntry<T>>,
    id_index: Vec<(u64, u32)>,
    expanded: Vec<bool>,
    visible: Vec<u32>,
    visible_position: Vec<u32>,
    selected: Option<u32>,
    root_count: usize,
    loading_label: Arc<str>,
    list: ListState,
    layout: TreeLayout,
}

impl<T> fmt::Debug for TreeState<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TreeState")
            .field("nodes", &self.entries.len())
            .field("visible", &self.visible.len())
            .field(
                "expanded",
                &self.expanded.iter().filter(|value| **value).count(),
            )
            .field("selected", &self.selected_id())
            .field("root_count", &self.root_count)
            .field("list", &self.list)
            .field("layout", &self.layout)
            .finish()
    }
}

impl<T> TreeState<T> {
    pub fn new(nodes: impl IntoIterator<Item = TreeNode<T>>) -> Result<Self, TreeError> {
        let arena = build_tree_arena(nodes)?;
        let layout = TreeLayout::default();
        let mut state = Self {
            expanded: vec![false; arena.entries.len()],
            visible: Vec::with_capacity(arena.root_count.min(256)),
            visible_position: vec![TREE_VISIBLE_UNSET; arena.entries.len()],
            selected: None,
            root_count: arena.root_count,
            loading_label: Arc::from(DEFAULT_TREE_LOADING_LABEL),
            list: ListState::new(0, layout.row_height).with_overscan(2),
            layout,
            entries: arena.entries,
            id_index: arena.id_index,
        };
        state.rebuild_visible();
        Ok(state)
    }

    pub fn with_layout(mut self, layout: TreeLayout) -> Self {
        self.layout = layout.sanitized();
        self.list = ListState::new(self.visible.len(), self.layout.row_height).with_overscan(2);
        self
    }

    /// Replace the accessible name of the placeholder row shown while children load.
    ///
    /// Longer text than [`MAX_TREE_LABEL_BYTES`] keeps [`DEFAULT_TREE_LOADING_LABEL`].
    pub fn with_loading_label(mut self, label: impl Into<Arc<str>>) -> Self {
        let label = label.into();
        if label.len() <= MAX_TREE_LABEL_BYTES {
            self.loading_label = label;
        }
        self
    }

    pub fn loading_label(&self) -> &Arc<str> {
        &self.loading_label
    }

    /// The entry index behind one visible row, or `None` for a loading placeholder.
    fn visible_entry(&self, visible_index: usize) -> Option<usize> {
        let value = *self.visible.get(visible_index)?;
        (value & TREE_PLACEHOLDER_FLAG == 0).then_some(value as usize)
    }

    /// The entry a visible row belongs to, including the parent of a loading placeholder.
    fn visible_owner(&self, visible_index: usize) -> Option<usize> {
        let value = *self.visible.get(visible_index)?;
        Some((value & !TREE_PLACEHOLDER_FLAG) as usize)
    }

    fn is_placeholder(&self, visible_index: usize) -> bool {
        self.visible
            .get(visible_index)
            .is_some_and(|value| value & TREE_PLACEHOLDER_FLAG != 0)
    }

    /// Whether keyboard or pointer selection may land on one visible row.
    fn is_selectable(&self, visible_index: usize) -> bool {
        self.visible_entry(visible_index)
            .is_some_and(|index| !self.entries[index].disabled)
    }

    /// Whether one branch is expanded with its children still outstanding.
    pub fn is_loading(&self, id: impl Into<ElementId>) -> bool {
        self.index_for_id(id.into()).is_some_and(|index| {
            self.entries[index].pending
                && self.entries[index].child_count == 0
                && self.expanded[index]
        })
    }

    /// Whether one branch still has to load its children.
    pub fn is_pending(&self, id: impl Into<ElementId>) -> bool {
        self.index_for_id(id.into())
            .is_some_and(|index| self.entries[index].pending)
    }

    pub const fn layout(&self) -> TreeLayout {
        self.layout
    }

    pub fn set_layout(&mut self, layout: TreeLayout) -> bool {
        let layout = layout.sanitized();
        if self.layout == layout {
            return false;
        }
        if self.layout.row_height != layout.row_height {
            let viewport = self.list.viewport_size();
            let offset = self.list.logical_scroll_top();
            self.list = ListState::new(self.visible.len(), layout.row_height).with_overscan(2);
            self.list.set_viewport_size(viewport.width, viewport.height);
            self.list.scroll_to(offset);
        }
        self.layout = layout;
        true
    }

    pub fn set_nodes(
        &mut self,
        nodes: impl IntoIterator<Item = TreeNode<T>>,
    ) -> Result<(), TreeError> {
        let arena = build_tree_arena(nodes)?;
        let selected_id = self.selected_id();
        let previous_offset = self.list.logical_scroll_top();
        let previous_top = self
            .visible_owner(previous_offset.item_ix)
            .map(|index| self.entries[index].id);
        let expanded_ids = self
            .entries
            .iter()
            .zip(&self.expanded)
            .filter_map(|(entry, expanded)| (*expanded).then_some(entry.id))
            .collect::<Vec<_>>();

        self.entries = arena.entries;
        self.id_index = arena.id_index;
        self.root_count = arena.root_count;
        self.expanded.clear();
        self.expanded.resize(self.entries.len(), false);
        self.visible_position.clear();
        self.visible_position
            .resize(self.entries.len(), TREE_VISIBLE_UNSET);
        for id in expanded_ids {
            if let Some(index) = self.index_for_id(id)
                && self.entries[index].child_count != 0
            {
                self.expanded[index] = true;
            }
        }
        self.selected = selected_id.and_then(|id| self.index_for_id(id).map(|index| index as u32));
        self.rebuild_visible_from_anchor(previous_offset, previous_top);
        Ok(())
    }

    pub fn node_count(&self) -> usize {
        self.entries.len()
    }

    pub fn visible_count(&self) -> usize {
        self.visible.len()
    }

    pub const fn root_count(&self) -> usize {
        self.root_count
    }

    pub fn selected_id(&self) -> Option<ElementId> {
        self.selected.map(|index| self.entries[index as usize].id)
    }

    pub fn selected_value(&self) -> Option<&T> {
        self.selected
            .map(|index| &self.entries[index as usize].value)
    }

    pub fn row(&self, visible_index: usize) -> Option<TreeRow<'_, T>> {
        let placeholder = self.is_placeholder(visible_index);
        let index = self.visible_owner(visible_index)?;
        Some(TreeRow {
            entry: &self.entries[index],
            expanded: self.expanded[index],
            selected: self.selected == Some(index as u32),
            loading: placeholder.then_some(&self.loading_label),
        })
    }

    pub fn visible_rows(&self) -> std::ops::Range<usize> {
        self.list.visible_rows().range
    }

    pub fn list_state(&self) -> &ListState {
        &self.list
    }

    pub fn is_expanded(&self, id: impl Into<ElementId>) -> bool {
        self.index_for_id(id.into())
            .is_some_and(|index| self.expanded[index])
    }

    pub fn set_expanded(&mut self, id: impl Into<ElementId>, expanded: bool) -> bool {
        let Some(index) = self.index_for_id(id.into()) else {
            return false;
        };
        let branch = self.entries[index].child_count != 0 || self.entries[index].pending;
        if !branch || self.expanded[index] == expanded {
            return false;
        }
        self.expanded[index] = expanded;
        self.rebuild_visible();
        true
    }

    pub fn toggle_expanded(&mut self, id: impl Into<ElementId>) -> bool {
        let id = id.into();
        let Some(index) = self.index_for_id(id) else {
            return false;
        };
        self.set_expanded(id, !self.expanded[index])
    }

    pub fn select(&mut self, id: impl Into<ElementId>) -> bool {
        let Some(index) = self.index_for_id(id.into()) else {
            return false;
        };
        let visible = self.visible_position[index];
        if visible == TREE_VISIBLE_UNSET || self.entries[index].disabled {
            return false;
        }
        self.select_visible(visible as usize)
    }

    pub fn focus_handle(id: impl Into<ElementId>) -> FocusHandle {
        FocusHandle::new(id)
    }

    pub fn row_id(id: impl Into<ElementId>, node: ElementId) -> ElementId {
        derived_tree_id(id.into(), TREE_ROW_ID_TAG, node.as_u64())
    }

    pub fn disclosure_id(id: impl Into<ElementId>, node: ElementId) -> ElementId {
        derived_tree_id(id.into(), TREE_DISCLOSURE_ID_TAG, node.as_u64())
    }

    /// The stable identity of the placeholder row mounted while one branch loads.
    pub fn loading_row_id(id: impl Into<ElementId>, node: ElementId) -> ElementId {
        derived_tree_id(id.into(), TREE_LOADING_ID_TAG, node.as_u64())
    }

    /// Supply the children of a pending branch in one atomic, validated splice.
    ///
    /// The replacement is built and validated before any live state changes: an over-deep, oversize,
    /// or duplicate-ID payload leaves the tree exactly as it was. On success the branch stops being
    /// pending, its placeholder row disappears, and selection, expansion, and the logical scroll
    /// anchor are preserved. An unknown node ID reports `Ok(false)`.
    pub fn set_children(
        &mut self,
        id: impl Into<ElementId>,
        children: impl IntoIterator<Item = TreeNode<T>>,
    ) -> Result<bool, TreeError> {
        let id = id.into();
        let Some(target) = self.index_for_id(id) else {
            return Ok(false);
        };
        if self.entries[target].child_count != 0 {
            return Ok(false);
        }
        let children = children.into_iter().collect::<Vec<_>>();
        let child_count = children.len();
        let level = self.entries[target].level as usize + 1;
        let mut total_text_bytes = self.entries.iter().fold(0usize, |bytes, entry| {
            bytes.saturating_add(entry.label.len())
        });
        let mut added = Vec::with_capacity(child_count);
        for (position, child) in children.into_iter().enumerate() {
            push_tree_node(
                child,
                None,
                level,
                position,
                child_count,
                &mut added,
                &mut total_text_bytes,
            )?;
        }
        if self.entries.len().saturating_add(added.len()) > MAX_TREE_NODES {
            return Err(TreeError::TooManyNodes {
                limit: MAX_TREE_NODES,
            });
        }

        let insert_at = self.entries[target].subtree_end as usize;
        let shift = added.len() as u32;
        let mut id_index = self
            .entries
            .iter()
            .map(|entry| entry.id)
            .chain(added.iter().map(|entry| entry.id))
            .enumerate()
            .map(|(index, id)| (id.as_u64(), index as u32))
            .collect::<Vec<_>>();
        id_index.sort_unstable_by_key(|(id, _)| *id);
        if let Some(duplicate) = id_index.windows(2).find(|pair| pair[0].0 == pair[1].0) {
            return Err(TreeError::DuplicateId {
                id: ElementId::new(duplicate[0].0),
            });
        }

        // Every check passed: from here the splice cannot fail.
        let previous_offset = self.list.logical_scroll_top();
        let previous_top = self
            .visible_owner(previous_offset.item_ix)
            .map(|index| self.entries[index].id);
        let offset = insert_at as u32;
        for entry in &mut added {
            entry.parent = Some(entry.parent.map_or(target as u32, |parent| parent + offset));
            entry.subtree_end += offset;
        }
        for entry in &mut self.entries {
            if entry.subtree_end >= offset {
                entry.subtree_end += shift;
            }
            if let Some(parent) = entry.parent
                && parent >= offset
            {
                entry.parent = Some(parent + shift);
            }
        }
        self.entries[target].child_count = shift;
        self.entries[target].pending = false;
        self.entries.splice(insert_at..insert_at, added);
        if self.selected.is_some_and(|selected| selected >= offset) {
            self.selected = self.selected.map(|selected| selected + shift);
        }
        self.expanded.splice(
            insert_at..insert_at,
            std::iter::repeat_n(false, shift as usize),
        );
        self.visible_position.clear();
        self.visible_position
            .resize(self.entries.len(), TREE_VISIBLE_UNSET);
        self.id_index = self
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.id.as_u64(), index as u32))
            .collect();
        self.id_index.sort_unstable_by_key(|(id, _)| *id);
        self.rebuild_visible_from_anchor(previous_offset, previous_top);
        Ok(true)
    }

    /// Build a complete unstyled virtualized, expandable, keyboard-navigable tree.
    ///
    /// `render_row` runs only for mounted rows and receives an optional behavior-decorated,
    /// appearance-free disclosure element. The caller decides where to place that control and owns
    /// the complete row layout and paint. QuickGUI decorates the returned row with fixed virtual
    /// geometry, tree semantics, selection, and pointer behavior. `activate` receives the selected
    /// application value by stable node ID; the tree retains no closure or scheduler afterward.
    pub fn element<V, E, RenderRow, Activate>(
        &mut self,
        cx: &mut ViewContext<'_, V>,
        id: impl Into<ElementId>,
        access: fn(&mut V) -> &mut TreeState<T>,
        render_row: RenderRow,
        activate: Activate,
    ) -> Element
    where
        V: 'static,
        T: 'static,
        E: IntoElement,
        RenderRow: FnMut(TreeRow<'_, T>, Option<Element>) -> E,
        Activate: Fn(&mut V, ElementId, &mut EventContext) + Clone + 'static,
    {
        self.element_with(cx, id, StateAccessor::from(access), render_row, activate)
    }

    /// Build the tree against a per-instance retained-state accessor.
    ///
    /// A host that renders many declared trees through one view passes an accessor that captures
    /// which [`TreeState`] each registered listener resolves.
    pub fn element_with<V, E, RenderRow, Activate>(
        &mut self,
        cx: &mut ViewContext<'_, V>,
        id: impl Into<ElementId>,
        access_source: StateAccessor<V, TreeState<T>>,
        mut render_row: RenderRow,
        activate: Activate,
    ) -> Element
    where
        V: 'static,
        T: 'static,
        E: IntoElement,
        RenderRow: FnMut(TreeRow<'_, T>, Option<Element>) -> E,
        Activate: Fn(&mut V, ElementId, &mut EventContext) + Clone + 'static,
    {
        let id = id.into();
        let root_focus = Self::focus_handle(id);
        let layout = self.layout;

        let access = access_source.clone();
        let previous = cx.action_listener(id, move |view, _: &TreePrevious, cx| {
            if access.get(view).move_selection(false) {
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let next = cx.action_listener(id, move |view, _: &TreeNext, cx| {
            if access.get(view).move_selection(true) {
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let collapse = cx.action_listener(id, move |view, _: &TreeCollapseOrParent, cx| {
            if access.get(view).collapse_or_parent() {
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let expand = cx.action_listener(id, move |view, _: &TreeExpandOrChild, cx| {
            let node = access.get(view).selected_id();
            if access.get(view).expand_or_child() {
                cx.invalidate();
                if let Some(node) = node.filter(|node| access.get(view).is_loading(*node)) {
                    cx.dispatch_action(TreeLoadChildren { node });
                }
            }
        });
        let access = access_source.clone();
        let first = cx.action_listener(id, move |view, _: &TreeFirst, cx| {
            if access.get(view).select_edge(false) {
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let last = cx.action_listener(id, move |view, _: &TreeLast, cx| {
            if access.get(view).select_edge(true) {
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let page_up = cx.action_listener(id, move |view, _: &TreePageUp, cx| {
            if access.get(view).move_page(false) {
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let page_down = cx.action_listener(id, move |view, _: &TreePageDown, cx| {
            if access.get(view).move_page(true) {
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let toggle = cx.action_listener(id, move |view, _: &TreeToggle, cx| {
            let selected = access.get(view).selected_id();
            if selected.is_some_and(|selected| access.get(view).toggle_expanded(selected)) {
                cx.invalidate();
                if let Some(node) = selected.filter(|node| access.get(view).is_loading(*node)) {
                    cx.dispatch_action(TreeLoadChildren { node });
                }
            }
        });
        let confirm_activate = activate.clone();
        let access = access_source.clone();
        let confirm = cx.action_listener(id, move |view, _: &TreeActivate, cx| {
            if let Some(selected) = access.get(view).selected_id() {
                confirm_activate(view, selected, cx);
            }
        });

        let selected = self.selected;
        let list = self.list.clone();
        let visible = list.visible_rows().range;
        let rows = list.render_rows(visible, |visible_index| {
            let loading = self.is_placeholder(visible_index);
            let node_index = self
                .visible_owner(visible_index)
                .expect("a mounted tree row is inside the visible range");
            let entry = &self.entries[node_index];
            let node_id = entry.id;
            let row_id = if loading {
                Self::loading_row_id(id, node_id)
            } else {
                Self::row_id(id, node_id)
            };
            let expanded = self.expanded[node_index];
            let row_selected = !loading && selected == Some(node_index as u32);
            let row = TreeRow {
                entry,
                expanded,
                selected: row_selected,
                loading: loading.then_some(&self.loading_label),
            };
            let disclosure = if !loading && (entry.child_count != 0 || entry.pending) {
                let disclosure_id = Self::disclosure_id(id, node_id);
                let access = access_source.clone();
                let disclosure_click = cx.listener(disclosure_id, move |view, cx| {
                    cx.stop_propagation();
                    if access.get(view).toggle_expanded(node_id) {
                        cx.focus(root_focus);
                        cx.invalidate();
                        if access.get(view).is_loading(node_id) {
                            cx.dispatch_action(TreeLoadChildren { node: node_id });
                        }
                    }
                });
                Some(
                    div()
                        .id(disclosure_id)
                        .on_click(disclosure_click)
                        .tab_index(-1)
                        .accessibility_role(AccessibilityRole::Button)
                        .accessibility_label(if expanded { "Collapse" } else { "Expand" })
                        .app_region_no_drag()
                        .cursor_default()
                        .user_select_none(),
                )
            } else {
                None
            };

            let row_label = row.label().clone();
            let row_level = row.level();
            let row_position = row.position_in_set();
            let row_size = row.size_of_set();
            let row_disabled = row.is_disabled();
            let mut row_element = render_row(row, disclosure)
                .into_element()
                .id(row_id)
                .tab_index(-1)
                .accessibility_role(AccessibilityRole::TreeItem)
                .accessibility_label(row_label)
                .accessibility_level(row_level)
                .accessibility_position_in_set(row_position)
                .accessibility_size_of_set(row_size)
                .selected(row_selected)
                .disabled(row_disabled)
                .h(layout.row_height)
                .w_full()
                .min_w(0.0)
                .overflow_hidden()
                .cursor_default()
                .app_region_no_drag();
            if !loading && (entry.child_count != 0 || entry.pending) {
                row_element = row_element.accessibility_expanded(expanded);
            }
            if !loading && !entry.disabled {
                let access = access_source.clone();
                let clicked = cx.listener(row_id, move |view, cx| {
                    if access.get(view).select(node_id) {
                        cx.invalidate();
                    }
                    cx.focus(root_focus);
                });
                row_element = row_element.on_click(clicked);
            }
            row_element
        });

        let body = div()
            .relative()
            .size_full()
            .min_w(0.0)
            .min_h(0.0)
            .overflow_hidden()
            .variable_virtual_scroll(&list)
            .app_region_no_drag()
            .child(rows);

        let mut root = div()
            .id(id)
            .track_focus(root_focus)
            .key_context(TREE_KEY_CONTEXT)
            .on_action(previous)
            .on_action(next)
            .on_action(collapse)
            .on_action(expand)
            .on_action(first)
            .on_action(last)
            .on_action(page_up)
            .on_action(page_down)
            .on_action(toggle)
            .on_action(confirm)
            .accessibility_role(AccessibilityRole::Tree)
            .accessibility_size_of_set(self.root_count)
            .size_full()
            .min_w(0.0)
            .min_h(0.0)
            .overflow_hidden()
            .app_region_no_drag()
            .child(body);
        if let Some(selected) = self.selected {
            root = root.accessibility_active_descendant(Self::row_id(
                id,
                self.entries[selected as usize].id,
            ));
        }
        root
    }

    fn index_for_id(&self, id: ElementId) -> Option<usize> {
        self.id_index
            .binary_search_by_key(&id.as_u64(), |(id, _)| *id)
            .ok()
            .map(|position| self.id_index[position].1 as usize)
    }

    fn rebuild_visible(&mut self) {
        let previous_offset = self.list.logical_scroll_top();
        let previous_top = self
            .visible_owner(previous_offset.item_ix)
            .map(|index| self.entries[index].id);

        self.rebuild_visible_from_anchor(previous_offset, previous_top);
    }

    fn rebuild_visible_from_anchor(
        &mut self,
        previous_offset: ListOffset,
        previous_top: Option<ElementId>,
    ) {
        self.visible.clear();
        self.visible_position.fill(TREE_VISIBLE_UNSET);
        let mut index = 0usize;
        while index < self.entries.len() {
            let visible_index = self.visible.len();
            self.visible.push(index as u32);
            self.visible_position[index] = visible_index as u32;
            let entry = &self.entries[index];
            if entry.pending && entry.child_count == 0 && self.expanded[index] {
                self.visible.push(index as u32 | TREE_PLACEHOLDER_FLAG);
            }
            if entry.child_count != 0 && !self.expanded[index] {
                index = entry.subtree_end as usize;
            } else {
                index += 1;
            }
        }
        self.list.set_item_count(self.visible.len());

        if let Some(previous_top) = previous_top
            && let Some(index) = self.index_for_id(previous_top)
            && self.visible_position[index] != TREE_VISIBLE_UNSET
        {
            self.list.scroll_to(ListOffset {
                item_ix: self.visible_position[index] as usize,
                offset_in_item: previous_offset.offset_in_item,
            });
        }

        self.normalize_selection();
    }

    fn normalize_selection(&mut self) {
        if let Some(mut selected) = self.selected {
            while self.visible_position[selected as usize] == TREE_VISIBLE_UNSET {
                let Some(parent) = self.entries[selected as usize].parent else {
                    self.selected = None;
                    break;
                };
                selected = parent;
                self.selected = Some(parent);
            }
        }
        if self
            .selected
            .is_some_and(|index| self.entries[index as usize].disabled)
        {
            self.selected = None;
        }
        if self.selected.is_none() {
            self.selected = (0..self.visible.len())
                .find(|visible| self.is_selectable(*visible))
                .and_then(|visible| self.visible_entry(visible))
                .map(|index| index as u32);
        }
        if let Some(selected) = self.selected {
            let visible = self.visible_position[selected as usize];
            if visible != TREE_VISIBLE_UNSET {
                self.list.scroll_to_reveal_item(visible as usize);
            }
        }
    }

    fn select_visible(&mut self, visible_index: usize) -> bool {
        let Some(index) = self.visible_entry(visible_index).map(|index| index as u32) else {
            return false;
        };
        if self.entries[index as usize].disabled {
            return false;
        }
        let changed = self.selected != Some(index);
        self.selected = Some(index);
        self.list.scroll_to_reveal_item(visible_index) || changed
    }

    fn move_selection(&mut self, forward: bool) -> bool {
        self.normalize_selection();
        let Some(selected) = self.selected else {
            return false;
        };
        let current = self.visible_position[selected as usize] as usize;
        let candidate = if forward {
            (current + 1..self.visible.len()).find(|index| self.is_selectable(*index))
        } else {
            (0..current).rev().find(|index| self.is_selectable(*index))
        };
        candidate.is_some_and(|candidate| self.select_visible(candidate))
    }

    fn select_edge(&mut self, end: bool) -> bool {
        let candidate = if end {
            (0..self.visible.len())
                .rev()
                .find(|index| self.is_selectable(*index))
        } else {
            (0..self.visible.len()).find(|index| self.is_selectable(*index))
        };
        candidate.is_some_and(|candidate| self.select_visible(candidate))
    }

    fn move_page(&mut self, forward: bool) -> bool {
        self.normalize_selection();
        let Some(selected) = self.selected else {
            return false;
        };
        let current = self.visible_position[selected as usize] as usize;
        let page = (self.list.viewport_size().height / self.layout.row_height)
            .floor()
            .max(1.0) as usize;
        let target = if forward {
            current.saturating_add(page).min(self.visible.len() - 1)
        } else {
            current.saturating_sub(page)
        };
        let candidate = if forward {
            (target..self.visible.len())
                .find(|index| self.is_selectable(*index))
                .or_else(|| (0..target).rev().find(|index| self.is_selectable(*index)))
        } else {
            (0..=target)
                .rev()
                .find(|index| self.is_selectable(*index))
                .or_else(|| {
                    (target + 1..self.visible.len()).find(|index| self.is_selectable(*index))
                })
        };
        candidate.is_some_and(|candidate| self.select_visible(candidate))
    }

    fn collapse_or_parent(&mut self) -> bool {
        self.normalize_selection();
        let Some(selected) = self.selected else {
            return false;
        };
        let index = selected as usize;
        let branch = self.entries[index].child_count != 0 || self.entries[index].pending;
        if branch && self.expanded[index] {
            self.expanded[index] = false;
            self.rebuild_visible();
            return true;
        }
        let Some(parent) = self.entries[index].parent else {
            return false;
        };
        let parent = parent as usize;
        if self.entries[parent].disabled {
            return false;
        }
        self.select_visible(self.visible_position[parent] as usize)
    }

    fn expand_or_child(&mut self) -> bool {
        self.normalize_selection();
        let Some(selected) = self.selected else {
            return false;
        };
        let index = selected as usize;
        if self.entries[index].child_count == 0 && !self.entries[index].pending {
            return false;
        }
        if !self.expanded[index] {
            self.expanded[index] = true;
            self.rebuild_visible();
            return true;
        }
        let current = self.visible_position[index] as usize;
        let end = self.entries[index].subtree_end as usize;
        (current + 1..self.visible.len())
            .take_while(|visible| {
                self.visible_owner(*visible)
                    .is_some_and(|owner| owner < end || self.is_placeholder(*visible))
            })
            .find(|visible| self.is_selectable(*visible))
            .is_some_and(|visible| self.select_visible(visible))
    }
}

fn build_tree_arena<T>(
    nodes: impl IntoIterator<Item = TreeNode<T>>,
) -> Result<TreeArena<T>, TreeError> {
    let roots = nodes.into_iter().collect::<Vec<_>>();
    let root_count = roots.len();
    let mut entries = Vec::with_capacity(root_count.min(256));
    let mut total_text_bytes = 0usize;
    for (position, node) in roots.into_iter().enumerate() {
        push_tree_node(
            node,
            None,
            0,
            position,
            root_count,
            &mut entries,
            &mut total_text_bytes,
        )?;
    }

    let mut id_index = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| (entry.id.as_u64(), index as u32))
        .collect::<Vec<_>>();
    id_index.sort_unstable_by_key(|(id, _)| *id);
    if let Some(duplicate) = id_index.windows(2).find(|pair| pair[0].0 == pair[1].0) {
        return Err(TreeError::DuplicateId {
            id: ElementId::new(duplicate[0].0),
        });
    }

    Ok(TreeArena {
        entries,
        id_index,
        root_count,
    })
}

#[allow(clippy::too_many_arguments)]
fn push_tree_node<T>(
    node: TreeNode<T>,
    parent: Option<u32>,
    level: usize,
    position_in_set: usize,
    size_of_set: usize,
    entries: &mut Vec<TreeEntry<T>>,
    total_text_bytes: &mut usize,
) -> Result<(), TreeError> {
    if entries.len() == MAX_TREE_NODES {
        return Err(TreeError::TooManyNodes {
            limit: MAX_TREE_NODES,
        });
    }
    if level >= MAX_TREE_DEPTH {
        return Err(TreeError::TooDeep {
            depth: level + 1,
            limit: MAX_TREE_DEPTH,
        });
    }
    if node.label.len() > MAX_TREE_LABEL_BYTES {
        return Err(TreeError::LabelTooLong {
            id: node.id,
            bytes: node.label.len(),
            limit: MAX_TREE_LABEL_BYTES,
        });
    }
    *total_text_bytes = total_text_bytes.saturating_add(node.label.len());
    if *total_text_bytes > MAX_TREE_TEXT_BYTES {
        return Err(TreeError::TextBudgetExceeded {
            bytes: *total_text_bytes,
            limit: MAX_TREE_TEXT_BYTES,
        });
    }

    let index = entries.len() as u32;
    let child_count = node.children.len();
    entries.push(TreeEntry {
        id: node.id,
        label: node.label,
        value: node.value,
        parent,
        level: level as u32,
        position_in_set: position_in_set as u32,
        size_of_set: size_of_set as u32,
        subtree_end: index + 1,
        child_count: child_count as u32,
        disabled: node.disabled,
        pending: node.pending && child_count == 0,
    });
    for (position, child) in node.children.into_iter().enumerate() {
        push_tree_node(
            child,
            Some(index),
            level + 1,
            position,
            child_count,
            entries,
            total_text_bytes,
        )?;
    }
    entries[index as usize].subtree_end = entries.len() as u32;
    Ok(())
}

fn derived_tree_id(parent: ElementId, tag: u64, node: u64) -> ElementId {
    let mut hash = parent.as_u64() ^ tag ^ node.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    if hash == parent.as_u64() || hash == u64::MAX {
        hash ^= tag.rotate_left(17);
    }
    ElementId::new(hash)
}

fn finite_clamped(value: f32, minimum: f32, maximum: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(minimum, maximum)
    } else {
        fallback.clamp(minimum, maximum)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Application, View, WindowOptions, text};

    fn sample_nodes() -> Vec<TreeNode<usize>> {
        vec![
            TreeNode::new("src", "src", 0)
                .child(TreeNode::new("lib", "lib.rs", 1))
                .child(
                    TreeNode::new("runtime", "runtime", 2)
                        .child(TreeNode::new("macos", "macos.rs", 3))
                        .child(TreeNode::new("test", "test_context.rs", 4).disabled(true)),
                ),
            TreeNode::new("cargo", "Cargo.toml", 5),
        ]
    }

    #[test]
    fn preorder_arena_expansion_navigation_and_top_anchor_are_bounded() {
        let mut tree = TreeState::new(sample_nodes()).unwrap();
        tree.list.set_viewport_size(300.0, 60.0);
        assert_eq!(tree.node_count(), 6);
        assert_eq!(tree.visible_count(), 2);
        assert_eq!(tree.selected_id(), Some("src".into()));

        assert!(tree.set_expanded("src", true));
        assert_eq!(tree.visible_count(), 4);
        assert!(tree.select("runtime"));
        assert!(tree.expand_or_child());
        assert_eq!(tree.visible_count(), 6);
        assert!(tree.expand_or_child());
        assert_eq!(tree.selected_id(), Some("macos".into()));
        assert!(tree.collapse_or_parent());
        assert_eq!(tree.selected_id(), Some("runtime".into()));
        assert!(tree.collapse_or_parent());
        assert!(!tree.is_expanded("runtime"));
        assert!(tree.move_selection(true));
        assert_eq!(tree.selected_id(), Some("cargo".into()));
        assert!(!tree.collapse_or_parent());
        assert!(tree.visible_rows().len() <= tree.visible_count());
    }

    #[test]
    fn large_flat_trees_mount_only_the_viewport_and_overscan() {
        let nodes = (0..100_000)
            .map(|index| TreeNode::new(index as u64 + 1, format!("Node {index}"), index));
        let tree = TreeState::new(nodes).unwrap();
        tree.list.set_viewport_size(320.0, 90.0);
        assert_eq!(tree.node_count(), 100_000);
        assert_eq!(tree.visible_count(), 100_000);
        assert!(tree.visible_rows().len() <= 7);
        assert_eq!(tree.list.stats().measured_items, 0);
    }

    #[test]
    fn layout_is_bounded_and_row_height_changes_preserve_the_logical_anchor() {
        let mut tree = TreeState::new(sample_nodes()).unwrap();
        tree.set_expanded("src", true);
        tree.list.set_viewport_size(300.0, 60.0);
        tree.list.scroll_to(ListOffset {
            item_ix: 2,
            offset_in_item: 4.0,
        });
        let before = tree.list.logical_scroll_top();

        assert!(tree.set_layout(TreeLayout::new(44.0)));
        assert_eq!(tree.layout().row_height, 44.0);
        assert_eq!(tree.list.logical_scroll_top().item_ix, before.item_ix);

        let invalid = TreeLayout {
            row_height: f32::NAN,
        };
        assert!(tree.set_layout(invalid));
        assert_eq!(tree.layout().row_height, 30.0);
        assert_eq!(tree.list.logical_scroll_top().item_ix, before.item_ix);
    }

    #[test]
    fn source_replacement_is_atomic_and_preserves_stable_state() {
        let mut tree = TreeState::new(sample_nodes()).unwrap();
        tree.list.set_viewport_size(300.0, 30.0);
        assert!(tree.set_expanded("src", true));
        assert!(tree.set_expanded("runtime", true));
        assert!(tree.select("macos"));
        tree.list.scroll_to(ListOffset {
            item_ix: 3,
            offset_in_item: 4.0,
        });

        tree.set_nodes([
            TreeNode::new("cargo", "Cargo.toml", 50),
            TreeNode::new("src", "source", 10)
                .child(
                    TreeNode::new("runtime", "platform", 20)
                        .child(TreeNode::new("macos", "macos.rs", 30))
                        .child(TreeNode::new("linux", "linux.rs", 40)),
                )
                .child(TreeNode::new("lib", "lib.rs", 11)),
        ])
        .unwrap();

        assert_eq!(tree.selected_id(), Some("macos".into()));
        assert_eq!(tree.selected_value(), Some(&30));
        assert!(tree.is_expanded("src"));
        assert!(tree.is_expanded("runtime"));
        let top = tree.list.logical_scroll_top();
        assert_eq!(tree.row(top.item_ix).map(TreeRow::id), Some("macos".into()));

        let error = tree
            .set_nodes([
                TreeNode::new("duplicate", "A", 1),
                TreeNode::new("duplicate", "B", 2),
            ])
            .unwrap_err();
        assert_eq!(
            error,
            TreeError::DuplicateId {
                id: "duplicate".into()
            }
        );
        assert_eq!(tree.node_count(), 6);
        assert_eq!(tree.selected_id(), Some("macos".into()));
        assert_eq!(tree.selected_value(), Some(&30));
        assert!(tree.is_expanded("src"));
        assert!(tree.is_expanded("runtime"));

        tree.set_nodes([TreeNode::new("cargo", "Cargo.toml", 60)])
            .unwrap();
        assert_eq!(tree.node_count(), 1);
        assert_eq!(tree.selected_id(), Some("cargo".into()));
    }

    #[test]
    fn duplicate_depth_and_text_limits_fail_deterministically() {
        assert_eq!(
            TreeState::new([
                TreeNode::new("same", "A", ()),
                TreeNode::new("same", "B", ()),
            ])
            .unwrap_err(),
            TreeError::DuplicateId { id: "same".into() }
        );

        let oversized = "x".repeat(MAX_TREE_LABEL_BYTES + 1);
        assert_eq!(
            TreeState::new([TreeNode::new("large", oversized, ())]).unwrap_err(),
            TreeError::LabelTooLong {
                id: "large".into(),
                bytes: MAX_TREE_LABEL_BYTES + 1,
                limit: MAX_TREE_LABEL_BYTES,
            }
        );

        let mut node = TreeNode::new(0_u64, "0", ());
        for level in 1..=MAX_TREE_DEPTH {
            node = TreeNode::new(level, level.to_string(), ()).child(node);
        }
        assert!(matches!(
            TreeState::new([node]),
            Err(TreeError::TooDeep {
                limit: MAX_TREE_DEPTH,
                ..
            })
        ));
    }

    #[derive(Debug)]
    struct TreeView {
        tree: TreeState<usize>,
        activated: Option<ElementId>,
    }

    impl Default for TreeView {
        fn default() -> Self {
            Self {
                tree: TreeState::new(sample_nodes()).unwrap(),
                activated: None,
            }
        }
    }

    impl TreeView {
        fn tree(view: &mut Self) -> &mut TreeState<usize> {
            &mut view.tree
        }
    }

    impl View for TreeView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            self.tree.element(
                cx,
                "tree",
                Self::tree,
                |row, disclosure| {
                    let disclosure = disclosure.map_or_else(
                        || div().w(20.0).h(30.0).flex_none(),
                        |disclosure| {
                            disclosure.w(20.0).h(30.0).child(if row.is_expanded() {
                                "⌄"
                            } else {
                                "›"
                            })
                        },
                    );
                    div()
                        .flex_row()
                        .items_center()
                        .child(disclosure)
                        .child(text(row.label().clone()).no_wrap().text_ellipsis())
                },
                |view, id, cx| {
                    view.activated = Some(id);
                    cx.invalidate();
                },
            )
        }
    }

    #[test]
    fn tree_uses_composite_focus_click_keyboard_activation_and_idle_paths() {
        let (mut cx, view) = Application::new()
            .bind_keys(tree_key_bindings())
            .into_test_context(WindowOptions::default(), TreeView::default())
            .unwrap();
        let window = view.window_handle();

        cx.simulate_keystrokes(window, "tab right down down right right enter")
            .unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some("tree".into()));
        assert_eq!(
            cx.read(view, |view| view.tree.selected_id()).unwrap(),
            Some("macos".into())
        );
        assert_eq!(
            cx.read(view, |view| view.activated).unwrap(),
            Some("macos".into())
        );

        let cargo_row = TreeState::<usize>::row_id("tree", "cargo".into());
        cx.click(window, cargo_row).unwrap();
        assert_eq!(
            cx.read(view, |view| view.tree.selected_id()).unwrap(),
            Some("cargo".into())
        );
        assert_eq!(cx.focused(window).unwrap(), Some("tree".into()));

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }

    #[test]
    fn tree_bindings_are_contextual_and_complete() {
        let bindings = tree_key_bindings();
        assert_eq!(bindings.len(), 10);
        assert!(
            bindings.iter().all(
                |binding| binding.context_predicate().is_some_and(|context| context
                    .depth_of(&[crate::KeyContext::parse(TREE_KEY_CONTEXT).unwrap()])
                    .is_some())
            )
        );
    }

    #[test]
    fn pending_branches_mount_one_placeholder_and_load_atomically() {
        let mut tree = TreeState::new(vec![
            TreeNode::new("src", "src", 0).child(TreeNode::new("lib", "lib.rs", 1)),
            TreeNode::new("remote", "remote", 2).pending(true),
        ])
        .unwrap();
        assert_eq!(tree.node_count(), 3);
        assert_eq!(tree.visible_count(), 2);
        assert!(tree.is_pending("remote"));
        assert!(
            !tree.is_loading("remote"),
            "a collapsed branch loads nothing"
        );

        assert!(tree.set_expanded("remote", true));
        assert!(tree.is_loading("remote"));
        assert_eq!(tree.visible_count(), 3, "exactly one placeholder row");
        let placeholder = tree.row(2).expect("the placeholder row is visible");
        assert!(placeholder.is_loading());
        assert_eq!(&**placeholder.label(), DEFAULT_TREE_LOADING_LABEL);
        assert_eq!(
            placeholder.level(),
            1,
            "the placeholder sits under its branch"
        );
        assert!(placeholder.is_disabled());
        assert!(!placeholder.is_selected());
        assert_eq!(placeholder.id(), "remote".into());

        assert!(tree.select("remote"));
        assert!(
            !tree.move_selection(true),
            "keyboard selection never lands on a placeholder"
        );
        assert_eq!(tree.selected_id(), Some("remote".into()));

        // A rejected payload leaves every retained field untouched.
        let error = tree
            .set_children("remote", [TreeNode::new("lib", "duplicate", 9)])
            .unwrap_err();
        assert_eq!(error, TreeError::DuplicateId { id: "lib".into() });
        assert_eq!(tree.node_count(), 3);
        assert_eq!(tree.visible_count(), 3);
        assert!(tree.is_loading("remote"));

        assert!(
            tree.set_children(
                "remote",
                [
                    TreeNode::new("alpha", "alpha.rs", 10),
                    TreeNode::new("beta", "beta.rs", 11),
                ],
            )
            .unwrap()
        );
        assert_eq!(tree.node_count(), 5);
        assert!(!tree.is_pending("remote"));
        assert!(!tree.is_loading("remote"));
        assert_eq!(tree.visible_count(), 4);
        assert_eq!(tree.row(2).map(TreeRow::id), Some("alpha".into()));
        assert_eq!(tree.row(2).map(TreeRow::level), Some(1));
        assert!(!tree.row(2).unwrap().is_loading());
        assert_eq!(tree.selected_id(), Some("remote".into()));
        assert!(tree.select("beta"));
        assert_eq!(tree.selected_value(), Some(&11));
        assert!(tree.set_expanded("src", true));
        assert_eq!(
            tree.row(1).map(TreeRow::id),
            Some("lib".into()),
            "the untouched subtree keeps its order"
        );

        assert!(
            !tree
                .set_children("missing", [TreeNode::new("z", "z", 0)])
                .unwrap(),
            "an unknown node reports no load"
        );
        assert!(
            !tree
                .set_children("src", [TreeNode::new("z", "z", 0)])
                .unwrap(),
            "a loaded branch refuses a second splice"
        );

        // Loading nothing turns the branch into an ordinary leaf.
        let mut empty = TreeState::new([TreeNode::new("empty", "empty", 0).pending(true)]).unwrap();
        assert!(empty.set_expanded("empty", true));
        assert_eq!(empty.visible_count(), 2);
        assert!(
            empty
                .set_children("empty", Vec::<TreeNode<usize>>::new())
                .unwrap()
        );
        assert_eq!(empty.visible_count(), 1);
        assert!(!empty.is_pending("empty"));
        assert!(!empty.set_expanded("empty", true), "a leaf does not expand");
    }

    #[test]
    fn oversized_lazy_payloads_are_rejected_before_the_tree_changes() {
        let mut tree = TreeState::new([TreeNode::new("root", "root", 0).pending(true)]).unwrap();
        assert!(tree.set_expanded("root", true));

        let oversized = "x".repeat(MAX_TREE_LABEL_BYTES + 1);
        assert_eq!(
            tree.set_children("root", [TreeNode::new("big", oversized, 1)])
                .unwrap_err(),
            TreeError::LabelTooLong {
                id: "big".into(),
                bytes: MAX_TREE_LABEL_BYTES + 1,
                limit: MAX_TREE_LABEL_BYTES,
            }
        );
        assert_eq!(tree.node_count(), 1);
        assert!(tree.is_loading("root"));

        let mut deep = TreeNode::new(0_u64, "0", 0);
        for level in 1..MAX_TREE_DEPTH {
            deep = TreeNode::new(level as u64, level.to_string(), 0).child(deep);
        }
        assert!(matches!(
            tree.set_children("root", [deep]),
            Err(TreeError::TooDeep {
                limit: MAX_TREE_DEPTH,
                ..
            })
        ));
        assert_eq!(tree.node_count(), 1);
        assert!(tree.is_loading("root"));
    }

    #[derive(Debug)]
    struct LazyTreeView {
        tree: TreeState<usize>,
        requests: Vec<ElementId>,
    }

    impl Default for LazyTreeView {
        fn default() -> Self {
            Self {
                tree: TreeState::new([
                    TreeNode::new("remote", "remote", 0).pending(true),
                    TreeNode::new("local", "local", 1),
                ])
                .unwrap()
                .with_loading_label("Fetching…"),
                requests: Vec::new(),
            }
        }
    }

    impl LazyTreeView {
        fn tree(view: &mut Self) -> &mut TreeState<usize> {
            &mut view.tree
        }
    }

    impl View for LazyTreeView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let load = cx.action_listener(
                "lazy-tree",
                |view: &mut Self, action: &TreeLoadChildren, cx| {
                    view.requests.push(action.node);
                    view.tree
                        .set_children(action.node, [TreeNode::new("child", "child.rs", 7)])
                        .expect("the payload is valid");
                    cx.invalidate();
                },
            );
            self.tree
                .element(
                    cx,
                    "lazy-tree",
                    Self::tree,
                    |row, disclosure| {
                        let disclosure = disclosure.map_or_else(
                            || div().w(20.0).h(30.0).flex_none(),
                            |handle| handle.w(20.0).h(30.0),
                        );
                        div()
                            .flex_row()
                            .items_center()
                            .child(disclosure)
                            .child(text(row.label().clone()).no_wrap())
                    },
                    |_view, _id, _cx| {},
                )
                .on_action(load)
        }
    }

    #[test]
    fn expanding_a_pending_branch_requests_children_once_through_a_typed_action() {
        let (mut cx, view) = Application::new()
            .bind_keys(tree_key_bindings())
            .into_test_context(WindowOptions::default(), LazyTreeView::default())
            .unwrap();
        let window = view.window_handle();

        cx.simulate_keystrokes(window, "tab").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some("lazy-tree".into()));
        assert_eq!(cx.read(view, |view| view.tree.node_count()).unwrap(), 2);

        cx.simulate_keystrokes(window, "right").unwrap();
        assert_eq!(
            cx.read(view, |view| view.requests.clone()).unwrap(),
            vec![ElementId::from("remote")]
        );
        assert_eq!(cx.read(view, |view| view.tree.node_count()).unwrap(), 3);
        assert!(
            !cx.read(view, |view| view.tree.is_loading("remote"))
                .unwrap()
        );
        assert_eq!(cx.read(view, |view| view.tree.visible_count()).unwrap(), 3);

        // A second expansion of a loaded branch asks for nothing.
        cx.simulate_keystrokes(window, "left right").unwrap();
        assert_eq!(cx.read(view, |view| view.requests.len()).unwrap(), 1);

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }
}
