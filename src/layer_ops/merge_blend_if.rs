//! An isolated merge cannot bake a condition whose backdrop is left behind.
use super::*;
use std::collections::HashMap;

fn active(layer: &Layer) -> bool {
    layer
        .blend_if
        .is_some_and(|settings| !settings.is_identity())
}

fn dependencies(doc: &Document, layer: &Layer) -> Vec<Uuid> {
    let mut result = vec![layer.id];
    let mut next = layer.clip_source;
    for _ in 0..=256 {
        let Some(layer) = next.and_then(|id| doc.layer(id)) else {
            break;
        };
        result.push(layer.id);
        next = layer.clip_source;
    }
    result
}

fn rejected(layer: &Layer) -> crate::Error {
    invalid(format!(
        "{}: merging these layers would change Blend If because its backdrop or clipping context is outside the selection. Include the underlying layers and complete clipping stack or containing folder/artboard, or use Merge All. The document is unchanged.",
        layer.name
    ))
}

pub(super) fn validate(doc: &Document, selected: &HashSet<Uuid>) -> Result<()> {
    if !doc.layers.iter().any(active) {
        return Ok(());
    }
    let mut ordered = Vec::new();
    super::visit(doc, None, &mut ordered);
    let mut stack_context = HashMap::new();
    for stack in crate::clipping::stacks(doc) {
        let mut missing_context = !selected.contains(&doc.layers[stack.base].id);
        for index in stack.contiguous.into_iter().chain(stack.adjustments) {
            stack_context.insert(doc.layers[index].id, missing_context);
            missing_context |= !selected.contains(&doc.layers[index].id);
        }
    }
    for (position, id) in ordered.iter().enumerate() {
        let Some(layer) = doc.layer(*id) else {
            continue;
        };
        if layer.is_group() {
            continue;
        }
        let dependencies = dependencies(doc, layer);
        let conditional = dependencies
            .iter()
            .filter_map(|id| doc.layer(*id))
            .any(active);
        if !conditional {
            continue;
        }
        if !doc.layer_is_visible(*id) {
            if layer.visible
                && selected.contains(id)
                && super::ancestors(doc, *id)
                    .into_iter()
                    .flatten()
                    .any(|ancestor| {
                        !selected.contains(&ancestor)
                            && doc.layer(ancestor).is_some_and(|parent| !parent.visible)
                    })
            {
                return Err(rejected(layer));
            }
            continue;
        }
        if !selected.contains(id) {
            // Retargeting an unmerged conditional clipping consumer to a new
            // raster would replace the source tones its mask depends on.
            if dependencies.iter().skip(1).any(|id| selected.contains(id)) {
                return Err(rejected(layer));
            }
            continue;
        }
        if dependencies.iter().any(|id| !selected.contains(id)) {
            return Err(rejected(layer));
        }
        for ancestor in super::ancestors(doc, *id).into_iter().flatten() {
            let Some(parent) = doc.layer(ancestor) else {
                continue;
            };
            if !selected.contains(&ancestor)
                && (parent.opacity != 1.
                    || parent.mask.as_ref().is_some_and(|mask| mask.enabled)
                    || parent.is_artboard())
            {
                return Err(rejected(layer));
            }
        }
        if !dependencies
            .iter()
            .filter_map(|id| doc.layer(*id))
            .any(|layer| {
                layer
                    .blend_if
                    .is_some_and(|settings| settings.enabled && !settings.underlying.is_identity())
            })
        {
            continue;
        }
        if let Some(missing_context) = stack_context.get(id) {
            if *missing_context {
                return Err(rejected(layer));
            }
            // The base's own conditional backdrop is checked when visiting it.
            continue;
        }
        let board = crate::artboard::owner(doc, *id);
        if ordered[..position].iter().any(|earlier| {
            !selected.contains(earlier)
                && doc.layer_is_visible(*earlier)
                && (crate::artboard::owner(doc, *earlier) == board
                    || (board.is_none() && doc.layer(*earlier).is_some_and(Layer::is_artboard)))
                && doc.layer(*earlier).is_some_and(|layer| {
                    layer.raster().is_some() || layer.is_adjustment() || layer.is_artboard()
                })
        }) {
            return Err(rejected(layer));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
