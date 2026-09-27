//! Integer interval selection preserves hard-stop sides across CPU and GPU.
use super::{Geometry, Overlay};
use crate::Result;

struct Stop<const N: usize> {
    key: i32,
    position: f32,
    color: [f32; N],
}
fn sample<const N: usize>(stops: &[Stop<N>], key: i32, position: f32) -> [f32; N] {
    let mut left = &stops[0];
    if key < left.key {
        return left.color;
    }
    for right in &stops[1..] {
        if key < right.key {
            let distance = right.position - left.position;
            if distance <= 0. {
                return left.color;
            }
            let fraction = ((position - left.position) / distance).clamp(0., 1.);
            return std::array::from_fn(|axis| {
                left.color[axis] + (right.color[axis] - left.color[axis]) * fraction
            });
        }
        left = right;
    }
    left.color
}
pub(crate) struct Prepared {
    geometry: Geometry,
    colors: Vec<Stop<3>>,
    opacity: Vec<Stop<1>>,
}
impl Prepared {
    pub(crate) fn new(overlay: &Overlay, size: [u32; 2], inset: u32) -> Result<Self> {
        let geometry = Geometry::padded(overlay, size, inset)?;
        let colors = overlay
            .stops
            .as_slice()
            .iter()
            .map(|stop| Stop {
                key: geometry.stop_key(stop.position),
                position: stop.position as f32,
                color: [
                    f32::from(stop.color[0]) / 255.,
                    f32::from(stop.color[1]) / 255.,
                    f32::from(stop.color[2]) / 255.,
                ],
            })
            .collect();
        let opacity = overlay
            .opacity_stops
            .as_slice()
            .iter()
            .map(|stop| Stop {
                key: geometry.stop_key(stop.position),
                position: stop.position as f32,
                color: [stop.opacity as f32],
            })
            .collect();
        Ok(Self {
            geometry,
            colors,
            opacity,
        })
    }
    pub(crate) fn sample(&self, point: [f64; 2]) -> [f32; 4] {
        let key = self.geometry.key(point);
        let position = self.geometry.position(point);
        let color = sample(&self.colors, key, position);
        let [alpha] = sample(&self.opacity, key, position);
        [color[0], color[1], color[2], alpha]
    }
}
