//! Editable surface lighting derived from a layer's blurred alpha coverage.
use crate::{Result, invalid};
use serde::{Deserialize, Serialize};
pub(crate) mod surface;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Style {
    #[default]
    Inner,
    Outer,
    Emboss,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "Saved")]
pub struct Settings {
    pub enabled: bool,
    pub style: Style,
    /// Relief strength as a percentage, from 1 to 1000.
    pub depth: f64,
    /// Blur diameter in source pixels, from 0 to 250.
    pub size: f64,
    /// Light direction counterclockwise from right, in degrees.
    pub angle: f64,
    /// Light elevation above the image plane, from 0 to 90 degrees.
    pub altitude: f64,
    pub highlight_opacity: f64,
    pub shadow_opacity: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            style: Style::Inner,
            depth: 100.,
            size: 5.,
            angle: 120.,
            altitude: 30.,
            highlight_opacity: 0.75,
            shadow_opacity: 0.75,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<()> {
        for (name, value, minimum, maximum) in [
            ("depth", self.depth, 1., 1000.),
            ("size", self.size, 0., 250.),
            ("angle", self.angle, -360., 360.),
            ("altitude", self.altitude, 0., 90.),
            ("highlight opacity", self.highlight_opacity, 0., 1.),
            ("shadow opacity", self.shadow_opacity, 0., 1.),
        ] {
            if !value.is_finite() || !(minimum..=maximum).contains(&value) {
                return Err(invalid(format!(
                    "Bevel/Emboss {name} must be a finite number between {minimum} and {maximum}."
                )));
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Saved {
    enabled: bool,
    style: Style,
    depth: f64,
    size: f64,
    angle: f64,
    altitude: f64,
    highlight_opacity: f64,
    shadow_opacity: f64,
}

impl TryFrom<Saved> for Settings {
    type Error = crate::Error;

    fn try_from(saved: Saved) -> Result<Self> {
        let settings = Self {
            enabled: saved.enabled,
            style: saved.style,
            depth: saved.depth,
            size: saved.size,
            angle: saved.angle,
            altitude: saved.altitude,
            highlight_opacity: saved.highlight_opacity,
            shadow_opacity: saved.shadow_opacity,
        };
        settings.validate()?;
        Ok(settings)
    }
}

/// Precomputed per-surface coefficients, shared with the GPU parameter setup.
pub(crate) struct Lighting {
    pub(crate) light: [f32; 3],
    pub(crate) slope_scale: f32,
    opacity: [f32; 2],
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Shading {
    pub(crate) highlight: f32,
    pub(crate) shadow: f32,
}

impl Lighting {
    pub(crate) fn new(settings: &Settings) -> Result<Self> {
        settings.validate()?;
        let (sin_angle, cos_angle) = settings.angle.to_radians().sin_cos();
        // Exact endpoints avoid a small directional bias with overhead light.
        let (sin_altitude, cos_altitude) = match settings.altitude {
            0. => (0., 1.),
            90. => (1., 0.),
            altitude => altitude.to_radians().sin_cos(),
        };
        Ok(Self {
            light: [
                (cos_angle * cos_altitude) as f32,
                (-sin_angle * cos_altitude) as f32,
                sin_altitude as f32,
            ],
            slope_scale: (settings.size * settings.depth / 100. * 0.75) as f32,
            opacity: if settings.enabled {
                [
                    settings.highlight_opacity as f32,
                    settings.shadow_opacity as f32,
                ]
            } else {
                [0.; 2]
            },
        })
    }

    /// `difference` contains H(x+1)-H(x-1) and H(y+1)-H(y-1), where H is
    /// floating-point alpha blurred with sigma = size/2. The 1/2 factor for
    /// centered derivatives is included in `slope_scale`.
    pub(crate) fn sample(&self, difference: [f32; 2]) -> Shading {
        let nx = -difference[0] * self.slope_scale;
        let ny = -difference[1] * self.slope_scale;
        let length = (nx * nx + ny * ny + 1.).sqrt();
        let delta =
            (nx * self.light[0] + ny * self.light[1] + self.light[2]) / length - self.light[2];
        Shading {
            highlight: (2. * delta.max(0.)).min(1.) * self.opacity[0],
            shadow: (2. * (-delta).max(0.)).min(1.) * self.opacity[1],
        }
    }
}

#[cfg(test)]
#[path = "bevel/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "bevel/render_tests.rs"]
mod render_tests;
