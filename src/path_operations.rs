//! Apply saved working paths through the existing selection and brush engines.
use crate::{
    Result,
    brush::{self, Brush, sampled},
    document::Document,
    invalid,
    selection::{Selection, SelectionMode},
    vector_path::{BezierPath, FlattenOptions},
};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub enum Operation {
    Select {
        mode: SelectionMode,
        antialiased: bool,
    },
    Fill {
        color: [u8; 4],
        mask: bool,
        antialiased: bool,
    },
    Stroke {
        brush: Brush,
        shape: sampled::Shape,
        mask: bool,
    },
}

impl Operation {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Select { .. } => "Select from Path",
            Self::Fill { .. } => "Fill Path",
            Self::Stroke { .. } => "Stroke Path",
        }
    }

    pub fn apply(&self, document: &mut Document, path: Uuid) -> Result<()> {
        match self {
            Self::Select { mode, antialiased } => select(document, path, *mode, *antialiased),
            Self::Fill {
                color,
                mask,
                antialiased,
            } => fill(document, path, *color, *mask, *antialiased),
            Self::Stroke { brush, shape, mask } => {
                stroke(document, path, *brush, shape.clone(), *mask)
            }
        }
    }
}

fn geometry(document: &Document, path: Uuid) -> Result<&BezierPath> {
    document.paths.iter().find(|saved| saved.id == path)
        .map(|saved| &saved.geometry)
        .ok_or_else(|| invalid("The saved path no longer exists. Select another path and retry; the document is unchanged."))
}

fn area(document: &Document, path: Uuid, antialiased: bool) -> Result<Selection> {
    geometry(document, path)?.selection(document.width, document.height, antialiased)
}

pub fn select(
    document: &mut Document,
    path: Uuid,
    mode: SelectionMode,
    antialiased: bool,
) -> Result<()> {
    let next = area(document, path, antialiased)?;
    let next = match (&document.selection, mode) {
        (_, SelectionMode::Replace) | (None, SelectionMode::Add) => Some(next),
        (Some(previous), _) => Some(previous.combine(&next, mode)?),
        (None, SelectionMode::Subtract | SelectionMode::Intersect) => None,
    };
    document.selection = next.filter(|selection| selection.bounds().is_some());
    Ok(())
}

/// The working path limits the fill together with any current selection. The
/// original selection and editable path remain intact, including on failure.
pub fn fill(
    document: &mut Document,
    path: Uuid,
    color: [u8; 4],
    mask: bool,
    antialiased: bool,
) -> Result<()> {
    let path_area = area(document, path, antialiased)?;
    let fill_area = match &document.selection {
        Some(selection) => selection.combine(&path_area, SelectionMode::Intersect)?,
        None => path_area,
    };
    let mut next = document.clone();
    next.selection = Some(fill_area);
    crate::edits::fill(&mut next, color, false, mask)?;
    next.selection = document.selection.clone();
    next.validate()?;
    *document = next;
    Ok(())
}

pub fn stroke(
    document: &mut Document,
    path: Uuid,
    brush: Brush,
    shape: sampled::Shape,
    mask: bool,
) -> Result<()> {
    let points = geometry(document, path)?.flatten(FlattenOptions::default())?;
    brush::paint_polyline(document, &points, brush, shape, mask)
}

#[cfg(test)]
mod tests;
