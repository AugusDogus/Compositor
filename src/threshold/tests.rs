use super::*;
use image::Rgba;

#[test]
fn exact_colored_integer_luminance_boundaries_remain_white() {
    for r in 0..=255_u32 {
        for g in 0..=255_u32 {
            for b in 0..=255_u32 {
                let numerator = r * 299 + g * 587 + b * 114;
                if numerator % 1000 != 0 {
                    continue;
                }
                let settings = Threshold {
                    level: (numerator / 1000) as u8,
                };
                let rgb = [r, g, b].map(|v| v as f32 / 255.);
                assert_eq!(settings.apply_rgb(rgb), [1.; 3], "{r},{g},{b}");
                assert_eq!(settings.pixel([r as u8, g as u8, b as u8, 255]), [255; 4]);
            }
        }
    }
    for (rgb, level) in [
        ([0., 6., 127.], 18),
        ([0., 122., 249.], 100),
        ([0., 208., 236.], 149),
    ] {
        let settings = Threshold { level };
        for offset in [-0.001, 0., 0.001] {
            let input = [rgb[0] / 255., (rgb[1] + offset) / 255., rgb[2] / 255.];
            assert_eq!(
                settings.apply_rgb(input),
                [if offset < 0. { 0. } else { 1. }; 3]
            );
        }
    }
}

#[test]
fn every_gray_and_cutoff_preserves_exact_boundary_in_pixels_and_continuous_rgb() {
    for level in 0..=255 {
        let settings = Threshold { level };
        for gray in 0..=255 {
            let expected = if gray >= level { 255 } else { 0 };
            assert_eq!(
                settings.pixel([gray, gray, gray, 91]),
                [expected, expected, expected, 91],
                "level={level} gray={gray}"
            );
            assert_eq!(
                settings.apply_rgb([f32::from(gray) / 255.; 3]),
                [f32::from(expected) / 255.; 3]
            );
        }
    }
}

#[test]
fn color_uses_encoded_luminance_instead_of_per_channel_or_maximum_cutoffs() {
    for (source, below, above) in [
        ([255, 0, 0, 255], 76, 77),
        ([0, 255, 0, 255], 149, 150),
        ([0, 0, 255, 255], 29, 30),
        ([255, 255, 0, 255], 225, 226),
        ([0, 255, 255, 255], 178, 179),
        ([255, 0, 255, 255], 105, 106),
    ] {
        assert_eq!(Threshold { level: below }.pixel(source), [255; 4]);
        assert_eq!(Threshold { level: above }.pixel(source), [0, 0, 0, 255]);
    }
    assert_eq!(Threshold::default().pixel([255, 0, 0, 255]), [0, 0, 0, 255]);
    assert_eq!(Threshold::default().pixel([0, 255, 0, 255]), [255; 4]);
}

#[test]
fn continuous_backdrop_is_not_quantized_before_comparing_to_cutoff() {
    let settings = Threshold::default();
    assert_eq!(settings.apply_rgb([127.75 / 255.; 3]), [0.; 3]);
    assert_eq!(settings.apply_rgb([128. / 255.; 3]), [1.; 3]);
    assert_eq!(settings.apply_rgb([128.25 / 255.; 3]), [1.; 3]);
    // .299*100 + .587*140 + .114*129 = 126.786, not 127.
    assert_eq!(
        Threshold { level: 127 }.pixel([100, 140, 129, 255]),
        [0, 0, 0, 255]
    );
}

#[test]
fn endpoints_handle_black_white_and_saturated_colors() {
    for source in [
        [0, 0, 0, 255],
        [255, 255, 255, 255],
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
    ] {
        assert_eq!(Threshold { level: 0 }.pixel(source), [255; 4]);
        let expected = if source == [255; 4] { 255 } else { 0 };
        assert_eq!(
            Threshold { level: 255 }.pixel(source),
            [expected, expected, expected, 255]
        );
    }
}

#[test]
fn alpha_and_invisible_colors_are_preserved_in_image_processing() {
    let source = RgbaImage::from_fn(3, 2, |x, y| {
        Rgba([
            if y == 0 { 80 } else { 180 },
            100,
            200,
            [0, 73, 255][x as usize],
        ])
    });
    let before = source.clone();
    let result = reference(&source, Threshold::default());
    assert_eq!(source, before);
    for (input, output) in source.pixels().zip(result.pixels()) {
        assert_eq!(input[3], output[3]);
        if input[3] == 0 {
            assert_eq!(input, output);
        } else {
            assert!(output[0] == 0 || output[0] == 255);
            assert_eq!(output[0], output[1]);
            assert_eq!(output[1], output[2]);
        }
    }
}

#[test]
fn serialization_rejects_invalid_cutoffs_without_normalizing_them() {
    for level in [0, 1, 128, 254, 255] {
        let settings = Threshold { level };
        let encoded = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<Threshold>(&encoded).unwrap(),
            settings
        );
    }
    for encoded in [
        r#"{"level":-1}"#,
        r#"{"level":256}"#,
        r#"{"level":127.5}"#,
        r#"{"level":null}"#,
        r#"{"level":"128"}"#,
        r#"{}"#,
        r#"{"level":128,"extra":true}"#,
    ] {
        assert!(
            serde_json::from_str::<Threshold>(encoded).is_err(),
            "{encoded}"
        );
    }
}
