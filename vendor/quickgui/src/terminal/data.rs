use crate::Color;
use std::sync::Arc;

/// Resolved backgrounds along the four edges of the terminal grid.
///
/// Ghostty's `window-padding-color=extend` paints each padding pixel with the nearest edge cell.
/// Retaining only the edges keeps that behavior bounded to O(columns + rows) snapshot storage.
#[derive(Clone, Debug)]
pub(crate) struct EdgeBackgrounds {
    pub(crate) top: Arc<[Option<Color>]>,
    pub(crate) right: Arc<[Option<Color>]>,
    pub(crate) bottom: Arc<[Option<Color>]>,
    pub(crate) left: Arc<[Option<Color>]>,
}

impl EdgeBackgrounds {
    pub(crate) fn new(
        top: Vec<Option<Color>>,
        right: Vec<Option<Color>>,
        bottom: Vec<Option<Color>>,
        left: Vec<Option<Color>>,
    ) -> Self {
        Self {
            top: top.into(),
            right: right.into(),
            bottom: bottom.into(),
            left: left.into(),
        }
    }

    pub(crate) fn empty(columns: u16, rows: u16) -> Self {
        Self::new(
            vec![None; usize::from(columns)],
            vec![None; usize::from(rows)],
            vec![None; usize::from(columns)],
            vec![None; usize::from(rows)],
        )
    }

    #[cfg(test)]
    pub(crate) fn solid(columns: u16, rows: u16, color: Color) -> Self {
        Self::new(
            vec![Some(color); usize::from(columns)],
            vec![Some(color); usize::from(rows)],
            vec![Some(color); usize::from(columns)],
            vec![Some(color); usize::from(rows)],
        )
    }
}
#[cfg(any(feature = "terminal", quickgui_terminal_extension))]
pub(crate) fn is_block_element(character: char) -> bool {
    matches!(character as u32, 0x2580..=0x259f)
}
