use super::*;
use image::Rgba;

fn run(image: &RgbaImage, shadows: f64, highlights: f64, radius: f64) -> RgbaImage {
    apply(
        image,
        Settings::new(shadows, highlights, radius).unwrap(),
        1.,
        false,
    )
    .unwrap()
}

#[test]
fn settings_validate_each_boundary_and_nonfinite_values() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.001, 100.001] {
        assert!(Settings::new(value, 0., 1.).is_err());
        assert!(Settings::new(0., value, 1.).is_err());
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.999, 500.001] {
        assert!(Settings::new(0., 0., value).is_err());
    }
    for settings in [Settings::new(0., 0., 1.), Settings::new(100., 100., 500.)] {
        assert!(settings.is_ok());
    }
    assert_eq!(Settings::default(), Settings::new(35., 0., 30.).unwrap());
}

#[test]
fn saved_settings_validate_and_retain_fractional_values() {
    let settings = Settings::new(35.123, 42.456, 17.891).unwrap();
    let json = serde_json::to_string(&settings).unwrap();
    assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), settings);
    for json in [
        r#"{"shadows":101,"highlights":0,"radius":1}"#,
        r#"{"shadows":0,"highlights":-1,"radius":1}"#,
        r#"{"shadows":0,"highlights":0,"radius":0}"#,
        r#"{"shadows":0,"highlights":0,"radius":1,"extra":true}"#,
    ] {
        assert!(serde_json::from_str::<Settings>(json).is_err());
    }
}

#[test]
fn identity_preserves_every_byte_including_hidden_rgb() {
    let image = RgbaImage::from_fn(17, 13, |x, y| {
        Rgba([
            (x * 11) as u8,
            (y * 19) as u8,
            (x * y) as u8,
            (x * 17) as u8,
        ])
    });
    assert_eq!(run(&image, 0., 0., 500.), image);
}

#[test]
fn dark_regions_lift_and_bright_regions_drop() {
    let dark = RgbaImage::from_pixel(1, 1, Rgba([32, 32, 32, 255]));
    let bright = RgbaImage::from_pixel(1, 1, Rgba([220, 220, 220, 255]));
    assert!(run(&dark, 75., 0., 1.)[(0, 0)][0] > 32);
    assert!(run(&bright, 0., 75., 1.)[(0, 0)][0] < 220);
    // At a constant gray value, the independently evaluated neighborhood is exact.
    for (image, shadows, highlights) in [(dark, 75., 0.), (bright, 0., 75.)] {
        let value = f64::from(image[(0, 0)][0]) / 255.;
        let exponent = (1. + highlights / 100. * value.powi(2) * 2.)
            / (1. + shadows / 100. * (1. - value).powi(2) * 2.);
        let expected = (value.powf(exponent) * 255.).round() as u8;
        assert_eq!(run(&image, shadows, highlights, 1.)[(0, 0)][0], expected);
    }
}

#[test]
fn radius_changes_the_response_to_surrounding_pixels() {
    let image = RgbaImage::from_fn(31, 1, |x, _| {
        let value = if (13..=17).contains(&x) { 32 } else { 240 };
        Rgba([value, value, value, 255])
    });
    let narrow = run(&image, 100., 0., 1.);
    let broad = run(&image, 100., 0., 20.);
    assert!(narrow[(15, 0)][0] > broad[(15, 0)][0] + 20);
}

#[test]
fn correction_preserves_alpha_and_hidden_rgb() {
    let image = RgbaImage::from_fn(16, 16, |x, y| {
        Rgba([33, (x * 15) as u8, (y * 13) as u8, (y * 16 + x) as u8])
    });
    let result = run(&image, 100., 100., 7.);
    for (before, after) in image.pixels().zip(result.pixels()) {
        assert_eq!(before[3], after[3]);
        if before[3] == 0 {
            assert_eq!(before, after);
        }
    }
}

#[test]
fn transparent_colors_do_not_change_visible_neighbors() {
    let source = |hidden| {
        RgbaImage::from_fn(13, 13, |x, y| {
            if (4..9).contains(&x) && (4..9).contains(&y) {
                Rgba([64, 91, 120, 192])
            } else {
                Rgba([hidden, hidden, hidden, 0])
            }
        })
    };
    let dark_hidden = run(&source(0), 67., 44., 10.);
    let light_hidden = run(&source(255), 67., 44., 10.);
    for (a, b) in dark_hidden.pixels().zip(light_hidden.pixels()) {
        if a[3] != 0 {
            assert_eq!(a, b);
        }
    }
}

#[test]
fn transparent_border_and_low_coverage_are_neutral() {
    let color = [43, 100, 173];
    let opaque = RgbaImage::from_pixel(1, 1, Rgba([color[0], color[1], color[2], 255]));
    let expected = run(&opaque, 73., 37., 30.)[(0, 0)];
    for alpha in [1, 23, 128, 255] {
        let mut padded = RgbaImage::from_pixel(25, 25, Rgba([255, 255, 255, 0]));
        padded[(12, 12)] = Rgba([color[0], color[1], color[2], alpha]);
        let result = run(&padded, 73., 37., 30.);
        assert_eq!(&result[(12, 12)].0[..3], &expected.0[..3]);
        assert_eq!(result[(12, 12)][3], alpha);
    }
}

#[test]
fn constant_colors_remain_spatially_constant_at_image_edges() {
    let image = RgbaImage::from_pixel(7, 5, Rgba([64, 101, 170, 127]));
    let result = run(&image, 40., 60., 18.);
    assert!(result.pixels().all(|p| p == &result[(0, 0)]));
}

#[test]
fn preview_radius_scales_below_one_without_clamping() {
    let image = RgbaImage::from_fn(3, 1, |x, _| {
        Rgba(if x == 1 {
            [32, 32, 32, 255]
        } else {
            [240, 240, 240, 255]
        })
    });
    let settings = Settings::new(100., 0., 1.).unwrap();
    let tiny = apply(&image, settings, 0.01, false).unwrap();
    let full = apply(&image, settings, 1., false).unwrap();
    assert!(tiny[(1, 0)][0] > full[(1, 0)][0]);
    for scale in [f64::MIN_POSITIVE, 1e-30, 0.0001] {
        assert_eq!(apply(&image, settings, scale, false).unwrap(), tiny);
    }
}

#[test]
fn invalid_scale_and_empty_images_are_rejected() {
    let image = RgbaImage::from_pixel(1, 1, Rgba([128; 4]));
    for scale in [0., -1., 1.01, f64::NAN, f64::INFINITY] {
        assert!(apply(&image, Settings::default(), scale, false).is_err());
    }
    assert!(apply(&RgbaImage::new(0, 0), Settings::default(), 1., false).is_err());
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn gpu_neighborhood_blur_matches_cpu_correction() {
    let probe = FloatPlane::from_pixel(1, 1, Luma([0.5]));
    assert!(crate::render::gpu_float_blur(&probe, 1.).unwrap().is_some());
    let image = RgbaImage::from_fn(29, 19, |x, y| {
        Rgba([
            (x * 17) as u8,
            (y * 23) as u8,
            (x * y * 7) as u8,
            ((x + y) * 13) as u8,
        ])
    });
    for (shadows, highlights, radius, scale) in [
        (100., 0., 1., 1.),
        (0., 100., 30., 1.),
        (74., 37., 500., 0.05),
        (74., 37., 1., 0.125),
    ] {
        let settings = Settings::new(shadows, highlights, radius).unwrap();
        let cpu = apply(&image, settings, scale, false).unwrap();
        let gpu = apply(&image, settings, scale, true).unwrap();
        for (a, b) in cpu.pixels().zip(gpu.pixels()) {
            assert_eq!(a[3], b[3]);
            for channel in 0..3 {
                assert!(a[channel].abs_diff(b[channel]) <= 1, "{a:?} != {b:?}");
            }
        }
    }
}
