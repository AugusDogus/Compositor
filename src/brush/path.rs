use super::*;
use image::{GenericImage, GenericImageView, ImageBuffer, Pixel};

enum Pixels {
    Image(RgbaImage),
    Mask(GrayImage),
}

/// Use the image crate's contiguous-row copy for preview snapshots. SubImage's
/// to_image currently copies each pixel separately, which dominates large tips.
fn copy_region<P: Pixel + 'static>(
    source: &ImageBuffer<P, Vec<P::Subpixel>>,
    origin: [u32; 2],
    size: [u32; 2],
) -> Result<ImageBuffer<P, Vec<P::Subpixel>>> {
    let view = image::imageops::crop_imm(source, origin[0], origin[1], size[0], size[1]);
    let mut pixels = source.buffer_with_dimensions(size[0], size[1]);
    pixels.copy_from(&*view, 0, 0)?;
    Ok(pixels)
}

/// Only the rectangle touched by the live straight tail is backed up. Settled
/// coverage and pixels remain in place when the next sample replaces that tail.
pub(super) struct Tail {
    origin: [u32; 2],
    coverage: coverage::Plane,
    pixels: Pixels,
}

impl Tail {
    pub(super) fn shift(&mut self, offset: [u32; 2]) {
        self.origin[0] += offset[0];
        self.origin[1] += offset[1];
    }
}

impl Stroke {
    pub(super) fn append(&mut self, doc: &mut Document, point: Point) -> Result<()> {
        if self.samples.last() == Some(&point) {
            return Ok(());
        }
        self.remove_tail(doc)?;
        self.samples.push(point);
        if self.samples.len() > 4 {
            self.samples.remove(0);
        }
        let n = self.samples.len();
        if n >= 3 {
            self.curve(
                doc,
                self.samples[n.saturating_sub(4)],
                self.samples[n - 3],
                self.samples[n - 2],
                point,
            )?;
        }
        self.draw_tail(doc, self.samples[n - 2], point)
    }

    pub(super) fn flush(&mut self, doc: &mut Document) -> Result<()> {
        self.remove_tail(doc)?;
        let n = self.samples.len();
        if n >= 2 {
            let end = self.samples[n - 1];
            self.curve(
                doc,
                self.samples[n.saturating_sub(3)],
                self.samples[n - 2],
                end,
                end,
            )?;
            self.samples.clear();
            self.samples.push(end);
        }
        Ok(())
    }

    fn curve(
        &mut self,
        doc: &mut Document,
        before: Point,
        start: Point,
        end: Point,
        after: Point,
    ) -> Result<()> {
        let knot =
            |t: f64, a: Point, b: Point| t + (b[0] - a[0]).hypot(b[1] - a[1]).sqrt().max(0.0001);
        let mix = |a: Point, b: Point, ta: f64, tb: f64, t: f64| {
            let wa = (tb - t) / (tb - ta);
            let wb = (t - ta) / (tb - ta);
            [a[0] * wa + b[0] * wb, a[1] * wa + b[1] * wb]
        };
        let t0 = 0.;
        let t1 = knot(t0, before, start);
        let t2 = knot(t1, start, end);
        let t3 = knot(t2, end, after);
        let point = |u: f64| {
            if u == 0. {
                return start;
            }
            if u == 1. {
                return end;
            }
            let t = t1 + (t2 - t1) * u;
            let a1 = mix(before, start, t0, t1, t);
            let a2 = mix(start, end, t1, t2, t);
            let a3 = mix(end, after, t2, t3, t);
            mix(mix(a1, a2, t0, t2, t), mix(a2, a3, t1, t3, t), t1, t2, t)
        };
        // Match the GPU path: subdivide only until the centerline is within
        // 0.2 document pixels of its chord. Straight runs need one segment.
        let mut stack = vec![(start, end, 0., 1., 0)];
        while let Some((a, b, lo, hi, depth)) = stack.pop() {
            let delta = [b[0] - a[0], b[1] - a[1]];
            let length_squared = delta[0].powi(2) + delta[1].powi(2);
            let error = |p: Point| {
                let t = if length_squared > 0. {
                    (((p[0] - a[0]) * delta[0] + (p[1] - a[1]) * delta[1]) / length_squared)
                        .clamp(0., 1.)
                } else {
                    0.
                };
                (p[0] - a[0] - t * delta[0]).hypot(p[1] - a[1] - t * delta[1])
            };
            let mid = (lo + hi) / 2.;
            let m = point(mid);
            let deviation = error(m)
                .max(error(point((lo + mid) / 2.)))
                .max(error(point((mid + hi) / 2.)));
            if deviation <= 0.2 || depth >= 10 {
                self.walk(doc, b)?;
            } else {
                stack.push((m, b, mid, hi, depth + 1));
                stack.push((a, m, lo, mid, depth + 1));
            }
        }
        Ok(())
    }

    fn draw_tail(&mut self, doc: &mut Document, start: Point, end: Point) -> Result<()> {
        let radius = self.brush.diameter / 2. + 2.;
        let bounds = [
            (start[0].min(end[0]) - radius).max(0.),
            (start[1].min(end[1]) - radius).max(0.),
            (start[0].max(end[0]) + radius).min(doc.width as f64),
            (start[1].max(end[1]) + radius).min(doc.height as f64),
        ];
        self.grow_bounds(doc, bounds)?;
        if bounds[2] > bounds[0] && bounds[3] > bounds[1] {
            self.tail = self.backup_tail(doc, bounds)?;
        }
        let saved = self.last;
        let result = self.walk(doc, end);
        self.last = saved;
        result
    }

    fn backup_tail(&self, doc: &Document, bounds: [f64; 4]) -> Result<Option<Tail>> {
        let layer = doc.active_layer().ok_or_else(|| {
            invalid("The brush layer disappeared while preparing the stroke preview.")
        })?;
        let transform = if self.mask {
            layer
                .mask
                .as_ref()
                .and_then(|m| m.placement)
                .unwrap_or(layer.transform)
        } else {
            layer.transform
        };
        let (width, height) = self.coverage.dimensions();
        let corners = [
            [bounds[0], bounds[1]],
            [bounds[2], bounds[1]],
            [bounds[2], bounds[3]],
            [bounds[0], bounds[3]],
        ]
        .map(|p| {
            let u = transform.unit(p);
            [u[0] * width as f64, u[1] * height as f64]
        });
        let min = [0, 1].map(|axis| {
            corners
                .iter()
                .map(|p| p[axis])
                .fold(f64::INFINITY, f64::min)
                .floor()
                .max(0.) as u32
        });
        let max = [0, 1].map(|axis| {
            corners
                .iter()
                .map(|p| p[axis])
                .fold(f64::NEG_INFINITY, f64::max)
                .ceil()
                .max(0.) as u32
        });
        let (w, h) = (
            max[0].min(width).saturating_sub(min[0]),
            max[1].min(height).saturating_sub(min[1]),
        );
        if w == 0 || h == 0 {
            return Ok(None);
        }
        let pixels = if self.mask {
            let mask = layer.mask.as_ref().ok_or_else(|| {
                invalid("The brush mask disappeared while preparing the stroke preview.")
            })?;
            Pixels::Mask(copy_region(&mask.pixels, min, [w, h])?)
        } else {
            let image = layer.raster().ok_or_else(|| {
                invalid("The brush pixels disappeared while preparing the stroke preview.")
            })?;
            Pixels::Image(copy_region(image, min, [w, h])?)
        };
        Ok(Some(Tail {
            origin: min,
            pixels,
            coverage: copy_region(&self.coverage, min, [w, h])?,
        }))
    }

    fn remove_tail(&mut self, doc: &mut Document) -> Result<()> {
        let Some(tail) = self.tail.take() else {
            return Ok(());
        };
        let [x, y] = tail.origin;
        self.coverage.copy_from(&tail.coverage, x, y)?;
        let layer = doc.active_layer_mut().ok_or_else(|| {
            invalid("The brush layer disappeared while replacing the stroke preview.")
        })?;
        match tail.pixels {
            Pixels::Mask(pixels) => {
                let mask = layer.mask.as_mut().ok_or_else(|| {
                    invalid("The brush mask disappeared while replacing the stroke preview.")
                })?;
                Arc::make_mut(&mut mask.pixels).copy_from(&pixels, x, y)?;
            }
            Pixels::Image(pixels) => {
                let LayerContent::Raster(Some(image)) = &mut layer.content else {
                    return Err(invalid(
                        "The brush pixels disappeared while replacing the stroke preview.",
                    ));
                };
                Arc::make_mut(image).copy_from(&pixels, x, y)?;
            }
        }
        Ok(())
    }
}
