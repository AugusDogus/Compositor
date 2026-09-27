//! Spin and zoom blur by repeated two-tap resampling. Composing powers of two
//! covers a dense angular/log-scale interval with O(log(distance)) passes.
use crate::{Result, document::validate_size, invalid};
use half::f16;
use image::RgbaImage;
use rayon::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Spin,
    Zoom,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Radial {
    pub mode: Mode,
    /// Spin angle in degrees, or zoom strength as a percentage, from 0 to 100.
    pub amount: f64,
    /// Center in normalized layer coordinates, from 0 to 1.
    pub center: [f64; 2],
}
impl Default for Radial {
    fn default() -> Self {
        Self {
            mode: Mode::Spin,
            amount: 10.,
            center: [0.5; 2],
        }
    }
}

pub(crate) type Pixel = [u16; 4];
#[derive(Clone, Copy)]
pub(crate) struct Pass {
    pub mapping: [f32; 4],
}

impl Radial {
    pub fn validate(self) -> Result<()> {
        if !(0. ..=100.).contains(&self.amount)
            || self.center.iter().any(|v| !(0. ..=1.).contains(v))
        {
            return Err(invalid(
                "Radial blur amount must be between 0 and 100, and its center must stay within the layer. The original pixels are preserved.",
            ));
        }
        Ok(())
    }

    pub(crate) fn passes(self, size: [u32; 2]) -> Vec<Pass> {
        let extent = (f64::from(size[0]) * self.center[0].max(1. - self.center[0]))
            .hypot(f64::from(size[1]) * self.center[1].max(1. - self.center[1]));
        let span = match self.mode {
            Mode::Spin => self.amount.to_radians(),
            Mode::Zoom => 2. * (self.amount / 200.).ln_1p(),
        };
        let count = (extent * span).max(2.).log2().ceil().clamp(1., 16.) as u32;
        (0..count)
            .map(|index| {
                let offset = span * f64::from(1u32 << index) / (2. * f64::from(1u32 << count));
                let mapping = match self.mode {
                    Mode::Spin => {
                        let (sin, cos) = offset.sin_cos();
                        [cos as f32, sin as f32, cos as f32, -sin as f32]
                    }
                    Mode::Zoom => [offset.exp() as f32, 0., (-offset).exp() as f32, 0.],
                };
                Pass { mapping }
            })
            .collect()
    }
}

pub(super) fn apply(image: &RgbaImage, settings: Radial) -> Result<RgbaImage> {
    settings.validate()?;
    validate_size(image.width(), image.height())?;
    if settings.amount == 0. {
        return Ok(image.clone());
    }
    let size = [image.width(), image.height()];
    let source = premultiply(image);
    let passes = settings.passes(size);
    let center = [
        settings.center[0] as f32 * size[0] as f32,
        settings.center[1] as f32 * size[1] as f32,
    ];
    if let Some(result) = crate::render::gpu::radial::blur(&source, size, center, &passes)? {
        return Ok(result);
    }
    Ok(reference(source, size, center, &passes))
}

pub(crate) fn premultiply(image: &RgbaImage) -> Vec<Pixel> {
    image
        .as_raw()
        .par_chunks_exact(4)
        .map(|pixel| {
            let alpha = f32::from(pixel[3]) / 255.;
            [
                f32::from(pixel[0]) / 255. * alpha,
                f32::from(pixel[1]) / 255. * alpha,
                f32::from(pixel[2]) / 255. * alpha,
                alpha,
            ]
            .map(|v| f16::from_f32(v).to_bits())
        })
        .collect()
}

fn sample(source: &[Pixel], size: [u32; 2], point: [f32; 2]) -> [f32; 4] {
    let point = [
        point[0].clamp(0.5, size[0] as f32 - 0.5) - 0.5,
        point[1].clamp(0.5, size[1] as f32 - 0.5) - 0.5,
    ];
    let base = [point[0].floor() as u32, point[1].floor() as u32];
    let fraction = [point[0] - base[0] as f32, point[1] - base[1] as f32];
    let mut result = [0.; 4];
    for y in 0..2 {
        for x in 0..2 {
            let at = [
                (base[0] + x).min(size[0] - 1),
                (base[1] + y).min(size[1] - 1),
            ];
            let weight = if x == 0 {
                1. - fraction[0]
            } else {
                fraction[0]
            } * if y == 0 {
                1. - fraction[1]
            } else {
                fraction[1]
            };
            let pixel = source[(at[1] * size[0] + at[0]) as usize];
            for channel in 0..4 {
                result[channel] += f16::from_bits(pixel[channel]).to_f32() * weight;
            }
        }
    }
    result
}

pub(crate) fn reference(
    mut source: Vec<Pixel>,
    size: [u32; 2],
    center: [f32; 2],
    passes: &[Pass],
) -> RgbaImage {
    let mut target = vec![[0; 4]; source.len()];
    for pass in passes {
        target
            .par_iter_mut()
            .enumerate()
            .for_each(|(index, pixel)| {
                let delta = [
                    (index % size[0] as usize) as f32 + 0.5 - center[0],
                    (index / size[0] as usize) as f32 + 0.5 - center[1],
                ];
                let [a, b, c, d] = pass.mapping;
                let first = sample(
                    &source,
                    size,
                    [
                        center[0] + delta[0] * a - delta[1] * b,
                        center[1] + delta[0] * b + delta[1] * a,
                    ],
                );
                let second = sample(
                    &source,
                    size,
                    [
                        center[0] + delta[0] * c - delta[1] * d,
                        center[1] + delta[0] * d + delta[1] * c,
                    ],
                );
                *pixel = std::array::from_fn(|channel| {
                    f16::from_f32((first[channel] + second[channel]) * 0.5).to_bits()
                });
            });
        std::mem::swap(&mut source, &mut target);
    }
    let mut output = RgbaImage::new(size[0], size[1]);
    let bytes: &mut [u8] = output.as_mut();
    bytes
        .par_chunks_exact_mut(4)
        .zip(source)
        .for_each(|(out, pixel)| {
            let pixel = pixel.map(|v| f16::from_bits(v).to_f32());
            for channel in 0..3 {
                out[channel] = if pixel[3] > 0. {
                    (pixel[channel] / pixel[3] * 255.).clamp(0., 255.).round() as u8
                } else {
                    0
                };
            }
            out[3] = (pixel[3] * 255.).clamp(0., 255.).round() as u8;
        });
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    #[test]
    fn radial_reference_preserves_flat_colors_and_spreads_alpha_without_hidden_rgb() {
        for mode in [Mode::Spin, Mode::Zoom] {
            let settings = Radial {
                mode,
                amount: 80.,
                center: [0.5; 2],
            };
            let passes = settings.passes([33, 33]);
            let flat = RgbaImage::from_pixel(33, 33, Rgba([71, 83, 111, 37]));
            assert_eq!(
                reference(premultiply(&flat), [33, 33], [16.5; 2], &passes),
                flat
            );
            let mut source = RgbaImage::from_pixel(33, 33, Rgba([0, 0, 255, 0]));
            source[(24, 16)] = Rgba([255, 0, 0, 128]);
            let result = reference(premultiply(&source), [33, 33], [16.5; 2], &passes);
            assert!(result.pixels().filter(|p| p[3] > 0).count() > 1);
            for pixel in result.pixels().filter(|p| p[3] > 0) {
                assert_eq!([pixel[0], pixel[1], pixel[2]], [255, 0, 0]);
                assert!(pixel[3] <= 128);
            }
            if mode == Mode::Spin {
                assert!(
                    result
                        .enumerate_pixels()
                        .any(|(_, y, p)| y != 16 && p[3] > 0)
                );
            } else {
                let visible: Vec<_> = result
                    .enumerate_pixels()
                    .filter(|(_, _, p)| p[3] > 0)
                    .collect();
                let width = visible.iter().map(|(x, _, _)| x).max().unwrap()
                    - visible.iter().map(|(x, _, _)| x).min().unwrap();
                let height = visible.iter().map(|(_, y, _)| y).max().unwrap()
                    - visible.iter().map(|(_, y, _)| y).min().unwrap();
                assert!(
                    width > height,
                    "Zoom should spread farther along the radius"
                );
            }
        }
    }

    #[test]
    fn radial_reference_preserves_low_alpha_high_rgb_flat_colors() {
        for mode in [Mode::Spin, Mode::Zoom] {
            let settings = Radial {
                mode,
                amount: 100.,
                center: [0.1, 0.8],
            };
            let passes = settings.passes([13, 11]);
            for alpha in [1, 2, 3, 7, 19, 37, 127, 254, 255] {
                for rgb in [
                    [1, 128, 254],
                    [253, 251, 249],
                    [17, 239, 254],
                    [0, 255, 255],
                ] {
                    let flat = RgbaImage::from_pixel(13, 11, Rgba([rgb[0], rgb[1], rgb[2], alpha]));
                    let result = reference(premultiply(&flat), [13, 11], [1.3, 8.8], &passes);
                    assert_eq!(result, flat, "{mode:?} rgb={rgb:?} alpha={alpha}");
                }
            }
        }
    }

    #[test]
    fn zero_and_invalid_radial_settings_preserve_document_metadata_and_pixels() {
        use crate::{
            document::Document,
            filters::{self, Filter},
        };
        let mut doc = Document::new(5, 5).unwrap();
        crate::edits::fill(&mut doc, [70, 90, 120, 255], false, false).unwrap();
        doc.layers[0].content = crate::document::LayerContent::Raster(Some(std::sync::Arc::new(
            RgbaImage::from_fn(5, 5, |x, _| {
                Rgba([70, 90, 120, if x == 0 { 0 } else { 255 }])
            }),
        )));
        doc.selection = Some(crate::selection::Selection::rectangle(
            5,
            5,
            [0., 0.],
            [4., 4.],
            false,
        ));
        doc.layers[0].shape = Some(crate::document::Shape {
            geometry: crate::document::ShapeGeometry::Rectangle,
            red: 70. / 255.,
            green: 90. / 255.,
            blue: 120. / 255.,
            corner_radius: 0.,
        });
        let original = doc.clone();
        filters::apply(
            &mut doc,
            Filter::Radial(Radial {
                amount: 0.,
                ..Default::default()
            }),
            false,
        )
        .unwrap();
        assert_eq!(doc, original);
        for settings in [
            Radial {
                amount: f64::NAN,
                ..Default::default()
            },
            Radial {
                amount: 101.,
                ..Default::default()
            },
            Radial {
                center: [f64::INFINITY, 0.5],
                ..Default::default()
            },
            Radial {
                center: [-0.1, 0.5],
                ..Default::default()
            },
        ] {
            assert!(filters::apply(&mut doc, Filter::Radial(settings), false).is_err());
            assert_eq!(doc, original);
        }
    }
}
