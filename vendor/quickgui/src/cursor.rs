/// Native cursor shown while the pointer is over an element.
///
/// The variants mirror GPUI's cursor vocabulary and map directly to the corresponding
/// platform cursor. Use [`crate::Element::cursor`] for the typed API or one of its
/// Tailwind-compatible cursor helpers.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum CursorStyle {
    /// The platform's default arrow cursor (`default`).
    #[default]
    Arrow,
    /// The horizontal text-selection cursor (`text`).
    IBeam,
    /// A crosshair cursor (`crosshair`).
    Crosshair,
    /// A closed hand (`grabbing`).
    ClosedHand,
    /// An open hand (`grab`).
    OpenHand,
    /// A pointing hand (`pointer`).
    PointingHand,
    /// A west-edge resize cursor (`w-resize`).
    ResizeLeft,
    /// An east-edge resize cursor (`e-resize`).
    ResizeRight,
    /// A horizontal resize cursor (`ew-resize`).
    ResizeLeftRight,
    /// A north-edge resize cursor (`n-resize`).
    ResizeUp,
    /// A south-edge resize cursor (`s-resize`).
    ResizeDown,
    /// A vertical resize cursor (`ns-resize`).
    ResizeUpDown,
    /// A north-west/south-east resize cursor (`nwse-resize`).
    ResizeUpLeftDownRight,
    /// A north-east/south-west resize cursor (`nesw-resize`).
    ResizeUpRightDownLeft,
    /// A column resize cursor (`col-resize`).
    ResizeColumn,
    /// A row resize cursor (`row-resize`).
    ResizeRow,
    /// The vertical text-selection cursor (`vertical-text`).
    IBeamCursorForVerticalLayout,
    /// A cursor indicating that an operation is unavailable (`not-allowed`).
    OperationNotAllowed,
    /// A cursor indicating that a drag will create an alias (`alias`).
    DragLink,
    /// A cursor indicating that a drag will copy its payload (`copy`).
    DragCopy,
    /// A cursor indicating that a context menu is available (`context-menu`).
    ContextualMenu,
}
