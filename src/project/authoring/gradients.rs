//! Gradient overlays keep independent color and opacity stops in the editing snapshot.
use super::*;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Saved {
    layer: Uuid,
    overlay: crate::gradient_overlay::Overlay,
}

pub(super) fn extract(document: &mut Document) -> Vec<Saved> {
    let mut saved = Vec::new();
    for layer in &mut document.layers {
        if let Some(overlay) = layer
            .effects
            .as_mut()
            .and_then(|e| e.gradient_overlay.take())
        {
            saved.push(Saved {
                layer: layer.id,
                overlay: *overlay,
            });
        }
    }
    saved
}

pub(super) fn validate(gradients: &[Saved]) -> Result<()> {
    let mut seen = HashSet::new();
    for saved in gradients {
        saved.overlay.validate()?;
        if saved.layer.is_nil() || !seen.insert(saved.layer) {
            return Err(invalid(
                "The Gradient Overlay snapshot has an invalid or repeated layer.",
            ));
        }
    }
    Ok(())
}

pub(super) fn preflight(document: &Document, gradients: &[Saved]) -> Result<()> {
    for saved in gradients {
        if document
            .layer(saved.layer)
            .is_none_or(|layer| !matches!(layer.content, LayerContent::Raster(None)))
        {
            return Err(invalid(
                "A Gradient Overlay must reference a pixel source layer.",
            ));
        }
    }
    Ok(())
}

pub(super) fn restore(document: &mut Document, gradients: Vec<Saved>) -> Result<()> {
    for saved in gradients {
        let layer = document
            .layers
            .iter_mut()
            .find(|layer| layer.id == saved.layer)
            .ok_or_else(|| invalid("A Gradient Overlay refers to a missing source layer."))?;
        if layer.raster().is_none() {
            return Err(invalid("A Gradient Overlay requires cached source pixels."));
        }
        layer
            .effects
            .get_or_insert_with(Default::default)
            .gradient_overlay = Some(Box::new(saved.overlay));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
