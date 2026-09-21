use super::*;

#[derive(Clone, Copy)]
pub(crate) enum Mapping {
    Affine(Transform),
    Perspective {
        forward: Homography,
        inverse: Homography,
    },
    Folded([Point; 4]),
}

fn area(a: Point, b: Point, c: Point) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

impl Mapping {
    /// Piecewise affine folds can have extrema where an edge crosses the fold.
    pub(crate) fn mapped_outline(self, corners: [Point; 4]) -> Vec<Point> {
        let mut points: Vec<_> = corners.into_iter().map(|p| self.map(p)).collect();
        if matches!(self, Self::Folded(_)) {
            for i in 0..4 {
                let a = corners[i];
                let b = corners[(i + 1) % 4];
                let da = a[0] - a[1];
                let db = b[0] - b[1];
                if da * db < 0. {
                    let t = da / (da - db);
                    points.push(self.map(std::array::from_fn(|axis| {
                        a[axis] + t * (b[axis] - a[axis])
                    })));
                }
            }
        }
        points
    }

    pub(crate) fn new(corners: [Point; 4]) -> Result<Self> {
        if corners
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
        {
            return Err(invalid("Distortion corners exceed supported bounds."));
        }
        if area(corners[0], corners[1], corners[2]).abs() <= 0.01
            || area(corners[0], corners[2], corners[3]).abs() <= 0.01
        {
            return Err(invalid(
                "A distortion triangle collapsed to a line. Move its corner to restore some area.",
            ));
        }
        match Homography::new(corners) {
            Ok(forward) => Ok(Self::Perspective {
                inverse: forward.inverse()?,
                forward,
            }),
            Err(_) => Ok(Self::Folded(corners)),
        }
    }

    pub(crate) fn map(self, p: Point) -> Point {
        match self {
            Self::Affine(transform) => transform.point(p),
            Self::Perspective { forward, .. } => forward.map(p),
            Self::Folded(c) => {
                let (a, b, u, v) = if p[1] <= p[0] {
                    (c[1], c[2], p[0] - p[1], p[1])
                } else {
                    (c[2], c[3], p[0], p[1] - p[0])
                };
                std::array::from_fn(|i| c[0][i] + (a[i] - c[0][i]) * u + (b[i] - c[0][i]) * v)
            }
        }
    }

    /// A fold can place both triangles over the same output pixel. Return them in
    /// paint order so translucent content retains source-over coverage.
    pub(crate) fn inverse_points(self, point: Point) -> [Option<Point>; 2] {
        match self {
            Self::Affine(transform) => [Some(transform.unit(point)), None],
            Self::Perspective { inverse, .. } => [Some(inverse.map(point)), None],
            Self::Folded(c) => std::array::from_fn(|i| {
                let (a, b) = if i == 0 { (c[1], c[2]) } else { (c[2], c[3]) };
                let denominator = area(c[0], a, b);
                let u = area(c[0], point, b) / denominator;
                let v = area(c[0], a, point) / denominator;
                // Give the shared diagonal to just one triangle.
                if u < 0. || v < 0. || u + v > 1. || (i == 1 && v == 0.) {
                    return None;
                }
                Some(if i == 0 { [u + v, v] } else { [u, u + v] })
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folded_bounds_include_extrema_at_diagonal_crossings() {
        let mapping = Mapping::new([[0., 0.], [10., 10.], [10., 0.], [0., 10.]]).unwrap();
        let diamond = [[0.5, 0.], [1., 0.5], [0.5, 1.], [0., 0.5]];
        let outline = mapping.mapped_outline(diamond);
        assert_eq!(outline.len(), 6);
        assert!(outline.contains(&[7.5, 0.]));
        assert!(outline.contains(&[2.5, 0.]));
        assert!(diamond.into_iter().all(|p| mapping.map(p)[1] == 5.));
    }
}
