//! Bevel/Emboss settings accompany the unchanged layer pixels in native projects.
use super::*;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Saved {
    layer: Uuid,
    settings: crate::bevel::Settings,
}

pub(super) fn extract(document: &mut Document) -> Vec<Saved> {
    document
        .layers
        .iter_mut()
        .filter_map(|layer| {
            layer.effects.as_mut()?.bevel.take().map(|settings| Saved {
                layer: layer.id,
                settings: *settings,
            })
        })
        .collect()
}

pub(super) fn validate(bevels: &[Saved]) -> Result<()> {
    let mut seen = HashSet::new();
    for saved in bevels {
        saved.settings.validate()?;
        if saved.layer.is_nil() || !seen.insert(saved.layer) {
            return Err(invalid(
                "The Bevel/Emboss snapshot has an invalid or repeated layer.",
            ));
        }
    }
    Ok(())
}

pub(super) fn preflight(document: &Document, bevels: &[Saved]) -> Result<()> {
    for saved in bevels {
        if document
            .layer(saved.layer)
            .is_none_or(|layer| !matches!(layer.content, LayerContent::Raster(None)))
        {
            return Err(invalid(
                "A Bevel/Emboss effect must reference a pixel source layer.",
            ));
        }
    }
    Ok(())
}

pub(super) fn restore(document: &mut Document, bevels: Vec<Saved>) -> Result<()> {
    for saved in bevels {
        let layer = document
            .layers
            .iter_mut()
            .find(|layer| layer.id == saved.layer)
            .ok_or_else(|| invalid("A Bevel/Emboss effect refers to a missing source layer."))?;
        if layer.raster().is_none() {
            return Err(invalid(
                "A Bevel/Emboss effect requires cached source pixels.",
            ));
        }
        layer.effects.get_or_insert_with(Default::default).bevel = Some(Box::new(saved.settings));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
