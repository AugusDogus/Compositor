use crate::{Result, document::Layer, geometry::Sampling, invalid, resample::RasterSampler};
use image::{Rgba, RgbaImage};
use std::borrow::Cow;

pub const LAYER_BOX: u32 = 36;
pub const MASK_BOX: u32 = 30;
const BACKING_SCALE: u32 = 2;

/// Match CanvasThumbnail's whole-point frame before allocating its Retina raster.
pub fn fitted_size(canvas: [u32; 2], box_points: u32) -> [u32; 2] {
    if canvas.contains(&0) {
        return [box_points; 2];
    }
    let scale = box_points as f64 / canvas[0].max(canvas[1]) as f64;
    canvas.map(|v| (v as f64 * scale).round().max(1.) as u32)
}

pub struct Thumbnails {
    pub layer: RgbaImage,
    pub mask: Option<RgbaImage>,
}

impl Thumbnails {
    /// Thumbnails show a layer's position on the whole canvas, independently of its visibility or opacity.
    pub fn render(layer: &Layer, canvas: [u32; 2]) -> Result<Self> {
        if canvas.contains(&0) {
            return Err(invalid("Layer thumbnails require a nonempty canvas."));
        }
        let size = fitted_size(canvas, LAYER_BOX).map(|v| v * BACKING_SCALE);
        // Core Graphics scales both axes by the fitted width, preserving layer proportions.
        let step = canvas[0] as f64 / size[0] as f64;
        let source = layer
            .raster()
            .map(|image| RasterSampler::new(image, layer.transform, [1. / step; 2]));
        let pixels = RgbaImage::from_fn(size[0], size[1], |x, y| {
            let point = [(x as f64 + 0.5) * step, (y as f64 + 0.5) * step];
            let pixel = source
                .as_ref()
                .map_or([0.; 4], |source| source.sample(layer.transform.unit(point)));
            let tile = 6 * BACKING_SCALE;
            let checker = if (x / tile + y / tile).is_multiple_of(2) {
                82.
            } else {
                56.
            };
            Rgba([
                (pixel[0] * pixel[3] * 255. + checker * (1. - pixel[3])).round() as u8,
                (pixel[1] * pixel[3] * 255. + checker * (1. - pixel[3])).round() as u8,
                (pixel[2] * pixel[3] * 255. + checker * (1. - pixel[3])).round() as u8,
                255,
            ])
        });
        let mask = layer.mask.as_ref().map(|mask| {
            let size = fitted_size(canvas, MASK_BOX).map(|v| v * BACKING_SCALE);
            let step = canvas[0] as f64 / size[0] as f64;
            let placement = mask.placement.unwrap_or(layer.transform);
            let background = (mask.edge_tone() * 255.).round() as u8;
            let mut pixels = Cow::Borrowed(mask.pixels.as_ref());
            if placement.sampling != Sampling::Nearest {
                let width = (placement.size[0] / step).ceil().max(1.) as u32;
                let height = (placement.size[1] / step).ceil().max(1.) as u32;
                let reduced = (width.min(pixels.width()), height.min(pixels.height()));
                if reduced != pixels.dimensions() {
                    pixels = Cow::Owned(image::imageops::resize(
                        pixels.as_ref(),
                        reduced.0,
                        reduced.1,
                        image::imageops::FilterType::Lanczos3,
                    ));
                }
            }
            RgbaImage::from_fn(size[0], size[1], |x, y| {
                let unit = placement.unit([(x as f64 + 0.5) * step, (y as f64 + 0.5) * step]);
                let level = if unit.iter().all(|v| (0. ..1.).contains(v)) {
                    (crate::render::mask_pixel(&pixels, unit, placement.sampling) * 255.).round()
                        as u8
                } else {
                    background
                };
                Rgba([level, level, level, 255])
            })
        });
        Ok(Self {
            layer: pixels,
            mask,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{LayerContent, Mask};
    use image::{GrayImage, Luma};
    use std::sync::Arc;

    #[test]
    fn mask_thumbnail_preserves_gray_edge_tone_outside_a_transformed_mask() {
        let mut layer = Layer::blank("Gray mask", 20, 20);
        let mut placement = crate::geometry::Transform::new(20, 20);
        placement.origin = [20., 20.];
        placement.rotation = 30.;
        layer.mask = Some(Mask {
            pixels: Arc::new(GrayImage::from_pixel(20, 20, Luma([96]))),
            enabled: true,
            linked: false,
            placement: Some(placement),
        });
        let pixels = Thumbnails::render(&layer, [60, 60]).unwrap().mask.unwrap();
        assert!(
            pixels
                .pixels()
                .all(|pixel| *pixel == Rgba([96, 96, 96, 255]))
        );
        // Canvas compositing still uses its binary edge extension policy.
        assert_eq!(layer.mask.as_ref().unwrap().background(), 0.);
        layer.mask.as_mut().unwrap().pixels = Arc::new(
            GrayImage::from_raw(3, 3, vec![0, 32, 64, 96, 255, 128, 160, 192, 224]).unwrap(),
        );
        let pixels = Thumbnails::render(&layer, [60, 60]).unwrap().mask.unwrap();
        // The eight outer pixels average 112; the bright center is not an edge pixel.
        assert_eq!(pixels[(0, 0)], Rgba([112, 112, 112, 255]));
        assert_eq!(layer.mask.as_ref().unwrap().background(), 0.);
    }

    #[test]
    fn mask_thumbnail_filters_reductions_but_preserves_nearest_sampling() {
        let mut layer = Layer::blank("Striped mask", 1024, 1024);
        layer.mask = Some(Mask {
            pixels: Arc::new(GrayImage::from_fn(1024, 1024, |x, _| {
                Luma([if x % 2 == 0 { 0 } else { 255 }])
            })),
            enabled: true,
            linked: true,
            placement: None,
        });
        for sampling in [
            crate::geometry::Sampling::Smooth,
            crate::geometry::Sampling::High,
        ] {
            layer.transform.sampling = sampling;
            let pixels = Thumbnails::render(&layer, [1024, 1024])
                .unwrap()
                .mask
                .unwrap();
            assert!(
                pixels.pixels().all(|pixel| (120..=136).contains(&pixel[0])),
                "Aliased {sampling:?} mask thumbnail"
            );
        }
        layer.transform.sampling = crate::geometry::Sampling::Nearest;
        let pixels = Thumbnails::render(&layer, [1024, 1024])
            .unwrap()
            .mask
            .unwrap();
        assert!(pixels.pixels().all(|pixel| matches!(pixel[0], 0 | 255)));
        assert!(pixels.pixels().any(|pixel| pixel[0] == 0));
        assert!(pixels.pixels().any(|pixel| pixel[0] == 255));
    }

    #[test]
    fn layer_and_mask_rasters_double_their_separate_whole_point_frames() {
        let mut layer = Layer::blank("Masked", 100, 100);
        layer.mask = Some(Mask {
            pixels: Arc::new(GrayImage::from_pixel(1, 1, Luma([255]))),
            enabled: true,
            linked: true,
            placement: None,
        });
        for (canvas, layer_size, mask_size) in [
            ([1003, 317], (72, 22), (60, 18)),
            ([317, 1003], (22, 72), (18, 60)),
            ([10_000, 1], (72, 2), (60, 2)),
            ([1, 10_000], (2, 72), (2, 60)),
        ] {
            let thumbnails = Thumbnails::render(&layer, canvas).unwrap();
            assert_eq!(thumbnails.layer.dimensions(), layer_size);
            assert_eq!(thumbnails.mask.unwrap().dimensions(), mask_size);
        }
        assert!(Thumbnails::render(&layer, [0, 100]).is_err());
    }

    #[test]
    fn transparent_thumbnail_uses_dark_six_point_checker_tiles() {
        let layer = Layer::blank("Empty", 100, 100);
        let pixels = Thumbnails::render(&layer, [100, 100]).unwrap().layer;
        for (x, y, value) in [
            (0, 0, 82),
            (11, 11, 82),
            (12, 0, 56),
            (0, 12, 56),
            (12, 12, 82),
        ] {
            assert_eq!(pixels[(x, y)], Rgba([value, value, value, 255]));
        }
    }
    #[test]
    fn thumbnails_follow_canvas_aspect_and_source_placement() {
        let mut layer = Layer::blank("Red", 100, 100);
        layer.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
            100,
            100,
            Rgba([255, 0, 0, 255]),
        ))));
        let pixels = Thumbnails::render(&layer, [400, 200]).unwrap().layer;
        assert_eq!(pixels.dimensions(), (72, 36));
        assert_eq!(pixels[(4, 4)], Rgba([255, 0, 0, 255]));
        assert!(pixels[(60, 30)][0] < 220);
        assert!(pixels[(4, 30)][0] < 220);
        assert_eq!(
            Thumbnails::render(&layer, [300, 600])
                .unwrap()
                .layer
                .dimensions(),
            (36, 72)
        );
    }
    #[test]
    fn mask_thumbnail_extends_edge_tone_outside_its_placement() {
        let mut layer = Layer::blank("Mask", 100, 100);
        layer.transform.origin = [100., 50.];
        layer.mask = Some(Mask {
            pixels: Arc::new(GrayImage::from_fn(20, 20, |x, y| {
                Luma([if (5..15).contains(&x) && (5..15).contains(&y) {
                    0
                } else {
                    255
                }])
            })),
            enabled: true,
            linked: true,
            placement: None,
        });
        let pixels = Thumbnails::render(&layer, [400, 200])
            .unwrap()
            .mask
            .unwrap();
        assert_eq!(pixels[(2, 2)][0], 255);
        assert_eq!(pixels[(22, 15)][0], 0);
        layer.mask.as_mut().unwrap().pixels = Arc::new(GrayImage::from_pixel(1, 1, Luma([0])));
        assert!(
            Thumbnails::render(&layer, [400, 200])
                .unwrap()
                .mask
                .unwrap()
                .pixels()
                .all(|p| p[0] == 0)
        );
    }
}
