//! Immutable pattern tiles are shared by content digest across effect settings.
use super::*;
use crate::pattern::{Overlay, Pattern};
use std::collections::HashMap;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Saved {
    layer: Uuid,
    id: String,
    name: String,
    asset: String,
    width: u32,
    height: u32,
    settings: crate::pattern::Settings,
}

pub(super) fn extract(document: &mut Document) -> Vec<Saved> {
    let mut result = Vec::new();
    for layer in &mut document.layers {
        if let Some(overlay) = layer
            .effects
            .as_mut()
            .and_then(|e| e.pattern_overlay.take())
        {
            result.push(Saved {
                layer: layer.id,
                id: overlay.pattern.id().into(),
                name: overlay.pattern.name().into(),
                asset: overlay.pattern.digest().into(),
                width: overlay.pattern.pixels().width(),
                height: overlay.pattern.pixels().height(),
                settings: overlay.settings,
            });
        }
    }
    result
}

pub(super) fn asset_names(patterns: &[Saved]) -> Vec<String> {
    let mut seen = HashSet::new();
    patterns
        .iter()
        .filter(|s| seen.insert(&s.asset))
        .map(|s| format!("{SOURCE}/patterns/{}.png", s.asset))
        .collect()
}

pub(super) fn write(document: &Document, source: &Path) -> Result<()> {
    let mut seen = HashSet::new();
    for overlay in document.layers.iter().filter_map(|layer| {
        layer
            .effects
            .as_ref()
            .and_then(|e| e.pattern_overlay.as_ref())
    }) {
        let pattern = &overlay.pattern;
        if seen.insert(pattern.digest()) {
            let directory = source.join("patterns");
            fs::create_dir_all(&directory)?;
            let path = directory.join(format!("{}.png", pattern.digest()));
            pattern.pixels().save_with_format(&path, ImageFormat::Png)?;
            File::open(path)?.sync_all()?;
        }
    }
    if !seen.is_empty() {
        File::open(source.join("patterns"))?.sync_all()?;
    }
    Ok(())
}

/// Read only PNG headers until all references and the aggregate budget pass.
pub(super) fn preflight(root: &Path, source: &Path, patterns: &[Saved]) -> Result<u64> {
    let mut layers = HashSet::new();
    let mut assets = HashMap::new();
    let mut pixels = 0;
    for saved in patterns {
        saved.settings.validate()?;
        crate::pattern::validate_labels(&saved.id, &saved.name)?;
        crate::pattern::validate_size(saved.width, saved.height)?;
        if saved.layer.is_nil()
            || !layers.insert(saved.layer)
            || saved.asset.len() != 64
            || !saved
                .asset
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid(
                "The Pattern Overlay snapshot has an invalid asset or repeated layer.",
            ));
        }
        let size = (saved.width, saved.height);
        if let Some(previous) = assets.insert(&saved.asset, size) {
            if previous != size {
                return Err(invalid("A shared pattern tile has conflicting dimensions."));
            }
            continue;
        }
        let path = source.join("patterns").join(format!("{}.png", saved.asset));
        checked_file(root, &path, ASSET_LIMIT)?;
        let reader = image::ImageReader::open(&path)?.with_guessed_format()?;
        if reader.format() != Some(ImageFormat::Png) || reader.into_dimensions()? != size {
            return Err(invalid(
                "A pattern asset is not a PNG with its declared dimensions. Restore a matching project backup.",
            ));
        }
        pixels += u64::from(saved.width) * u64::from(saved.height);
        crate::document::validate_pixel_budget(pixels)?;
    }
    Ok(pixels)
}

pub(super) fn validate_layers(document: &Document, patterns: &[Saved]) -> Result<()> {
    for saved in patterns {
        if document
            .layer(saved.layer)
            .is_none_or(|layer| !matches!(layer.content, LayerContent::Raster(None)))
        {
            return Err(invalid(
                "A Pattern Overlay must reference a pixel source layer.",
            ));
        }
    }
    Ok(())
}

pub(super) fn restore(document: &mut Document, source: &Path, patterns: Vec<Saved>) -> Result<()> {
    let mut assets = HashMap::<String, Pattern>::new();
    for saved in patterns {
        let pattern = if let Some(pattern) = assets.get(&saved.asset) {
            pattern.relabel(saved.id, saved.name)?
        } else {
            let pixels = Arc::new(image_io::read_image(
                &source.join("patterns").join(format!("{}.png", saved.asset)),
            )?);
            let pattern = Pattern::from_shared(saved.id, saved.name, pixels)?;
            if pattern.digest() != saved.asset {
                return Err(invalid(
                    "A pattern tile no longer matches its content identifier. Restore a matching backup; no files were changed.",
                ));
            }
            assets.insert(saved.asset.clone(), pattern.clone());
            pattern
        };
        let layer = document
            .layers
            .iter_mut()
            .find(|layer| layer.id == saved.layer)
            .ok_or_else(|| invalid("A Pattern Overlay refers to a missing source layer."))?;
        if layer.raster().is_none() {
            return Err(invalid("A Pattern Overlay requires cached source pixels."));
        }
        layer
            .effects
            .get_or_insert_with(Default::default)
            .pattern_overlay = Some(Box::new(Overlay {
            pattern,
            settings: saved.settings,
        }));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
