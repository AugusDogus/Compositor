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
    blend_if: Option<compositor::blend_if::Settings>,
    clip_source: Option<Uuid>,
    content: Pixels,
    mask: Option<MaskKey>,
}

enum Pixels {
    Raster(Option<Weak<RgbaImage>>),
    PathShape(Weak<RgbaImage>),
    Group,
    Artboard(compositor::artboard::Artboard),
    Adjustment(Box<Adjustment>),
    ExtendedAdjustment(Box<compositor::adjustment::ExtendedAdjustment>),
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
            blend_if: layer.blend_if,
            clip_source: layer.clip_source,
            content: match &layer.content {
                LayerContent::Raster(pixels) => Pixels::Raster(pixels.as_ref().map(Arc::downgrade)),
                LayerContent::PathShape(shape) => Pixels::PathShape(Arc::downgrade(shape.pixels())),
                LayerContent::Group => Pixels::Group,
                LayerContent::Artboard(board) => Pixels::Artboard(*board),
                LayerContent::Adjustment(value) => Pixels::Adjustment(value.clone()),
                LayerContent::ExtendedAdjustment(value) => {
                    Pixels::ExtendedAdjustment(value.clone())
                }
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
            && self.blend_if == layer.blend_if
            && self.blend == layer.blend
            && self.clip_source == layer.clip_source
            && match (&self.content, &layer.content) {
                (Pixels::Raster(Some(a)), LayerContent::Raster(Some(b))) => {
                    a.ptr_eq(&Arc::downgrade(b))
                }
                (Pixels::PathShape(a), LayerContent::PathShape(b)) => {
                    a.ptr_eq(&Arc::downgrade(b.pixels()))
                }
                (Pixels::Raster(None), LayerContent::Raster(None))
                | (Pixels::Group, LayerContent::Group) => true,
                (Pixels::Artboard(a), LayerContent::Artboard(b)) => a == b,
                (Pixels::Adjustment(a), LayerContent::Adjustment(b)) => a == b,
                (Pixels::ExtendedAdjustment(a), LayerContent::ExtendedAdjustment(b)) => a == b,
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
    fn path_shape_geometry_style_and_resolution_changes_invalidate_canvas_cache() {
        use compositor::{
            path_shape::{Content, Style},
            vector_path::{Anchor, BezierPath, Closure},
        };
        let geometry = BezierPath {
            anchors: [[1., 1.], [8., 1.], [8., 8.], [1., 8.]]
                .map(Anchor::corner)
                .to_vec(),
            closure: Closure::Closed,
        };
        let (shape, transform) = Content::from_document_path(
            geometry,
            Style {
                fill: Some([255, 0, 0, 255]),
                stroke: None,
            },
        )
        .unwrap();
        let mut doc = Document::new(16, 16).unwrap();
        doc.layers[0].transform = transform;
        doc.layers[0].content = LayerContent::PathShape(Box::new(shape));
        let key = CanvasContent::new(&doc);
        assert!(key.matches(&doc));
        let original = doc.layers[0].path_shape().unwrap();
        assert_eq!(
            Arc::strong_count(original.pixels()),
            1,
            "Canvas identity must not retain full-resolution pixels"
        );
        let source = original.source().clone();
        for change in 0..3 {
            let key = CanvasContent::new(&doc);
            let shape = if change == 2 {
                Content::from_source(source.clone())
                    .unwrap()
                    .with_resolution([24, 24])
                    .unwrap()
            } else {
                let mut edited = source.clone();
                if change == 0 {
                    edited.style.fill = Some([0, 0, 255, 255]);
                } else {
                    edited.geometry.anchors[0].point[0] += 1.;
                }
                Content::from_source(edited).unwrap()
            };
            doc.layers[0].content = LayerContent::PathShape(Box::new(shape));
            assert!(!key.matches(&doc), "change={change}");
            assert!(CanvasContent::new(&doc).matches(&doc));
        }
    }

    #[test]
    fn editable_filter_settings_invalidate_canvas_cache() {
        use compositor::adjustment::{ExtendedAdjustment, PhotoFilter};
        let mut doc = Document::new(2, 2).unwrap();
        doc.layers[0].content = LayerContent::ExtendedAdjustment(Box::new(
            ExtendedAdjustment::PhotoFilter(PhotoFilter::default()),
        ));
        let key = CanvasContent::new(&doc);
        assert!(key.matches(&doc));
        doc.layers[0].content = LayerContent::ExtendedAdjustment(Box::new(
            ExtendedAdjustment::PhotoFilter(PhotoFilter {
                density: 75.,
                ..Default::default()
            }),
        ));
        assert!(!key.matches(&doc));
    }

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
