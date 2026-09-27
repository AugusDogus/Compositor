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
        let geometry = geometry(document, path)?.clone();
        self.apply_geometry(document, &geometry)
    }

    pub fn apply_geometry(&self, document: &mut Document, geometry: &BezierPath) -> Result<()> {
        match self {
            Self::Select { mode, antialiased } => {
                select_geometry(document, geometry, *mode, *antialiased)
            }
            Self::Fill {
                color,
                mask,
                antialiased,
            } => fill_geometry(document, geometry, *color, *mask, *antialiased),
            Self::Stroke { brush, shape, mask } => {
                stroke_geometry(document, geometry, *brush, shape.clone(), *mask)
            }
        }
    }
}

fn geometry(document: &Document, path: Uuid) -> Result<&BezierPath> {
    document.paths.iter().find(|saved| saved.id == path)
        .map(|saved| &saved.geometry)
        .ok_or_else(|| invalid("The saved path no longer exists. Select another path and retry; the document is unchanged."))
}

fn area(document: &Document, path: &BezierPath, antialiased: bool) -> Result<Selection> {
    path.selection(document.width, document.height, antialiased)
}

pub fn select(
    document: &mut Document,
    path: Uuid,
    mode: SelectionMode,
    antialiased: bool,
) -> Result<()> {
    let geometry = geometry(document, path)?.clone();
    select_geometry(document, &geometry, mode, antialiased)
}

fn select_geometry(
    document: &mut Document,
    path: &BezierPath,
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
    let geometry = geometry(document, path)?.clone();
    fill_geometry(document, &geometry, color, mask, antialiased)
}

fn fill_geometry(
    document: &mut Document,
    path: &BezierPath,
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
    let geometry = geometry(document, path)?.clone();
    stroke_geometry(document, &geometry, brush, shape, mask)
}

fn stroke_geometry(
    document: &mut Document,
    geometry: &BezierPath,
    brush: Brush,
    shape: sampled::Shape,
    mask: bool,
) -> Result<()> {
    let points = geometry.flatten(FlattenOptions::default())?;
    brush::paint_polyline(document, &points, brush, shape, mask)
}

#[cfg(test)]
mod tests;
