use std::sync::LazyLock;

static SRGB_U8_TO_LINEAR: LazyLock<[f32; 256]> =
    LazyLock::new(|| std::array::from_fn(|component| srgb_to_linear(component as f32 / 255.0)));

/// A premultiplication-neutral, linear-light RGBA color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const TRANSPARENT: Self = Self::linear(0.0, 0.0, 0.0, 0.0);
    pub const BLACK: Self = Self::linear(0.0, 0.0, 0.0, 1.0);
    pub const WHITE: Self = Self::linear(1.0, 1.0, 1.0, 1.0);

    /// Construct a color whose RGB components are already in linear-light space.
    pub const fn linear(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Construct from 8-bit sRGB components, converting to linear light.
    pub fn rgb8(r: u8, g: u8, b: u8) -> Self {
        Self::rgba8(r, g, b, 255)
    }

    /// Construct from 8-bit sRGB components and an 8-bit linear alpha.
    pub fn rgba8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: SRGB_U8_TO_LINEAR[r as usize],
            g: SRGB_U8_TO_LINEAR[g as usize],
            b: SRGB_U8_TO_LINEAR[b as usize],
            a: a as f32 / 255.0,
        }
    }

    pub fn with_alpha(self, alpha: f32) -> Self {
        Self {
            a: alpha.clamp(0.0, 1.0),
            ..self
        }
    }

    pub(crate) fn multiply_alpha(self, opacity: f32) -> Self {
        let opacity = if opacity.is_finite() {
            opacity.clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.with_alpha(self.a * opacity)
    }

    /// Interpolate colors in premultiplied-alpha space, then return straight RGBA.
    ///
    /// Paint transitions need this form so fading from transparent to an opaque color does not
    /// pass through transparent black and produce a dark fringe when composited.
    pub(crate) fn interpolate_premultiplied(from: Self, to: Self, phase: f32) -> Self {
        if phase == 0.0 {
            return from;
        }
        if phase == 1.0 {
            return to;
        }

        let alpha = from.a + (to.a - from.a) * phase;
        if alpha.abs() <= f32::EPSILON {
            return Self::linear(0.0, 0.0, 0.0, alpha);
        }
        let component = |from_component: f32, to_component: f32| {
            let from_premultiplied = from_component * from.a;
            let to_premultiplied = to_component * to.a;
            (from_premultiplied + (to_premultiplied - from_premultiplied) * phase) / alpha
        };
        Self::linear(
            component(from.r, to.r),
            component(from.g, to.g),
            component(from.b, to.b),
            alpha,
        )
    }

    pub(crate) fn as_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    pub(crate) fn premultiplied_srgba(self) -> [f32; 4] {
        [
            linear_to_srgb(self.r) * self.a,
            linear_to_srgb(self.g) * self.a,
            linear_to_srgb(self.b) * self.a,
            self.a,
        ]
    }

    pub(crate) fn to_srgba8(self) -> [u8; 4] {
        [
            (linear_to_srgb(self.r) * 255.0).round() as u8,
            (linear_to_srgb(self.g) * 255.0).round() as u8,
            (linear_to_srgb(self.b) * 255.0).round() as u8,
            (self.a.clamp(0.0, 1.0) * 255.0).round() as u8,
        ]
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::TRANSPARENT
    }
}

#[cfg(not(quickgui_terminal_extension))]
impl From<quickgui_system::SystemColor> for Color {
    fn from(color: quickgui_system::SystemColor) -> Self {
        Self::rgba8(color.red, color.green, color.blue, color.alpha)
    }
}

fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_round_trip_is_exact_to_one_byte() {
        for value in 0..=255 {
            let color = Color::rgba8(value, value, value, value);
            assert_eq!(color.to_srgba8(), [value, value, value, value]);
        }
    }
}
