use crate::{
    Result,
    document::{Document, Mask},
    invalid,
};
use image::{GrayImage, Luma, RgbaImage};
use std::sync::Arc;

mod inference;
pub use inference::shutdown;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Quality {
    Basic,
    Advanced {
        refine_edges: f64,
        contrast: f64,
        shift_edge: f64,
    },
}

impl Quality {
    pub fn validate(self) -> Result<()> {
        if let Quality::Advanced {
            refine_edges,
            contrast,
            shift_edge,
        } = self
            && (!(0. ..=40.).contains(&refine_edges)
                || !(0. ..=100.).contains(&contrast)
                || !(-10. ..=10.).contains(&shift_edge))
        {
            return Err(invalid(
                "Refine edges must be 0 to 40, contrast 0 to 100, and shift edge -10 to 10.",
            ));
        }
        Ok(())
    }
}

/// A model result tied to its immutable source pixels. Refinement reuses it while a panel is open.
#[derive(Clone)]
pub struct SubjectMask {
    source: Arc<RgbaImage>,
    pixels: Arc<GrayImage>,
}

pub fn remove(doc: &mut Document, quality: Quality) -> Result<()> {
    quality.validate()?;
    SubjectMask::detect(doc)?.apply(doc, quality)
}

impl SubjectMask {
    pub fn detect(doc: &Document) -> Result<Self> {
        let image = doc
            .active_layer()
            .and_then(|layer| layer.raster())
            .ok_or_else(|| invalid("Select a layer containing pixels to remove its background."))?;
        let mask = inference::detect(image)?;
        if mask.pixels().all(|p| p[0] < 8) {
            return Err(invalid(
                "No foreground subject was detected. Try an image with a more distinct subject.",
            ));
        }
        Ok(Self {
            source: image.clone(),
            pixels: Arc::new(mask),
        })
    }

    pub fn apply(&self, doc: &mut Document, quality: Quality) -> Result<()> {
        quality.validate()?;
        let selection = doc.selection.clone();
        let layer = doc.active_layer_mut().ok_or_else(|| {
            invalid("The background-removal layer is missing. Select the source layer and retry.")
        })?;
        let image = layer.raster()
            .filter(|image| Arc::ptr_eq(image, &self.source))
            .ok_or_else(|| invalid("The layer pixels changed while removing the background. Reopen Remove Background to process the current image."))?;
        let mut mask = self.pixels.as_ref().clone();
        if let Quality::Advanced {
            refine_edges,
            contrast,
            shift_edge,
        } = quality
        {
            if refine_edges > 0. {
                mask = refine(&mask, image, refine_edges.round().max(1.) as usize);
            }
            if shift_edge != 0. {
                mask = image::imageops::blur(&mask, (shift_edge.abs() / 2.) as f32);
                let level = if shift_edge < 0. { 0.75 } else { 0.25 };
                for p in mask.pixels_mut() {
                    p[0] =
                        (((p[0] as f64 / 255. - level) / 0.001).clamp(0., 1.) * 255.).round() as u8;
                }
            }
            let slope = 1. / (1. - contrast / 100. * 0.98).max(0.02);
            for p in mask.pixels_mut() {
                p[0] =
                    (((p[0] as f64 / 255. - 0.5) * slope + 0.5).clamp(0., 1.) * 255.).round() as u8;
            }
        }
        let (w, h) = mask.dimensions();
        // Swift combines existing coverage only when it uses the source image's
        // local grid. Independently placed and uniform masks retain their metadata,
        // but are not sampled into the new segmentation grid.
        let existing = layer.mask.as_ref().filter(|existing| {
            existing.placement.is_none() && existing.pixels.dimensions() == (w, h)
        });
        for (x, y, pixel) in mask.enumerate_pixels_mut() {
            let point = layer
                .transform
                .point([(x as f64 + 0.5) / w as f64, (y as f64 + 0.5) / h as f64]);
            let before = existing.map_or(1., |existing| existing.pixels[(x, y)][0] as f64 / 255.);
            let amount = selection.as_ref().map_or(1., |s| s.coverage(point));
            let refined = pixel[0] as f64 * before;
            pixel[0] = (255. * before * (1. - amount) + refined * amount).round() as u8;
        }
        layer.mask = Some(Mask {
            pixels: Arc::new(mask),
            enabled: true,
            linked: layer.mask.as_ref().is_none_or(|mask| mask.linked),
            placement: layer.mask.as_ref().and_then(|mask| mask.placement),
        });
        Ok(())
    }
}

fn box_mean(source: &[f32], w: usize, h: usize, radius: usize) -> Vec<f32> {
    let r = radius as isize;
    let span = (radius * 2 + 1) as f32;
    let mut pass = vec![0.; w * h];
    let clamp = |i: isize, size: usize| i.clamp(0, size as isize - 1) as usize;
    for y in 0..h {
        let row = y * w;
        let mut sum: f32 = (-r..=r).map(|x| source[row + clamp(x, w)]).sum();
        for x in 0..w {
            pass[row + x] = sum / span;
            sum -= source[row + clamp(x as isize - r, w)];
            sum += source[row + clamp(x as isize + r + 1, w)];
        }
    }
    let mut result = vec![0.; w * h];
    for x in 0..w {
        let mut sum: f32 = (-r..=r).map(|y| pass[clamp(y, h) * w + x]).sum();
        for y in 0..h {
            result[y * w + x] = sum / span;
            sum -= pass[clamp(y as isize - r, h) * w + x];
            sum += pass[clamp(y as isize + r + 1, h) * w + x];
        }
    }
    result
}

fn refine(mask: &GrayImage, guide: &RgbaImage, radius: usize) -> GrayImage {
    let (w, h) = (mask.width() as usize, mask.height() as usize);
    let g: Vec<f32> = guide
        .pixels()
        .map(|p| (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.)
        .collect();
    let m: Vec<f32> = mask.pixels().map(|p| p[0] as f32 / 255.).collect();
    let mean_g = box_mean(&g, w, h, radius);
    let mean_m = box_mean(&m, w, h, radius);
    let squares: Vec<_> = g.iter().map(|v| v * v).collect();
    let products: Vec<_> = g.iter().zip(&m).map(|(a, b)| a * b).collect();
    let mean_squares = box_mean(&squares, w, h, radius);
    let mean_products = box_mean(&products, w, h, radius);
    let slopes: Vec<_> = (0..w * h)
        .map(|i| {
            (mean_products[i] - mean_g[i] * mean_m[i])
                / (mean_squares[i] - mean_g[i] * mean_g[i] + 1e-4)
        })
        .collect();
    let offsets: Vec<_> = (0..w * h)
        .map(|i| mean_m[i] - slopes[i] * mean_g[i])
        .collect();
    let mean_slopes = box_mean(&slopes, w, h, radius);
    let mean_offsets = box_mean(&offsets, w, h, radius);
    GrayImage::from_fn(w as u32, h as u32, |x, y| {
        let i = y as usize * w + x as usize;
        Luma([((mean_slopes[i] * g[i] + mean_offsets[i]).clamp(0., 1.) * 255.).round() as u8])
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removal_preserves_mask_placement_and_combines_only_matching_local_grids() {
        for (width, placement, existing_coverage) in [
            (4, None, 128),
            (1, None, 255),
            (4, Some(crate::geometry::Transform::new(2, 2)), 255),
        ] {
            let mut doc = Document::new(4, 2).unwrap();
            let source = Arc::new(RgbaImage::from_pixel(
                4,
                2,
                image::Rgba([200, 100, 50, 255]),
            ));
            doc.layers[0].content = crate::document::LayerContent::Raster(Some(source.clone()));
            doc.layers[0].mask = Some(Mask {
                pixels: Arc::new(GrayImage::from_pixel(width, 2, Luma([128]))),
                enabled: false,
                linked: false,
                placement,
            });
            doc.selection = Some(crate::selection::Selection::rectangle(
                4,
                2,
                [0., 0.],
                [2., 2.],
                false,
            ));
            let subject = SubjectMask {
                source,
                pixels: Arc::new(GrayImage::from_pixel(4, 2, Luma([128]))),
            };
            subject.apply(&mut doc, Quality::Basic).unwrap();
            let mask = doc.layers[0].mask.as_ref().unwrap();
            assert!(mask.enabled);
            assert!(!mask.linked);
            assert_eq!(mask.placement, placement);
            assert_eq!(
                mask.pixels[(0, 0)][0],
                if existing_coverage == 128 { 64 } else { 128 }
            );
            assert_eq!(mask.pixels[(3, 0)][0], existing_coverage);
        }
    }
    #[test]
    fn background_masks_respect_selection_and_reuse_original_pixels() {
        let mut doc = Document::new(4, 2).unwrap();
        let image = Arc::new(RgbaImage::from_pixel(
            4,
            2,
            image::Rgba([200, 100, 50, 255]),
        ));
        doc.layers[0].content = crate::document::LayerContent::Raster(Some(image.clone()));
        let subject = SubjectMask {
            source: image.clone(),
            pixels: Arc::new(GrayImage::from_pixel(4, 2, Luma([0]))),
        };
        doc.selection = Some(crate::selection::Selection::rectangle(
            4,
            2,
            [0., 0.],
            [2., 2.],
            false,
        ));
        for existing in [None, Some(128)] {
            doc.layers[0].mask = existing.map(|level| Mask {
                pixels: Arc::new(GrayImage::from_pixel(4, 2, Luma([level]))),
                enabled: true,
                linked: false,
                placement: None,
            });
            let original = doc.clone();
            subject.apply(&mut doc, Quality::Basic).unwrap();
            let mask = doc.layers[0].mask.as_ref().unwrap();
            assert_eq!(mask.pixels[(0, 0)][0], 0);
            assert_eq!(mask.pixels[(3, 0)][0], existing.unwrap_or(255));
            assert_eq!(mask.linked, existing.is_none());
            assert!(Arc::ptr_eq(doc.layers[0].raster().unwrap(), &image));
            doc = original;
            subject
                .apply(
                    &mut doc,
                    Quality::Advanced {
                        refine_edges: 0.,
                        contrast: 100.,
                        shift_edge: 0.,
                    },
                )
                .unwrap();
            assert_eq!(
                doc.layers[0].mask.as_ref().unwrap().pixels[(3, 0)][0],
                existing.unwrap_or(255)
            );
        }
        doc.layers[0].content =
            crate::document::LayerContent::Raster(Some(Arc::new(RgbaImage::new(4, 2))));
        let before = doc.clone();
        assert!(subject.apply(&mut doc, Quality::Basic).is_err());
        assert_eq!(doc, before);
    }
    #[test]
    fn guided_refinement_keeps_uniform_masks_uniform() {
        let guide = RgbaImage::from_pixel(5, 5, image::Rgba([100, 100, 100, 255]));
        let mask = GrayImage::from_pixel(5, 5, Luma([128]));
        assert_eq!(refine(&mask, &guide, 2), mask);
    }
}
