pub mod finishing;
pub(crate) mod motion;
pub use finishing::Vignette;

use crate::{
    Result,
    document::{Document, LayerContent, validate_size},
    geometry::Transform,
    invalid, native_pixels,
};
use image::{GrayImage, Luma, RgbaImage};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Filter {
    Gaussian {
        radius: f64,
    },
    Motion {
        distance: f64,
        angle: f64,
    },
    Noise {
        amount: f32,
        gaussian: bool,
        monochromatic: bool,
        seed: u32,
    },
    Lens {
        distortion: f64,
    },
    Vignette(Vignette),
    Bloom {
        amount: f64,
        radius: f64,
    },
    TonalContrast {
        amount: f64,
        radius: f64,
        tones: [f64; 3],
    },
    ContentFill,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Healing {
    ContentAware,
    CreateTexture,
    Proximity,
}

pub fn heal(
    image: &RgbaImage,
    coverage: &GrayImage,
    mode: Healing,
    opacity: f32,
) -> Result<RgbaImage> {
    native_pixels::heal(
        image,
        coverage,
        match mode {
            Healing::ContentAware => 0,
            Healing::CreateTexture => 1,
            Healing::Proximity => 2,
        },
        opacity,
    )
}

pub fn apply(doc: &mut Document, filter: Filter, mask_target: bool) -> Result<()> {
    let selection = doc.selection.clone();
    let canvas = (doc.width, doc.height);
    let layer = doc
        .active_layer_mut()
        .ok_or_else(|| invalid("Select a layer to filter."))?;
    if mask_target {
        let mask = layer
            .mask
            .as_mut()
            .ok_or_else(|| invalid("Select a layer mask to blur."))?;
        let Filter::Gaussian { radius } = filter else {
            return Err(invalid(
                "Only Gaussian Blur applies to a mask. Switch to image pixels for this filter.",
            ));
        };
        if !(0.1..=250.).contains(&radius) {
            return Err(invalid("Blur radius must be 0.1 to 250."));
        }
        let mut result = image::imageops::blur(mask.pixels.as_ref(), radius as f32);
        if let Some(selection) = &selection {
            let t = mask.placement.unwrap_or(layer.transform);
            let (w, h) = result.dimensions();
            for (x, y, p) in result.enumerate_pixels_mut() {
                let amount = selection
                    .coverage(t.point([(x as f64 + 0.5) / w as f64, (y as f64 + 0.5) / h as f64]));
                let before = mask.pixels[(x, y)][0] as f64;
                p[0] = (before + (p[0] as f64 - before) * amount).round() as u8;
            }
        }
        mask.pixels = Arc::new(result);
        return Ok(());
    }
    layer.require_rasterized()?;
    let mut expanded = layer.clone();
    if matches!(filter, Filter::ContentFill) {
        let bounds = selection
            .as_ref()
            .and_then(|s| s.bounds())
            .ok_or_else(|| invalid("Make a nonempty selection around the area to fill first."))?;
        crate::raster_extent::expand(&mut expanded, bounds)?;
    }
    let fills_clear = matches!(filter, Filter::Vignette(_))
        && matches!(expanded.content, LayerContent::Raster(None));
    if fills_clear {
        validate_size(canvas.0, canvas.1)?;
        expanded.content = LayerContent::Raster(Some(Arc::new(RgbaImage::new(canvas.0, canvas.1))));
        expanded.transform = Transform::new(canvas.0, canvas.1);
    }
    let original = expanded
        .raster()
        .ok_or_else(|| invalid("Select a layer containing pixels to filter."))?
        .clone();
    let original_transform = expanded.transform;
    let padding = match filter {
        Filter::Gaussian { radius } => {
            if !(0.1..=250.).contains(&radius) {
                return Err(invalid("Blur radius must be 0.1 to 250."));
            }
            (radius * 3.).ceil() as u32
        }
        Filter::Motion { distance, angle } => {
            if !(1. ..=2000.).contains(&distance) || !(-90. ..=90.).contains(&angle) {
                return Err(invalid(
                    "Motion blur distance must be 1 to 2000 and angle -90 to 90.",
                ));
            }
            (distance / 2.).ceil() as u32
        }
        _ => 0,
    };
    let (source, transform) = if padding > 0 && selection.is_none() {
        pad(&original, original_transform, padding)?
    } else {
        (original.as_ref().clone(), original_transform)
    };
    let (w, h) = source.dimensions();
    let mut result = match filter {
        Filter::Gaussian { radius } => native_pixels::unpremultiply(image::imageops::blur(
            &native_pixels::premultiply(&source),
            radius as f32,
        )),
        Filter::Motion { distance, angle } => motion::apply(&source, distance, angle),
        Filter::Noise {
            amount,
            gaussian,
            monochromatic,
            seed,
        } => native_pixels::noise(&source, amount, gaussian, monochromatic, seed)?,
        Filter::Lens { distortion } => native_pixels::lens(&source, distortion)?,
        Filter::Vignette(settings) => finishing::vignette(&source, settings, fills_clear)?,
        Filter::Bloom { amount, radius } => finishing::bloom(&source, amount, radius)?,
        Filter::TonalContrast {
            amount,
            radius,
            tones,
        } => finishing::tonal(&source, amount, radius, tones)?,
        Filter::ContentFill => {
            let selection = selection
                .as_ref()
                .ok_or_else(|| invalid("Make a selection around the area to fill first."))?;
            let mask = GrayImage::from_fn(w, h, |x, y| {
                Luma([(selection.coverage(
                    transform.point([(x as f64 + 0.5) / w as f64, (y as f64 + 0.5) / h as f64]),
                ) * 255.)
                    .round() as u8])
            });
            native_pixels::fill(&source, &mask)?
        }
    };
    if let Some(selection) = selection {
        for (x, y, p) in result.enumerate_pixels_mut() {
            let amount = selection.coverage(
                transform.point([(x as f64 + 0.5) / w as f64, (y as f64 + 0.5) / h as f64]),
            );
            let before = source[(x, y)];
            if amount == 0. {
                *p = before;
                continue;
            }
            let base_alpha = before[3] as f64 / 255. * (1. - amount);
            let filtered_alpha = p[3] as f64 / 255. * amount;
            let alpha = base_alpha + filtered_alpha;
            for i in 0..3 {
                p[i] = if alpha > 0. {
                    ((before[i] as f64 * base_alpha + p[i] as f64 * filtered_alpha) / alpha).round()
                        as u8
                } else {
                    0
                };
            }
            p[3] = (alpha * 255.).round() as u8;
        }
    }
    layer.mask = expanded.mask;
    if transform != original_transform {
        // Keep a normalized mask at its original document placement when the source grows.
        if let Some(mask) = &mut layer.mask
            && mask.placement.is_none()
        {
            mask.placement = Some(original_transform);
        }
    }
    layer.transform = transform;
    layer.content = LayerContent::Raster(Some(Arc::new(result)));
    layer.shape = None;
    layer.text = None;
    Ok(())
}

fn pad(image: &RgbaImage, transform: Transform, margin: u32) -> Result<(RgbaImage, Transform)> {
    let w = image.width() + 2 * margin;
    let h = image.height() + 2 * margin;
    validate_size(w, h)?;
    let mut pixels = RgbaImage::new(w, h);
    image::imageops::replace(&mut pixels, image, margin as i64, margin as i64);
    let mut expanded = transform;
    expanded.size = [
        transform.size[0] * w as f64 / image.width() as f64,
        transform.size[1] * h as f64 / image.height() as f64,
    ];
    expanded.origin = [
        transform.origin[0] - (expanded.size[0] - transform.size[0]) / 2.,
        transform.origin[1] - (expanded.size[1] - transform.size[1]) / 2.,
    ];
    Ok((pixels, expanded))
}

/// Gaussian blur in premultiplied color, accelerated by Vulkan when available.
pub fn gaussian_rgba(image: &RgbaImage, radius: f32) -> Result<RgbaImage> {
    finishing::range(f64::from(radius), 0.1, 250.)?;
    Ok(native_pixels::unpremultiply(finishing::blur(
        &native_pixels::premultiply(image),
        radius,
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    #[test]
    fn vignette_rejects_oversized_sparse_canvas_before_allocating_pixels() {
        let mut doc = Document::new(30_000, 30_000).unwrap();
        let original = doc.clone();
        let error = apply(&mut doc, Filter::Vignette(Vignette::default()), false).unwrap_err();
        assert!(error.to_string().contains("200 million pixels"));
        assert_eq!(doc, original);
    }

    fn document() -> Document {
        let mut doc = Document::new(15, 15).unwrap();
        doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
            15,
            15,
            Rgba([80, 120, 160, 255]),
        ))));
        doc
    }
    #[test]
    fn content_fill_extends_the_source_to_cover_the_canvas_selection() {
        let mut doc = Document::new(20, 16).unwrap();
        doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
            12,
            16,
            Rgba([80, 120, 160, 255]),
        ))));
        doc.layers[0].transform = Transform::new(12, 16);
        doc.selection = Some(crate::selection::Selection::rectangle(
            20,
            16,
            [12., 0.],
            [16., 16.],
            false,
        ));
        apply(&mut doc, Filter::ContentFill, false).unwrap();
        let pixels = doc.layers[0].raster().unwrap();
        assert_eq!(pixels.dimensions(), (16, 16));
        assert_eq!(pixels[(15, 8)], Rgba([80, 120, 160, 255]));
        assert_eq!(pixels[(0, 8)], Rgba([80, 120, 160, 255]));
    }
    #[test]
    fn content_fill_uses_existing_texture_and_preserves_outside_pixels() {
        let mut doc = document();
        if let LayerContent::Raster(Some(pixels)) = &mut doc.layers[0].content {
            Arc::make_mut(pixels)[(7, 7)] = Rgba([255, 0, 0, 255]);
        }
        doc.selection = Some(crate::selection::Selection::rectangle(
            15,
            15,
            [7., 7.],
            [8., 8.],
            false,
        ));
        apply(&mut doc, Filter::ContentFill, false).unwrap();
        assert_eq!(
            doc.layers[0].raster().unwrap()[(7, 7)],
            Rgba([80, 120, 160, 255])
        );
        assert_eq!(
            doc.layers[0].raster().unwrap()[(0, 0)],
            Rgba([80, 120, 160, 255])
        );
    }
    #[test]
    fn gaussian_grows_layer_and_preserves_center() {
        let mut doc = document();
        apply(&mut doc, Filter::Gaussian { radius: 2. }, false).unwrap();
        let layer = &doc.layers[0];
        assert_eq!(layer.raster().unwrap().dimensions(), (27, 27));
        assert_eq!(layer.transform.origin, [-6., -6.]);
        let p = layer.raster().unwrap()[(5, 13)];
        assert!(p[3] > 0 && p[3] < 255);
        assert!(p[0].abs_diff(80) < 3);
    }
    #[test]
    fn noise_seed_is_repeatable_and_keeps_alpha() {
        let image = RgbaImage::from_pixel(5, 5, Rgba([100, 100, 100, 128]));
        let a = native_pixels::noise(&image, 20., true, true, 7).unwrap();
        assert_eq!(a, native_pixels::noise(&image, 20., true, true, 7).unwrap());
        assert!(
            a.pixels()
                .all(|p| p[3] == 128 && p[0] == p[1] && p[1] == p[2])
        );
    }
    #[test]
    fn mask_blur_only_changes_selected_document_pixels() {
        let mut doc = Document::new(4, 1).unwrap();
        doc.layers[0].mask = Some(crate::document::Mask {
            pixels: Arc::new(GrayImage::from_fn(4, 1, |x, _| {
                Luma([if x < 2 { 255 } else { 0 }])
            })),
            enabled: true,
            linked: true,
            placement: None,
        });
        doc.selection = Some(crate::selection::Selection::rectangle(
            4,
            1,
            [0., 0.],
            [2., 1.],
            false,
        ));
        apply(&mut doc, Filter::Gaussian { radius: 1. }, true).unwrap();
        let mask = &doc.layers[0].mask.as_ref().unwrap().pixels;
        assert!(mask[(1, 0)][0] < 255);
        assert_eq!(mask[(2, 0)][0], 0);
    }

    #[test]
    fn feathered_filter_selection_blends_premultiplied_color() {
        let mut doc = Document::new(3, 1).unwrap();
        doc.layers[0].content =
            LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(3, 1, |x, _| {
                if x == 1 {
                    Rgba([255, 0, 0, 255])
                } else {
                    Rgba([0, 255, 0, 0])
                }
            }))));
        doc.selection = Some(crate::selection::Selection::from_mask(
            GrayImage::from_pixel(3, 1, Luma([128])),
        ));
        apply(&mut doc, Filter::Gaussian { radius: 1. }, false).unwrap();
        let p = doc.layers[0].raster().unwrap()[(0, 0)];
        assert_eq!([p[0], p[1], p[2]], [255, 0, 0]);
        assert!(p[3] > 0 && p[3] < 128);
    }
}
