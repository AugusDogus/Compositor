use crate::{
    Result,
    blend::Blend,
    document::{Document, LayerContent, validate_size},
    geometry::Point,
    invalid,
};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Shape {
    #[default]
    Linear,
    Radial,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Style {
    ForegroundToBackground,
    #[default]
    ForegroundToTransparent,
}

#[derive(Clone, Copy, Debug)]
pub struct Gradient {
    pub shape: Shape,
    pub style: Style,
    pub reversed: bool,
    pub opacity: f64,
}

impl Default for Gradient {
    fn default() -> Self {
        Self {
            shape: Shape::Linear,
            style: Style::ForegroundToTransparent,
            reversed: false,
            opacity: 1.,
        }
    }
}

impl Gradient {
    pub fn apply(
        self,
        doc: &mut Document,
        start: Point,
        end: Point,
        foreground: [u8; 4],
        background: [u8; 4],
        mask_target: bool,
    ) -> Result<()> {
        let dx = end[0] - start[0];
        let dy = end[1] - start[1];
        let length = dx.hypot(dy);
        if !start.into_iter().chain(end).all(f64::is_finite) || length < 0.5 || !length.is_finite()
        {
            return Err(invalid(
                "Drag at least half a pixel to define the gradient.",
            ));
        }
        if !(0. ..=1.).contains(&self.opacity) {
            return Err(invalid("Gradient opacity must be between 0 and 100%."));
        }
        let selection = doc.selection.clone();
        let canvas = [doc.width as f64, doc.height as f64];
        let layer = doc
            .active_layer_mut()
            .ok_or_else(|| invalid("Select a layer or mask for the gradient."))?;
        if !mask_target {
            crate::raster_extent::expand(layer, [0., 0., canvas[0], canvas[1]])?;
        }
        let transform = if mask_target {
            let mask = layer
                .mask
                .as_ref()
                .ok_or_else(|| invalid("Add a mask before drawing a mask gradient."))?;
            mask.placement.unwrap_or(layer.transform)
        } else {
            layer.transform
        };
        let color = |point: Point| {
            let distance = match self.shape {
                Shape::Linear => {
                    ((point[0] - start[0]) * dx + (point[1] - start[1]) * dy) / (length * length)
                }
                Shape::Radial => (point[0] - start[0]).hypot(point[1] - start[1]) / length,
            }
            .clamp(0., 1.);
            let t = if self.reversed {
                1. - distance
            } else {
                distance
            };
            let a = foreground.map(|v| v as f64 / 255.);
            let b = match self.style {
                Style::ForegroundToBackground => background.map(|v| v as f64 / 255.),
                Style::ForegroundToTransparent => [a[0], a[1], a[2], 0.],
            };
            let mut result: [f64; 4] = std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t);
            let inside =
                point[0] >= 0. && point[1] >= 0. && point[0] < canvas[0] && point[1] < canvas[1];
            result[3] *= if inside {
                self.opacity * selection.as_ref().map_or(1., |s| s.coverage(point))
            } else {
                0.
            };
            result
        };
        if mask_target {
            let mask = layer
                .mask
                .as_mut()
                .ok_or_else(|| invalid("The selected layer has no mask."))?;
            if mask.pixels.dimensions() == (1, 1) {
                let w = transform.size[0].ceil() as u32;
                let h = transform.size[1].ceil() as u32;
                validate_size(w, h)?;
                mask.pixels = Arc::new(GrayImage::from_pixel(w, h, mask.pixels[(0, 0)]));
            }
            let (w, h) = mask.pixels.dimensions();
            for (x, y, pixel) in Arc::make_mut(&mut mask.pixels).enumerate_pixels_mut() {
                let top = color(
                    transform.point([(x as f64 + 0.5) / w as f64, (y as f64 + 0.5) / h as f64]),
                );
                let before = pixel[0] as f64 / 255.;
                *pixel = Luma([((before + (top[0] - before) * top[3]) * 255.).round() as u8]);
            }
        } else {
            let LayerContent::Raster(pixels) = &mut layer.content else {
                return Err(invalid(
                    "Select a pixel layer or target the layer's mask for the gradient.",
                ));
            };
            if pixels.is_none() {
                let w = transform.size[0].ceil() as u32;
                let h = transform.size[1].ceil() as u32;
                validate_size(w, h)?;
                *pixels = Some(Arc::new(RgbaImage::new(w, h)));
            }
            if let Some(pixels) = pixels {
                let (w, h) = pixels.dimensions();
                for (x, y, pixel) in Arc::make_mut(pixels).enumerate_pixels_mut() {
                    let top = color(
                        transform.point([(x as f64 + 0.5) / w as f64, (y as f64 + 0.5) / h as f64]),
                    );
                    *pixel = Rgba(
                        Blend::Normal
                            .composite(pixel.0.map(|v| v as f64 / 255.), top)
                            .map(|v| (v * 255.).round() as u8),
                    );
                }
            }
            layer.shape = None;
            layer.text = None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{edits, selection::Selection};

    #[test]
    fn transparent_gradient_keeps_foreground_rgb_and_reverses_alpha() {
        let mut doc = Document::new(5, 1).unwrap();
        Gradient {
            reversed: true,
            ..Gradient::default()
        }
        .apply(
            &mut doc,
            [0.5, 0.5],
            [4.5, 0.5],
            [255, 0, 0, 255],
            [0, 0, 255, 255],
            false,
        )
        .unwrap();
        let pixels = doc.layers[0].raster().unwrap();
        assert_eq!(pixels[(0, 0)][3], 0);
        assert_eq!(pixels[(2, 0)], Rgba([255, 0, 0, 128]));
        assert_eq!(pixels[(4, 0)], Rgba([255, 0, 0, 255]));
    }

    #[test]
    fn radial_mask_gradient_respects_selection_and_opacity() {
        let mut doc = Document::new(5, 5).unwrap();
        edits::add_mask(&mut doc, false).unwrap();
        doc.selection = Some(Selection::rectangle(5, 5, [0., 0.], [3., 5.], false));
        Gradient {
            shape: Shape::Radial,
            opacity: 0.5,
            ..Gradient::default()
        }
        .apply(
            &mut doc,
            [2.5, 2.5],
            [4.5, 2.5],
            [0, 0, 0, 255],
            [255; 4],
            true,
        )
        .unwrap();
        let mask = &doc.layers[0].mask.as_ref().unwrap().pixels;
        assert_eq!(mask.dimensions(), (5, 5));
        assert_eq!(mask[(2, 2)][0], 128);
        assert_eq!(mask[(2, 1)][0], 191);
        assert_eq!(mask[(1, 2)][0], 191);
        assert_eq!(mask[(3, 2)][0], 255);
        assert_eq!(mask[(2, 0)][0], 255);
    }

    #[test]
    fn invalid_gradient_does_not_allocate_or_mutate_pixels() {
        let mut doc = Document::new(2, 2).unwrap();
        let before = doc.clone();
        assert!(
            Gradient::default()
                .apply(&mut doc, [0., 0.], [0., 0.], [0; 4], [255; 4], false)
                .is_err()
        );
        assert_eq!(doc, before);
    }
}
