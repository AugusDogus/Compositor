use super::Tip;
impl Tip {
    /// Bilinear sampling with transparent pixels outside the imported rectangle.
    pub(in crate::brush) fn sample(&self, point: [f64; 2], diameter: f64, antialias: f64) -> f32 {
        let [width, height] = [self.pixels.width(), self.pixels.height()];
        let scale = f64::from(width.max(height)) / diameter;
        let half = [
            f64::from(width) / (2. * scale),
            f64::from(height) / (2. * scale),
        ];
        let edge = ((half[0] - point[0].abs()) / antialias + 0.5).clamp(0., 1.)
            * ((half[1] - point[1].abs()) / antialias + 0.5).clamp(0., 1.);
        if edge == 0. {
            return 0.;
        }
        let [x, y] = [
            (point[0] * scale + f64::from(width) / 2. - 0.5).clamp(0., f64::from(width - 1)),
            (point[1] * scale + f64::from(height) / 2. - 0.5).clamp(0., f64::from(height - 1)),
        ];
        let [left, top] = [x.floor() as i64, y.floor() as i64];
        let [fx, fy] = [x - x.floor(), y - y.floor()];
        let read = |x: i64, y: i64| {
            if x >= 0 && y >= 0 && x < i64::from(width) && y < i64::from(height) {
                f64::from(self.pixels[(x as u32, y as u32)][0]) / 255.
            } else {
                0.
            }
        };
        ((read(left, top) * (1. - fx) + read(left + 1, top) * fx) * (1. - fy)
            + (read(left, top + 1) * (1. - fx) + read(left + 1, top + 1) * fx) * fy) as f32
            * edge as f32
    }
}

pub(in crate::brush) fn deposit(
    stamp: &super::Stamp,
    diameter: f64,
    first: f64,
    spacing: f64,
    segment: &crate::brush::coverage::Segment<'_>,
    offset: [f64; 2],
) -> f32 {
    let (length, antialias, direction) = (segment.length, segment.antialias, segment.direction);
    if first > length {
        return 0.;
    }
    let projection = offset[0] * direction[0] + offset[1] * direction[1];
    let reach = diameter * std::f64::consts::FRAC_1_SQRT_2 + antialias;
    let last = ((length - first) / spacing).floor();
    let low = (((projection - reach - first) / spacing).ceil().max(0.)) as u64;
    let high = ((projection + reach - first) / spacing).floor().min(last);
    if high < low as f64 {
        return 0.;
    }
    let mut coverage: f32 = 0.;
    for index in low..=high as u64 {
        let distance = first + index as f64 * spacing;
        coverage = coverage.max(stamp.cell(index as u32).sample(
            [
                offset[0] - direction[0] * distance,
                offset[1] - direction[1] * distance,
            ],
            diameter,
            antialias,
        ));
    }
    coverage
}
