//! Export visible pixel, text, and shape layers as isolated, trimmed PNGs.
use crate::{
    Result,
    document::{Document, Layer},
    export_batch, image_io, invalid, render,
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};
use uuid::Uuid;

struct Target {
    id: Uuid,
    origin: [f64; 2],
    size: [u32; 2],
}

fn target(document: &Document, layer: &Layer) -> Result<Option<Target>> {
    if layer.raster().is_none() || !document.layer_is_visible(layer.id) {
        return Ok(None);
    }
    let transform = crate::effects::rendered_transform(layer)?;
    // Quarter turns can put an exact pixel edge a few ulps below an integer.
    // Avoid adding an empty export row solely from trigonometric roundoff.
    let bounds = transform.bounds().map(|edge| {
        let integer = edge.round();
        if (edge - integer).abs() <= f64::EPSILON * edge.abs().max(1.) * 8. {
            integer
        } else {
            edge
        }
    });
    let left = bounds[0].floor().max(0.);
    let top = bounds[1].floor().max(0.);
    let right = bounds[2].ceil().min(f64::from(document.width));
    let bottom = bounds[3].ceil().min(f64::from(document.height));
    Ok((right > left && bottom > top).then_some(Target {
        id: layer.id,
        origin: [left, top],
        size: [(right - left) as u32, (bottom - top) as u32],
    }))
}

pub fn has_exportable_layers(document: &Document) -> bool {
    let absorbed: HashSet<_> = crate::clipping::stacks(document)
        .into_iter()
        .flat_map(|stack| stack.contiguous.into_iter().chain(stack.adjustments))
        .map(|index| document.layers[index].id)
        .collect();
    document.layers.iter().any(|layer| {
        !absorbed.contains(&layer.id)
            && target(document, layer).is_ok_and(|target| target.is_some())
    })
}

fn isolated(document: &Document, target: Uuid, stacks: &HashMap<Uuid, Vec<Uuid>>) -> Document {
    let mut included = HashSet::from([target]);
    included.extend(stacks.get(&target).into_iter().flatten().copied());
    // Detached raster clipping links remain independent layers. Keep their
    // coverage sources as hidden dependencies so rendering samples masks,
    // transforms, and effects at the original document coordinates.
    let mut dependencies = HashSet::new();
    let mut source = document.layer(target).and_then(|layer| layer.clip_source);
    while let Some(id) = source {
        dependencies.insert(id);
        source = document.layer(id).and_then(|layer| layer.clip_source);
    }
    included.extend(&dependencies);
    for id in included.clone() {
        let mut parent = document.layer(id).and_then(|layer| layer.parent);
        while let Some(id) = parent {
            included.insert(id);
            parent = document.layer(id).and_then(|layer| layer.parent);
        }
    }
    let mut result = document.clone();
    result.layers.retain(|layer| included.contains(&layer.id));
    for layer in &mut result.layers {
        if let crate::document::LayerContent::Artboard(board) = &mut layer.content {
            board.background = [0; 4];
        }

        if dependencies.contains(&layer.id) {
            layer.visible = false;
        }
    }
    result.active = Some(target);
    result.selected = HashSet::from([target]);
    result.selection = None;
    result
}

/// Publish a new `<title>-layers` directory only after every PNG is encoded.
/// Numbering follows document layer order. Stack members accompany their base;
/// detached clipping links export separately with their source coverage.
/// Ancestor masks and opacity remain active.
pub fn export(document: &Document, parent: &Path, title: &str) -> Result<PathBuf> {
    document.validate()?;
    fn visit(document: &Document, parent: Option<Uuid>, targets: &mut Vec<Target>) -> Result<()> {
        for layer in document
            .layers
            .iter()
            .filter(|layer| layer.parent == parent && layer.visible)
        {
            if layer.is_group() {
                visit(document, Some(layer.id), targets)?;
            } else if let Some(target) = target(document, layer)? {
                targets.push(target);
            }
        }
        Ok(())
    }
    let mut targets = Vec::new();
    visit(document, None, &mut targets)?;
    let stacks: HashMap<Uuid, Vec<Uuid>> = crate::clipping::stacks(document)
        .into_iter()
        .map(|stack| {
            (
                document.layers[stack.base].id,
                stack
                    .contiguous
                    .into_iter()
                    .chain(stack.adjustments)
                    .map(|index| document.layers[index].id)
                    .collect(),
            )
        })
        .collect();
    let absorbed: HashSet<_> = stacks.values().flatten().copied().collect();
    targets.retain(|target| !absorbed.contains(&target.id));
    if targets.is_empty() {
        return Err(invalid(
            "There are no visible pixel, text, or shape layers inside the canvas to export. Show a layer and retry; the project is unchanged.",
        ));
    }
    let mut total = 0_u64;
    for target in &targets {
        crate::document::validate_size(target.size[0], target.size[1])?;
        total += u64::from(target.size[0]) * u64::from(target.size[1]);
        if total > 800_000_000 {
            return Err(invalid(
                "Layer export exceeds 800 million pixels. Hide some layers and export in smaller batches; no export folder was created.",
            ));
        }
    }
    let stem = export_batch::safe_stem(title);
    export_batch::publish(parent, &format!("{stem}-layers"), |directory| {
        let mut cache = render::DownsampleCache::default();
        for (index, target) in targets.iter().enumerate() {
            let layer = document
                .layer(target.id)
                .ok_or_else(|| invalid("An export layer is missing."))?;
            let source = isolated(document, target.id, &stacks);
            let pixels = render::region_accelerated(
                &source,
                target.size[0],
                target.size[1],
                target.origin,
                [1., 1.],
                &mut cache,
            )?;
            let path = directory.join(format!(
                "{:02}-{}.png",
                index + 1,
                export_batch::safe_stem(&layer.name)
            ));
            image_io::export_pixels(pixels, document.resolution, &path, 100)?;
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests;
