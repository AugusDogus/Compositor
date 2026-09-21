pub(super) mod gpu;
use super::{Brush, Point};

pub(super) type Plane = image::ImageBuffer<image::Luma<f32>, Vec<f32>>;

/// The Metal reference stores optical density for soft tips, coverage for hard tips.
pub(super) fn alpha(value: f32, brush: Brush) -> f64 {
    if brush.hardness >= 1. {
        f64::from(value)
    } else {
        1. - (-f64::from(value)).exp()
    }
}

const TABLE_STEPS: usize = 4096;
static DENSITY: std::sync::OnceLock<[f32; TABLE_STEPS + 1]> = std::sync::OnceLock::new();

pub(super) enum Kernel {
    Hard {
        radius: f64,
    },
    Soft {
        radius: f64,
        hardness: f64,
        softness: f64,
        spacing: f64,
        density: &'static [f32; TABLE_STEPS + 1],
    },
}

impl Kernel {
    pub(super) fn new(brush: Brush) -> Self {
        let radius = brush.diameter / 2.;
        if brush.hardness >= 1. {
            return Self::Hard { radius };
        }
        let density = DENSITY.get_or_init(|| {
            std::array::from_fn(|i| {
                let t = i as f64 / TABLE_STEPS as f64;
                let coverage = ((-2.5 * t * t).exp() - (-2.5_f64).exp()) / (1. - (-2.5_f64).exp());
                -(1. - coverage).max(0.001).ln() as f32
            })
        });
        Self::Soft {
            radius,
            hardness: brush.hardness,
            softness: 1. - brush.hardness,
            spacing: (brush.diameter * 0.025).max(0.25),
            density,
        }
    }

    pub(super) fn segment(&self, start: Point, end: Point, antialias: f64) -> Segment<'_> {
        let delta = [end[0] - start[0], end[1] - start[1]];
        let length = delta[0].hypot(delta[1]);
        Segment {
            kernel: self,
            start,
            direction: if length > 0. {
                delta.map(|v| v / length)
            } else {
                [0.; 2]
            },
            length,
            antialias,
        }
    }
}

pub(super) struct Segment<'a> {
    kernel: &'a Kernel,
    start: Point,
    direction: Point,
    length: f64,
    antialias: f64,
}

impl Segment<'_> {
    pub(super) fn deposit(&self, point: Point) -> f32 {
        let offset = [point[0] - self.start[0], point[1] - self.start[1]];
        let projection = offset[0] * self.direction[0] + offset[1] * self.direction[1];
        let (radius, hardness, softness, spacing, table) = match self.kernel {
            Kernel::Hard { radius } => {
                let t = projection.clamp(0., self.length);
                let distance =
                    (offset[0] - t * self.direction[0]).hypot(offset[1] - t * self.direction[1]);
                return ((radius - distance) / self.antialias + 0.5).clamp(0., 1.) as f32;
            }
            Kernel::Soft {
                radius,
                hardness,
                softness,
                spacing,
                density,
            } => (*radius, *hardness, *softness, *spacing, *density),
        };
        // Interpolate the shared density curve instead of evaluating exp/log eight
        // times per pixel. Table error stays below one 8-bit coverage level.
        let density = |distance_squared: f64| {
            let t = ((distance_squared.sqrt() / radius - hardness) / softness).clamp(0., 1.);
            let index = t * TABLE_STEPS as f64;
            let i = (index as usize).min(TABLE_STEPS - 1);
            f64::from(table[i]) + f64::from(table[i + 1] - table[i]) * (index - i as f64)
        };
        if self.length < 1e-6 {
            return density(offset[0].powi(2) + offset[1].powi(2)) as f32;
        }
        let perpendicular_squared =
            (offset[0] * self.direction[1] - offset[1] * self.direction[0]).powi(2);
        if perpendicular_squared >= radius * radius {
            return 0.;
        }
        let reach = (radius * radius - perpendicular_squared).sqrt();
        let lo = (projection - reach).max(0.);
        let hi = (projection + reach).min(self.length);
        if hi <= lo {
            return 0.;
        }
        let midpoint = (lo + hi) / 2.;
        let half_length = (hi - lo) / 2.;
        // Eight-point Gauss-Legendre quadrature, matching MetalBrushCoverage.swift.
        let integral: f64 = [
            (0.1834346425, 0.3626837834),
            (0.5255324099, 0.3137066459),
            (0.7966664774, 0.2223810345),
            (0.9602898565, 0.1012285363),
        ]
        .into_iter()
        .map(|(node, weight)| {
            let a = midpoint - half_length * node - projection;
            let b = midpoint + half_length * node - projection;
            weight
                * (density(perpendicular_squared + a * a) + density(perpendicular_squared + b * b))
        })
        .sum();
        (integral * half_length / spacing) as f32
    }
}

pub(super) struct Region {
    pub bounds: [u32; 4],
    pub origin: Point,
    pub dx: Point,
    pub dy: Point,
    pub canvas: Point,
}

impl Region {
    pub(super) fn point(&self, x: u32, y: u32) -> Point {
        [
            self.origin[0] + self.dx[0] * x as f64 + self.dy[0] * y as f64,
            self.origin[1] + self.dx[1] * x as f64 + self.dy[1] * y as f64,
        ]
    }

    /// Rasterize independent rows concurrently, then return only newly changed
    /// 8-bit coverage. Document pixels are updated after every worker has joined.
    pub(super) fn rasterize(
        &self,
        plane: &mut Plane,
        segment: &Segment<'_>,
        brush: Brush,
    ) -> crate::Result<image::GrayImage> {
        let [left, top, right, bottom] = self.bounds;
        let columns = right.saturating_sub(left) as usize;
        let rows = bottom.saturating_sub(top) as usize;
        if columns == 0 || rows == 0 {
            return Ok(image::GrayImage::new(0, 0));
        }
        let stride = plane.width() as usize;
        let data: &mut [f32] = plane.as_mut();
        let data = &mut data[top as usize * stride..bottom as usize * stride];
        let mut changed = vec![0; columns * rows];
        let saturated = if brush.hardness >= 1. {
            254.5 / 255.
        } else {
            -(0.5_f32 / 255.).ln()
        };
        let render_rows = |data: &mut [f32], output: &mut [u8], first_row: usize| {
            for (row, (values, output)) in data
                .chunks_mut(stride)
                .zip(output.chunks_mut(columns))
                .enumerate()
            {
                let y = top + (first_row + row) as u32;
                for (column, (value, output)) in values[left as usize..right as usize]
                    .iter_mut()
                    .zip(output)
                    .enumerate()
                {
                    if *value >= saturated {
                        continue;
                    }
                    let point = self.point(left + column as u32, y);
                    if point
                        .iter()
                        .zip(self.canvas)
                        .any(|(v, limit)| *v < 0. || *v >= limit)
                    {
                        continue;
                    }
                    let previous_alpha = (alpha(*value, brush) * 255.).round() as u8;
                    let added = segment.deposit(point);
                    *value = if brush.hardness >= 1. {
                        value.max(added)
                    } else {
                        *value + added
                    };
                    let next = (alpha(*value, brush) * 255.).round() as u8;
                    if next > previous_alpha {
                        *output = next;
                    }
                }
            }
        };
        // Small strokes stay on the caller. Cap large work at eight workers so
        // painting leaves capacity for the canvas renderer and other applications.
        let workers = if columns * rows < 65_536 {
            1
        } else {
            std::thread::available_parallelism().map_or(1, |n| n.get().min(8))
        };
        if workers == 1 {
            render_rows(data, &mut changed, 0);
        } else {
            let chunk_rows = rows.div_ceil(workers);
            std::thread::scope(|scope| -> crate::Result<()> {
                let mut jobs = Vec::new();
                for (index, (values, output)) in data
                    .chunks_mut(chunk_rows * stride)
                    .zip(changed.chunks_mut(chunk_rows * columns))
                    .enumerate()
                {
                    let render_rows = &render_rows;
                    jobs.push(std::thread::Builder::new().name("brush-coverage".into()).spawn_scoped(scope, move || render_rows(values, output, index * chunk_rows)).map_err(|error| crate::invalid(format!("Could not start brush coverage processing: {error}. Cancel the stroke and try again.")))?);
                }
                for job in jobs {
                    job.join().map_err(|_| crate::invalid("Brush coverage processing stopped unexpectedly. Cancel the stroke and try again."))?;
                }
                Ok(())
            })?;
        }
        image::GrayImage::from_raw(columns as u32, rows as u32, changed).ok_or_else(|| crate::invalid("Brush coverage dimensions do not match its pixel buffer. Cancel the stroke and try again."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallel_rows_match_serial_coverage_without_touching_pixels_outside_the_region() {
        for hardness in [0., 1.] {
            let brush = Brush {
                diameter: 80.,
                hardness,
                ..Brush::default()
            };
            let kernel = Kernel::new(brush);
            let segment = kernel.segment([30., 350.], [370., 40.], 1.);
            let region = Region {
                bounds: [21, 18, 387, 391],
                origin: [0.5, 0.5],
                dx: [1., 0.],
                dy: [0., 1.],
                canvas: [400.; 2],
            };
            let mut plane = Plane::from_fn(400, 400, |x, _| {
                image::Luma([if x < 200 { 0. } else { 0.2 }])
            });
            let before = plane.clone();
            let changed = region.rasterize(&mut plane, &segment, brush).unwrap();
            for y in 0..400 {
                for x in 0..400 {
                    let previous = before[(x, y)][0];
                    if !(21..387).contains(&x) || !(18..391).contains(&y) {
                        assert_eq!(plane[(x, y)][0], previous);
                        continue;
                    }
                    let added = segment.deposit(region.point(x, y));
                    let expected = if hardness == 1. {
                        previous.max(added)
                    } else {
                        previous + added
                    };
                    assert_eq!(plane[(x, y)][0], expected);
                    let previous = (alpha(previous, brush) * 255.).round() as u8;
                    let expected = (alpha(expected, brush) * 255.).round() as u8;
                    assert_eq!(
                        changed[(x - 21, y - 18)][0],
                        if expected > previous { expected } else { 0 }
                    );
                }
            }
        }
    }

    #[test]
    fn density_lookup_keeps_click_coverage_within_one_8_bit_level() {
        for hardness in [0., 0.5, 0.999] {
            let brush = Brush {
                diameter: 100.,
                hardness,
                ..Brush::default()
            };
            let kernel = Kernel::new(brush);
            let segment = kernel.segment([0.; 2], [0.; 2], 1.);
            for i in 0..100_000 {
                let t = i as f64 / 100_000.;
                let distance = (hardness + t * (1. - hardness)) * 50.;
                let expected = ((-2.5 * t * t).exp() - (-2.5_f64).exp()) / (1. - (-2.5_f64).exp());
                let actual = alpha(segment.deposit([distance, 0.]), brush);
                assert!((actual - expected).abs() < 1. / 255.);
            }
        }
    }

    #[test]
    fn continuous_soft_deposition_matches_independent_dense_integration() {
        for hardness in [0., 0.35, 0.8] {
            let brush = Brush {
                diameter: 60.,
                hardness,
                ..Brush::default()
            };
            for point in [[10., 28.], [75., 25.], [102., 17.], [50., 3.]] {
                let steps = 100_000;
                let mut density = 0.;
                for i in 0..steps {
                    let position = (i as f64 + 0.5) * 100. / steps as f64;
                    let distance = (point[0] - position).hypot(point[1]);
                    let t = ((distance / 30. - hardness) / (1. - hardness)).clamp(0., 1.);
                    let coverage =
                        ((-2.5 * t * t).exp() - (-2.5_f64).exp()) / (1. - (-2.5_f64).exp());
                    density += -(1. - coverage).max(0.001).ln() * 100. / steps as f64 / 1.5;
                }
                let expected = 1. - (-density).exp();
                let actual = alpha(
                    Kernel::new(brush)
                        .segment([0., 0.], [100., 0.], 1.)
                        .deposit(point),
                    brush,
                );
                assert!(
                    (actual - expected).abs() < 0.006,
                    "hardness {hardness}, point {point:?}: {actual} vs {expected}"
                );
            }
        }
    }
}
