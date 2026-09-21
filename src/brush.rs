mod blur;
mod coverage;
mod path;
mod source;
use source::Source;

use crate::{
    Result,
    blend::Blend,
    document::{Document, LayerContent},
    geometry::Point,
    invalid,
};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use std::sync::Arc;

#[derive(Clone, Copy, Debug)]
pub struct Brush {
    pub diameter: f64,
    pub hardness: f64,
    pub opacity: f64,
    pub color: [u8; 4],
}
impl Default for Brush {
    fn default() -> Self {
        Self {
            diameter: 40.,
            hardness: 1.,
            opacity: 1.,
            color: [0, 0, 0, 255],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PaintMode {
    Paint,
    Erase,
    Clone { offset: Point },
    Blur,
    Heal(crate::filters::Healing),
}

enum Destination<'a> {
    Mask {
        pixels: &'a mut GrayImage,
        original: &'a GrayImage,
    },
    Image {
        pixels: &'a mut RgbaImage,
        original: &'a RgbaImage,
    },
}

pub struct Stroke {
    original: Document,
    coverage: coverage::Plane,
    samples: Vec<Point>,
    tail: Option<path::Tail>,
    last: Point,
    brush: Brush,
    kernel: coverage::Kernel,
    mode: PaintMode,
    mask: bool,
    source: Option<Source>,
}

/// Warm the shared Vulkan compute pipeline before the first large brush stroke.
pub fn initialize_gpu() -> Result<String> {
    coverage::gpu::initialize()
}

impl Stroke {
    pub fn start(
        doc: &mut Document,
        point: Point,
        brush: Brush,
        mode: PaintMode,
        mask: bool,
        sample_all: bool,
    ) -> Result<Self> {
        validate_point(point)?;
        let layer = doc
            .active_layer()
            .ok_or_else(|| invalid("Select a layer before painting."))?;
        if !(1. ..=5000.).contains(&brush.diameter)
            || !(0. ..=1.).contains(&brush.hardness)
            || !(0. ..=1.).contains(&brush.opacity)
        {
            return Err(invalid(
                "Brush size, hardness, or opacity is outside its supported range.",
            ));
        }
        if mask && matches!(mode, PaintMode::Clone { .. } | PaintMode::Heal(_)) {
            return Err(invalid(
                "Clone and healing edit image pixels. Switch from the mask to the layer.",
            ));
        }
        let source = if matches!(mode, PaintMode::Clone { .. }) {
            Some(Source::clone_snapshot(doc, layer, sample_all))
        } else if mode == PaintMode::Blur {
            Some(Source::Blur(Box::new(blur::Blur::new(
                layer,
                [doc.width, doc.height],
                brush.diameter,
                mask,
            )?)))
        } else {
            None
        };
        if mask {
            if layer.mask.is_none() {
                return Err(invalid("Add a mask before painting it."));
            }
        } else if !matches!(layer.content, LayerContent::Raster(_)) {
            return Err(invalid(
                "Select a pixel layer to paint, or select this layer's mask.",
            ));
        }
        if matches!(mode, PaintMode::Heal(_)) && (mask || layer.raster().is_none()) {
            return Err(invalid(
                "Spot healing needs an existing pixel layer. Switch from the mask to image pixels.",
            ));
        }
        let (width, height) = if mask {
            // Expand uniform masks once so a brush can edit individual pixels.
            let layer = doc
                .active_layer_mut()
                .ok_or_else(|| invalid("No active layer."))?;
            let mask = layer
                .mask
                .as_mut()
                .ok_or_else(|| invalid("No active mask."))?;
            if mask.pixels.width() == 1 && mask.pixels.height() == 1 {
                let t = mask.placement.unwrap_or(layer.transform);
                let (w, h) = (t.size[0].ceil() as u32, t.size[1].ceil() as u32);
                crate::document::validate_size(w, h)?;
                mask.pixels = Arc::new(GrayImage::from_pixel(w, h, mask.pixels[(0, 0)]));
            }
            (mask.pixels.width(), mask.pixels.height())
        } else {
            let layer = doc
                .active_layer_mut()
                .ok_or_else(|| invalid("No active layer."))?;
            if matches!(layer.content, LayerContent::Raster(None))
                && layer.transform.size[0].ceil() * layer.transform.size[1].ceil()
                    > crate::document::MAX_PIXELS as f64
            {
                crate::raster_extent::seed(layer, point)?;
            }
            if let LayerContent::Raster(pixels) = &mut layer.content
                && pixels.is_none()
            {
                let w = layer.transform.size[0].ceil() as u32;
                let h = layer.transform.size[1].ceil() as u32;
                crate::document::validate_size(w, h)?;
                *pixels = Some(Arc::new(RgbaImage::new(w, h)));
            }
            let image = layer
                .raster()
                .ok_or_else(|| invalid("Layer pixels could not be allocated."))?;
            (image.width(), image.height())
        };
        let mut stroke = Self {
            original: doc.clone(),
            coverage: coverage::Plane::new(width, height),
            samples: vec![point],
            tail: None,
            last: point,
            brush,
            kernel: coverage::Kernel::new(brush),
            mode,
            mask,
            source,
        };
        stroke.deposit(doc, point, point)?;
        Ok(stroke)
    }

    pub fn to(&mut self, doc: &mut Document, point: Point) -> Result<()> {
        validate_point(point)?;
        self.append(doc, point)
    }

    fn walk(&mut self, doc: &mut Document, point: Point) -> Result<()> {
        if point != self.last {
            self.deposit(doc, self.last, point)?;
        }
        self.last = point;
        Ok(())
    }

    pub fn finish(&mut self, doc: &mut Document) -> Result<()> {
        self.flush(doc)?;
        if let PaintMode::Heal(mode) = self.mode {
            let layer = doc
                .active_layer_mut()
                .ok_or_else(|| invalid("Healing layer is missing."))?;
            let image = self
                .original
                .layer(layer.id)
                .and_then(|l| l.raster())
                .ok_or_else(|| invalid("Original healing pixels are missing."))?;
            let coverage =
                GrayImage::from_fn(self.coverage.width(), self.coverage.height(), |x, y| {
                    let point = layer.transform.point([
                        (x as f64 + 0.5) / image.width() as f64,
                        (y as f64 + 0.5) / image.height() as f64,
                    ]);
                    let selection = self
                        .original
                        .selection
                        .as_ref()
                        .map_or(1., |s| s.coverage(point));
                    Luma([
                        ((coverage::alpha(self.coverage[(x, y)][0], self.brush) * 255.).round()
                            * self.brush.opacity
                            * selection)
                            .round() as u8,
                    ])
                });
            let healed = crate::filters::heal(image, &coverage, mode, 1.)?;
            layer.content = LayerContent::Raster(Some(Arc::new(healed)));
            layer.shape = None;
        }
        Ok(())
    }

    fn grow_bounds(&mut self, doc: &mut Document, bounds: [f64; 4]) -> Result<()> {
        if !self.mask
            && matches!(
                self.mode,
                PaintMode::Paint | PaintMode::Clone { .. } | PaintMode::Blur
            )
            && bounds[2] > bounds[0]
            && bounds[3] > bounds[1]
        {
            let layer = doc
                .active_layer_mut()
                .ok_or_else(|| invalid("The active layer is missing."))?;
            if let Some(expansion) = crate::raster_extent::expand(layer, bounds)? {
                let original = self
                    .original
                    .active_layer_mut()
                    .ok_or_else(|| invalid("Original stroke layer is missing."))?;
                crate::raster_extent::expand(original, bounds)?;
                let mut coverage = coverage::Plane::new(expansion.size[0], expansion.size[1]);
                image::imageops::replace(
                    &mut coverage,
                    &self.coverage,
                    expansion.offset[0] as i64,
                    expansion.offset[1] as i64,
                );
                self.coverage = coverage;
                if let Some(tail) = &mut self.tail {
                    tail.shift(expansion.offset);
                }
            }
        }
        Ok(())
    }

    fn deposit(&mut self, doc: &mut Document, start: Point, end: Point) -> Result<()> {
        let selection = doc.selection.clone();
        let canvas = [doc.width as f64, doc.height as f64];
        let radius = self.brush.diameter / 2.;
        self.grow_bounds(
            doc,
            [
                (start[0].min(end[0]) - radius).max(0.),
                (start[1].min(end[1]) - radius).max(0.),
                (start[0].max(end[0]) + radius).min(canvas[0]),
                (start[1].max(end[1]) + radius).min(canvas[1]),
            ],
        )?;
        let layer = doc
            .active_layer_mut()
            .ok_or_else(|| invalid("Active layer disappeared during the stroke."))?;
        let original_layer = self
            .original
            .layer(layer.id)
            .ok_or_else(|| invalid("Original stroke layer is missing."))?;
        let t = if self.mask {
            layer
                .mask
                .as_ref()
                .and_then(|m| m.placement)
                .unwrap_or(layer.transform)
        } else {
            layer.transform
        };
        let width = self.coverage.width();
        let height = self.coverage.height();
        // Match MetalBrushCoverage's one-pixel edge ramp in document coordinates.
        let antialias_width = (t.size[0] / width as f64)
            .min(t.size[1] / height as f64)
            .max(0.001);
        let a = t.unit(start);
        let b = t.unit(end);
        let rx = radius / t.size[0] * width as f64 + 1.;
        let ry = radius / t.size[1] * height as f64 + 1.;
        let left = (a[0].min(b[0]) * width as f64 - rx).floor().max(0.) as u32;
        let top = (a[1].min(b[1]) * height as f64 - ry).floor().max(0.) as u32;
        let right = ((a[0].max(b[0]) * width as f64 + rx).ceil().max(0.) as u32).min(width);
        let bottom = ((a[1].max(b[1]) * height as f64 + ry).ceil().max(0.) as u32).min(height);
        let p0 = t.point([0.5 / width as f64, 0.5 / height as f64]);
        let px = t.point([1.5 / width as f64, 0.5 / height as f64]);
        let py = t.point([0.5 / width as f64, 1.5 / height as f64]);
        let dx = [px[0] - p0[0], px[1] - p0[1]];
        let dy = [py[0] - p0[0], py[1] - p0[1]];
        let mut destination = if self.mask {
            let mask = layer
                .mask
                .as_mut()
                .ok_or_else(|| invalid("Mask disappeared during the stroke."))?;
            let original = original_layer
                .mask
                .as_ref()
                .ok_or_else(|| invalid("Original mask is missing."))?;
            Destination::Mask {
                pixels: Arc::make_mut(&mut mask.pixels),
                original: &original.pixels,
            }
        } else {
            let LayerContent::Raster(Some(pixels)) = &mut layer.content else {
                return Err(invalid("Layer pixels disappeared during the stroke."));
            };
            let original = original_layer
                .raster()
                .ok_or_else(|| invalid("Original layer pixels are missing."))?;
            Destination::Image {
                pixels: Arc::make_mut(pixels),
                original,
            }
        };
        let mut changed = false;
        let region = coverage::Region {
            bounds: [left, top, right, bottom],
            origin: p0,
            dx,
            dy,
            canvas,
        };
        let segment = self.kernel.segment(start, end, antialias_width);
        if selection.is_none()
            && let Some(changed) = coverage::gpu::paint(
                &region,
                &mut self.coverage,
                &segment,
                self.brush,
                &mut destination,
                self.mode,
            )?
        {
            if changed {
                layer.shape = None;
            }
            return Ok(());
        }
        let deposits = coverage::gpu::rasterize(&region, &mut self.coverage, &segment, self.brush)?;
        for (local_x, local_y, coverage) in deposits.enumerate_pixels() {
            let coverage = coverage[0];
            if coverage == 0 {
                continue;
            }
            let x = left + local_x;
            let y = top + local_y;
            let doc_point = region.point(x, y);
            let alpha = coverage as f64 / 255.
                * self.brush.opacity
                * selection.as_ref().map_or(1., |s| s.coverage(doc_point));
            changed = true;
            match &mut destination {
                Destination::Mask { pixels, original } => {
                    let ox = x.min(original.width() - 1);
                    let oy = y.min(original.height() - 1);
                    let before = original[(ox, oy)][0] as f64;
                    let target = if self.mode == PaintMode::Blur {
                        self.source.as_ref().map_or(before, |source| {
                            source.sample(doc_point, t.sampling)[0] * 255.
                        })
                    } else if self.mode == PaintMode::Erase {
                        0.
                    } else {
                        self.brush.color[0] as f64
                    };
                    pixels[(x, y)] = Luma([(before + (target - before) * alpha).round() as u8]);
                }
                Destination::Image { pixels, original } => {
                    let before = original
                        .get_pixel_checked(x, y)
                        .map_or([0.; 4], |p| p.0.map(|v| v as f64 / 255.));
                    let mut top = self.brush.color.map(|v| v as f64 / 255.);
                    if let Some(source) = &self.source {
                        match self.mode {
                            PaintMode::Clone { offset } => {
                                top = source.sample(
                                    [doc_point[0] + offset[0], doc_point[1] + offset[1]],
                                    t.sampling,
                                );
                            }
                            PaintMode::Blur => {
                                top = source.sample(doc_point, t.sampling);
                            }
                            _ => {}
                        }
                    }
                    let result = if self.mode == PaintMode::Erase {
                        [before[0], before[1], before[2], before[3] * (1. - alpha)]
                    } else if self.mode == PaintMode::Blur {
                        let out_alpha = before[3] + (top[3] - before[3]) * alpha;
                        let mut result = [0., 0., 0., out_alpha];
                        if out_alpha > 0. {
                            for i in 0..3 {
                                result[i] = (before[i] * before[3] * (1. - alpha)
                                    + top[i] * top[3] * alpha)
                                    / out_alpha;
                            }
                        }
                        result
                    } else {
                        top[3] *= alpha;
                        Blend::Normal.composite(before, top)
                    };
                    pixels[(x, y)] = Rgba(result.map(|v| (v.clamp(0., 1.) * 255.).round() as u8));
                }
            }
        }
        if changed && !self.mask {
            layer.shape = None;
        }
        Ok(())
    }
}

fn validate_point(point: Point) -> Result<()> {
    if point.iter().any(|v| !v.is_finite() || v.abs() > 1_000_000.) {
        return Err(invalid(
            "Brush coordinates exceed supported bounds. Paint closer to the canvas.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render;
    #[test]
    fn painting_past_source_bounds_preserves_source_and_stroke_opacity() {
        let mut doc = Document::new(12, 4).unwrap();
        doc.layers[0].transform = crate::geometry::Transform::new(2, 2);
        doc.layers[0].transform.origin = [5., 0.];
        doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
            2,
            2,
            Rgba([0, 255, 0, 255]),
        ))));
        let before = render::render(&doc, 12, 4);
        let brush = Brush {
            diameter: 2.,
            opacity: 0.5,
            color: [255, 0, 0, 255],
            ..Brush::default()
        };
        let mut stroke =
            Stroke::start(&mut doc, [2.5, 2.5], brush, PaintMode::Paint, false, false).unwrap();
        stroke.to(&mut doc, [8.5, 2.5]).unwrap();
        stroke.to(&mut doc, [2.5, 2.5]).unwrap();
        let after = render::render(&doc, 12, 4);
        assert_eq!(after[(2, 2)], Rgba([255, 0, 0, 128]));
        assert_eq!(after[(5, 0)], before[(5, 0)]);
        assert_eq!(after[(8, 2)], Rgba([255, 0, 0, 128]));
        doc.validate().unwrap();
    }
    #[test]
    fn clone_current_layer_samples_its_pixels_without_masks_or_opacity() {
        let mut doc = Document::new(12, 3).unwrap();
        doc.layers[0].content =
            LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(12, 3, |x, _| {
                if x < 5 {
                    Rgba([255, 0, 0, 255])
                } else {
                    Rgba([0, 0, 255, 255])
                }
            }))));
        crate::edits::add_mask(&mut doc, true).unwrap();
        doc.layers[0].opacity = 0.2;
        Stroke::start(
            &mut doc,
            [8.5, 1.5],
            Brush {
                diameter: 2.,
                ..Brush::default()
            },
            PaintMode::Clone { offset: [-6., 0.] },
            false,
            false,
        )
        .unwrap();
        assert_eq!(
            doc.layers[0].raster().unwrap()[(8, 1)],
            Rgba([255, 0, 0, 255])
        );
    }

    #[test]
    fn blur_mask_softens_edges_without_painting_the_foreground() {
        let mut doc = Document::new(20, 3).unwrap();
        crate::edits::add_mask(&mut doc, false).unwrap();
        doc.layers[0].mask.as_mut().unwrap().pixels =
            Arc::new(GrayImage::from_fn(20, 3, |x, _| {
                Luma([if x < 10 { 0 } else { 255 }])
            }));
        Stroke::start(
            &mut doc,
            [10., 1.5],
            Brush {
                diameter: 8.,
                color: [255; 4],
                ..Brush::default()
            },
            PaintMode::Blur,
            true,
            false,
        )
        .unwrap();
        let mask = &doc.layers[0].mask.as_ref().unwrap().pixels;
        assert!(mask[(9, 1)][0] > 0 && mask[(9, 1)][0] < 128);
        assert!(mask[(10, 1)][0] > 128 && mask[(10, 1)][0] < 255);
        assert_eq!(mask[(0, 1)][0], 0);
    }
    #[test]
    fn overlapping_dabs_do_not_accumulate_stroke_opacity() {
        let mut doc = Document::new(10, 10).unwrap();
        let brush = Brush {
            diameter: 10.,
            hardness: 1.,
            opacity: 0.5,
            color: [255, 0, 0, 255],
        };
        let mut stroke =
            Stroke::start(&mut doc, [5., 5.], brush, PaintMode::Paint, false, false).unwrap();
        stroke.to(&mut doc, [5., 5.]).unwrap();
        assert_eq!(
            doc.layers[0].raster().unwrap()[(5, 5)],
            Rgba([255, 0, 0, 128])
        );
    }
    #[test]
    fn stroke_respects_selection() {
        let mut doc = Document::new(10, 10).unwrap();
        doc.selection = Some(crate::selection::Selection::rectangle(
            10,
            10,
            [0., 0.],
            [5., 10.],
            false,
        ));
        Stroke::start(
            &mut doc,
            [5., 5.],
            Brush {
                diameter: 20.,
                hardness: 1.,
                ..Brush::default()
            },
            PaintMode::Paint,
            false,
            false,
        )
        .unwrap();
        assert_eq!(doc.layers[0].raster().unwrap()[(6, 5)][3], 0);
        assert_eq!(doc.layers[0].raster().unwrap()[(4, 5)][3], 255);
    }
}
