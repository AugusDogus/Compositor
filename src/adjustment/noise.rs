//! Coordinate-stable Add Noise, matching upstream NoisePixels.c at document pixels.
use super::Adjustment;

fn hash(mut value: u32) -> u32 {
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846ca68b);
    value ^ (value >> 16)
}
fn unit(key: u32) -> f32 {
    (hash(key) >> 8) as f32 / 16_777_216.
}

pub(super) fn apply(settings: &Adjustment, rgb: [f64; 3], point: [f64; 2]) -> [f64; 3] {
    let [x, y] = point.map(|v| v.floor() as i64 as u32);
    let base = hash(
        settings.noise_seed.unwrap_or(0)
            ^ hash(x.wrapping_mul(0x9e3779b9) ^ hash(y.wrapping_mul(0x85ebca6b))),
    );
    let spread = settings.noise_amount.unwrap_or(10.) as f32 / 200.;
    std::array::from_fn(|channel| {
        let key = if settings.noise_monochromatic.unwrap_or(false) {
            base
        } else {
            base.wrapping_add((channel as u32).wrapping_mul(0x9e3779b9))
        };
        let value = if settings.noise_gaussian.unwrap_or(false) {
            (-2. * (1. - unit(key)).ln()).sqrt()
                * (std::f32::consts::TAU * unit(key ^ 0x68e31da4)).cos()
                * spread
                * (2. / 3.)
        } else {
            (unit(key) * 2. - 1.) * spread
        };
        (rgb[channel] + f64::from(value)).clamp(0., 1.)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{adjustment::Kind, native_pixels};
    use image::{Rgba, RgbaImage};
    #[test]
    fn noise_matches_native_upstream_formula_and_keeps_alpha() {
        let image = RgbaImage::from_pixel(7, 5, Rgba([127, 127, 127, 255]));
        for gaussian in [false, true] {
            for monochromatic in [false, true] {
                let mut settings = Adjustment::new(Kind::AddNoise);
                settings.noise_amount = Some(37.);
                settings.noise_seed = Some(123456789);
                settings.noise_gaussian = Some(gaussian);
                settings.noise_monochromatic = Some(monochromatic);
                let expected =
                    native_pixels::noise(&image, 37., gaussian, monochromatic, 123456789).unwrap();
                for (x, y, pixel) in expected.enumerate_pixels() {
                    let actual = settings.apply(
                        [127. / 255., 127. / 255., 127. / 255., 0.5],
                        [x as f64 + 0.5, y as f64 + 0.5],
                    );
                    assert_eq!(actual[3], 0.5);
                    for c in 0..3 {
                        assert!(((actual[c] * 255.).round() as u8).abs_diff(pixel[c]) <= 1);
                    }
                }
                assert_eq!(
                    settings.apply([0.5; 4], [-2.5, 3.5]),
                    settings.apply([0.5; 4], [-2.1, 3.9])
                );
                let decoded: Adjustment =
                    serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
                assert_eq!(settings, decoded);
            }
        }
    }
    #[test]
    fn noise_amount_checks_limits() {
        let mut settings = Adjustment::new(Kind::AddNoise);
        for value in [0., 400.1, f64::NAN, f64::INFINITY] {
            settings.noise_amount = Some(value);
            assert!(settings.validate().is_err());
        }
        for value in [0.1, 400.] {
            settings.noise_amount = Some(value);
            assert!(settings.validate().is_ok());
        }
    }
}
