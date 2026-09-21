use crate::{
    Result,
    document::{Document, LayerContent, validate_size},
    geometry::{Sampling, Transform},
    invalid,
    resample::RasterSampler,
};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use std::sync::Arc;

/// Image Size bakes each layer's transformed pixels independently. Scaling a rotated rectangle
/// nonuniformly creates shear, so changing only its width, height, and angle cannot preserve it.
pub fn resize(
    doc: &mut Document,
    width: u32,
    height: u32,
    resolution: f64,
    sampling: Sampling,
) -> Result<()> {
    crate::document::validate_canvas_size(width, height)?;
    if !(1. ..=9600.).contains(&resolution) {
        return Err(invalid(
            "Resolution must be between 1 and 9600 pixels per inch.",
        ));
    }
    if (doc.width, doc.height) == (width, height) {
        doc.resolution = resolution;
        return Ok(());
    }
    validate_size(width, height)?;
    let sx = width as f64 / doc.width as f64;
    let sy = height as f64 / doc.height as f64;
    let old_canvas = Transform::new(doc.width, doc.height);
    let new_canvas = Transform::new(width, height);
    for layer in &mut doc.layers {
        let old = Transform {
            sampling,
            ..layer.transform
        };
        let bounds = old.bounds();
        let left = (bounds[0] * sx).floor();
        let top = (bounds[1] * sy).floor();
        let w = ((bounds[2] * sx).ceil() - left).max(1.) as u32;
        let h = ((bounds[3] * sy).ceil() - top).max(1.) as u32;
        validate_size(w, h)?;
        let transform = Transform {
            origin: [left, top],
            sampling: old.sampling,
            ..Transform::new(w, h)
        };
        if !transform.valid() {
            return Err(invalid(
                "Resizing would move a layer outside the supported bounds.",
            ));
        }
        let unit = |x, y| old.unit([(left + x as f64 + 0.5) / sx, (top + y as f64 + 0.5) / sy]);
        if let LayerContent::Raster(Some(source)) = &mut layer.content {
            let sampler = RasterSampler::new(source, old, [sx, sy]);
            let result = RgbaImage::from_fn(w, h, |x, y| {
                Rgba(sampler.sample(unit(x, y)).map(|v| (v * 255.).round() as u8))
            });
            *source = Arc::new(result);
        }
        if let Some(mask) = &mut layer.mask {
            if let Some(placement) = mask.placement {
                mask.placement = Some(placement.following(old_canvas, new_canvas));
            } else if mask.pixels.dimensions() != (1, 1) {
                let source =
                    RgbaImage::from_fn(mask.pixels.width(), mask.pixels.height(), |x, y| {
                        let value = mask.pixels[(x, y)][0];
                        Rgba([value, value, value, 255])
                    });
                let sampler = RasterSampler::new(&source, old, [sx, sy]);
                mask.pixels = Arc::new(GrayImage::from_fn(w, h, |x, y| {
                    let p = sampler.sample(unit(x, y));
                    Luma([(p[0] * p[3] * 255.).round() as u8])
                }));
            }
        }
        layer.transform = transform;
        layer.shape = None;
        layer.text = None;
    }
    for guide in &mut doc.guides {
        guide.position *= [sx, sy][guide.axis.index()];
    }
    doc.width = width;
    doc.height = height;
    doc.resolution = resolution;
    doc.selection = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render;
    #[test]
    fn nonuniform_resize_preserves_rotated_layer_geometry() {
        let mut doc = Document::new(10, 10).unwrap();
        doc.layers[0].content =
            LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(2, 1, |x, _| {
                if x == 0 {
                    Rgba([255, 0, 0, 255])
                } else {
                    Rgba([0, 0, 255, 255])
                }
            }))));
        doc.layers[0].transform = Transform {
            origin: [4., 4.],
            rotation: 37.,
            sampling: crate::geometry::Sampling::Nearest,
            ..Transform::new(2, 1)
        };
        let original = doc.clone();
        resize(&mut doc, 20, 30, 300., Sampling::Nearest).unwrap();
        assert_eq!(doc.layers[0].transform.rotation, 0.);
        let result = render::render(&doc, 20, 30).unwrap();
        let sampler = render::Sampler::new(&original).unwrap();
        for (x, y, p) in result.enumerate_pixels() {
            assert_eq!(
                p.0,
                sampler
                    .sample([(x as f64 + 0.5) / 2., (y as f64 + 0.5) / 3.])
                    .map(|v| (v * 255.).round() as u8)
            );
        }
        doc.validate().unwrap();
    }
    #[test]
    fn resolution_only_keeps_source_assets() {
        let mut doc = Document::new(2, 2).unwrap();
        crate::edits::fill(&mut doc, [255; 4], false, false).unwrap();
        let source = doc.layers[0].raster().unwrap().clone();
        resize(&mut doc, 2, 2, 300., Sampling::High).unwrap();
        assert!(Arc::ptr_eq(&source, doc.layers[0].raster().unwrap()));
    }

    #[test]
    fn high_quality_reduction_averages_stripes_without_transparent_color_bleed() {
        let mut doc = Document::new(30, 3).unwrap();
        doc.layers[0].content =
            LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(30, 3, |x, _| {
                if x % 3 == 0 {
                    Rgba([255, 0, 0, 255])
                } else {
                    Rgba([0, 255, 0, 0])
                }
            }))));
        resize(&mut doc, 2, 1, 72., Sampling::High).unwrap();
        for p in doc.layers[0].raster().unwrap().pixels() {
            assert_eq!([p[0], p[1], p[2]], [255, 0, 0]);
            assert!((70..=100).contains(&p[3]), "{p:?}");
        }
    }
}
