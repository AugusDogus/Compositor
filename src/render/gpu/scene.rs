//! Compile the reference renderer's traversal into a bounded, linear GPU program.
use super::super::RenderState;
use crate::{
    document::{Document, LayerContent},
    geometry::{Point, Sampling, Transform},
};
use std::{collections::HashMap, sync::Arc};

pub(super) const ASSET_BYTES: usize = 128 * 1024 * 1024;
pub(in crate::render) struct Scene {
    pub layers: Vec<u32>,
    pub operations: Vec<[u32; 2]>,
    pub adjustments: Vec<f32>,
    pub pixels: Vec<u32>,
}

impl Scene {
    #[cfg(test)]
    pub fn compile(doc: &Document, origin: Point, step: Point) -> Option<Self> {
        Self::compile_surfaces(doc, origin, step, &HashMap::new(), None)
    }
    pub fn compile_surfaces(
        doc: &Document,
        origin: Point,
        step: Point,
        surfaces: &super::super::spatial::Surfaces,
        before: Option<uuid::Uuid>,
    ) -> Option<Self> {
        if doc.layers.len() > 4096 {
            return None;
        }
        let mut scene = Self {
            layers: Vec::new(),
            operations: Vec::new(),
            adjustments: Vec::new(),
            pixels: Vec::new(),
        };
        let indices: HashMap<_, _> = doc
            .layers
            .iter()
            .enumerate()
            .map(|(i, l)| (l.id, i as u32))
            .collect();
        let mut colors = HashMap::new();
        let mut masks = HashMap::new();
        let mapping = |t: Transform| {
            let a = t.unit([origin[0] + step[0] * 0.5, origin[1] + step[1] * 0.5]);
            let b = t.unit([origin[0] + step[0] * 1.5, origin[1] + step[1] * 0.5]);
            let c = t.unit([origin[0] + step[0] * 0.5, origin[1] + step[1] * 1.5]);
            [
                a[0],
                a[1],
                b[0] - a[0],
                b[1] - a[1],
                c[0] - a[0],
                c[1] - a[1],
            ]
        };
        for layer in &doc.layers {
            let surface = surfaces.get(&layer.id);
            let image = if let Some(image) = surface.map(|s| &s.image).or_else(|| layer.raster()) {
                let key = Arc::as_ptr(image);
                let offset = if let Some(offset) = colors.get(&key) {
                    *offset
                } else {
                    if (scene.pixels.len() + image.width() as usize * image.height() as usize) * 4
                        > ASSET_BYTES
                    {
                        return None;
                    }
                    let offset = scene.pixels.len() as u32;
                    scene.pixels.extend(
                        image
                            .as_raw()
                            .chunks_exact(4)
                            .map(|p| u32::from_le_bytes([p[0], p[1], p[2], p[3]])),
                    );
                    colors.insert(key, offset);
                    offset
                };
                [
                    offset,
                    image.width(),
                    image.height(),
                    u32::from(surface.is_some() || layer.transform.sampling != Sampling::Nearest),
                ]
            } else {
                [0; 4]
            };
            let mut mask_info = [0; 4];
            let mut background = 0.;
            let mut placement = false;
            let mut mask_transform = layer.transform;
            if let Some(mask) = &layer.mask
                && mask.enabled
            {
                let key = Arc::as_ptr(&mask.pixels);
                let offset = if let Some(offset) = masks.get(&key) {
                    *offset
                } else {
                    if (scene.pixels.len() + mask.pixels.len()) * 4 > ASSET_BYTES {
                        return None;
                    }
                    let offset = scene.pixels.len() as u32;
                    scene
                        .pixels
                        .extend(mask.pixels.as_raw().iter().map(|v| u32::from(*v)));
                    masks.insert(key, offset);
                    offset
                };
                mask_transform = mask.placement.unwrap_or(layer.transform);
                mask_info = [
                    offset,
                    mask.pixels.width(),
                    mask.pixels.height(),
                    u32::from(mask_transform.sampling != Sampling::Nearest),
                ];
                background = mask.background();
                placement = mask.placement.is_some();
            }
            let a = mapping(surface.map_or(layer.transform, |s| s.transform));
            let m = mapping(mask_transform);
            scene.layers.extend(
                [
                    a[0],
                    a[1],
                    a[2],
                    a[3],
                    a[4],
                    a[5],
                    layer.opacity,
                    background,
                    m[0],
                    m[1],
                    m[2],
                    m[3],
                    m[4],
                    m[5],
                    0.,
                    0.,
                ]
                .map(|v| (v as f32).to_bits()),
            );
            scene.layers.extend(image);
            scene.layers.extend(mask_info);
            let (kind, adjustment, settings) = match &layer.content {
                LayerContent::Raster(_) => (0, 0, 0),
                LayerContent::Group => (1, 0, 0),
                LayerContent::Adjustment(a) => {
                    let offset = scene.adjustments.len() as u32;
                    let kind = super::adjustments::encode(a, &mut scene.adjustments);
                    (2, kind, offset)
                }
            };
            let blend = layer.blend as u32;
            scene.layers.extend([
                blend,
                layer
                    .clip_source
                    .and_then(|id| indices.get(&id).map(|i| i + 1))
                    .unwrap_or(0),
                kind,
                settings,
            ]);
            scene
                .layers
                .extend([u32::from(placement), adjustment, 0, 0]);
        }
        let state = RenderState::new(doc);
        fn visit(
            scene: &mut Scene,
            doc: &Document,
            state: &RenderState,
            parent: Option<uuid::Uuid>,
            depth: usize,
        ) {
            if depth > 64 {
                return;
            }
            for (index, layer) in doc
                .layers
                .iter()
                .enumerate()
                .filter(|(_, l)| l.parent == parent && l.visible)
            {
                if state.stacked.contains(&layer.id) {
                    continue;
                }
                let index = index as u32;
                if layer.is_group() {
                    scene.operations.push([0, index]); // Push inherited group mask.
                    visit(scene, doc, state, Some(layer.id), depth + 1);
                    scene.operations.push([1, 0]);
                } else if let Some(children) = state.stacks.get(&layer.id) {
                    scene.operations.push([3, index]); // Start stack with base coverage.
                    for child in children {
                        let op =
                            if matches!(doc.layers[*child].content, LayerContent::Adjustment(_)) {
                                5
                            } else {
                                4
                            };
                        scene.operations.push([op, *child as u32]);
                    }
                    scene.operations.push([6, index]);
                } else if matches!(layer.content, LayerContent::Adjustment(_)) {
                    if layer.clip_source.is_none() {
                        scene.operations.push([7, index]);
                    }
                } else if layer.raster().is_some() {
                    scene.operations.push([2, index]);
                }
            }
        }
        visit(&mut scene, doc, &state, None, 0);
        if let Some(target) = before {
            let target = *indices.get(&target)?;
            if let Some(position) = scene
                .operations
                .iter()
                .position(|op| op[1] == target && matches!(op[0], 5 | 7))
            {
                let clipped = scene.operations[position][0] == 5;
                scene.operations.truncate(position);
                if clipped {
                    scene.operations.push([8, 0]);
                }
            }
        }
        // Storage bindings cannot be empty, even for a blank document.
        if scene.layers.is_empty() {
            scene.layers.resize(32, 0);
        }
        if scene.adjustments.is_empty() {
            scene.adjustments.push(0.);
        }
        if scene.pixels.is_empty() {
            scene.pixels.push(0);
        }
        Some(scene)
    }
}
