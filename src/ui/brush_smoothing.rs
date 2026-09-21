//! Screen-space trailing rope, applied before the existing brush stroke renderer.
use super::Tool;
use compositor::geometry::Point;

pub(super) struct Rope {
    anchor: Point,
    length: f64,
}

impl Rope {
    pub(super) fn start(tool: Tool, length: f64, anchor: Point) -> Option<Self> {
        (matches!(tool, Tool::Brush | Tool::Erase) && length.is_finite() && length > 0.).then_some(
            Self {
                anchor,
                length: length.min(100.),
            },
        )
    }

    pub(super) fn pull(&mut self, point: Point, zoom: f64) -> Option<Point> {
        let radius = self.length / zoom.max(0.01);
        let delta = [point[0] - self.anchor[0], point[1] - self.anchor[1]];
        let distance = delta[0].hypot(delta[1]);
        if distance <= radius {
            return None;
        }
        let step = (distance - radius) / distance;
        self.anchor = [
            self.anchor[0] + delta[0] * step,
            self.anchor[1] + delta[1] * step,
        ];
        Some(self.anchor)
    }
}

#[cfg(test)]
#[path = "brush_smoothing_tests.rs"]
mod tests;
