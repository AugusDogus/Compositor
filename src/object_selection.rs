//! Point-prompt object selection and whole-subject selection use separate models.
mod inference;
mod refinement;
use crate::{
    Result,
    document::Document,
    geometry::Point,
    invalid,
    selection::{Selection, SelectionMode},
};
use image::GrayImage;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    Subject,
    Object(Point),
    ObjectBox { start: Point, end: Point },
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub target: Target,
    pub sample_all: bool,
    pub antialiased: bool,
    /// Signed object edge adjustment in pixels, positive contracts, negative expands.
    pub edge_offset: i8,
    pub mode: SelectionMode,
}
struct Cached {
    source: Document,
    mask: Arc<GrayImage>,
}
static CACHE: Mutex<Option<Cached>> = Mutex::new(None);
fn source_document(document: &Document, all: bool) -> Result<Document> {
    crate::document::validate_size(document.width, document.height).map_err(|_| {
        invalid("Object and subject selection need a canvas of at most 100 million pixels. Crop or resize a copy and retry; the current document is unchanged.")
    })?;
    let mut source = document.clone();
    source.selection = None;
    source.selected.clear();
    if !all {
        let layer = document
            .active_layer()
            .filter(|l| l.raster().is_some())
            .ok_or_else(|| {
                invalid("Select a layer with pixels, or choose All Layers for object selection.")
            })?;
        // Keep dependency layers for clipping and group masks, hiding unrelated branches.
        let mut visible = std::collections::HashSet::from([layer.id]);
        let mut next = layer.parent;
        while let Some(id) = next {
            visible.insert(id);
            next = document.layer(id).and_then(|l| l.parent);
        }
        for other in &mut source.layers {
            other.visible = visible.contains(&other.id);
        }
    } else {
        source.active = None;
    }
    Ok(source)
}
fn detect_subject(document: &Document, sample_all: bool) -> Result<Arc<GrayImage>> {
    let source = source_document(document, sample_all)?;
    {
        let cache=CACHE.lock().map_err(|_|invalid("The subject selection cache stopped. Restart the editor; the document is unchanged."))?;
        if let Some(cached) = cache.as_ref().filter(|cached| cached.source == source) {
            return Ok(cached.mask.clone());
        }
    }
    let pixels = crate::render::region_accelerated(
        &source,
        source.width,
        source.height,
        [0.; 2],
        [1.; 2],
        &mut crate::render::DownsampleCache::default(),
    )?;
    let mut coverage = crate::background::foreground_mask(&pixels)?;
    for (mask, pixel) in coverage.pixels_mut().zip(pixels.pixels()) {
        mask[0] = ((u16::from(mask[0]) * u16::from(pixel[3]) + 127) / 255) as u8;
    }
    let mask = Arc::new(coverage);
    let mut cache = CACHE.lock().map_err(|_| {
        invalid(
            "The subject selection cache stopped. Restart the editor; the document is unchanged.",
        )
    })?;
    *cache = Some(Cached {
        source,
        mask: mask.clone(),
    });
    Ok(mask)
}
fn validate_settings(document: &Document, settings: Settings) -> Result<()> {
    if !(-10..=10).contains(&settings.edge_offset) {
        return Err(invalid(
            "Object edge adjustment must be between -10 and 10 pixels. The current selection is unchanged.",
        ));
    }
    let valid_point = |point: Point, edge: bool| {
        point
            .iter()
            .zip([document.width, document.height])
            .all(|(value, limit)| {
                value.is_finite()
                    && *value >= 0.
                    && if edge {
                        *value <= f64::from(limit)
                    } else {
                        *value < f64::from(limit)
                    }
            })
    };
    let valid = match settings.target {
        Target::Subject => true,
        Target::Object(point) => valid_point(point, false),
        Target::ObjectBox { start, end } => {
            valid_point(start, true)
                && valid_point(end, true)
                && start[0] != end[0]
                && start[1] != end[1]
        }
    };
    if !valid {
        return Err(invalid(
            "Click inside the canvas or drag a box with width and height to select an object. The current selection is unchanged.",
        ));
    }
    Ok(())
}
/// Release native object sessions before graphics driver teardown.
pub fn shutdown() {
    inference::shutdown();
}
/// Apply an inferred object or subject mask, including source alpha coverage.
/// Object masks receive edge adjustment and optional geometric smoothing before combining.
pub fn select_from_mask(
    document: &mut Document,
    mask: &GrayImage,
    settings: Settings,
) -> Result<()> {
    validate_settings(document, settings)?;
    crate::document::validate_size(document.width, document.height)?;
    if mask.dimensions() != (document.width, document.height) {
        return Err(invalid(
            "The selection mask does not match the canvas size. Retry selection on the current document.",
        ));
    }

    let next = if !matches!(settings.target, Target::Subject) {
        refinement::selection(mask, settings.edge_offset, settings.antialiased)?
    } else if mask.pixels().any(|p| {
        if settings.antialiased {
            p[0] > 0
        } else {
            p[0] >= 128
        }
    }) {
        Some(if settings.antialiased {
            Selection::from_mask(mask.clone())
        } else {
            Selection::from_coverage(
                mask,
                crate::geometry::Transform::new(document.width, document.height),
                document.width,
                document.height,
                false,
            )?
        })
    } else {
        None
    };
    document.selection = match (settings.mode, &document.selection, next) {
        (SelectionMode::Replace, _, next) => next,
        (_, Some(previous), Some(next)) => Some(previous.combine(&next, settings.mode)?),
        (SelectionMode::Add, None, next) => next,
        (SelectionMode::Intersect, _, None) => None,
        (_, previous, None) => previous.clone(),
        (SelectionMode::Subtract | SelectionMode::Intersect, None, Some(_)) => None,
    };
    Ok(())
}
pub fn select(document: &mut Document, settings: Settings) -> Result<()> {
    validate_settings(document, settings)?;
    match settings.target {
        Target::Subject => {
            let mask = detect_subject(document, settings.sample_all)?;
            select_from_mask(document, &mask, settings)
        }
        Target::Object(point) => {
            let source = source_document(document, settings.sample_all)?;
            let mask = inference::detect(&source, point)?;
            select_from_mask(document, &mask, settings)
        }
        Target::ObjectBox { start, end } => {
            let source = source_document(document, settings.sample_all)?;
            let mask = inference::detect_box(&source, start, end)?;
            select_from_mask(document, &mask, settings)
        }
    }
}

#[cfg(test)]
mod tests;
