use quickgui::Color;

/// Chrome colors. Artwork, swatches, histograms and selection overlays keep their
/// source colors. The neutral scale retains the original dark UI exactly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::ui) struct Palette {
    pub background: [u8; 3],
    pub foreground: [u8; 3],
    pub accent: [u8; 3],
}
impl Default for Palette {
    fn default() -> Self {
        Self {
            background: [30; 3],
            foreground: [224; 3],
            accent: [0, 122, 255],
        }
    }
}
impl Palette {
    pub const LIGHT: Self = Self {
        background: [242; 3],
        foreground: [32; 3],
        accent: [0, 99, 204],
    };

    pub fn neutral(self, shade: u8) -> Color {
        let rgb = std::array::from_fn::<_, 3, _>(|i| {
            (self.background[i] as f32
                + (shade as f32 - 30.) / 194.
                    * (self.foreground[i] as f32 - self.background[i] as f32))
                .round()
                .clamp(0., 255.) as u8
        });
        Color::rgb8(rgb[0], rgb[1], rgb[2])
    }
    pub fn accent_style(
        self,
        style: quickgui::ElementStateStyle,
        original: [u8; 3],
    ) -> quickgui::ElementStateStyle {
        style
            .bg(self.accent_variant(original))
            .text_color(self.accent_text())
    }
    pub fn selection(self) -> Color {
        self.accent_variant([65, 107, 158])
    }
    pub fn accent_variant(self, original: [u8; 3]) -> Color {
        if self == Self::default() {
            Color::rgb8(original[0], original[1], original[2])
        } else {
            self.accent()
        }
    }
    pub fn tinted_neutral(self, original: [u8; 3]) -> Color {
        if self == Self::default() {
            Color::rgb8(original[0], original[1], original[2])
        } else {
            self.neutral(
                ((u16::from(original[0]) + u16::from(original[1]) + u16::from(original[2])) / 3)
                    as u8,
            )
        }
    }
    pub fn error(self, original: [u8; 3]) -> Color {
        if self.background.iter().map(|c| u16::from(*c)).sum::<u16>() > 384 {
            Color::rgb8(172, 26, 20)
        } else {
            Color::rgb8(original[0], original[1], original[2])
        }
    }
    pub fn warning(self) -> Color {
        if self.background.iter().map(|c| u16::from(*c)).sum::<u16>() > 384 {
            Color::rgb8(145, 76, 0)
        } else {
            Color::rgb8(255, 159, 10)
        }
    }
    pub fn accent(self) -> Color {
        Color::rgb8(self.accent[0], self.accent[1], self.accent[2])
    }
    pub fn accent_text(self) -> Color {
        if self == Self::default() {
            return Color::WHITE;
        }
        let linear = self.accent.map(|value| {
            let s = f32::from(value) / 255.;
            if s <= 0.04045 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        });
        let luminance = 0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2];
        if (luminance + 0.05) / 0.05 >= 1.05 / (luminance + 0.05) {
            Color::BLACK
        } else {
            Color::WHITE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contrast_with_accent(accent: [u8; 3], text: Color) -> f64 {
        let mut luminance = 0.;
        for (channel, weight) in accent.into_iter().zip([0.2126, 0.7152, 0.0722]) {
            let value = f64::from(channel) / 255.;
            luminance += weight
                * if value <= 0.04045 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                };
        }
        if text == Color::BLACK {
            (luminance + 0.05) / 0.05
        } else {
            assert_eq!(text, Color::WHITE);
            1.05 / (luminance + 0.05)
        }
    }

    #[test]
    fn saturated_green_uses_black_text_and_dark_default_remains_unchanged() {
        let colors = Palette {
            accent: [0, 180, 0],
            ..Palette::LIGHT
        };
        assert_eq!(colors.accent_text(), Color::BLACK);
        assert!(contrast_with_accent(colors.accent, colors.accent_text()) > 7.);
        assert_eq!(Palette::default().accent_text(), Color::WHITE);
    }

    #[test]
    fn custom_accent_foregrounds_meet_normal_text_contrast_across_rgb_cube() {
        for red in (0..=255).step_by(17) {
            for green in (0..=255).step_by(17) {
                for blue in (0..=255).step_by(17) {
                    let colors = Palette {
                        accent: [red, green, blue],
                        ..Palette::LIGHT
                    };
                    let contrast = contrast_with_accent(colors.accent, colors.accent_text());
                    assert!(contrast >= 4.5, "{:?}: contrast {contrast}", colors.accent);
                }
            }
        }
    }
}
