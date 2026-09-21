use serde::{Deserialize, Serialize};

pub type Point = [f64; 2];

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Sampling {
    Nearest,
    Smooth,
    #[default]
    #[serde(rename = "High quality")]
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transform {
    pub origin: Point,
    pub size: Point,
    #[serde(default)]
    pub rotation: f64,
    #[serde(default)]
    pub flip_x: bool,
    #[serde(default)]
    pub flip_y: bool,
    #[serde(default)]
    pub sampling: Sampling,
}

impl Transform {
    pub const HANDLES: [Point; 8] = [
        [0., 0.],
        [0.5, 0.],
        [1., 0.],
        [1., 0.5],
        [1., 1.],
        [0.5, 1.],
        [0., 1.],
        [0., 0.5],
    ];

    pub fn mirrored(mut self, horizontal: bool, axis: f64) -> Self {
        let index = usize::from(!horizontal);
        self.origin[index] = 2. * axis - self.origin[index] - self.size[index];
        if horizontal {
            self.flip_x = !self.flip_x;
        } else {
            self.flip_y = !self.flip_y;
        }
        self.rotation = -self.rotation;
        self
    }

    pub fn geometry_point(&self, unit: Point) -> Point {
        let mut t = *self;
        t.flip_x = false;
        t.flip_y = false;
        t.point(unit)
    }

    pub fn following(&self, old: Self, new: Self) -> Self {
        if *self == old {
            return new;
        }
        let map = |p| new.point(old.unit(self.point(p)));
        let a = map([0., 0.]);
        let b = map([1., 0.]);
        let c = map([0., 1.]);
        let center = map([0.5, 0.5]);
        let sign = if self.flip_x { -1. } else { 1. };
        let angle = ((b[1] - a[1]) * sign).atan2((b[0] - a[0]) * sign);
        let along = -(c[0] - a[0]) * angle.sin() + (c[1] - a[1]) * angle.cos();
        let size = [
            (b[0] - a[0]).hypot(b[1] - a[1]).max(1.),
            along.abs().max(1.),
        ];
        let degrees = angle.to_degrees();
        Self {
            origin: [center[0] - size[0] / 2., center[1] - size[1] / 2.],
            size,
            rotation: degrees + ((self.rotation - degrees) / 360.).round() * 360.,
            flip_x: self.flip_x,
            flip_y: along < 0.,
            sampling: self.sampling,
        }
    }
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            origin: [0., 0.],
            size: [width as f64, height as f64],
            rotation: 0.,
            flip_x: false,
            flip_y: false,
            sampling: Sampling::High,
        }
    }

    pub fn valid(&self) -> bool {
        self.origin
            .iter()
            .all(|n| n.is_finite() && n.abs() <= 1_000_000.)
            && self
                .size
                .iter()
                .all(|n| n.is_finite() && (1. ..=300_000.).contains(n))
            && self.rotation.is_finite()
    }

    pub fn point(&self, mut unit: Point) -> Point {
        if self.flip_x {
            unit[0] = 1. - unit[0];
        }
        if self.flip_y {
            unit[1] = 1. - unit[1];
        }
        let (sin, cos) = self.rotation.to_radians().sin_cos();
        let x = (unit[0] - 0.5) * self.size[0];
        let y = (unit[1] - 0.5) * self.size[1];
        [
            self.origin[0] + self.size[0] / 2. + x * cos - y * sin,
            self.origin[1] + self.size[1] / 2. + x * sin + y * cos,
        ]
    }

    pub fn unit(&self, point: Point) -> Point {
        let (sin, cos) = self.rotation.to_radians().sin_cos();
        let x = point[0] - self.origin[0] - self.size[0] / 2.;
        let y = point[1] - self.origin[1] - self.size[1] / 2.;
        let mut p = [
            (x * cos + y * sin) / self.size[0] + 0.5,
            (-x * sin + y * cos) / self.size[1] + 0.5,
        ];
        if self.flip_x {
            p[0] = 1. - p[0];
        }
        if self.flip_y {
            p[1] = 1. - p[1];
        }
        p
    }

    pub fn bounds(&self) -> [f64; 4] {
        let corners = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]].map(|p| self.point(p));
        let min_x = corners.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
        let min_y = corners.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
        let max_x = corners
            .iter()
            .map(|p| p[0])
            .fold(f64::NEG_INFINITY, f64::max);
        let max_y = corners
            .iter()
            .map(|p| p[1])
            .fold(f64::NEG_INFINITY, f64::max);
        [min_x, min_y, max_x, max_y]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_accounts_for_rotation_and_flips() {
        let mut transform = Transform::new(400, 200);
        transform.origin = [-30., 75.];
        transform.rotation = 37.;
        transform.flip_x = true;
        for p in [[0., 0.], [0.25, 0.7], [1., 1.]] {
            let recovered = transform.unit(transform.point(p));
            assert!((recovered[0] - p[0]).abs() < 1e-10);
            assert!((recovered[1] - p[1]).abs() < 1e-10);
        }
    }
}
