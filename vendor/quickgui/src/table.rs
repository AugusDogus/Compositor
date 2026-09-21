use std::{fmt, sync::Arc};

use crate::{
    AccessibilityOrientation, AccessibilityRole, AccessibilitySortDirection,
    AccessibilityValueRange, Element, ElementId, EventContext, FocusHandle, GridTrack, IntoElement,
    KeyBinding, ListState, MAX_GRID_TRACKS, Modifiers, MouseButton, PointerPhase, StateAccessor,
    ViewContext, div,
};

/// Maximum logical rows managed by one reusable table.
pub const MAX_TABLE_ROWS: usize = 1_000_000;
/// Maximum columns retained in one table declaration.
pub const MAX_TABLE_COLUMNS: usize = MAX_GRID_TRACKS as usize;
/// Maximum disjoint selected row ranges retained by one table.
///
/// Selection is retained as merged inclusive ranges rather than one entry per row, so selecting
/// every row of a million-row table costs one range. The bound caps how fragmented a selection may
/// become; a toggle that would exceed it is refused and leaves the selection unchanged.
pub const MAX_TABLE_SELECTION_RANGES: usize = 1_024;
/// Smallest logical width a resizable column may take when it declares no minimum.
pub const MIN_TABLE_COLUMN_WIDTH: f32 = 24.0;
/// Largest logical width a resizable column may take.
pub const MAX_TABLE_COLUMN_WIDTH: f32 = 4_096.0;
/// Logical pixels one keyboard step moves a focused column-resize handle.
pub const TABLE_COLUMN_RESIZE_STEP: f32 = 8.0;

const TABLE_KEY_CONTEXT: &str = "Table";
const TABLE_HANDLE_KEY_CONTEXT: &str = "TableColumnHandle";
const TABLE_EDITOR_KEY_CONTEXT: &str = "TableEditor";
const TABLE_ROW_ID_TAG: u64 = 0xa9e0_2f75_8315_a7d1;
const TABLE_CELL_ID_TAG: u64 = 0xd2e1_6bd2_435b_0f97;
const TABLE_HEADER_ID_TAG: u64 = 0x49a7_3c01_94d8_62ef;
const TABLE_RESIZE_ID_TAG: u64 = 0x6b12_dd84_0f3a_57c2;

/// Move the active table cell one row upward.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TablePreviousRow;
/// Move the active table cell one row downward.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TableNextRow;
/// Move the active table cell one column left.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TablePreviousColumn;
/// Move the active table cell one column right.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TableNextColumn;
/// Move the active table cell one visible page upward.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TablePageUp;
/// Move the active table cell one visible page downward.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TablePageDown;
/// Move the active table cell to the first row.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TableFirstRow;
/// Move the active table cell to the final row.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TableLastRow;
/// Activate the current table cell.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TableActivate;
/// Add or remove the active row from a multiple selection.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TableToggleSelection;
/// Move up one row and extend the selection from its anchor.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TableExtendSelectionUp;
/// Move down one row and extend the selection from its anchor.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TableExtendSelectionDown;
/// Select every row.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TableSelectAll;
/// Move the active column one position toward the start of the display order.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TableMoveColumnLeft;
/// Move the active column one position toward the end of the display order.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TableMoveColumnRight;
/// Commit the open inline editor.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TableCommitEdit;
/// Abandon the open inline editor.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TableCancelEdit;
/// Shrink the column the focused resize handle controls.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TableResizeColumnSmaller;
/// Grow the column the focused resize handle controls.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TableResizeColumnLarger;

/// Dispatched by the table after an interaction changed the row selection.
///
/// The action carries no payload: selection is controlled state, so the owning view reads
/// [`TableState::selection`] when it receives this. A gesture that leaves the selection unchanged
/// dispatches nothing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TableSelectionChanged;

/// Dispatched by the table when an open inline editor closed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TableEditEnded {
    pub position: TableCellPosition,
    /// `true` for Return, `false` for Escape.
    pub committed: bool,
}

/// Contextual bindings used by [`TableState::element`].
///
/// Most bindings live in the table's own key context. Column-resize bindings live in the deeper
/// context a mounted resize handle declares, and inline-edit bindings live in the deeper context
/// an open editor cell declares, so the same arrow, Return, and Escape keys keep their ordinary
/// table meaning everywhere else.
pub fn table_key_bindings() -> [KeyBinding; 19] {
    [
        KeyBinding::new("up", TablePreviousRow, Some(TABLE_KEY_CONTEXT)),
        KeyBinding::new("down", TableNextRow, Some(TABLE_KEY_CONTEXT)),
        KeyBinding::new("left", TablePreviousColumn, Some(TABLE_KEY_CONTEXT)),
        KeyBinding::new("right", TableNextColumn, Some(TABLE_KEY_CONTEXT)),
        KeyBinding::new("pageup", TablePageUp, Some(TABLE_KEY_CONTEXT)),
        KeyBinding::new("pagedown", TablePageDown, Some(TABLE_KEY_CONTEXT)),
        KeyBinding::new("platform-up", TableFirstRow, Some(TABLE_KEY_CONTEXT)),
        KeyBinding::new("platform-down", TableLastRow, Some(TABLE_KEY_CONTEXT)),
        KeyBinding::new("enter", TableActivate, Some(TABLE_KEY_CONTEXT)),
        KeyBinding::new("space", TableToggleSelection, Some(TABLE_KEY_CONTEXT)),
        KeyBinding::new("shift-up", TableExtendSelectionUp, Some(TABLE_KEY_CONTEXT)),
        KeyBinding::new(
            "shift-down",
            TableExtendSelectionDown,
            Some(TABLE_KEY_CONTEXT),
        ),
        KeyBinding::new("platform-a", TableSelectAll, Some(TABLE_KEY_CONTEXT)),
        KeyBinding::new("alt-left", TableMoveColumnLeft, Some(TABLE_KEY_CONTEXT)),
        KeyBinding::new("alt-right", TableMoveColumnRight, Some(TABLE_KEY_CONTEXT)),
        KeyBinding::new("enter", TableCommitEdit, Some(TABLE_EDITOR_KEY_CONTEXT)),
        KeyBinding::new("escape", TableCancelEdit, Some(TABLE_EDITOR_KEY_CONTEXT)),
        KeyBinding::new(
            "left",
            TableResizeColumnSmaller,
            Some(TABLE_HANDLE_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "right",
            TableResizeColumnLarger,
            Some(TABLE_HANDLE_KEY_CONTEXT),
        ),
    ]
}

/// Horizontal alignment for one table column.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TableColumnAlign {
    #[default]
    Start,
    Center,
    End,
}

/// One stable table column declaration.
#[derive(Clone, Debug, PartialEq)]
pub struct TableColumn {
    id: ElementId,
    label: Arc<str>,
    track: GridTrack,
    align: TableColumnAlign,
    sortable: bool,
    row_header: bool,
    width: Option<f32>,
    minimum_width: f32,
}

impl TableColumn {
    pub fn new(id: impl Into<ElementId>, label: impl Into<Arc<str>>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            track: GridTrack::fr(1.0),
            align: TableColumnAlign::Start,
            sortable: false,
            row_header: false,
            width: None,
            minimum_width: MIN_TABLE_COLUMN_WIDTH,
        }
    }

    /// Make this column resizable, starting at exactly this logical width.
    ///
    /// A resizable column is laid out as a fixed pixel track: without a retained width the resize
    /// behavior would not exist. Every other declaration on the column stays application-owned.
    /// The declared width seeds [`TableState`] the first time the column is rendered; afterwards
    /// the retained width wins, so a rebuild never discards a user's drag.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(clamped_column_width(width, self.minimum_width));
        self.track = GridTrack::px(clamped_column_width(width, self.minimum_width));
        self
    }

    /// Refuse to shrink this column below this logical width.
    pub fn minimum_width(mut self, minimum: f32) -> Self {
        self.minimum_width = if minimum.is_finite() {
            minimum.clamp(MIN_TABLE_COLUMN_WIDTH, MAX_TABLE_COLUMN_WIDTH)
        } else {
            MIN_TABLE_COLUMN_WIDTH
        };
        if let Some(width) = self.width {
            self.width = Some(clamped_column_width(width, self.minimum_width));
        }
        self
    }

    /// Whether this column declared a resizable pixel width.
    pub const fn is_resizable(&self) -> bool {
        self.width.is_some()
    }

    /// The declared starting width of a resizable column.
    pub const fn declared_width(&self) -> Option<f32> {
        self.width
    }

    pub const fn minimum_column_width(&self) -> f32 {
        self.minimum_width
    }

    pub fn track(mut self, track: GridTrack) -> Self {
        self.track = track;
        self
    }

    pub fn align(mut self, align: TableColumnAlign) -> Self {
        self.align = align;
        self
    }

    pub fn sortable(mut self, sortable: bool) -> Self {
        self.sortable = sortable;
        self
    }

    /// Mark data cells in this column as row headers instead of ordinary grid cells.
    pub fn row_header(mut self, row_header: bool) -> Self {
        self.row_header = row_header;
        self
    }

    pub const fn id(&self) -> ElementId {
        self.id
    }

    pub fn label(&self) -> &Arc<str> {
        &self.label
    }

    pub const fn grid_track(&self) -> GridTrack {
        self.track
    }

    pub const fn alignment(&self) -> TableColumnAlign {
        self.align
    }

    pub const fn is_sortable(&self) -> bool {
        self.sortable
    }

    pub const fn is_row_header(&self) -> bool {
        self.row_header
    }
}

/// Direction requested for an application-owned table ordering.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableSortDirection {
    Ascending,
    Descending,
}

impl TableSortDirection {
    const fn accessibility(self) -> AccessibilitySortDirection {
        match self {
            Self::Ascending => AccessibilitySortDirection::Ascending,
            Self::Descending => AccessibilitySortDirection::Descending,
        }
    }
}

/// Current sort declaration. QuickGUI renders and exposes it; the owning view orders its data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TableSort {
    pub column: ElementId,
    pub direction: TableSortDirection,
}

/// Zero-based logical position of one table cell.
///
/// `column` always indexes the caller's declared column slice, never the display order, so an
/// application reading a position never has to undo a user's column reordering.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TableCellPosition {
    pub row: usize,
    pub column: usize,
}

/// How many rows one table may select at a time.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TableSelectionMode {
    /// Exactly the active row, matching a plain list.
    #[default]
    Single,
    /// Shift ranges, platform-modified toggles, and Select All.
    Multiple,
}

/// The rows one table has selected, retained as merged inclusive ranges.
///
/// The representation is proportional to how fragmented the selection is, never to how many rows
/// are selected: Select All over a million rows retains one range. It holds at most
/// [`MAX_TABLE_SELECTION_RANGES`] ranges and owns no task, timer, observer, or idle source.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TableSelection {
    ranges: Vec<(usize, usize)>,
}

impl TableSelection {
    pub fn new() -> Self {
        Self::default()
    }

    /// One contiguous inclusive range.
    pub fn range(start: usize, end: usize) -> Self {
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        Self {
            ranges: vec![(start, end)],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    /// The number of selected rows.
    pub fn len(&self) -> usize {
        self.ranges.iter().map(|(start, end)| end - start + 1).sum()
    }

    /// The merged inclusive ranges in ascending order.
    pub fn ranges(&self) -> &[(usize, usize)] {
        &self.ranges
    }

    pub fn contains(&self, row: usize) -> bool {
        self.ranges
            .binary_search_by(|(start, end)| {
                if row < *start {
                    std::cmp::Ordering::Greater
                } else if row > *end {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .is_ok()
    }

    pub fn clear(&mut self) -> bool {
        let changed = !self.ranges.is_empty();
        self.ranges.clear();
        changed
    }

    /// Replace the selection with exactly one row.
    pub fn set_single(&mut self, row: usize) -> bool {
        if self.ranges.as_slice() == [(row, row)] {
            return false;
        }
        self.ranges.clear();
        self.ranges.push((row, row));
        true
    }

    /// Replace the selection with one inclusive range.
    pub fn set_range(&mut self, start: usize, end: usize) -> bool {
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        if self.ranges.as_slice() == [(start, end)] {
            return false;
        }
        self.ranges.clear();
        self.ranges.push((start, end));
        true
    }

    /// Add one inclusive range, merging it with anything it touches.
    ///
    /// The insertion is refused when the result would exceed [`MAX_TABLE_SELECTION_RANGES`].
    pub fn insert_range(&mut self, start: usize, end: usize) -> bool {
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        let mut merged = Vec::with_capacity(self.ranges.len() + 1);
        let mut current = (start, end);
        let mut inserted = false;
        for (existing_start, existing_end) in self.ranges.iter().copied() {
            if existing_end + 1 < current.0 {
                merged.push((existing_start, existing_end));
            } else if current.1 + 1 < existing_start {
                if !inserted {
                    merged.push(current);
                    inserted = true;
                }
                merged.push((existing_start, existing_end));
            } else {
                current.0 = current.0.min(existing_start);
                current.1 = current.1.max(existing_end);
            }
        }
        if !inserted {
            merged.push(current);
        }
        merged.sort_unstable();
        if merged.len() > MAX_TABLE_SELECTION_RANGES {
            return false;
        }
        if merged == self.ranges {
            return false;
        }
        self.ranges = merged;
        true
    }

    /// Remove one inclusive range, splitting any range it cuts through.
    pub fn remove_range(&mut self, start: usize, end: usize) -> bool {
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        let mut remaining = Vec::with_capacity(self.ranges.len() + 1);
        for (existing_start, existing_end) in self.ranges.iter().copied() {
            if existing_end < start || existing_start > end {
                remaining.push((existing_start, existing_end));
                continue;
            }
            if existing_start < start {
                remaining.push((existing_start, start - 1));
            }
            if existing_end > end {
                remaining.push((end + 1, existing_end));
            }
        }
        if remaining.len() > MAX_TABLE_SELECTION_RANGES || remaining == self.ranges {
            return false;
        }
        self.ranges = remaining;
        true
    }

    pub fn insert(&mut self, row: usize) -> bool {
        self.insert_range(row, row)
    }

    pub fn remove(&mut self, row: usize) -> bool {
        self.remove_range(row, row)
    }

    pub fn toggle(&mut self, row: usize) -> bool {
        if self.contains(row) {
            self.remove(row)
        } else {
            self.insert(row)
        }
    }

    /// Drop everything at or past `row_count`.
    fn clamp(&mut self, row_count: usize) -> bool {
        if row_count == 0 {
            return self.clear();
        }
        self.remove_range(row_count, usize::MAX)
    }
}

/// State supplied to one caller-owned column-header renderer.
#[derive(Clone, Debug)]
pub struct TableHeaderState<'a> {
    /// Index into the caller's declared column slice.
    pub column_index: usize,
    /// Position of this column in the current display order.
    pub display_index: usize,
    pub column: &'a TableColumn,
    pub sort_direction: Option<TableSortDirection>,
    /// The current retained width of a resizable column.
    pub width: Option<f32>,
    /// A behavior-decorated, appearance-free resize handle for a resizable column.
    ///
    /// QuickGUI attaches the captured pointer drag, the keyboard resize actions, the platform
    /// column-resize cursor, and the splitter semantics. The application decides where the handle
    /// sits, how wide its hit area is, and how it is painted.
    pub resize_handle: Option<Element>,
}

/// State supplied to one caller-owned table-row renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableRowState {
    /// Logical row index.
    pub row: usize,
    /// Whether this row is part of the retained row selection.
    pub selected: bool,
}

/// State supplied to one caller-owned table-cell renderer.
#[derive(Clone, Copy, Debug)]
pub struct TableCellState<'a> {
    pub position: TableCellPosition,
    /// Position of this cell's column in the current display order.
    pub display_column: usize,
    pub column: &'a TableColumn,
    /// Whether this cell's row is part of the retained row selection.
    pub row_selected: bool,
    /// Whether this cell is the active cell.
    pub selected: bool,
    /// Whether this cell is the open inline editor.
    pub editing: bool,
}

/// Structural geometry retained by one virtualized table.
///
/// No color, typography, padding, border, radius, shadow, or interaction-state paint is retained.
/// A `header_height` of zero mounts no header row at all, for a list that has nothing to label.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TableLayout {
    pub header_height: f32,
    pub row_height: f32,
}

impl TableLayout {
    pub fn new(header_height: f32, row_height: f32) -> Self {
        Self {
            header_height: sanitized_header_height(header_height),
            row_height: finite_clamped(row_height, 20.0, 256.0, 32.0),
        }
    }

    pub fn row_height(mut self, height: f32) -> Self {
        self.row_height = finite_clamped(height, 20.0, 256.0, 32.0);
        self
    }

    /// Height of the header row, or zero for no header row.
    pub fn header_height(mut self, height: f32) -> Self {
        self.header_height = sanitized_header_height(height);
        self
    }

    fn sanitized(mut self) -> Self {
        self.header_height = sanitized_header_height(self.header_height);
        self.row_height = finite_clamped(self.row_height, 20.0, 256.0, 32.0);
        self
    }
}

impl Default for TableLayout {
    fn default() -> Self {
        Self::new(34.0, 32.0)
    }
}

/// Retained scroll, cell selection, and sort state for a virtualized table.
///
/// The application owns row data and applies [`Self::sort`] to its ordering. QuickGUI retains only
/// constant-size interaction state plus the existing sparse [`ListState`] metrics, mounts visible
/// rows, and owns no timer or idle scheduler source.
pub struct TableState {
    row_count: usize,
    selected: Option<TableCellPosition>,
    selection: TableSelection,
    selection_mode: TableSelectionMode,
    selection_version: u64,
    anchor: Option<usize>,
    editing: Option<TableCellPosition>,
    order: Vec<u16>,
    column_ids: Vec<ElementId>,
    sizes: Vec<TableColumnSize>,
    sort: Option<TableSort>,
    list: ListState,
    layout: TableLayout,
}

/// One resizable column's retained pixel geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
struct TableColumnSize {
    column: ElementId,
    width: f32,
    minimum: f32,
}

impl fmt::Debug for TableState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TableState")
            .field("row_count", &self.row_count)
            .field("selected", &self.selected)
            .field("selection", &self.selection)
            .field("selection_mode", &self.selection_mode)
            .field("editing", &self.editing)
            .field("order", &self.order)
            .field("sizes", &self.sizes)
            .field("sort", &self.sort)
            .field("list", &self.list)
            .field("layout", &self.layout)
            .finish()
    }
}

impl TableState {
    pub fn new(row_count: usize) -> Self {
        assert!(
            row_count <= MAX_TABLE_ROWS,
            "a table supports at most {MAX_TABLE_ROWS} rows"
        );
        let layout = TableLayout::default();
        Self {
            row_count,
            selected: None,
            selection: TableSelection::new(),
            selection_mode: TableSelectionMode::Single,
            selection_version: 0,
            anchor: None,
            editing: None,
            order: Vec::new(),
            column_ids: Vec::new(),
            sizes: Vec::new(),
            sort: None,
            list: ListState::new(row_count, layout.row_height).with_overscan(2),
            layout,
        }
    }

    /// Allow Shift ranges, platform-modified toggles, and Select All.
    pub fn with_selection_mode(mut self, mode: TableSelectionMode) -> Self {
        self.selection_mode = mode;
        self
    }

    pub fn with_layout(mut self, layout: TableLayout) -> Self {
        self.layout = layout.sanitized();
        self.list = ListState::new(self.row_count, self.layout.row_height).with_overscan(2);
        self
    }

    pub const fn layout(&self) -> TableLayout {
        self.layout
    }

    pub fn set_layout(&mut self, layout: TableLayout) -> bool {
        let layout = layout.sanitized();
        if self.layout == layout {
            return false;
        }
        if self.layout.row_height != layout.row_height {
            let viewport = self.list.viewport_size();
            let offset = self.list.logical_scroll_top();
            self.list = ListState::new(self.row_count, layout.row_height).with_overscan(2);
            self.list.set_viewport_size(viewport.width, viewport.height);
            self.list.scroll_to(offset);
        }
        self.layout = layout;
        true
    }

    pub const fn row_count(&self) -> usize {
        self.row_count
    }

    pub fn set_row_count(&mut self, row_count: usize) -> bool {
        assert!(
            row_count <= MAX_TABLE_ROWS,
            "a table supports at most {MAX_TABLE_ROWS} rows"
        );
        if self.row_count == row_count {
            return false;
        }
        self.row_count = row_count;
        self.list.set_item_count(row_count);
        if let Some(selected) = &mut self.selected
            && selected.row >= row_count
        {
            self.selected = row_count.checked_sub(1).map(|row| TableCellPosition {
                row,
                column: selected.column,
            });
        }
        let clamped = self.selection.clamp(row_count);
        self.mark_selection(clamped);
        if self.anchor.is_some_and(|anchor| anchor >= row_count) {
            self.anchor = None;
        }
        if self.editing.is_some_and(|editing| editing.row >= row_count) {
            self.editing = None;
        }
        true
    }

    pub const fn selection_mode(&self) -> TableSelectionMode {
        self.selection_mode
    }

    pub fn set_selection_mode(&mut self, mode: TableSelectionMode) -> bool {
        if self.selection_mode == mode {
            return false;
        }
        self.selection_mode = mode;
        if mode == TableSelectionMode::Single {
            let single = self.selected.map(|cell| cell.row);
            return match single {
                Some(row) => self.mutate_selection(|selection| selection.set_single(row)),
                None => self.mutate_selection(TableSelection::clear),
            };
        }
        true
    }

    /// The retained row selection.
    pub const fn selection(&self) -> &TableSelection {
        &self.selection
    }

    /// A counter bumped once every time the retained selection actually changes.
    ///
    /// Comparing this across a frame is cheaper than cloning the selection, and it is what the
    /// table itself uses to decide whether to dispatch [`TableSelectionChanged`].
    pub const fn selection_version(&self) -> u64 {
        self.selection_version
    }

    fn mutate_selection(&mut self, mutate: impl FnOnce(&mut TableSelection) -> bool) -> bool {
        let changed = mutate(&mut self.selection);
        self.mark_selection(changed)
    }

    fn mark_selection(&mut self, changed: bool) -> bool {
        if changed {
            self.selection_version = self.selection_version.wrapping_add(1);
        }
        changed
    }

    /// Reconcile the retained display order and resizable widths with a column declaration.
    ///
    /// New columns are appended in declaration order, removed columns are dropped, and a retained
    /// width survives every rebuild so a rendered frame never discards a user's drag.
    fn sync_columns(&mut self, columns: &[TableColumn]) {
        let ids = columns.iter().map(|column| column.id).collect::<Vec<_>>();
        if self.column_ids != ids {
            let previous = self
                .order
                .iter()
                .filter_map(|index| self.column_ids.get(usize::from(*index)).copied())
                .collect::<Vec<_>>();
            let mut order = Vec::with_capacity(ids.len());
            for id in previous {
                if let Some(position) = ids.iter().position(|declared| *declared == id) {
                    order.push(position as u16);
                }
            }
            for index in 0..ids.len() as u16 {
                if !order.contains(&index) {
                    order.push(index);
                }
            }
            self.order = order;
            self.column_ids = ids;
        }
        self.sizes.retain(|size| {
            columns
                .iter()
                .any(|column| column.id == size.column && column.is_resizable())
        });
        for column in columns.iter().filter(|column| column.is_resizable()) {
            let width = column.width.unwrap_or(MIN_TABLE_COLUMN_WIDTH);
            match self.sizes.iter_mut().find(|size| size.column == column.id) {
                Some(size) => {
                    size.minimum = column.minimum_width;
                    size.width = clamped_column_width(size.width, size.minimum);
                }
                None => self.sizes.push(TableColumnSize {
                    column: column.id,
                    width: clamped_column_width(width, column.minimum_width),
                    minimum: column.minimum_width,
                }),
            }
        }
    }

    /// Replace the retained row selection, dropping rows past the current row count.
    pub fn set_selection(&mut self, selection: TableSelection) -> bool {
        let mut selection = selection;
        selection.clamp(self.row_count);
        if self.selection == selection {
            return false;
        }
        self.selection = selection;
        true
    }

    /// The row a Shift range extends from.
    pub const fn selection_anchor(&self) -> Option<usize> {
        self.anchor
    }

    pub fn is_row_selected(&self, row: usize) -> bool {
        self.selection.contains(row)
    }

    /// Select exactly one row and anchor future Shift ranges there.
    pub fn select_row(&mut self, row: usize) -> bool {
        if row >= self.row_count {
            return false;
        }
        self.anchor = Some(row);
        self.mutate_selection(|selection| selection.set_single(row))
    }

    /// Add or remove one row from a multiple selection.
    ///
    /// A single-selection table selects the row instead of toggling it, so the platform-modified
    /// click of a single-selection list behaves like an ordinary click.
    pub fn toggle_row_selection(&mut self, row: usize) -> bool {
        if row >= self.row_count {
            return false;
        }
        self.anchor = Some(row);
        match self.selection_mode {
            TableSelectionMode::Single => {
                self.mutate_selection(|selection| selection.set_single(row))
            }
            TableSelectionMode::Multiple => {
                self.mutate_selection(|selection| selection.toggle(row))
            }
        }
    }

    /// Replace the selection with the range between the anchor and `row`.
    pub fn select_row_range(&mut self, row: usize) -> bool {
        if row >= self.row_count {
            return false;
        }
        if self.selection_mode == TableSelectionMode::Single {
            return self.select_row(row);
        }
        let anchor = self.anchor.unwrap_or(row).min(self.row_count - 1);
        self.anchor = Some(anchor);
        self.mutate_selection(|selection| selection.set_range(anchor, row))
    }

    /// Select every row. A single-selection table refuses.
    pub fn select_all_rows(&mut self) -> bool {
        if self.selection_mode == TableSelectionMode::Single || self.row_count == 0 {
            return false;
        }
        let last = self.row_count - 1;
        self.mutate_selection(|selection| selection.set_range(0, last))
    }

    pub fn clear_row_selection(&mut self) -> bool {
        self.anchor = None;
        self.mutate_selection(TableSelection::clear)
    }

    /// The open inline editor, if any.
    pub const fn editing_cell(&self) -> Option<TableCellPosition> {
        self.editing
    }

    /// Open an inline editor over one cell and make it the active cell.
    ///
    /// The application owns the editor element and its draft value; QuickGUI retains only which
    /// cell is being edited, gives that cell its own key context, and reports Return and Escape.
    pub fn begin_edit(&mut self, position: TableCellPosition, column_count: usize) -> bool {
        if position.row >= self.row_count || position.column >= column_count {
            return false;
        }
        let changed = self.editing != Some(position);
        self.editing = Some(position);
        self.select_cell(position, column_count) || changed
    }

    /// Close the open editor. Returns the cell that was being edited.
    pub fn end_edit(&mut self) -> Option<TableCellPosition> {
        self.editing.take()
    }

    /// The retained width of a resizable column.
    pub fn column_width(&self, column: ElementId) -> Option<f32> {
        self.sizes
            .iter()
            .find(|size| size.column == column)
            .map(|size| size.width)
    }

    /// Replace one resizable column's width, clamped to its declared minimum and the table's
    /// maximum. A column that is not resizable is ignored.
    pub fn set_column_width(&mut self, column: ElementId, width: f32) -> bool {
        let Some(size) = self.sizes.iter_mut().find(|size| size.column == column) else {
            return false;
        };
        let width = clamped_column_width(width, size.minimum);
        if (size.width - width).abs() < f32::EPSILON {
            return false;
        }
        size.width = width;
        true
    }

    /// Move one resizable column's trailing edge by a logical-pixel delta.
    pub fn resize_column(&mut self, column: ElementId, delta: f32) -> bool {
        let Some(size) = self.sizes.iter().find(|size| size.column == column) else {
            return false;
        };
        if !delta.is_finite() {
            return false;
        }
        self.set_column_width(column, size.width + delta)
    }

    /// The declared column indices in their current display order.
    pub fn column_order(&self) -> &[u16] {
        &self.order
    }

    /// Move one declared column to a different position in the display order.
    pub fn move_column(&mut self, column_index: usize, delta: isize) -> bool {
        let Ok(column_index) = u16::try_from(column_index) else {
            return false;
        };
        let Some(from) = self.order.iter().position(|index| *index == column_index) else {
            return false;
        };
        let target = from as isize + delta;
        if target < 0 || target >= self.order.len() as isize {
            return false;
        }
        let target = target as usize;
        if target == from {
            return false;
        }
        let column = self.order.remove(from);
        self.order.insert(target, column);
        true
    }

    pub const fn selected_cell(&self) -> Option<TableCellPosition> {
        self.selected
    }

    pub fn selected_row(&self) -> Option<usize> {
        self.selected.map(|cell| cell.row)
    }

    pub const fn sort(&self) -> Option<TableSort> {
        self.sort
    }

    pub fn set_sort(&mut self, sort: Option<TableSort>) -> bool {
        if self.sort == sort {
            return false;
        }
        self.sort = sort;
        true
    }

    pub fn clear_selection(&mut self) -> bool {
        self.selected.take().is_some()
    }

    /// Make one cell active and collapse the row selection onto its row.
    ///
    /// This is the unmodified click and arrow-key behavior of every desktop list. Use
    /// [`Self::set_selection`] or [`Self::toggle_row_selection`] to change selection without
    /// moving the active cell.
    pub fn select_cell(&mut self, position: TableCellPosition, column_count: usize) -> bool {
        let moved = self.set_active_cell(position, column_count);
        if position.row >= self.row_count {
            return moved;
        }
        let selected = self.select_row(position.row);
        moved || selected
    }

    fn set_active_cell(&mut self, position: TableCellPosition, column_count: usize) -> bool {
        if position.row >= self.row_count || position.column >= column_count {
            return false;
        }
        let changed = self.selected != Some(position);
        self.selected = Some(position);
        self.list.scroll_to_reveal_item(position.row) || changed
    }

    pub fn visible_rows(&self) -> std::ops::Range<usize> {
        self.list.visible_rows().range
    }

    pub fn list_state(&self) -> &ListState {
        &self.list
    }

    pub fn focus_handle(id: impl Into<ElementId>) -> FocusHandle {
        FocusHandle::new(id)
    }

    pub fn row_id(id: impl Into<ElementId>, row: usize) -> ElementId {
        derived_table_id(id.into(), TABLE_ROW_ID_TAG, row as u64, 0)
    }

    pub fn cell_id(id: impl Into<ElementId>, position: TableCellPosition) -> ElementId {
        derived_table_id(
            id.into(),
            TABLE_CELL_ID_TAG,
            position.row as u64,
            position.column as u64,
        )
    }

    pub fn column_header_id(id: impl Into<ElementId>, column: ElementId) -> ElementId {
        derived_table_id(id.into(), TABLE_HEADER_ID_TAG, column.as_u64(), 0)
    }

    /// The stable identity of one column's resize handle.
    pub fn resize_handle_id(id: impl Into<ElementId>, column: ElementId) -> ElementId {
        derived_table_id(id.into(), TABLE_RESIZE_ID_TAG, column.as_u64(), 0)
    }

    /// Build a complete unstyled virtualized, sortable, keyboard-navigable table.
    ///
    /// `render_header` owns every visible header declaration, including label, sort indicator, and
    /// the placement of the resize handle QuickGUI hands it. `render_cell` owns every visible cell
    /// declaration, including the inline editor it mounts while `editing` is set, and runs only for
    /// mounted rows. QuickGUI decorates those elements with fixed grid geometry, identities,
    /// pointer/keyboard behavior, and accessibility semantics but adds no paint. `activate`
    /// receives the current logical cell when Return is dispatched outside an editor; selection,
    /// sort, width, order, and editing state remain directly readable from `access`.
    #[allow(clippy::too_many_arguments)]
    pub fn element<V, H, E, RenderHeader, RenderCell, Activate>(
        &mut self,
        cx: &mut ViewContext<'_, V>,
        id: impl Into<ElementId>,
        columns: &[TableColumn],
        access: fn(&mut V) -> &mut TableState,
        render_header: RenderHeader,
        render_cell: RenderCell,
        activate: Activate,
    ) -> Element
    where
        V: 'static,
        H: IntoElement,
        E: IntoElement,
        RenderHeader: FnMut(TableHeaderState<'_>) -> H,
        RenderCell: FnMut(TableCellState<'_>) -> E,
        Activate: Fn(&mut V, TableCellPosition, &mut EventContext) + Clone + 'static,
    {
        self.element_with(
            cx,
            id,
            columns,
            StateAccessor::from(access),
            render_header,
            render_cell,
            activate,
        )
    }

    /// Build the table against a per-instance retained-state accessor.
    ///
    /// A host that renders many declared tables through one view passes an accessor that captures
    /// which [`TableState`] each registered listener resolves. Every row is a plain container;
    /// [`Self::element_with_rows`] lets the caller paint the rows too.
    #[allow(clippy::too_many_arguments)]
    pub fn element_with<V, H, E, RenderHeader, RenderCell, Activate>(
        &mut self,
        cx: &mut ViewContext<'_, V>,
        id: impl Into<ElementId>,
        columns: &[TableColumn],
        access_source: StateAccessor<V, TableState>,
        render_header: RenderHeader,
        render_cell: RenderCell,
        activate: Activate,
    ) -> Element
    where
        V: 'static,
        H: IntoElement,
        E: IntoElement,
        RenderHeader: FnMut(TableHeaderState<'_>) -> H,
        RenderCell: FnMut(TableCellState<'_>) -> E,
        Activate: Fn(&mut V, TableCellPosition, &mut EventContext) + Clone + 'static,
    {
        self.element_with_rows(
            cx,
            id,
            columns,
            access_source,
            |_| div(),
            render_header,
            render_cell,
            activate,
        )
    }

    /// Build the table with a caller-owned container for every mounted row.
    ///
    /// `render_row` returns the element a row's cells are laid out in. QuickGUI adds the grid
    /// tracks, the row height, the row identity, and the collection semantics on top of it, so the
    /// caller declares only the row's paint — a background, a divider, a hover state, or a
    /// [`selected_style`](Element::selected_style) that paints while the row is selected — never
    /// its layout. The renderer runs only for mounted rows.
    #[allow(clippy::too_many_arguments)]
    pub fn element_with_rows<V, R, H, E, RenderRow, RenderHeader, RenderCell, Activate>(
        &mut self,
        cx: &mut ViewContext<'_, V>,
        id: impl Into<ElementId>,
        columns: &[TableColumn],
        access_source: StateAccessor<V, TableState>,
        mut render_row: RenderRow,
        mut render_header: RenderHeader,
        mut render_cell: RenderCell,
        activate: Activate,
    ) -> Element
    where
        V: 'static,
        R: IntoElement,
        H: IntoElement,
        E: IntoElement,
        RenderRow: FnMut(TableRowState) -> R,
        RenderHeader: FnMut(TableHeaderState<'_>) -> H,
        RenderCell: FnMut(TableCellState<'_>) -> E,
        Activate: Fn(&mut V, TableCellPosition, &mut EventContext) + Clone + 'static,
    {
        assert_table_columns(columns);
        let id = id.into();
        let column_count = columns.len();
        self.sync_columns(columns);
        self.normalize_selection(column_count);
        let layout = self.layout;
        let root_focus = Self::focus_handle(id);

        let access = access_source.clone();
        let previous_row = cx.action_listener(id, move |view, _: &TablePreviousRow, cx| {
            selection_aware(view, cx, &access, |state| {
                state.move_row(false, column_count)
            });
        });
        let access = access_source.clone();
        let next_row = cx.action_listener(id, move |view, _: &TableNextRow, cx| {
            selection_aware(view, cx, &access, |state| {
                state.move_row(true, column_count)
            });
        });
        let access = access_source.clone();
        let previous_column = cx.action_listener(id, move |view, _: &TablePreviousColumn, cx| {
            selection_aware(view, cx, &access, |state| {
                state.move_active_column(false, column_count)
            });
        });
        let access = access_source.clone();
        let next_column = cx.action_listener(id, move |view, _: &TableNextColumn, cx| {
            selection_aware(view, cx, &access, |state| {
                state.move_active_column(true, column_count)
            });
        });
        let access = access_source.clone();
        let page_up = cx.action_listener(id, move |view, _: &TablePageUp, cx| {
            selection_aware(view, cx, &access, |state| {
                state.move_page(false, column_count)
            });
        });
        let access = access_source.clone();
        let page_down = cx.action_listener(id, move |view, _: &TablePageDown, cx| {
            selection_aware(view, cx, &access, |state| {
                state.move_page(true, column_count)
            });
        });
        let access = access_source.clone();
        let first = cx.action_listener(id, move |view, _: &TableFirstRow, cx| {
            selection_aware(view, cx, &access, |state| {
                state.select_edge(false, column_count)
            });
        });
        let access = access_source.clone();
        let last = cx.action_listener(id, move |view, _: &TableLastRow, cx| {
            selection_aware(view, cx, &access, |state| {
                state.select_edge(true, column_count)
            });
        });
        let access = access_source.clone();
        let toggle_selection = cx.action_listener(id, move |view, _: &TableToggleSelection, cx| {
            selection_aware(view, cx, &access, |state| {
                state
                    .selected
                    .is_some_and(|selected| state.toggle_row_selection(selected.row))
            });
        });
        let access = access_source.clone();
        let extend_up = cx.action_listener(id, move |view, _: &TableExtendSelectionUp, cx| {
            selection_aware(view, cx, &access, |state| {
                state.extend_selection(false, column_count)
            });
        });
        let access = access_source.clone();
        let extend_down = cx.action_listener(id, move |view, _: &TableExtendSelectionDown, cx| {
            selection_aware(view, cx, &access, |state| {
                state.extend_selection(true, column_count)
            });
        });
        let access = access_source.clone();
        let select_all = cx.action_listener(id, move |view, _: &TableSelectAll, cx| {
            selection_aware(view, cx, &access, TableState::select_all_rows);
        });
        let access = access_source.clone();
        let move_left = cx.action_listener(id, move |view, _: &TableMoveColumnLeft, cx| {
            let state = access.get(view);
            let Some(selected) = state.selected else {
                return;
            };
            if state.move_column(selected.column, -1) {
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let move_right = cx.action_listener(id, move |view, _: &TableMoveColumnRight, cx| {
            let state = access.get(view);
            let Some(selected) = state.selected else {
                return;
            };
            if state.move_column(selected.column, 1) {
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let commit_edit = cx.action_listener(id, move |view, _: &TableCommitEdit, cx| {
            if let Some(position) = access.get(view).end_edit() {
                cx.focus(root_focus);
                cx.dispatch_action(TableEditEnded {
                    position,
                    committed: true,
                });
                cx.invalidate();
            }
        });
        let access = access_source.clone();
        let cancel_edit = cx.action_listener(id, move |view, _: &TableCancelEdit, cx| {
            if let Some(position) = access.get(view).end_edit() {
                cx.focus(root_focus);
                cx.dispatch_action(TableEditEnded {
                    position,
                    committed: false,
                });
                cx.invalidate();
            }
        });
        let confirm_activate = activate.clone();
        let access = access_source.clone();
        let confirm = cx.action_listener(id, move |view, _: &TableActivate, cx| {
            if let Some(selected) = access.get(view).selected_cell() {
                confirm_activate(view, selected, cx);
            }
        });

        let order = self.order.clone();
        let tracks = order
            .iter()
            .map(|index| {
                let column = &columns[usize::from(*index)];
                match self.column_width(column.id) {
                    Some(width) => GridTrack::px(width),
                    None => column.track,
                }
            })
            .collect::<Vec<_>>();

        let mut headers = Vec::with_capacity(column_count);
        for (display_index, column_index) in
            order.iter().map(|index| usize::from(*index)).enumerate()
        {
            let column = &columns[column_index];
            let header_id = Self::column_header_id(id, column.id);
            let sort_direction = self
                .sort
                .filter(|sort| sort.column == column.id)
                .map(|sort| sort.direction);
            let width = self.column_width(column.id);
            let resize_handle = width.map(|width| {
                let handle_id = Self::resize_handle_id(id, column.id);
                let column_id = column.id;
                let minimum = column.minimum_width;
                let access = access_source.clone();
                let drag = cx.pointer_listener(handle_id, move |view, event, cx| {
                    if event.phase == PointerPhase::Down {
                        return;
                    }
                    if access.get(view).resize_column(column_id, event.delta.x) {
                        cx.invalidate();
                    }
                });
                let access = access_source.clone();
                let smaller =
                    cx.action_listener(handle_id, move |view, _: &TableResizeColumnSmaller, cx| {
                        if access
                            .get(view)
                            .resize_column(column_id, -TABLE_COLUMN_RESIZE_STEP)
                        {
                            cx.invalidate();
                        }
                    });
                let access = access_source.clone();
                let larger =
                    cx.action_listener(handle_id, move |view, _: &TableResizeColumnLarger, cx| {
                        if access
                            .get(view)
                            .resize_column(column_id, TABLE_COLUMN_RESIZE_STEP)
                        {
                            cx.invalidate();
                        }
                    });
                div()
                    .id(handle_id)
                    .on_pointer(drag)
                    .on_action(smaller)
                    .on_action(larger)
                    .accessibility_role(AccessibilityRole::SplitterHandle)
                    .accessibility_label(column.label.clone())
                    .accessibility_orientation(AccessibilityOrientation::Vertical)
                    .accessibility_controls(header_id)
                    .accessibility_value_range(
                        AccessibilityValueRange::new(
                            f64::from(width),
                            f64::from(minimum),
                            f64::from(MAX_TABLE_COLUMN_WIDTH),
                        )
                        .step(f64::from(TABLE_COLUMN_RESIZE_STEP)),
                    )
                    .focusable()
                    .tab_index(0)
                    .key_context(TABLE_HANDLE_KEY_CONTEXT)
                    .cursor_col_resize()
                    .app_region_no_drag()
                    .user_select_none()
            });

            let mut header = render_header(TableHeaderState {
                column_index,
                display_index,
                column,
                sort_direction,
                width,
                resize_handle,
            })
            .into_element()
            .id(header_id)
            .accessibility_role(AccessibilityRole::ColumnHeader)
            .accessibility_label(column.label.clone())
            .accessibility_column_index(display_index)
            .h(layout.header_height)
            .min_w(0.0)
            .flex_row()
            .items_center()
            .overflow_hidden()
            .cursor_default()
            .app_region_no_drag()
            .user_select_none();
            header = align_cell(header, column.align);
            if let Some(direction) = sort_direction {
                header = header.accessibility_sort_direction(direction.accessibility());
            }
            if column.sortable {
                let column_id = column.id;
                let access = access_source.clone();
                let clicked = cx.listener(header_id, move |view, cx| {
                    if access.get(view).toggle_sort(column_id) {
                        cx.focus(root_focus);
                        cx.invalidate();
                    }
                });
                header = header.on_click(clicked).tab_index(-1);
            }
            headers.push(header);
        }

        // A zero header height mounts no header row: the grid then starts at logical row 1 with
        // nothing above it, and the declared header renderers are simply never invoked.
        let header = (layout.header_height > 0.0).then(|| {
            div()
                .accessibility_role(AccessibilityRole::Row)
                .accessibility_row_index(0)
                .grid()
                .grid_template_columns(tracks.clone())
                .h(layout.header_height)
                .flex_none()
                .app_region_no_drag()
                .children(std::mem::take(&mut headers))
        });

        let selected = self.selected;
        let editing = self.editing;
        let selection = self.selection.clone();
        let list = self.list.clone();
        let visible = list.visible_rows().range;
        let rows = list.render_rows(visible, |row_index| {
            let row_selected = selection.contains(row_index);
            let row_id = Self::row_id(id, row_index);
            let mut cells = Vec::with_capacity(column_count);
            for (display_column, column_index) in
                order.iter().map(|index| usize::from(*index)).enumerate()
            {
                let column = &columns[column_index];
                let position = TableCellPosition {
                    row: row_index,
                    column: column_index,
                };
                let cell_id = Self::cell_id(id, position);
                let cell_selected = selected == Some(position);
                let cell_editing = editing == Some(position);
                let access = access_source.clone();
                let clicked = cx.listener(cell_id, move |view, cx| {
                    if access.get(view).set_active_cell(position, column_count) {
                        cx.invalidate();
                    }
                    cx.focus(root_focus);
                });
                let access = access_source.clone();
                let pressed = cx.mouse_down_listener(cell_id, move |view, event, cx| {
                    let state = access.get(view);
                    let before = state.selection_version;
                    let changed = if event.modifiers.contains(Modifiers::SHIFT) {
                        state.select_row_range(position.row)
                    } else if event
                        .modifiers
                        .intersects(Modifiers::SUPER | Modifiers::CONTROL)
                    {
                        state.toggle_row_selection(position.row)
                    } else {
                        state.select_row(position.row)
                    };
                    if changed {
                        cx.invalidate();
                    }
                    if access.get(view).selection_version != before {
                        cx.dispatch_action(TableSelectionChanged);
                    }
                });
                let role = if column.row_header {
                    AccessibilityRole::RowHeader
                } else {
                    AccessibilityRole::GridCell
                };
                let mut cell = render_cell(TableCellState {
                    position,
                    display_column,
                    column,
                    row_selected,
                    selected: cell_selected,
                    editing: cell_editing,
                })
                .into_element()
                .id(cell_id)
                .on_click(clicked)
                .on_mouse_down(MouseButton::Left, pressed)
                .tab_index(-1)
                .accessibility_role(role)
                .accessibility_row_index(row_index + 1)
                .accessibility_column_index(display_column)
                .selected(cell_selected)
                .h(layout.row_height)
                .min_w(0.0)
                .flex_row()
                .items_center()
                .overflow_hidden()
                .cursor_default()
                .app_region_no_drag();
                if cell_editing {
                    cell = cell.key_context(TABLE_EDITOR_KEY_CONTEXT);
                }
                cell = align_cell(cell, column.align);
                cells.push(cell);
            }
            render_row(TableRowState {
                row: row_index,
                selected: row_selected,
            })
            .into_element()
            .id(row_id)
            .accessibility_role(AccessibilityRole::Row)
            .accessibility_row_index(row_index + 1)
            .selected(row_selected)
            .grid()
            .grid_template_columns(tracks.clone())
            .h(layout.row_height)
            .app_region_no_drag()
            .children(cells)
        });

        let body = div()
            .relative()
            .flex_1()
            .min_h(0.0)
            .w_full()
            .overflow_hidden()
            .variable_virtual_scroll(&list)
            .app_region_no_drag()
            .child(rows);

        let mut root = div()
            .id(id)
            .track_focus(root_focus)
            .key_context(TABLE_KEY_CONTEXT)
            .on_action(previous_row)
            .on_action(next_row)
            .on_action(previous_column)
            .on_action(next_column)
            .on_action(page_up)
            .on_action(page_down)
            .on_action(first)
            .on_action(last)
            .on_action(toggle_selection)
            .on_action(extend_up)
            .on_action(extend_down)
            .on_action(select_all)
            .on_action(move_left)
            .on_action(move_right)
            .on_action(commit_edit)
            .on_action(cancel_edit)
            .on_action(confirm)
            .accessibility_role(AccessibilityRole::Grid)
            .accessibility_row_count(self.row_count.saturating_add(1))
            .accessibility_column_count(column_count)
            .accessibility_multiselectable(self.selection_mode == TableSelectionMode::Multiple)
            .size_full()
            .min_w(0.0)
            .min_h(0.0)
            .flex_col()
            .overflow_hidden()
            .app_region_no_drag()
            .children(header)
            .child(body);
        if let Some(selected) = self.selected {
            root = root.accessibility_active_descendant(Self::cell_id(id, selected));
        }
        root
    }

    /// Clamp the active cell into the current grid without moving the viewport.
    ///
    /// This runs on every build, and a build happens whenever a scroll moves the mounted slice,
    /// so it must never scroll: revealing the active cell here snapped every wheel or scrollbar
    /// scroll back to that cell on the frame that followed it. The keyboard and pointer paths
    /// that move the active cell reveal it through [`Self::select_cell`] instead.
    fn normalize_selection(&mut self, column_count: usize) {
        if self.row_count == 0 || column_count == 0 {
            self.selected = None;
            return;
        }
        let position = self.selected.unwrap_or_default();
        self.selected = Some(TableCellPosition {
            row: position.row.min(self.row_count - 1),
            column: position.column.min(column_count - 1),
        });
    }

    fn move_row(&mut self, forward: bool, column_count: usize) -> bool {
        self.normalize_selection(column_count);
        let Some(mut selected) = self.selected else {
            return false;
        };
        selected.row = if forward {
            selected.row.saturating_add(1).min(self.row_count - 1)
        } else {
            selected.row.saturating_sub(1)
        };
        self.select_cell(selected, column_count)
    }

    /// Move the active cell one column along the current display order.
    fn move_active_column(&mut self, forward: bool, column_count: usize) -> bool {
        self.normalize_selection(column_count);
        let Some(mut selected) = self.selected else {
            return false;
        };
        let display = self
            .order
            .iter()
            .position(|index| usize::from(*index) == selected.column)
            .unwrap_or(selected.column);
        let target = if forward {
            display.saturating_add(1).min(self.order.len().max(1) - 1)
        } else {
            display.saturating_sub(1)
        };
        selected.column = self
            .order
            .get(target)
            .map_or(selected.column, |index| usize::from(*index));
        self.select_cell(selected, column_count)
    }

    /// Move the active cell one row and extend the selection from its anchor.
    fn extend_selection(&mut self, forward: bool, column_count: usize) -> bool {
        self.normalize_selection(column_count);
        let Some(mut selected) = self.selected else {
            return false;
        };
        if self.selection_mode == TableSelectionMode::Single {
            return self.move_row(forward, column_count);
        }
        let anchor = self.anchor.unwrap_or(selected.row);
        self.anchor = Some(anchor);
        selected.row = if forward {
            selected.row.saturating_add(1).min(self.row_count - 1)
        } else {
            selected.row.saturating_sub(1)
        };
        let moved = self.set_active_cell(selected, column_count);
        let row = selected.row;
        let extended = self.mutate_selection(|selection| selection.set_range(anchor, row));
        moved || extended
    }

    fn move_page(&mut self, forward: bool, column_count: usize) -> bool {
        self.normalize_selection(column_count);
        let Some(mut selected) = self.selected else {
            return false;
        };
        let page = (self.list.viewport_size().height / self.layout.row_height)
            .floor()
            .max(1.0) as usize;
        selected.row = if forward {
            selected.row.saturating_add(page).min(self.row_count - 1)
        } else {
            selected.row.saturating_sub(page)
        };
        self.select_cell(selected, column_count)
    }

    fn select_edge(&mut self, end: bool, column_count: usize) -> bool {
        self.normalize_selection(column_count);
        let Some(mut selected) = self.selected else {
            return false;
        };
        selected.row = if end { self.row_count - 1 } else { 0 };
        self.select_cell(selected, column_count)
    }

    fn toggle_sort(&mut self, column: ElementId) -> bool {
        self.sort = Some(match self.sort {
            Some(TableSort {
                column: current,
                direction: TableSortDirection::Ascending,
            }) if current == column => TableSort {
                column,
                direction: TableSortDirection::Descending,
            },
            _ => TableSort {
                column,
                direction: TableSortDirection::Ascending,
            },
        });
        true
    }
}

fn assert_table_columns(columns: &[TableColumn]) {
    assert!(
        columns.len() <= MAX_TABLE_COLUMNS,
        "a table supports at most {MAX_TABLE_COLUMNS} columns"
    );
    for (index, column) in columns.iter().enumerate() {
        assert!(
            columns[..index].iter().all(|other| other.id != column.id),
            "table column IDs must be unique"
        );
    }
}

/// Apply one navigation or selection mutation and report what actually changed.
///
/// A gesture that changes nothing invalidates nothing and dispatches nothing, which is what keeps
/// a settled table at zero extra frames.
fn selection_aware<V: 'static>(
    view: &mut V,
    cx: &mut EventContext,
    access: &StateAccessor<V, TableState>,
    mutate: impl FnOnce(&mut TableState) -> bool,
) {
    let state = access.get(view);
    let before = state.selection_version;
    let changed = mutate(state);
    let selection_changed = state.selection_version != before;
    if changed {
        cx.invalidate();
    }
    if selection_changed {
        cx.dispatch_action(TableSelectionChanged);
    }
}

fn clamped_column_width(width: f32, minimum: f32) -> f32 {
    let minimum = if minimum.is_finite() {
        minimum.clamp(MIN_TABLE_COLUMN_WIDTH, MAX_TABLE_COLUMN_WIDTH)
    } else {
        MIN_TABLE_COLUMN_WIDTH
    };
    if width.is_finite() {
        width.clamp(minimum, MAX_TABLE_COLUMN_WIDTH)
    } else {
        minimum
    }
}

fn align_cell(element: Element, alignment: TableColumnAlign) -> Element {
    match alignment {
        TableColumnAlign::Start => element.justify_start(),
        TableColumnAlign::Center => element.justify_center(),
        TableColumnAlign::End => element.justify_end(),
    }
}

fn derived_table_id(parent: ElementId, tag: u64, first: u64, second: u64) -> ElementId {
    let mut hash = parent.as_u64()
        ^ tag
        ^ first.wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ second.wrapping_mul(0xd6e8_feb8_6659_fd93);
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    if hash == parent.as_u64() || hash == u64::MAX {
        hash ^= tag.rotate_left(13);
    }
    ElementId::new(hash)
}

/// Zero (or a value that rounds to it) means no header row; anything else keeps the ordinary
/// header bounds, and a non-finite or negative value falls back to the default height.
fn sanitized_header_height(height: f32) -> f32 {
    if !height.is_finite() || height < 0.0 {
        34.0
    } else if height < 1.0 {
        0.0
    } else {
        height.clamp(20.0, 256.0)
    }
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
    use crate::{
        Application, Color, MouseDownEvent, Point, View, WindowOptions, div, text, text_input,
    };

    #[test]
    fn layout_and_state_stay_bounded_and_preserve_logical_scroll_on_geometry_changes() {
        let mut state = TableState::new(100);
        state.list.set_viewport_size(400.0, 96.0);
        assert!(state.select_cell(TableCellPosition { row: 50, column: 1 }, 3));
        let before = state.list.logical_scroll_top();
        assert!(
            state.set_layout(
                TableLayout::default()
                    .row_height(f32::NAN)
                    .header_height(0.0)
            )
        );
        assert_eq!(state.layout.row_height, 32.0);
        assert_eq!(state.layout.header_height, 0.0);
        assert_eq!(TableLayout::new(-10.0, 24.0).header_height, 34.0);
        assert_eq!(TableLayout::new(f32::NAN, 24.0).header_height, 34.0);
        assert_eq!(TableLayout::new(0.5, 24.0).header_height, 0.0);
        assert_eq!(TableLayout::new(5.0, 24.0).header_height, 20.0);
        assert_eq!(state.list.logical_scroll_top().item_ix, before.item_ix);
        assert!(state.visible_rows().len() < state.row_count());
    }

    #[test]
    fn a_build_clamps_the_active_cell_without_moving_a_scrolled_viewport() {
        let mut state = TableState::new(100);
        state.list.set_viewport_size(400.0, 96.0);
        // Scrolled far from row 0 by a wheel or a scrollbar drag, with no active cell yet.
        assert!(state.list.scroll_to_pixels(1_600.0));
        let scrolled = state.list.scroll_offset();
        assert!(state.visible_rows().start > 0);

        // The build that follows any scroll normalizes the active cell but must leave the
        // viewport where the scroll put it; revealing row 0 here undid every scroll.
        state.normalize_selection(2);
        assert_eq!(
            state.selected_cell(),
            Some(TableCellPosition { row: 0, column: 0 })
        );
        assert_eq!(state.list.scroll_offset(), scrolled);

        // Moving the active cell is what reveals it.
        assert!(state.select_cell(TableCellPosition { row: 0, column: 1 }, 2));
        assert_eq!(state.list.scroll_offset(), 0.0);
        state.normalize_selection(1);
        assert_eq!(
            state.selected_cell(),
            Some(TableCellPosition { row: 0, column: 0 })
        );
    }

    #[test]
    fn selection_merges_ranges_splits_removals_and_refuses_to_fragment_past_its_bound() {
        let mut selection = TableSelection::new();
        assert!(selection.is_empty());
        assert!(selection.insert_range(4, 8));
        assert!(selection.insert_range(0, 2));
        assert_eq!(selection.ranges(), [(0, 2), (4, 8)]);
        assert_eq!(selection.len(), 8);

        // Touching ranges merge instead of accumulating entries.
        assert!(selection.insert_range(3, 3));
        assert_eq!(selection.ranges(), [(0, 8)]);
        assert!(selection.contains(5));
        assert!(!selection.contains(9));

        // Removing from the middle splits exactly once.
        assert!(selection.remove(4));
        assert_eq!(selection.ranges(), [(0, 3), (5, 8)]);
        assert_eq!(selection.len(), 8);

        assert!(selection.toggle(4));
        assert_eq!(selection.ranges(), [(0, 8)]);
        assert!(selection.toggle(4));
        assert_eq!(selection.ranges(), [(0, 3), (5, 8)]);

        // A million-row Select All is one range.
        assert!(selection.set_range(0, 999_999));
        assert_eq!(selection.ranges().len(), 1);
        assert_eq!(selection.len(), 1_000_000);

        // Fragmentation stops at the bound instead of growing without limit.
        let mut fragmented = TableSelection::new();
        for row in 0..MAX_TABLE_SELECTION_RANGES {
            assert!(fragmented.insert(row * 2));
        }
        assert_eq!(fragmented.ranges().len(), MAX_TABLE_SELECTION_RANGES);
        assert!(
            !fragmented.insert(MAX_TABLE_SELECTION_RANGES * 2),
            "an over-budget toggle is refused"
        );
        assert_eq!(fragmented.ranges().len(), MAX_TABLE_SELECTION_RANGES);
    }

    #[test]
    fn selection_state_follows_the_declared_mode_and_row_count() {
        let mut state = TableState::new(10);
        assert_eq!(state.selection_mode(), TableSelectionMode::Single);
        assert!(state.select_cell(TableCellPosition { row: 3, column: 0 }, 2));
        assert!(state.is_row_selected(3));
        assert_eq!(state.selection().len(), 1);

        // A single-selection table never accumulates rows.
        assert!(state.toggle_row_selection(5));
        assert_eq!(state.selection().ranges(), [(5, 5)]);
        assert!(!state.select_all_rows());

        assert!(state.set_selection_mode(TableSelectionMode::Multiple));
        assert!(state.select_row(1));
        assert!(state.select_row_range(4));
        assert_eq!(state.selection().ranges(), [(1, 4)]);
        assert!(state.toggle_row_selection(2));
        assert_eq!(state.selection().ranges(), [(1, 1), (3, 4)]);
        assert!(state.select_all_rows());
        assert_eq!(state.selection().len(), 10);

        let version = state.selection_version();
        assert!(!state.select_row(20), "a row past the end is refused");
        assert_eq!(state.selection_version(), version);

        assert!(state.set_row_count(4));
        assert_eq!(
            state.selection().ranges(),
            [(0, 3)],
            "shrinking the table drops rows that no longer exist"
        );
        assert!(state.clear_row_selection());
        assert!(state.selection().is_empty());
    }

    #[test]
    fn column_widths_and_display_order_survive_declaration_changes() {
        let columns = [
            TableColumn::new("name", "Name")
                .width(200.0)
                .minimum_width(80.0),
            TableColumn::new("kind", "Kind"),
            TableColumn::new("size", "Size").width(90.0),
        ];
        let mut state = TableState::new(4);
        state.sync_columns(&columns);
        assert_eq!(state.column_order(), [0, 1, 2]);
        assert_eq!(state.column_width("name".into()), Some(200.0));
        assert_eq!(state.column_width("kind".into()), None);

        assert!(state.resize_column("name".into(), -40.0));
        assert_eq!(state.column_width("name".into()), Some(160.0));
        assert!(state.resize_column("name".into(), -10_000.0));
        assert_eq!(
            state.column_width("name".into()),
            Some(80.0),
            "a drag stops at the declared minimum"
        );
        assert!(state.resize_column("name".into(), 100_000.0));
        assert_eq!(
            state.column_width("name".into()),
            Some(MAX_TABLE_COLUMN_WIDTH)
        );
        assert!(!state.resize_column("kind".into(), 10.0));
        assert!(!state.resize_column("name".into(), f32::NAN));

        assert!(state.move_column(2, -2));
        assert_eq!(state.column_order(), [2, 0, 1]);
        assert!(
            !state.move_column(2, -1),
            "the first column cannot move left"
        );
        assert!(!state.move_column(9, 1));

        // A rebuild that drops one column keeps the remaining order and retained widths.
        let fewer = [
            TableColumn::new("name", "Name")
                .width(200.0)
                .minimum_width(80.0),
            TableColumn::new("size", "Size").width(90.0),
        ];
        state.sync_columns(&fewer);
        assert_eq!(state.column_order(), [1, 0]);
        assert_eq!(
            state.column_width("name".into()),
            Some(MAX_TABLE_COLUMN_WIDTH)
        );
        assert_eq!(state.column_width("size".into()), Some(90.0));
    }

    #[test]
    fn inline_editing_retains_only_the_edited_cell() {
        let mut state = TableState::new(6);
        assert_eq!(state.editing_cell(), None);
        assert!(state.begin_edit(TableCellPosition { row: 2, column: 1 }, 3));
        assert_eq!(
            state.editing_cell(),
            Some(TableCellPosition { row: 2, column: 1 })
        );
        assert_eq!(
            state.selected_cell(),
            Some(TableCellPosition { row: 2, column: 1 }),
            "editing a cell makes it active"
        );
        assert!(!state.begin_edit(TableCellPosition { row: 99, column: 1 }, 3));
        assert_eq!(
            state.end_edit(),
            Some(TableCellPosition { row: 2, column: 1 })
        );
        assert_eq!(state.end_edit(), None);

        assert!(state.begin_edit(TableCellPosition { row: 5, column: 0 }, 3));
        assert!(state.set_row_count(3));
        assert_eq!(
            state.editing_cell(),
            None,
            "a removed row closes its editor"
        );
    }

    #[derive(Debug)]
    struct TableView {
        table: TableState,
        activated: Option<TableCellPosition>,
        selection_changes: usize,
        edits: Vec<(TableCellPosition, bool)>,
    }

    impl Default for TableView {
        fn default() -> Self {
            Self {
                table: TableState::new(100).with_selection_mode(TableSelectionMode::Multiple),
                activated: None,
                selection_changes: 0,
                edits: Vec::new(),
            }
        }
    }

    impl TableView {
        fn table(view: &mut Self) -> &mut TableState {
            &mut view.table
        }

        fn columns() -> [TableColumn; 3] {
            [
                TableColumn::new("name", "Name")
                    .track(GridTrack::fr(2.0))
                    .sortable(true)
                    .row_header(true),
                TableColumn::new("status", "Status")
                    .sortable(true)
                    .width(120.0)
                    .minimum_width(60.0),
                TableColumn::new("cpu", "CPU").align(TableColumnAlign::End),
            ]
        }
    }

    impl View for TableView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let columns = Self::columns();
            let selection_changed =
                cx.action_listener("table", |view: &mut Self, _: &TableSelectionChanged, cx| {
                    view.selection_changes += 1;
                    cx.invalidate();
                });
            let edit_ended =
                cx.action_listener("table", |view: &mut Self, action: &TableEditEnded, cx| {
                    view.edits.push((action.position, action.committed));
                    cx.invalidate();
                });
            self.table
                .element(
                    cx,
                    "table",
                    &columns,
                    Self::table,
                    |header| {
                        let mut element =
                            div().child(text(header.column.label().clone()).no_wrap());
                        if let Some(direction) = header.sort_direction {
                            element = element.child(text(match direction {
                                TableSortDirection::Ascending => "↑",
                                TableSortDirection::Descending => "↓",
                            }));
                        }
                        if let Some(handle) = header.resize_handle {
                            element = element.child(handle.w(6.0).h_full());
                        }
                        element
                    },
                    |cell| {
                        if cell.editing {
                            return div().child(text_input("draft").id("cell-editor").auto_focus());
                        }
                        div()
                            .bg(if cell.row_selected {
                                Color::rgb8(10, 20, 30)
                            } else {
                                Color::TRANSPARENT
                            })
                            .child(
                                text(format!("{}:{}", cell.position.row, cell.position.column))
                                    .no_wrap(),
                            )
                    },
                    |view, position, cx| {
                        view.activated = Some(position);
                        cx.invalidate();
                    },
                )
                .on_action(selection_changed)
                .on_action(edit_ended)
                .bg(Color::rgb8(1, 2, 3))
        }
    }

    #[test]
    fn table_uses_composite_focus_keyboard_sort_click_and_idle_paths() {
        let (mut cx, view) = Application::new()
            .bind_keys(table_key_bindings())
            .into_test_context(WindowOptions::default(), TableView::default())
            .unwrap();
        let window = view.window_handle();

        cx.simulate_keystrokes(window, "tab down right enter")
            .unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some("table".into()));
        assert_eq!(
            cx.read(view, |view| view.table.selected_cell()).unwrap(),
            Some(TableCellPosition { row: 1, column: 1 })
        );
        assert_eq!(
            cx.read(view, |view| view.activated).unwrap(),
            Some(TableCellPosition { row: 1, column: 1 })
        );

        let header = TableState::column_header_id("table", "status".into());
        cx.click(window, header).unwrap();
        assert_eq!(
            cx.read(view, |view| view.table.sort()).unwrap(),
            Some(TableSort {
                column: "status".into(),
                direction: TableSortDirection::Ascending,
            })
        );
        cx.click(window, header).unwrap();
        assert_eq!(
            cx.read(view, |view| view.table.sort()).unwrap(),
            Some(TableSort {
                column: "status".into(),
                direction: TableSortDirection::Descending,
            })
        );

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }

    #[test]
    fn multiple_selection_resize_reorder_and_inline_edit_use_existing_input_paths() {
        let (mut cx, view) = Application::new()
            .bind_keys(table_key_bindings())
            .into_test_context(WindowOptions::default(), TableView::default())
            .unwrap();
        let window = view.window_handle();

        cx.simulate_keystrokes(window, "tab").unwrap();
        assert_eq!(cx.focused(window).unwrap(), Some("table".into()));
        assert!(
            cx.read(view, |view| view.table.selection().is_empty())
                .unwrap(),
            "mounting a table selects nothing until the user acts"
        );

        // Shift extends from the anchor; the table reports one selection change per gesture.
        cx.simulate_keystrokes(window, "shift-down shift-down")
            .unwrap();
        assert_eq!(
            cx.read(view, |view| view.table.selection().ranges().to_vec())
                .unwrap(),
            vec![(0, 2)]
        );
        assert_eq!(
            cx.read(view, |view| view.table.selected_cell().unwrap().row)
                .unwrap(),
            2
        );
        assert!(cx.read(view, |view| view.selection_changes).unwrap() >= 2);

        // Space toggles exactly the active row out of the range.
        cx.simulate_keystrokes(window, "space").unwrap();
        assert_eq!(
            cx.read(view, |view| view.table.selection().ranges().to_vec())
                .unwrap(),
            vec![(0, 1)]
        );

        // Select All is one retained range over every row.
        cx.simulate_keystrokes(window, "cmd-a").unwrap();
        assert_eq!(
            cx.read(view, |view| view.table.selection().ranges().to_vec())
                .unwrap(),
            vec![(0, 99)]
        );

        // A platform-modified press toggles one row through the ordinary mouse path.
        let cell = TableState::cell_id("table", TableCellPosition { row: 4, column: 0 });
        cx.simulate_mouse_down(
            window,
            cell,
            MouseDownEvent {
                button: crate::MouseButton::Left,
                position: Point::new(10.0, 10.0),
                modifiers: Modifiers::SUPER,
                click_count: 1,
                first_mouse: false,
            },
        )
        .unwrap();
        assert_eq!(
            cx.read(view, |view| view.table.selection().ranges().to_vec())
                .unwrap(),
            vec![(0, 3), (5, 99)]
        );

        // Keyboard column reorder moves the active column through the display order.
        cx.simulate_keystrokes(window, "alt-right").unwrap();
        assert_eq!(
            cx.read(view, |view| view.table.column_order().to_vec())
                .unwrap(),
            vec![1, 0, 2]
        );
        cx.simulate_keystrokes(window, "alt-left").unwrap();
        assert_eq!(
            cx.read(view, |view| view.table.column_order().to_vec())
                .unwrap(),
            vec![0, 1, 2]
        );

        // The resize handle is its own Tab stop with its own arrow-key meaning.
        let handle = TableState::resize_handle_id("table", "status".into());
        cx.focus(window, handle).unwrap();
        cx.simulate_keystrokes(window, "left left").unwrap();
        assert_eq!(
            cx.read(view, |view| view.table.column_width("status".into()))
                .unwrap(),
            Some(120.0 - 2.0 * TABLE_COLUMN_RESIZE_STEP)
        );
        cx.simulate_keystrokes(window, "right").unwrap();
        assert_eq!(
            cx.read(view, |view| view.table.column_width("status".into()))
                .unwrap(),
            Some(120.0 - TABLE_COLUMN_RESIZE_STEP)
        );

        // Inline editing: Escape abandons, Return commits, and both report the edited cell.
        let editing = TableCellPosition { row: 4, column: 0 };
        cx.update(view, |view, cx| {
            view.table.begin_edit(editing, 3);
            cx.invalidate();
        })
        .unwrap();
        cx.focus(window, "cell-editor").unwrap();
        cx.simulate_keystrokes(window, "escape").unwrap();
        assert_eq!(
            cx.read(view, |view| view.table.editing_cell()).unwrap(),
            None
        );
        assert_eq!(
            cx.read(view, |view| view.edits.clone()).unwrap(),
            vec![(editing, false)]
        );

        cx.update(view, |view, cx| {
            view.table.begin_edit(editing, 3);
            cx.invalidate();
        })
        .unwrap();
        cx.focus(window, "cell-editor").unwrap();
        cx.simulate_keystrokes(window, "enter").unwrap();
        assert_eq!(
            cx.read(view, |view| view.table.editing_cell()).unwrap(),
            None
        );
        assert_eq!(
            cx.read(view, |view| view.edits.clone()).unwrap(),
            vec![(editing, false), (editing, true)]
        );

        let renders = cx.render_count(window).unwrap();
        cx.run_until_idle().unwrap();
        assert_eq!(cx.render_count(window).unwrap(), renders);
    }

    #[test]
    #[should_panic(expected = "table column IDs must be unique")]
    fn duplicate_column_ids_fail_before_building_ambiguous_cells() {
        assert_table_columns(&[TableColumn::new("same", "A"), TableColumn::new("same", "B")]);
    }

    #[test]
    fn table_bindings_are_contextual_and_complete() {
        let bindings = table_key_bindings();
        assert_eq!(bindings.len(), 19);
        for (context, expected) in [
            (TABLE_KEY_CONTEXT, 15),
            (TABLE_EDITOR_KEY_CONTEXT, 2),
            (TABLE_HANDLE_KEY_CONTEXT, 2),
        ] {
            let context = crate::KeyContext::parse(context).unwrap();
            let matching = bindings
                .iter()
                .filter(|binding| {
                    binding.context_predicate().is_some_and(|predicate| {
                        predicate.depth_of(std::slice::from_ref(&context)).is_some()
                    })
                })
                .count();
            assert_eq!(matching, expected);
        }
    }

    struct RowsView {
        table: TableState,
        rendered: Vec<TableRowState>,
    }

    impl RowsView {
        fn new() -> Self {
            Self {
                table: TableState::new(8).with_selection_mode(TableSelectionMode::Multiple),
                rendered: Vec::new(),
            }
        }

        fn table(view: &mut Self) -> &mut TableState {
            &mut view.table
        }
    }

    impl View for RowsView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let columns = [TableColumn::new("name", "Name").track(GridTrack::fr(1.0))];
            let rendered = std::cell::RefCell::new(Vec::new());
            let element = self.table.element_with_rows(
                cx,
                "rows",
                &columns,
                StateAccessor::from(Self::table as fn(&mut Self) -> &mut TableState),
                |row| {
                    rendered.borrow_mut().push(row);
                    div().bg(if row.selected {
                        Color::rgb8(37, 99, 235)
                    } else {
                        Color::TRANSPARENT
                    })
                },
                |header| div().child(text(header.column.label().clone()).no_wrap()),
                |cell| div().child(text(cell.position.row.to_string()).no_wrap()),
                |_view, _position, _cx| {},
            );
            self.rendered = rendered.into_inner();
            element
        }
    }

    #[test]
    fn row_renderer_receives_the_mounted_rows_and_their_selection() {
        let (mut cx, view) = Application::new()
            .bind_keys(table_key_bindings())
            .into_test_context(WindowOptions::default(), RowsView::new())
            .unwrap();
        let window = view.window_handle();
        cx.run_until_idle().unwrap();

        // Only mounted rows are rendered, none of them selected yet, and the container the
        // renderer returned is the core row under the core's own identity.
        let rendered = cx.read(view, |view| view.rendered.clone()).unwrap();
        assert!(!rendered.is_empty());
        assert_eq!(
            rendered[0],
            TableRowState {
                row: 0,
                selected: false
            }
        );
        assert!(rendered.iter().all(|row| !row.selected));
        assert!(
            cx.contains_element(window, TableState::row_id("rows", 0))
                .unwrap()
        );

        // Moving the selection re-renders exactly one row as selected.
        cx.simulate_keystrokes(window, "tab down").unwrap();
        cx.run_until_idle().unwrap();
        let rendered = cx.read(view, |view| view.rendered.clone()).unwrap();
        assert_eq!(rendered.iter().filter(|row| row.selected).count(), 1);
        assert!(rendered.contains(&TableRowState {
            row: 1,
            selected: true
        }));
    }
}
