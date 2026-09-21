use crate::adjustment::{LevelRange, Levels};
use image::RgbaImage;

#[derive(Clone)]
pub struct Histogram(pub [[f64; 256]; 4]);

#[derive(Clone, Copy)]
pub enum AutoLevels {
    Contrast,
    Color,
    Neutral,
}

impl Histogram {
    pub fn for_layer(
        layer: &crate::document::Layer,
        selection: Option<&crate::selection::Selection>,
    ) -> crate::Result<Self> {
        let source = layer
            .raster()
            .ok_or_else(|| crate::invalid("Select a pixel layer to calculate its histogram."))?;
        let mut bins = [[0.; 256]; 4];
        for (x, y, pixel) in source.enumerate_pixels() {
            let point = layer.transform.point([
                (x as f64 + 0.5) / source.width() as f64,
                (y as f64 + 0.5) / source.height() as f64,
            ]);
            let weight = pixel[3] as f64 / 255. * selection.map_or(1., |s| s.coverage(point));
            for channel in 0..3 {
                let value = pixel[channel] as usize;
                bins[channel + 1][value] += weight;
                bins[0][value] += weight / 3.;
            }
        }
        Ok(Self(bins))
    }

    pub fn new(image: &RgbaImage) -> Self {
        let mut bins = [[0.; 256]; 4];
        for pixel in image.pixels() {
            let weight = pixel[3] as f64 / 255.;
            for channel in 0..3 {
                let value = pixel[channel] as usize;
                bins[channel + 1][value] += weight;
                bins[0][value] += weight / 3.;
            }
        }
        Self(bins)
    }

    pub fn automatic(&self, mode: AutoLevels) -> Levels {
        let mut settings = Levels::default();
        let endpoints = |bins: &[f64; 256]| {
            let total: f64 = bins.iter().sum();
            let mut sum = 0.;
            let low = bins.iter().position(|n| {
                sum += n;
                sum > total * 0.001
            });
            sum = 0.;
            let high = bins.iter().rposition(|n| {
                sum += n;
                sum > total * 0.001
            });
            low.zip(high).filter(|(a, b)| a < b)
        };
        match mode {
            AutoLevels::Contrast => {
                let limits: Vec<_> = self.0[1..].iter().filter_map(endpoints).collect();
                if let (Some(low), Some(high)) = (
                    limits.iter().map(|p| p.0).min(),
                    limits.iter().map(|p| p.1).max(),
                ) {
                    settings.ranges[0].black = low as f64;
                    settings.ranges[0].white = high as f64;
                }
            }
            AutoLevels::Color | AutoLevels::Neutral => {
                for (channel, bins) in self.0.iter().enumerate().skip(1) {
                    if let Some((low, high)) = endpoints(bins) {
                        let mut range = LevelRange {
                            black: low as f64,
                            white: high as f64,
                            ..LevelRange::default()
                        };
                        if matches!(mode, AutoLevels::Neutral) {
                            let total: f64 = bins.iter().sum();
                            let mean = bins
                                .iter()
                                .enumerate()
                                .map(|(i, n)| {
                                    ((i as f64 - range.black) / (range.white - range.black))
                                        .clamp(0., 1.)
                                        * n
                                })
                                .sum::<f64>()
                                / total;
                            if mean > 0. && mean < 1. {
                                range.gamma = (mean.ln() / 0.5_f64.ln()).clamp(0.1, 9.99);
                            }
                        }
                        settings.ranges[channel] = range;
                    }
                }
            }
        }
        settings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    #[test]
    fn histogram_weights_alpha_and_rgb_is_channel_mean() {
        let image = RgbaImage::from_fn(2, 1, |x, _| {
            if x == 0 {
                Rgba([30, 60, 90, 255])
            } else {
                Rgba([255, 255, 255, 0])
            }
        });
        let h = Histogram::new(&image);
        assert_eq!(h.0[0][30], 1. / 3.);
        assert_eq!(h.0[2][60], 1.);
        assert_eq!(h.0[0][255], 0.);
    }
    #[test]
    fn auto_contrast_shares_endpoints_and_color_stretches_each_channel() {
        let image = RgbaImage::from_fn(2, 1, |x, _| {
            if x == 0 {
                Rgba([10, 40, 70, 255])
            } else {
                Rgba([100, 150, 220, 255])
            }
        });
        let h = Histogram::new(&image);
        let contrast = h.automatic(AutoLevels::Contrast);
        assert_eq!(
            (contrast.ranges[0].black, contrast.ranges[0].white),
            (10., 220.)
        );
        let color = h.automatic(AutoLevels::Color);
        assert_eq!((color.ranges[2].black, color.ranges[2].white), (40., 150.));
        assert_eq!(color.ranges[0], LevelRange::default());
    }
}
