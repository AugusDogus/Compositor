//! Composite identity without retaining full-resolution paint buffers.
//! Arc::make_mut dissociates weak references when editing an otherwise unshared
//! image. That changes this identity without cloning its pixel buffer.
use compositor::{
    adjustment::Adjustment,
    blend::Blend,
    document::{Document, Layer, LayerContent, Mask},
    geometry::Transform,
};
use image::{GrayImage, RgbaImage};
use std::sync::{Arc, Weak};
use uuid::Uuid;

pub(super) struct CanvasContent {
    size: [u32; 2],
    layers: Vec<LayerKey>,
}

struct LayerKey {
    id: Uuid,
    visible: bool,
    parent: Option<Uuid>,
    transform: Transform,
    opacity: f64,
    effects: Option<compositor::effects::LayerEffects>,
    blend: Blend,
    clip_source: Option<Uuid>,
    content: Pixels,
    mask: Option<MaskKey>,
}

enum Pixels {
    Raster(Option<Weak<RgbaImage>>),
    Group,
    Adjustment(Box<Adjustment>),
}

struct MaskKey {
    enabled: bool,
    placement: Option<Transform>,
    pixels: Weak<GrayImage>,
}

impl CanvasContent {
    pub(super) fn new(document: &Document) -> Self {
        Self {
            size: [document.width, document.height],
            layers: document.layers.iter().map(LayerKey::new).collect(),
        }
    }

    pub(super) fn matches(&self, document: &Document) -> bool {
        self.size == [document.width, document.height]
            && self.layers.len() == document.layers.len()
            && self
                .layers
                .iter()
                .zip(&document.layers)
                .all(|(key, layer)| key.matches(layer))
    }
}

impl LayerKey {
    fn new(layer: &Layer) -> Self {
        Self {
            id: layer.id,
            visible: layer.visible,
            parent: layer.parent,
            transform: layer.transform,
            opacity: layer.opacity,
            effects: layer.effects.clone(),
            blend: layer.blend,
            clip_source: layer.clip_source,
            content: match &layer.content {
                LayerContent::Raster(pixels) => Pixels::Raster(pixels.as_ref().map(Arc::downgrade)),
                LayerContent::Group => Pixels::Group,
                LayerContent::Adjustment(value) => Pixels::Adjustment(value.clone()),
            },
            mask: layer.mask.as_ref().map(|mask| MaskKey {
                enabled: mask.enabled,
                placement: mask.placement,
                pixels: Arc::downgrade(&mask.pixels),
            }),
        }
    }

    fn matches(&self, layer: &Layer) -> bool {
        self.id == layer.id
            && self.visible == layer.visible
            && self.parent == layer.parent
            && self.transform == layer.transform
            && self.opacity == layer.opacity
            && self.effects == layer.effects
            && self.blend == layer.blend
            && self.clip_source == layer.clip_source
            && match (&self.content, &layer.content) {
                (Pixels::Raster(Some(a)), LayerContent::Raster(Some(b))) => {
                    a.ptr_eq(&Arc::downgrade(b))
                }
                (Pixels::Raster(None), LayerContent::Raster(None))
                | (Pixels::Group, LayerContent::Group) => true,
                (Pixels::Adjustment(a), LayerContent::Adjustment(b)) => a == b,
                _ => false,
            }
            && match (&self.mask, &layer.mask) {
                (Some(a), Some(b)) => a.matches(b),
                (None, None) => true,
                _ => false,
            }
    }
}

impl MaskKey {
    fn matches(&self, mask: &Mask) -> bool {
        self.enabled == mask.enabled
            && self.placement == mask.placement
            && self.pixels.ptr_eq(&Arc::downgrade(&mask.pixels))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_does_not_force_pixel_copies_and_detects_in_place_edits() {
        let mut doc = Document::new(4, 4).unwrap();
        compositor::edits::fill(&mut doc, [40, 80, 120, 255], false, false).unwrap();
        compositor::edits::add_mask(&mut doc, false).unwrap();
        let key = CanvasContent::new(&doc);
        assert!(key.matches(&doc));
        let LayerContent::Raster(Some(pixels)) = &mut doc.layers[0].content else {
            panic!("Raster required")
        };
        assert_eq!(Arc::strong_count(pixels), 1);
        let buffer = pixels.as_raw().as_ptr();
        Arc::make_mut(pixels)[(0, 0)][0] = 41;
        assert_eq!(
            pixels.as_raw().as_ptr(),
            buffer,
            "Identity must not cause a full-image copy"
        );
        assert!(!key.matches(&doc));
        let key = CanvasContent::new(&doc);
        let mask = doc.layers[0].mask.as_mut().unwrap();
        assert_eq!(Arc::strong_count(&mask.pixels), 1);
        Arc::make_mut(&mut mask.pixels)[(0, 0)][0] = 127;
        assert!(!key.matches(&doc));
    }
}
