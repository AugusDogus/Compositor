use super::{Document, Point, Result, Transform, invalid, target_ids};

/// Retain source pixels and editable text/shape content for convex perspective.
/// Corner handles describe the geometric outline, independently of source flips.
/// Every layer and mask is validated on a temporary document before publication.
pub fn apply_perspective(
    doc: &mut Document,
    mut bounds: Transform,
    corners: [Point; 4],
    mask_target: bool,
) -> Result<()> {
    let quad = crate::geometry::projective::Projective::new(corners)
        .map_err(|error| invalid(error.to_string()))?;
    if doc
        .layers
        .iter()
        .any(|layer| layer.is_artboard() && doc.selected.contains(&layer.id))
    {
        return Err(invalid(
            "Artboard frames must stay rectangular. Select an image layer or its mask to change perspective; the document is unchanged.",
        ));
    }
    bounds.flip_x = false;
    bounds.flip_y = false;
    let target = bounds.with_mapping(quad.mapping())?;
    let mut updated = doc.clone();
    if mask_target && doc.selected.len() == 1 {
        let layer = updated
            .active_layer_mut()
            .ok_or_else(|| invalid("Select a layer or mask to change perspective."))?;
        if let Some(mask) = layer.mask.as_mut().filter(|mask| !mask.linked) {
            mask.placement = Some(
                mask.placement
                    .unwrap_or(layer.transform)
                    .following(bounds, target)?,
            );
            updated.validate()?;
            *doc = updated;
            return Ok(());
        }
    }
    let ids = target_ids(doc);
    if ids.is_empty() {
        return Err(invalid(
            "Select a visible image, text, or shape layer to change perspective.",
        ));
    }
    for layer in updated
        .layers
        .iter_mut()
        .filter(|layer| ids.contains(&layer.id))
    {
        let old = layer.transform;
        layer.transform = old.following(bounds, target)?;
        if let Some(mask) = &mut layer.mask {
            if mask.linked {
                if let Some(placement) = mask.placement {
                    mask.placement = Some(placement.following(bounds, target)?);
                }
            } else {
                mask.placement.get_or_insert(old);
            }
        }
    }
    updated.validate()?;
    *doc = updated;
    Ok(())
}
