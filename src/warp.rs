mod plane;

use crate::{
    Result,
    brush::Brush,
    document::{Document, Layer, LayerContent},
    geometry::Point,
    invalid,
};
use image::{Rgba, RgbaImage};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Smudge,
    Liquify,
}

/// Works in document pixels, then writes only the stroke footprint back to the original layer.
/// Intermediate colors are premultiplied so transparent edges do not introduce dark fringes.
pub struct WarpStroke {
    layer: Layer,
    mode: Mode,
    brush: Brush,
    width: usize,
    height: usize,
    pixels: plane::Plane,
    carried: Vec<[f32; 4]>,
    touched_bounds: [f64; 4],
    last: Point,
}

impl WarpStroke {
    pub fn start(
        doc: &Document,
        point: Point,
        mode: Mode,
        brush: Brush,
        mask: bool,
    ) -> Result<Self> {
        validate_point(point)?;
        if mask {
            return Err(invalid(
                "Smudge and Liquify edit image pixels. Switch from the mask to the layer.",
            ));
        }
        if !(1. ..=5000.).contains(&brush.diameter)
            || !(0. ..=1.).contains(&brush.hardness)
            || !(0. ..=1.).contains(&brush.opacity)
        {
            return Err(invalid(
                "Brush size, hardness, or strength is outside its supported range.",
            ));
        }
        let layer = doc
            .active_layer()
            .ok_or_else(|| invalid("Select a pixel layer to smudge or liquify."))?;
        let source = layer
            .raster()
            .ok_or_else(|| invalid("Smudge and Liquify require an existing pixel layer."))?;
        let (width, height) = (doc.width as usize, doc.height as usize);
        let pixels = plane::Plane::new(source.clone(), layer.transform, [width, height]);
        let mut stroke = Self {
            layer: layer.clone(),
            mode,
            brush: Brush {
                diameter: brush.diameter.max(2.),
                hardness: brush.hardness.min(0.98),
                opacity: brush.opacity.max(0.01),
                ..brush
            },
            width,
            height,
            pixels,
            carried: Vec::new(),
            touched_bounds: [doc.width as f64, doc.height as f64, 0., 0.],
            last: point,
        };
        if mode == Mode::Smudge {
            stroke.pick_up(point);
        }
        Ok(stroke)
    }

    fn radius(&self) -> i32 {
        (self.brush.diameter / 2.).ceil() as i32
    }

    fn weight(&self, dx: i32, dy: i32) -> f32 {
        let u = (dx as f64).hypot(dy as f64) / (self.brush.diameter / 2.);
        if u >= 1. {
            return 0.;
        }
        if u <= self.brush.hardness {
            return 1.;
        }
        let t = (1. - u) / (1. - self.brush.hardness);
        (t * t * (3. - 2. * t)) as f32
    }

    fn pick_up(&mut self, center: Point) {
        let r = self.radius();
        let side = (2 * r + 1) as usize;
        self.carried = vec![[0.; 4]; side * side];
        let (cx, cy) = (center[0].round() as i32, center[1].round() as i32);
        for dy in -r..=r {
            for dx in -r..=r {
                let (x, y) = (cx + dx, cy + dy);
                if x >= 0 && y >= 0 && x < self.width as i32 && y < self.height as i32 {
                    self.carried[(dy + r) as usize * side + (dx + r) as usize] =
                        self.pixels.get(x as usize, y as usize);
                }
            }
        }
    }

    pub fn to(&mut self, doc: &mut Document, point: Point) -> Result<()> {
        validate_point(point)?;
        let from = self.last;
        let distance = (point[0] - from[0]).hypot(point[1] - from[1]);
        let spacing = (self.brush.diameter
            * if self.mode == Mode::Smudge {
                0.08
            } else {
                0.025
            })
        .max(1.);
        if distance < spacing {
            return Ok(());
        }
        let steps = (distance / spacing).ceil() as usize;
        let mut previous = from;
        for step in 1..=steps {
            let t = step as f64 / steps as f64;
            let next = [
                from[0] + (point[0] - from[0]) * t,
                from[1] + (point[1] - from[1]) * t,
            ];
            match self.mode {
                Mode::Smudge => self.smudge(next)?,
                Mode::Liquify => self.push(previous, next)?,
            }
            previous = next;
        }
        self.last = point;
        self.write_back(doc)
    }

    fn smudge(&mut self, center: Point) -> Result<()> {
        let r = self.radius();
        let side = (2 * r + 1) as usize;
        let (cx, cy) = (center[0].round() as i32, center[1].round() as i32);
        for dy in -r..=r {
            for dx in -r..=r {
                let (x, y) = (cx + dx, cy + dy);
                if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
                    continue;
                }
                let w = self.weight(dx, dy);
                if w == 0. {
                    continue;
                }
                let carried = (dy + r) as usize * side + (dx + r) as usize;
                let mut pixel = self.pixels.get(x as usize, y as usize);
                for (k, channel) in pixel.iter_mut().enumerate() {
                    let under = *channel;
                    let paint = under + (self.carried[carried][k] - under) * w;
                    *channel = paint;
                    self.carried[carried][k] =
                        paint + (self.carried[carried][k] - paint) * self.brush.opacity as f32;
                }
                self.pixels.set(x as usize, y as usize, pixel)?;
                self.touch(x, y);
            }
        }
        Ok(())
    }

    fn push(&mut self, from: Point, to: Point) -> Result<()> {
        let movement = [
            (to[0] - from[0]) * self.brush.opacity,
            (to[1] - from[1]) * self.brush.opacity,
        ];
        let r = self.radius();
        let margin = movement[0].abs().max(movement[1].abs()).ceil() as i32 + 2;
        let (cx, cy) = (to[0].round() as i32, to[1].round() as i32);
        let x0 = (cx - r - margin).max(0);
        let y0 = (cy - r - margin).max(0);
        let x1 = (cx + r + margin).min(self.width as i32 - 1);
        let y1 = (cy + r + margin).min(self.height as i32 - 1);
        if x0 > x1 || y0 > y1 {
            return Ok(());
        }
        let (w, h) = ((x1 - x0 + 1) as usize, (y1 - y0 + 1) as usize);
        let mut scratch = Vec::with_capacity(w * h);
        for y in y0..=y1 {
            scratch.extend((x0..=x1).map(|x| self.pixels.get(x as usize, y as usize)));
        }
        for y in (cy - r).max(y0)..=(cy + r).min(y1) {
            for x in (cx - r).max(x0)..=(cx + r).min(x1) {
                let weight = self.weight(x - cx, y - cy) as f64;
                if weight == 0. {
                    continue;
                }
                let sx = (x as f64 - x0 as f64 - movement[0] * weight).clamp(0., (w - 1) as f64);
                let sy = (y as f64 - y0 as f64 - movement[1] * weight).clamp(0., (h - 1) as f64);
                self.pixels.set(
                    x as usize,
                    y as usize,
                    plane::interpolate(w, h, sx, sy, |x, y| scratch[y * w + x]),
                )?;
                self.touch(x, y);
            }
        }
        Ok(())
    }

    fn touch(&mut self, x: i32, y: i32) {
        self.touched_bounds[0] = self.touched_bounds[0].min(x as f64);
        self.touched_bounds[1] = self.touched_bounds[1].min(y as f64);
        self.touched_bounds[2] = self.touched_bounds[2].max(x as f64 + 1.);
        self.touched_bounds[3] = self.touched_bounds[3].max(y as f64 + 1.);
    }

    fn write_back(&self, doc: &mut Document) -> Result<()> {
        let mut expanded = self.layer.clone();
        let mut bounds = self.touched_bounds;
        // Only extend for selected stroke pixels, including antialiased selection edges.
        if let Some(selection) = &doc.selection {
            bounds = [self.width as f64, self.height as f64, 0., 0.];
            for y in self.touched_bounds[1] as u32..self.touched_bounds[3] as u32 {
                for x in self.touched_bounds[0] as u32..self.touched_bounds[2] as u32 {
                    if self.pixels.touched(x as usize, y as usize)
                        && selection.coverage([x as f64 + 0.5, y as f64 + 0.5]) > 0.
                    {
                        bounds[0] = bounds[0].min(x as f64);
                        bounds[1] = bounds[1].min(y as f64);
                        bounds[2] = bounds[2].max(x as f64 + 1.);
                        bounds[3] = bounds[3].max(y as f64 + 1.);
                    }
                }
            }
        }
        if bounds[0] >= bounds[2] || bounds[1] >= bounds[3] {
            return Ok(());
        }
        crate::raster_extent::expand(&mut expanded, bounds)?;
        let source = expanded
            .raster()
            .ok_or_else(|| invalid("Original warp pixels are missing."))?;
        let t = expanded.transform;
        let selection = doc.selection.clone();
        let result = RgbaImage::from_fn(source.width(), source.height(), |x, y| {
            let point = t.point([
                (x as f64 + 0.5) / source.width() as f64,
                (y as f64 + 0.5) / source.height() as f64,
            ]);
            let before = source[(x, y)];
            if point[0] < 0.
                || point[1] < 0.
                || point[0] >= self.width as f64
                || point[1] >= self.height as f64
                || !self.pixels.touched(point[0] as usize, point[1] as usize)
            {
                return before;
            }
            let coverage = selection.as_ref().map_or(1., |s| s.coverage(point)) as f32;
            let top = self.pixels.sample(
                (point[0] - 0.5).clamp(0., (self.width - 1) as f64),
                (point[1] - 0.5).clamp(0., (self.height - 1) as f64),
            );
            let alpha = before[3] as f32 / 255.;
            let out_alpha = alpha + (top[3] - alpha) * coverage;
            let mut out = [0, 0, 0, (out_alpha * 255.).round() as u8];
            if out_alpha > 0. {
                for k in 0..3 {
                    let base = before[k] as f32 / 255. * alpha;
                    out[k] = ((base + (top[k] - base) * coverage) / out_alpha * 255.).round() as u8;
                }
            }
            Rgba(out)
        });
        let layer = doc
            .layers
            .iter_mut()
            .find(|l| l.id == self.layer.id)
            .ok_or_else(|| invalid("The layer being warped is missing."))?;
        layer.content = LayerContent::Raster(Some(Arc::new(result)));
        layer.transform = expanded.transform;
        layer.mask = expanded.mask;
        layer.shape = None;
        Ok(())
    }
}

fn validate_point(point: Point) -> Result<()> {
    if point.iter().any(|v| !v.is_finite() || v.abs() > 1_000_000.) {
        return Err(invalid(
            "Warp coordinates exceed supported bounds. Paint closer to the canvas.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{render, selection::Selection};
    #[test]
    fn warp_can_carry_pixels_beyond_the_original_raster() {
        for mode in [Mode::Smudge, Mode::Liquify] {
            let mut doc = Document::new(20, 10).unwrap();
            doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
                6,
                6,
                Rgba([255, 0, 0, 128]),
            ))));
            doc.layers[0].transform = crate::geometry::Transform::new(6, 6);
            doc.layers[0].transform.origin = [2., 2.];
            let mut stroke = WarpStroke::start(
                &doc,
                [5., 5.],
                mode,
                Brush {
                    diameter: 6.,
                    hardness: 0.8,
                    opacity: 1.,
                    ..Brush::default()
                },
                false,
            )
            .unwrap();
            stroke.to(&mut doc, [12., 5.]).unwrap();
            let result = render::render(&doc, 20, 10);
            assert!(result[(10, 5)][3] > 0, "{mode:?}");
            assert_eq!(result[(10, 5)][0], 255);
            doc.validate().unwrap();
        }
    }
    #[test]
    fn warp_moves_color_and_preserves_unselected_pixels_and_layer_appearance() {
        for mode in [Mode::Smudge, Mode::Liquify] {
            let mut doc = Document::new(16, 8).unwrap();
            let pixels = Arc::new(RgbaImage::from_fn(16, 8, |x, _| {
                if x < 8 {
                    Rgba([255, 0, 0, 128])
                } else {
                    Rgba([0, 0, 255, 128])
                }
            }));
            doc.layers[0].content = LayerContent::Raster(Some(pixels.clone()));
            doc.layers[0].opacity = 0.3;
            doc.selection = Some(Selection::rectangle(16, 8, [0., 2.], [16., 6.], false));
            let mut stroke = WarpStroke::start(
                &doc,
                [4., 4.],
                mode,
                Brush {
                    diameter: 8.,
                    hardness: 0.8,
                    opacity: 1.,
                    ..Brush::default()
                },
                false,
            )
            .unwrap();
            stroke.to(&mut doc, [11., 4.]).unwrap();
            let result = doc.layers[0].raster().unwrap();
            assert!(result[(9, 4)][0] > 150, "{mode:?}");
            assert_eq!(result[(9, 4)][3], 128);
            assert_eq!(result[(9, 0)], pixels[(9, 0)]);
            assert_eq!(doc.layers[0].opacity, 0.3);
        }
    }
    #[test]
    fn click_without_movement_is_a_noop_and_mask_target_is_rejected() {
        let mut doc = Document::new(4, 4).unwrap();
        crate::edits::fill(&mut doc, [200, 50, 20, 128], false, false).unwrap();
        let before = doc.clone();
        let mut stroke =
            WarpStroke::start(&doc, [2., 2.], Mode::Liquify, Brush::default(), false).unwrap();
        stroke.to(&mut doc, [2., 2.]).unwrap();
        assert_eq!(doc, before);
        assert!(WarpStroke::start(&doc, [2., 2.], Mode::Smudge, Brush::default(), true).is_err());
    }
}
