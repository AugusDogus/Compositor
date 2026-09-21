//! Tensor layout and mask decoding for the pinned BiRefNet Dynamic exports.
use crate::Result;
use image::{GrayImage, RgbaImage, buffer::ConvertBuffer, imageops::FilterType};

pub(super) const SIDE: u32 = 1024;

pub(super) fn normalize(image: &RgbaImage) -> Vec<f32> {
    // Follow the export's RGB, bilinear, ImageNet preprocessing. Ignore source alpha
    // so transparent pixels do not change RGB through premultiplied-alpha resizing.
    let rgb: image::RgbImage = image.convert();
    let resized = image::imageops::resize(&rgb, SIDE, SIDE, FilterType::Triangle);
    normalize_rgb(&resized)
}

fn normalize_rgb(image: &image::RgbImage) -> Vec<f32> {
    let plane = (image.width() * image.height()) as usize;
    let mut result = vec![0.; plane * 3];
    for (index, pixel) in image.pixels().enumerate() {
        for channel in 0..3 {
            result[channel * plane + index] = (f32::from(pixel[channel]) / 255.
                - [0.485, 0.456, 0.406][channel])
                / [0.229, 0.224, 0.225][channel];
        }
    }
    result
}

pub(super) fn decode_mask(probabilities: &[f32], side: u32, size: (u32, u32)) -> Result<GrayImage> {
    if probabilities.len() != (side * side) as usize
        || probabilities
            .iter()
            .any(|value| !value.is_finite() || !(0. ..=1.).contains(value))
    {
        return Err(super::failed(
            "decode the subject mask",
            "model output contains invalid probabilities or dimensions",
        ));
    }
    let bytes = probabilities.iter().map(|v| (v * 255.) as u8).collect();
    let mask = GrayImage::from_raw(side, side, bytes)
        .ok_or_else(|| super::failed("decode the subject mask", "invalid mask dimensions"))?;
    Ok(image::imageops::resize(
        &mask,
        size.0,
        size.1,
        FilterType::Lanczos3,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_uses_rgb_planes_and_imagenet_normalization() {
        let image = image::RgbImage::from_raw(2, 1, vec![255, 128, 64, 0, 255, 128]).unwrap();
        let actual = normalize_rgb(&image);
        let expected = [
            (1. - 0.485) / 0.229,
            -0.485 / 0.229,
            (128. / 255. - 0.456) / 0.224,
            (1. - 0.456) / 0.224,
            (64. / 255. - 0.406) / 0.225,
            (128. / 255. - 0.406) / 0.225,
        ];
        for (actual, expected) in actual.iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-6);
        }
        assert!(
            normalize_rgb(&image::RgbImage::new(1, 1))
                .iter()
                .all(|v| v.is_finite())
        );
    }

    #[test]
    fn mask_preserves_probabilities_and_source_dimensions() {
        let mask = decode_mask(&[0., 0.5, 0.5, 1.], 2, (2, 2)).unwrap();
        assert_eq!(mask.as_raw(), &[0, 127, 127, 255]);
        assert_eq!(
            decode_mask(&[0., 0.5, 0.5, 1.], 2, (9, 7))
                .unwrap()
                .dimensions(),
            (9, 7)
        );
        // Uniform alpha is valid. Do not divide by zero or stretch a weak prediction to opaque.
        assert_eq!(
            decode_mask(&[1.; 4], 2, (2, 2)).unwrap().as_raw(),
            &[255; 4]
        );
        assert_eq!(decode_mask(&[0.; 4], 2, (2, 2)).unwrap().as_raw(), &[0; 4]);
        assert_eq!(
            decode_mask(&[0.25; 4], 2, (2, 2)).unwrap().as_raw(),
            &[63; 4]
        );
    }

    #[test]
    fn malformed_predictions_are_errors() {
        for values in [
            vec![-1.; 4],
            vec![2.; 4],
            vec![f32::NAN; 4],
            vec![f32::INFINITY; 4],
            vec![0.; 3],
        ] {
            assert!(decode_mask(&values, 2, (2, 2)).is_err());
        }
    }
}
