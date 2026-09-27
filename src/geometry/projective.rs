//! Checked projective geometry. Matrix composition never assumes an affine map.

use super::Point;
use serde::{Deserialize, Serialize};
use std::fmt;

const TOLERANCE: f64 = 128. * f64::EPSILON;
const MAX_COORDINATE: f64 = 1_000_000.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    NonFinite,
    Singular,
    Horizon,
    InvalidRectangle,
    InvalidQuad,
    UnsupportedBounds,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NonFinite => "Perspective coordinates must be finite.",
            Self::Singular => "Perspective placement is not stably invertible.",
            Self::Horizon => "Perspective projects this region through infinity. Reduce the distortion or source padding.",
            Self::InvalidRectangle => "Perspective source bounds must have finite, positive width and height.",
            Self::InvalidQuad => "Perspective corners must form a convex shape without crossing edges.",
            Self::UnsupportedBounds => "Perspective corners exceed the supported coordinate bounds.",
        })
    }
}

impl std::error::Error for Error {}

/// A finite, invertible row-major homography. Its horizon may cross some domains;
/// callers must validate each rectangle they intend to map with `map_rectangle`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Homography {
    matrix: [f64; 9],
}

impl Homography {
    pub const IDENTITY: Self = Self {
        matrix: [1., 0., 0., 0., 1., 0., 0., 0., 1.],
    };

    pub fn from_matrix(matrix: [f64; 9]) -> Result<Self, Error> {
        if matrix.iter().any(|value| !value.is_finite()) {
            return Err(Error::NonFinite);
        }
        let scale = matrix.iter().map(|value| value.abs()).fold(0., f64::max);
        if scale == 0. {
            return Err(Error::Singular);
        }
        let matrix = matrix.map(|value| value / scale);
        let [a, b, c, d, e, f, g, h, i] = matrix;
        let terms = [
            a * e * i,
            b * f * g,
            c * d * h,
            -c * e * g,
            -b * d * i,
            -a * f * h,
        ];
        let determinant = terms.iter().sum::<f64>();
        let magnitude = terms.iter().map(|value| value.abs()).sum::<f64>();
        if determinant.abs() <= TOLERANCE * magnitude || determinant == 0. {
            return Err(Error::Singular);
        }
        Ok(Self { matrix })
    }

    pub fn matrix(self) -> [f64; 9] {
        self.matrix
    }

    pub fn inverse(self) -> Result<Self, Error> {
        let [a, b, c, d, e, f, g, h, i] = self.matrix;
        // A homography is unchanged by a common scalar, so no determinant
        // division is necessary. Normalizing the adjugate avoids overflow.
        Self::from_matrix([
            e * i - f * h,
            c * h - b * i,
            b * f - c * e,
            f * g - d * i,
            a * i - c * g,
            c * d - a * f,
            d * h - e * g,
            b * g - a * h,
            a * e - b * d,
        ])
    }

    /// Returns `self(inner(point))`. It validates the matrix, not a source domain.
    pub fn compose(self, inner: Self) -> Result<Self, Error> {
        let mut matrix = [0.; 9];
        for row in 0..3 {
            for column in 0..3 {
                matrix[row * 3 + column] = (0..3)
                    .map(|k| self.matrix[row * 3 + k] * inner.matrix[k * 3 + column])
                    .sum();
            }
        }
        Self::from_matrix(matrix)
    }

    fn denominator(self, point: Point) -> Result<f64, Error> {
        if point.iter().any(|value| !value.is_finite()) {
            return Err(Error::NonFinite);
        }
        let terms = [
            self.matrix[6] * point[0],
            self.matrix[7] * point[1],
            self.matrix[8],
        ];
        let value = terms.iter().sum::<f64>();
        let magnitude = terms.iter().map(|value| value.abs()).sum::<f64>();
        if !value.is_finite() || !magnitude.is_finite() {
            return Err(Error::NonFinite);
        }
        if value.abs() <= TOLERANCE * magnitude || value == 0. {
            return Err(Error::Horizon);
        }
        Ok(value)
    }

    /// Homogeneous coordinates for sampling. A point on the horizon produces
    /// nonfinite coordinates, which sample as outside the source. Mutations and
    /// source expansion must use `apply` or `map_rectangle` instead.
    pub(crate) fn coordinates(self, point: Point) -> Point {
        let m = self.matrix;
        let w = m[6] * point[0] + m[7] * point[1] + m[8];
        [
            (m[0] * point[0] + m[1] * point[1] + m[2]) / w,
            (m[3] * point[0] + m[4] * point[1] + m[5]) / w,
        ]
    }

    /// Outside the validated source square this can fail at the horizon.
    pub fn apply(self, point: Point) -> Result<Point, Error> {
        let denominator = self.denominator(point)?;
        let m = self.matrix;
        let mapped = [
            (m[0] * point[0] + m[1] * point[1] + m[2]) / denominator,
            (m[3] * point[0] + m[4] * point[1] + m[5]) / denominator,
        ];
        if mapped.iter().all(|value| value.is_finite()) {
            Ok(mapped)
        } else {
            Err(Error::NonFinite)
        }
    }

    /// Jacobian columns at a source point, for local brush pixel footprints.
    pub fn derivative(self, point: Point) -> Result<[Point; 2], Error> {
        let mapped = self.apply(point)?;
        let w = self.denominator(point)?;
        let m = self.matrix;
        let columns = [
            [(m[0] - mapped[0] * m[6]) / w, (m[3] - mapped[1] * m[6]) / w],
            [(m[1] - mapped[0] * m[7]) / w, (m[4] - mapped[1] * m[7]) / w],
        ];
        if columns.iter().flatten().any(|v| !v.is_finite()) {
            return Err(Error::NonFinite);
        }
        Ok(columns)
    }

    /// Conservative column-length bounds over a rectangle on one side of the
    /// horizon. Numerators are linear and the denominator has a positive minimum.
    pub fn derivative_bounds(self, bounds: [f64; 4]) -> Result<Point, Error> {
        self.map_rectangle(bounds)?;
        let [left, top, right, bottom] = bounds;
        let corners = [[left, top], [right, top], [right, bottom], [left, bottom]];
        let minimum = corners
            .into_iter()
            .map(|p| self.denominator(p).map(f64::abs))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .fold(f64::INFINITY, f64::min);
        let [a, b, c, d, e, f, g, h, i] = self.matrix;
        let bound = |slope: f64, offset: f64, lo: f64, hi: f64| {
            (slope * lo + offset).abs().max((slope * hi + offset).abs()) / minimum / minimum
        };
        let x = [
            bound(a * h - b * g, a * i - c * g, top, bottom),
            bound(d * h - e * g, d * i - f * g, top, bottom),
        ];
        let y = [
            bound(b * g - a * h, b * i - c * h, left, right),
            bound(e * g - d * h, e * i - f * h, left, right),
        ];
        let result = [x[0].hypot(x[1]), y[0].hypot(y[1])];
        if result.iter().any(|v| !v.is_finite()) {
            return Err(Error::NonFinite);
        }
        Ok(result)
    }

    /// Maps an axis-aligned rectangle, rejecting a horizon anywhere inside it.
    /// The four returned corners bound the entire mapped rectangle because the
    /// homogeneous denominator has one strict sign throughout that rectangle.
    pub fn map_rectangle(self, bounds: [f64; 4]) -> Result<[Point; 4], Error> {
        let [left, top, right, bottom] = bounds;
        if bounds.iter().any(|value| !value.is_finite()) || left >= right || top >= bottom {
            return Err(Error::InvalidRectangle);
        }
        let corners = [[left, top], [right, top], [right, bottom], [left, bottom]];
        let sign = self.denominator(corners[0])?.is_sign_positive();
        let mut mapped = [[0.; 2]; 4];
        for (index, point) in corners.into_iter().enumerate() {
            if self.denominator(point)?.is_sign_positive() != sign {
                return Err(Error::Horizon);
            }
            mapped[index] = self.apply(point)?;
        }
        Ok(mapped)
    }
}

/// A homography valid over the complete normalized source square. Serialized
/// matrices pass the same finite, convex, horizon and coordinate-limit checks.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[f64; 9]", into = "[f64; 9]")]
pub struct Projective {
    mapping: Homography,
    inverse: Homography,
}

impl Projective {
    pub const IDENTITY: Self = Self {
        mapping: Homography::IDENTITY,
        inverse: Homography::IDENTITY,
    };

    /// Corners are source top-left, top-right, bottom-right, bottom-left.
    pub fn new(corners: [Point; 4]) -> Result<Self, Error> {
        validate_quad(corners)?;
        let affine = (0..2)
            .all(|axis| corners[1][axis] - corners[0][axis] == corners[2][axis] - corners[3][axis]);
        // Solve in a translated, scaled frame so large translations do not
        // hide a small but meaningful perspective term through cancellation.
        let origin = corners[0];
        let scale = corners
            .iter()
            .flat_map(|point| [(point[0] - origin[0]).abs(), (point[1] - origin[1]).abs()])
            .fold(0., f64::max);
        let c = corners.map(|point| {
            [
                (point[0] - origin[0]) / scale,
                (point[1] - origin[1]) / scale,
            ]
        });
        let dx1 = c[1][0] - c[2][0];
        let dx2 = c[3][0] - c[2][0];
        let dy1 = c[1][1] - c[2][1];
        let dy2 = c[3][1] - c[2][1];
        let sx = c[0][0] - c[1][0] + c[2][0] - c[3][0];
        let sy = c[0][1] - c[1][1] + c[2][1] - c[3][1];
        let denominator = dx1 * dy2 - dx2 * dy1;
        let (g, h) = if affine {
            (0., 0.)
        } else {
            (
                (sx * dy2 - dx2 * sy) / denominator,
                (dx1 * sy - sx * dy1) / denominator,
            )
        };
        let local = Homography::from_matrix([
            c[1][0] + g * c[1][0],
            c[3][0] + h * c[3][0],
            0.,
            c[1][1] + g * c[1][1],
            c[3][1] + h * c[3][1],
            0.,
            g,
            h,
            1.,
        ])?;
        let frame =
            Homography::from_matrix([scale, 0., origin[0], 0., scale, origin[1], 0., 0., 1.])?;
        Self::from_mapping(frame.compose(local)?)
    }

    pub fn from_mapping(mapping: Homography) -> Result<Self, Error> {
        validate_quad(mapping.map_rectangle([0., 0., 1., 1.])?)?;
        // Require a stable inverse as well as a stable forward map.
        let inverse = mapping.inverse()?;
        Ok(Self { mapping, inverse })
    }

    pub fn mapping(self) -> Homography {
        self.mapping
    }

    pub fn apply(self, point: Point) -> Result<Point, Error> {
        self.mapping.apply(point)
    }

    pub fn inverse(self) -> Result<Homography, Error> {
        Ok(self.inverse)
    }

    pub fn inverse_mapping(self) -> Homography {
        self.inverse
    }

    /// Replaces the source square with a rectangle in the old unit coordinates.
    /// For padding, `origin` is negative and `size` exceeds one. Original source
    /// points retain their placement. Horizon-crossing expansions are rejected.
    pub fn rebind(self, origin: Point, size: Point) -> Result<Self, Error> {
        let bounds = [
            origin[0],
            origin[1],
            origin[0] + size[0],
            origin[1] + size[1],
        ];
        self.mapping.map_rectangle(bounds)?;
        let domain =
            Homography::from_matrix([size[0], 0., origin[0], 0., size[1], origin[1], 0., 0., 1.])?;
        Self::from_mapping(self.mapping.compose(domain)?)
    }
}

impl TryFrom<[f64; 9]> for Projective {
    type Error = Error;

    fn try_from(matrix: [f64; 9]) -> Result<Self, Self::Error> {
        Self::from_mapping(Homography::from_matrix(matrix)?)
    }
}

impl From<Projective> for [f64; 9] {
    fn from(value: Projective) -> Self {
        value.mapping.matrix()
    }
}

fn validate_quad(corners: [Point; 4]) -> Result<(), Error> {
    if corners.iter().flatten().any(|value| !value.is_finite()) {
        return Err(Error::NonFinite);
    }
    if corners
        .iter()
        .flatten()
        .any(|value| value.abs() > MAX_COORDINATE)
    {
        return Err(Error::UnsupportedBounds);
    }
    let mut winding = None;
    for index in 0..4 {
        let a = corners[index];
        let b = corners[(index + 1) % 4];
        let c = corners[(index + 2) % 4];
        let first = [b[0] - a[0], b[1] - a[1]];
        let second = [c[0] - b[0], c[1] - b[1]];
        let cross = first[0] * second[1] - first[1] * second[0];
        let magnitude = first[0].hypot(first[1]) * second[0].hypot(second[1]);
        if cross.abs() <= TOLERANCE * magnitude || cross == 0. {
            return Err(Error::InvalidQuad);
        }
        let sign = cross.is_sign_positive();
        if winding.is_some_and(|previous| previous != sign) {
            return Err(Error::InvalidQuad);
        }
        winding = Some(sign);
    }
    Ok(())
}

#[cfg(test)]
#[path = "projective/tests.rs"]
mod tests;
