//! Editable layer recoloring with validated multi-stop gradients.
use crate::{
    Result,
    gradient::stops::{Stop, Stops},
    invalid,
};
use serde::{Deserialize, Serialize};
mod opacity;
mod rendering;
pub use opacity::{OpacityStop, OpacityStops};
pub(crate) use rendering::Prepared;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Style {
    #[default]
    Linear,
    Radial,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Overlay {
    pub enabled: bool,
    #[serde(with = "stop_serde")]
    pub stops: Stops,
    pub opacity_stops: OpacityStops,
    pub style: Style,
    /// Degrees counterclockwise: 0 runs left to right, 90 bottom to top.
    pub angle: f64,
    pub opacity: f64,
    pub reverse: bool,
}
impl Default for Overlay {
    fn default() -> Self {
        Self {
            enabled: true,
            stops: Stops::endpoints([0, 0, 0, 255], [255; 4]),
            opacity_stops: OpacityStops::default(),
            style: Style::Linear,
            angle: 90.,
            opacity: 1.,
            reverse: false,
        }
    }
}
impl Overlay {
    pub(crate) fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.stops.retained_bytes()
            + self.opacity_stops.retained_bytes()
    }
    pub fn sample(&self, position: f64) -> [f64; 4] {
        let color = self.stops.sample(position);
        [
            color[0],
            color[1],
            color[2],
            self.opacity_stops.sample(position),
        ]
    }
    pub fn validate(&self) -> Result<()> {
        if self
            .stops
            .as_slice()
            .iter()
            .any(|stop| stop.color[3] != 255)
        {
            return Err(invalid(
                "Gradient color stops must be opaque. Use opacity stops to change transparency.",
            ));
        }
        if !self.angle.is_finite()
            || !(-360. ..=360.).contains(&self.angle)
            || !self.opacity.is_finite()
            || !(0. ..=1.).contains(&self.opacity)
        {
            return Err(invalid(
                "Gradient Overlay requires an angle between -360 and 360 degrees and opacity between 0% and 100%. The layer is unchanged.",
            ));
        }
        Ok(())
    }
}

/// Precompute the angle once per surface. Positions are measured from the
/// source image's top-left corner, independent of effect padding and masks.
pub(crate) struct Geometry {
    pub(crate) direction: [i32; 2],
    pub(crate) size: [u32; 2],
    pub(crate) extent: i32,
    pub(crate) minimum: i32,
    style: Style,
    reverse: bool,
}
impl Geometry {
    pub(crate) fn padded(overlay: &Overlay, size: [u32; 2], inset: u32) -> Result<Self> {
        let source = size.map(|axis| axis.checked_sub(inset * 2).filter(|axis| *axis > 0));
        let [Some(width), Some(height)] = source else {
            return Err(invalid(
                "The gradient effect surface is missing its source image bounds.",
            ));
        };
        Self::new(overlay, [width, height])
    }
    pub(crate) fn new(overlay: &Overlay, size: [u32; 2]) -> Result<Self> {
        overlay.validate()?;
        if size.iter().any(|axis| *axis == 0 || *axis > 30_000) {
            return Err(invalid(
                "Gradient Overlay requires source dimensions between 1 and 30000 pixels.",
            ));
        }
        // Half-pixel coordinates and integer projections make stop-side tests
        // deterministic on CPUs and Vulkan devices. Adapt precision to size:
        // 2*(width+height)*scale never exceeds i32::MAX, including both axes.
        let scale = i32::MAX / (2 * (size[0] + size[1]) as i32);
        let radians = -overlay.angle.to_radians();
        let direction =
            [radians.cos(), radians.sin()].map(|axis| (axis * f64::from(scale)).round() as i32);
        let widths = [size[0] as i32 * 2, size[1] as i32 * 2];
        let extent = widths[0] * direction[0].abs() + widths[1] * direction[1].abs();
        let minimum = (widths[0] * direction[0]).min(0) + (widths[1] * direction[1]).min(0);
        Ok(Self {
            direction,
            size,
            extent,
            minimum,
            style: overlay.style,
            reverse: overlay.reverse,
        })
    }
    pub(crate) fn key(&self, point: [f64; 2]) -> i32 {
        let point = std::array::from_fn::<_, 2, _>(|i| {
            (point[i] * 2.)
                .round()
                .clamp(0., f64::from(self.size[i]) * 2.) as i32
        });
        match self.style {
            Style::Linear => {
                let key =
                    point[0] * self.direction[0] + point[1] * self.direction[1] - self.minimum;
                if self.reverse { self.extent - key } else { key }
            }
            Style::Radial => {
                let centered = [
                    point[0] - self.size[0] as i32,
                    point[1] - self.size[1] as i32,
                ];
                let radius = self.size[0].min(self.size[1]) as i32;
                let key =
                    (centered[0] * centered[0] + centered[1] * centered[1]).min(radius * radius);
                if self.reverse { -key } else { key }
            }
        }
    }
    fn value(&self, key: i32) -> f64 {
        match self.style {
            Style::Linear => f64::from(key) / f64::from(self.extent),
            Style::Radial => {
                let value = f64::from(key.abs()).sqrt() / f64::from(self.size[0].min(self.size[1]));
                if self.reverse { 1. - value } else { value }
            }
        }
        .clamp(0., 1.)
    }
    pub(crate) fn position(&self, point: [f64; 2]) -> f32 {
        self.value(self.key(point)) as f32
    }
    /// First discrete projection whose mathematical position reaches this stop.
    /// Binary search avoids a blanket epsilon at user-selected hard boundaries.
    pub(crate) fn stop_key(&self, position: f64) -> i32 {
        let radius = self.size[0].min(self.size[1]) as i32;
        let (mut low, mut high) = match (self.style, self.reverse) {
            (Style::Linear, _) => (0, self.extent),
            (Style::Radial, false) => (0, radius * radius),
            (Style::Radial, true) => (-radius * radius, 0),
        };
        while low < high {
            let middle = low + (high - low) / 2;
            if self.value(middle) < position {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        low
    }
}

mod stop_serde {
    use super::*;
    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Saved {
        position: f64,
        color: [u8; 4],
    }
    pub(super) fn serialize<S: serde::Serializer>(
        stops: &Stops,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        let values: Vec<_> = stops
            .as_slice()
            .iter()
            .map(|s| Saved {
                position: s.position,
                color: s.color,
            })
            .collect();
        values.serialize(serializer)
    }
    pub(super) fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Stops, D::Error> {
        let values = Vec::<Saved>::deserialize(deserializer)?;
        Stops::new(
            values
                .into_iter()
                .map(|s| Stop {
                    position: s.position,
                    color: s.color,
                })
                .collect(),
        )
        .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests;
