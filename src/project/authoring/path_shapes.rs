use crate::{
    Result,
    document::{Document, LayerContent},
    invalid,
    path_shape::{Content, Source},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Saved {
    layer: Uuid,
    source: Source,
}

pub(super) fn extract(document: &mut Document) -> Vec<Saved> {
    let mut result = Vec::new();
    for layer in &mut document.layers {
        if let Some(content) = layer.path_shape() {
            result.push(Saved {
                layer: layer.id,
                source: content.source().clone(),
            });
            layer.content = LayerContent::Raster(Some(content.pixels().clone()));
        }
    }
    result
}

pub(super) fn validate(shapes: &[Saved], seen: &mut HashSet<Uuid>) -> Result<()> {
    for shape in shapes {
        shape.source.validate()?;
        if shape.layer.is_nil() || !seen.insert(shape.layer) {
            return Err(invalid(
                "The Linux path-shape snapshot contains an invalid or repeated layer ID.",
            ));
        }
    }
    Ok(())
}

pub(super) fn preflight(document: &Document, shapes: &[Saved]) -> Result<()> {
    for shape in shapes {
        let layer = document
            .layer(shape.layer)
            .ok_or_else(|| invalid("A Linux path shape refers to a missing source layer."))?;
        if !matches!(layer.content, LayerContent::Raster(None)) {
            return Err(invalid(
                "A Linux path shape must reference a pixel source layer.",
            ));
        }
    }
    Ok(())
}

pub(super) fn restore(document: &mut Document, shapes: Vec<Saved>) -> Result<()> {
    for shape in shapes {
        let layer = document
            .layers
            .iter_mut()
            .find(|layer| layer.id == shape.layer)
            .ok_or_else(|| invalid("A Linux path shape refers to a missing source layer."))?;
        if layer.text.is_some() || layer.shape.is_some() || layer.raw.is_some() {
            return Err(invalid(
                "A Linux path shape cannot replace text, primitive shape, or RAW source metadata.",
            ));
        }
        let LayerContent::Raster(Some(pixels)) = &layer.content else {
            return Err(invalid(
                "A Linux path shape requires a cached pixel source.",
            ));
        };
        layer.content = LayerContent::PathShape(Box::new(Content::from_cached_source(
            shape.source,
            pixels.clone(),
        )?));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
