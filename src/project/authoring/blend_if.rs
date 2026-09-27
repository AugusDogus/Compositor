//! Conditional blending retains its live backdrop dependency in editing sources.
use super::*;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Saved {
    layer: Uuid,
    settings: crate::blend_if::Settings,
}

pub(super) fn extract(document: &mut Document) -> Vec<Saved> {
    document
        .layers
        .iter_mut()
        .filter_map(|layer| {
            layer.blend_if.take().map(|settings| Saved {
                layer: layer.id,
                settings,
            })
        })
        .collect()
}

pub(super) fn validate(settings: &[Saved]) -> Result<()> {
    let mut seen = HashSet::new();
    for saved in settings {
        if saved.layer.is_nil() || !seen.insert(saved.layer) {
            return Err(invalid(
                "The Blend If snapshot has an invalid or repeated layer.",
            ));
        }
    }
    Ok(())
}

pub(super) fn preflight(document: &Document, settings: &[Saved]) -> Result<()> {
    for saved in settings {
        if document
            .layer(saved.layer)
            .is_none_or(|layer| !matches!(layer.content, LayerContent::Raster(None)))
        {
            return Err(invalid("Blend If must reference a pixel source layer."));
        }
    }
    Ok(())
}

pub(super) fn restore(document: &mut Document, settings: Vec<Saved>) -> Result<()> {
    for saved in settings {
        let layer = document
            .layers
            .iter_mut()
            .find(|layer| layer.id == saved.layer)
            .ok_or_else(|| invalid("Blend If refers to a missing source layer."))?;
        if layer.is_group() || layer.is_adjustment() {
            return Err(invalid("Blend If requires a pixel source layer."));
        }
        layer.blend_if = Some(saved.settings);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
