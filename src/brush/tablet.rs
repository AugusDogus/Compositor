//! Pressure changes tip size; tilt gives a bounded elliptical contact patch.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tip {
    pub(super) pressure: Option<f64>,
    pub(super) tilt: Option<[f64; 2]>,
}
impl Tip {
    pub fn new(pressure: Option<f64>, tilt: Option<[f64; 2]>) -> Self {
        Self {
            pressure: pressure.filter(|p| p.is_finite()).map(|p| p.clamp(0., 1.)),
            tilt: tilt
                .filter(|t| t.iter().all(|v| v.is_finite()))
                .map(|t| t.map(|v| v.clamp(-90., 90.))),
        }
    }
    fn force(self) -> f64 {
        self.pressure.unwrap_or(1.)
    }
    pub(super) fn brush(self, mut brush: Brush) -> Brush {
        brush.diameter = (brush.diameter * self.force()).max(0.1);
        brush
    }
    pub(super) fn metric(self) -> [f64; 4] {
        let [x, y] = self.tilt.unwrap_or([0.; 2]);
        let angle = y.atan2(x);
        let (sin, cos) = angle.sin_cos();
        let ratio = (x.hypot(y).min(75.).to_radians().cos()).max(0.25);
        // Keep the configured diameter as the major axis; flatten across the
        // tilt direction without expanding the stroke's allocation bounds.
        [cos, sin, -sin / ratio, cos / ratio]
    }
    fn mix(self, next: Self, t: f64) -> Self {
        let a = self.tilt.unwrap_or([0.; 2]);
        let b = next.tilt.unwrap_or([0.; 2]);
        Self {
            pressure: Some(self.force() + (next.force() - self.force()) * t),
            tilt: Some([0, 1].map(|i| a[i] + (b[i] - a[i]) * t)),
        }
    }
}

#[derive(Clone, Copy)]
pub struct Input {
    pub point: Point,
    pub tip: Option<Tip>,
}

impl Stroke {
    pub fn to_input(&mut self, doc: &mut Document, input: Input) -> Result<()> {
        validate_point(input.point)?;
        let Some(next) = input.tip else {
            return self.to(doc, input.point);
        };
        if input.point == self.last && self.tip == Some(next) {
            return Ok(());
        }
        // Pen samples settle immediately; a future point must never redraw an
        // earlier pressure sample using a different tip. Mouse curves retain
        // the existing provisional-tail interpolation unchanged.
        self.flush(doc)?;
        let previous = self.tip.unwrap_or(Tip {
            pressure: None,
            tilt: None,
        });
        let start = self.last;
        let steps = ((previous.force() - next.force()).abs() * 16.)
            .ceil()
            .clamp(1., 16.) as usize;
        for step in 1..=steps {
            let t = step as f64 / steps as f64;
            let tip = previous.mix(next, t);
            self.brush = tip.brush(self.base_brush);
            self.kernel = coverage::Kernel::new(self.brush);
            self.tip = Some(tip);
            let point = [0, 1].map(|i| start[i] + (input.point[i] - start[i]) * t);
            if tip.force() > 0. {
                self.deposit(doc, self.last, point)?;
            }
            self.last = point;
        }
        self.tip = Some(next);
        self.samples.clear();
        self.samples.push(input.point);
        Ok(())
    }
}
