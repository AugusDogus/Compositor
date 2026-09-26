//! Dither settings and the upstream portable pixel kernel. Glyphs use native Cosmic Text.
mod glyphs;
mod native;
use crate::{Result, document::validate_size, invalid, native_pixels};
use image::{Rgba, RgbaImage};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Style {
    Atkinson,
    FloydSteinberg,
    Bayer2,
    Bayer4,
    Bayer8,
    Dots,
    Lines,
    Diamonds,
    Patterns,
    Ascii,
}
impl Style {
    pub const ALL: [Self; 10] = [
        Self::Atkinson,
        Self::FloydSteinberg,
        Self::Bayer2,
        Self::Bayer4,
        Self::Bayer8,
        Self::Dots,
        Self::Lines,
        Self::Diamonds,
        Self::Patterns,
        Self::Ascii,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Atkinson => "Atkinson (Classic Mac)",
            Self::FloydSteinberg => "Floyd–Steinberg",
            Self::Bayer2 => "Bayer 2 × 2",
            Self::Bayer4 => "Bayer 4 × 4",
            Self::Bayer8 => "Bayer 8 × 8",
            Self::Dots => "Halftone Dots",
            Self::Lines => "Halftone Lines",
            Self::Diamonds => "Halftone Diamonds",
            Self::Patterns => "Mac Patterns",
            Self::Ascii => "ASCII",
        }
    }
    pub fn diffuses(self) -> bool {
        matches!(self, Self::Atkinson | Self::FloydSteinberg)
    }
    pub fn has_tones(self) -> bool {
        self.diffuses() || matches!(self, Self::Bayer2 | Self::Bayer4 | Self::Bayer8)
    }
    pub fn halftone(self) -> bool {
        matches!(self, Self::Dots | Self::Lines | Self::Diamonds)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Colors {
    BlackWhite,
    TwoColors,
    Original,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelShape {
    Square,
    Dot,
}

/// The upstream 64-character limit bounds the glyph atlas independently of image size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Characters([char; 64]);
impl Default for Characters {
    fn default() -> Self {
        Self::parse(" .:-=+*#%@").expect("Default characters fit")
    }
}
impl Characters {
    pub fn parse(value: &str) -> Result<Self> {
        let mut chars = ['\0'; 64];
        for (index, character) in value.chars().enumerate() {
            if index >= 64 || character == '\0' || character.is_control() {
                return Err(invalid(
                    "Use up to 64 printable dither characters on one line.",
                ));
            }
            chars[index] = character;
        }
        Ok(Self(chars))
    }
    pub fn text(self) -> String {
        self.0.into_iter().take_while(|c| *c != '\0').collect()
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub style: Style,
    pub pixel_size: u8,
    pub pixel_shape: PixelShape,
    pub cell_size: u8,
    pub text_size: u8,
    pub angle: f32,
    pub levels: u8,
    pub diffusion: f32,
    pub density: f32,
    pub contrast: f32,
    pub colors: Colors,
    pub dark: [u8; 3],
    pub light: [u8; 3],
    pub light_on_dark: bool,
    pub characters: Characters,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            style: Style::Atkinson,
            pixel_size: 2,
            pixel_shape: PixelShape::Square,
            cell_size: 8,
            text_size: 14,
            angle: 45.,
            levels: 2,
            diffusion: 100.,
            density: 0.,
            contrast: 0.,
            colors: Colors::BlackWhite,
            dark: [0; 3],
            light: [255; 3],
            light_on_dark: true,
            characters: Characters::default(),
        }
    }
}
impl Settings {
    pub fn validate(self) -> Result<()> {
        if !(1..=32).contains(&self.pixel_size)
            || !(4..=64).contains(&self.cell_size)
            || !(6..=64).contains(&self.text_size)
            || !(2..=8).contains(&self.levels)
            || !(-90. ..=90.).contains(&self.angle)
            || !(0. ..=100.).contains(&self.diffusion)
            || !(-100. ..=100.).contains(&self.density)
            || !(-100. ..=100.).contains(&self.contrast)
        {
            return Err(invalid(
                "Dither settings are out of range. Use 1–32 px pixels, 4–64 px cells, 6–64 px text, 2–8 tones, and values within the displayed slider ranges.",
            ));
        }
        Ok(())
    }
}

pub fn apply(image: &RgbaImage, settings: Settings) -> Result<RgbaImage> {
    settings.validate()?;
    validate_size(image.width(), image.height())?;
    let block = if settings.style == Style::Ascii {
        1
    } else {
        u32::from(settings.pixel_size)
    };
    let (w, h) = (
        image.width().div_ceil(block),
        image.height().div_ceil(block),
    );
    // The C kernel uses up to 29 bytes per working pixel, plus input/output buffers.
    // Reject before allocating so oversized jobs cannot exhaust the editor process.
    let working_bytes = u64::from(w) * u64::from(h) * 33;
    let output_bytes = u64::from(image.width()) * u64::from(image.height()) * 4;
    if working_bytes + output_bytes > 768 * 1024 * 1024 {
        return Err(invalid(
            "Dither needs more than 768 MiB of working memory. The layer is unchanged. Increase Pixel Size or resize the layer before filtering.",
        ));
    }
    // Area-average premultiplied pixels so transparent RGB does not leak into edges.
    let mut small = RgbaImage::from_fn(w, h, |x, y| {
        let mut sum = [0_u32; 4];
        let mut count = 0;
        for sy in y * block..((y + 1) * block).min(image.height()) {
            for sx in x * block..((x + 1) * block).min(image.width()) {
                let p = image[(sx, sy)];
                for i in 0..3 {
                    sum[i] += (u32::from(p[i]) * u32::from(p[3]) + 127) / 255;
                }
                sum[3] += u32::from(p[3]);
                count += 1;
            }
        }
        Rgba(sum.map(|v| ((v + count / 2) / count) as u8))
    });
    native::apply(&mut small, settings)?;
    if block == 1 {
        return Ok(native_pixels::unpremultiply(small));
    }
    let mut full = RgbaImage::from_fn(image.width(), image.height(), |x, y| {
        small[(x / block, y / block)]
    });
    if settings.pixel_shape == PixelShape::Dot {
        native::dots(&mut full, settings);
    }
    Ok(native_pixels::unpremultiply(full))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_style_preserves_size_and_alpha_and_changes_gray() {
        let source = RgbaImage::from_fn(64, 32, |x, _| {
            Rgba([128, 128, 128, if x < 32 { 255 } else { 0 }])
        });
        for style in Style::ALL {
            let result = apply(
                &source,
                Settings {
                    style,
                    pixel_size: 1,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(result.dimensions(), source.dimensions());
            assert!(
                result
                    .pixels()
                    .zip(source.pixels())
                    .all(|(a, b)| a[3] == b[3])
            );
            assert!(
                result.pixels().any(|p| p[3] > 0 && p[0] != 128),
                "{style:?}"
            );
        }
    }
    #[test]
    fn original_quantizes_channels_and_two_colors_uses_palette() {
        let source = RgbaImage::from_pixel(8, 8, Rgba([200, 60, 20, 255]));
        let original = apply(
            &source,
            Settings {
                colors: Colors::Original,
                pixel_size: 1,
                diffusion: 0.,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(original[(0, 0)].0, [255, 0, 0, 255]);
        let two = apply(
            &source,
            Settings {
                colors: Colors::TwoColors,
                dark: [10, 20, 30],
                light: [210, 220, 230],
                pixel_size: 1,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            two.pixels()
                .all(|p| p.0 == [10, 20, 30, 255] || p.0 == [210, 220, 230, 255])
        );
    }
    #[test]
    fn ascii_ignores_chunky_pixel_settings_and_uses_custom_characters() {
        let source = RgbaImage::from_pixel(64, 32, Rgba([200, 200, 200, 255]));
        let s = Settings {
            style: Style::Ascii,
            ..Default::default()
        };
        assert_eq!(
            apply(&source, s).unwrap(),
            apply(
                &source,
                Settings {
                    pixel_size: 32,
                    pixel_shape: PixelShape::Dot,
                    ..s
                }
            )
            .unwrap()
        );
        let blank = apply(
            &source,
            Settings {
                characters: Characters::parse(" ").unwrap(),
                ..s
            },
        )
        .unwrap();
        assert!(blank.pixels().all(|p| p.0 == [0, 0, 0, 255]));
        assert_ne!(blank, apply(&source, s).unwrap());
    }
    #[test]
    fn selection_restricts_changes_and_layer_metadata_survives() {
        let mut doc = crate::document::Document::new(8, 8).unwrap();
        crate::edits::fill(&mut doc, [120, 120, 120, 255], false, false).unwrap();
        doc.selection = Some(crate::selection::Selection::rectangle(
            8,
            8,
            [0., 0.],
            [4., 8.],
            false,
        ));
        let original = doc.layers[0].clone();
        super::super::apply_dither(
            &mut doc,
            Settings {
                pixel_size: 1,
                ..Default::default()
            },
        )
        .unwrap();
        let pixels = doc.layers[0].raster().unwrap();
        assert_ne!(pixels[(1, 1)], Rgba([120, 120, 120, 255]));
        assert_eq!(pixels[(6, 1)], Rgba([120, 120, 120, 255]));
        assert_eq!(doc.layers[0].transform, original.transform);
        assert_eq!(doc.layers[0].mask, original.mask);
    }
    #[test]
    fn invalid_settings_and_characters_fail() {
        let source = RgbaImage::new(1, 1);
        assert!(
            apply(
                &source,
                Settings {
                    density: f32::NAN,
                    ..Default::default()
                }
            )
            .is_err()
        );
        assert!(
            apply(
                &source,
                Settings {
                    pixel_size: 0,
                    ..Default::default()
                }
            )
            .is_err()
        );
        assert!(Characters::parse(&"x".repeat(65)).is_err());
    }
}
