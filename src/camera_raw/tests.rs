use super::*;
use image::Rgba;

#[test]
fn color_noise_reduction_initializes_transparent_neighbors() {
    let mut source = RgbaImage::new(3, 1);
    source[(1, 0)] = Rgba([255, 0, 0, 255]);
    let mut settings = Settings::default();
    settings.detail.noise_color = 100.;
    settings.detail.noise_color_detail = 0.;
    settings.detail.noise_color_smoothness = 0.;

    let adjusted = render(&source, &settings).unwrap();
    // Radius one averages saturation over the red pixel and two transparent
    // (zero-chroma) neighbors: HSL (0, 1/3, 1/2) becomes RGB (170, 85, 85).
    assert_eq!(adjusted[(1, 0)], Rgba([170, 85, 85, 255]));
    assert_eq!(adjusted[(0, 0)], Rgba([0, 0, 0, 0]));
    assert_eq!(adjusted[(2, 0)], Rgba([0, 0, 0, 0]));
}

#[test]
fn curve_saturation_refinement_uses_percentage_strength() {
    use crate::adjustment::CurvePoint;
    let source = RgbaImage::from_pixel(1, 1, Rgba([160, 100, 60, 255]));
    let mut settings = Settings::default();
    settings.curves.channels[0] = vec![
        CurvePoint { x: 0., y: 0. },
        CurvePoint { x: 255., y: 127.5 },
    ];
    // Halving luminance gives RGB (80, 50, 30). At +/-50% refinement,
    // saturation scales by 0.75/1.25 instead of clipping the color channels.
    for (strength, expected) in [
        (-50., [86, 49, 24, 255]),
        (0., [80, 50, 30, 255]),
        (50., [74, 51, 36, 255]),
    ] {
        settings.curve.refine_saturation = strength;
        assert_eq!(render(&source, &settings).unwrap()[(0, 0)], Rgba(expected));
    }
}

#[test]
fn grading_blending_and_balance_match_upstream_percentage_weights() {
    let source = RgbaImage::from_fn(2, 1, |x, _| {
        let gray = if x == 0 { 64 } else { 192 };
        Rgba([gray, gray, gray, 255])
    });
    let mut settings = Settings::default();
    settings.grading.wheels[0].saturation = 100.;
    // A red shadow wheel on dark/light gray, using the upstream kernel's
    // normalized blending and balance. Include endpoints and intermediate
    // values so either slider accidentally saturating is observable.
    for (blending, balance, dark, light) in [
        (0., -100., [168, 0, 0], [208, 176, 176]),
        (0., 0., [138, 0, 0], [192, 192, 192]),
        (0., 100., [104, 24, 24], [192, 192, 192]),
        (50., -100., [146, 0, 0], [215, 169, 169]),
        (50., 0., [124, 4, 4], [198, 186, 186]),
        (50., 100., [97, 31, 31], [192, 192, 192]),
        (100., -100., [134, 0, 0], [217, 167, 167]),
        (100., 0., [112, 16, 16], [208, 176, 176]),
        (100., 100., [95, 33, 33], [195, 189, 189]),
    ] {
        settings.grading.blending = blending;
        settings.grading.balance = balance;
        let actual = render(&source, &settings).unwrap();
        for (x, expected) in [dark, light].into_iter().enumerate() {
            assert_eq!(
                actual[(x as u32, 0)],
                Rgba([expected[0], expected[1], expected[2], 255]),
                "blending={blending}, balance={balance}, pixel={x}"
            );
        }
    }
}

#[test]
fn neutral_grade_preserves_all_bytes_and_exposure_preserves_alpha() {
    let source = RgbaImage::from_fn(16, 16, |x, y| Rgba([x as u8 * 10, y as u8 * 10, 83, 127]));
    assert_eq!(render(&source, &Settings::default()).unwrap(), source);
    let mut settings = Settings::default();
    settings.light.exposure = 1.;
    let adjusted = render(&source, &settings).unwrap();
    assert!(adjusted[(8, 8)][0] > source[(8, 8)][0]);
    assert!(adjusted.pixels().all(|p| p[3] == 127));
}
#[test]
fn hidden_group_keeps_settings_but_contributes_no_grade() {
    let source = RgbaImage::from_pixel(4, 4, Rgba([80, 100, 120, 255]));
    let mut settings = Settings::default();
    settings.light.exposure = 2.;
    settings.enabled[Group::Light as usize] = false;
    assert_eq!(render(&source, &settings).unwrap(), source);
    assert_eq!(settings.light.exposure, 2.);
}
#[test]
fn rejects_invalid_settings_before_mutating_document() {
    let mut document = Document::new(8, 8).unwrap();
    let original = document.clone();
    let mut settings = Settings::default();
    settings.color.temperature = f64::NAN;
    assert!(apply(&mut document, &settings).is_err());
    assert_eq!(document, original);
}

#[test]
fn every_camera_group_changes_pixels_and_preserves_coverage() {
    let source = RgbaImage::from_fn(32, 24, |x, y| {
        Rgba([
            30 + (x * 6) as u8,
            20 + (y * 8) as u8,
            70 + ((x + y) * 2) as u8,
            180,
        ])
    });
    type Change = fn(&mut Settings);
    let cases: [(Group, Change); 9] = [
        (Group::Light, |s| s.light.exposure = 1.),
        (Group::Color, |s| s.color.temperature = 40.),
        (Group::Effects, |s| {
            s.effects.texture = 50.;
            s.effects.glow = 30.;
        }),
        (Group::Curve, |s| s.curve.darks = 50.),
        (Group::Mixer, |s| s.mixer.saturation = [-80.; 8]),
        (Group::Grading, |s| {
            s.grading.wheels[3].hue = 120.;
            s.grading.wheels[3].saturation = 60.;
        }),
        (Group::Detail, |s| s.detail.noise_luminance = 60.),
        (Group::Optics, |s| s.optics.vignette_amount = 60.),
        (Group::Calibration, |s| s.calibration.red_saturation = 60.),
    ];
    for (group, change) in cases {
        let mut settings = Settings::default();
        change(&mut settings);
        let adjusted = render(&source, &settings).unwrap();
        assert_ne!(adjusted, source, "{group:?}");
        assert!(adjusted.pixels().all(|p| p[3] == 180), "{group:?}");
    }
}
#[test]
fn preview_diagnostics_never_enter_the_committed_grade() {
    let mut doc = Document::new(4, 1).unwrap();
    doc.layers[0].content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(4, 1, |x, _| {
            if x < 2 {
                Rgba([0, 0, 0, 255])
            } else {
                Rgba([255, 255, 255, 255])
            }
        }))));
    let original = doc.clone();
    let settings = Settings::default();
    preview(
        &mut doc,
        &settings,
        Preview {
            shadow_overlay: true,
            highlight_overlay: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_ne!(doc.layers[0].raster(), original.layers[0].raster());
    let mut committed = original.clone();
    apply(&mut committed, &settings).unwrap();
    assert_eq!(committed, original);
}
#[test]
fn geometry_warps_through_premultiplied_color_without_creating_hidden_rgb_edges() {
    let source = RgbaImage::from_fn(24, 24, |x, y| {
        if (5..19).contains(&x) && (5..19).contains(&y) {
            Rgba([255, 0, 0, 255])
        } else {
            Rgba([0, 255, 0, 0])
        }
    });
    let mut settings = Settings::default();
    settings.geometry.rotate = 17.;
    settings.geometry.vertical = 20.;
    let result = render(&source, &settings).unwrap();
    assert_eq!(result.dimensions(), source.dimensions());
    assert_ne!(result, source);
    for pixel in result.pixels().filter(|p| p[3] > 0) {
        assert_eq!(pixel[1], 0);
        assert_eq!(pixel[2], 0);
        assert!(pixel[0] > 245);
    }
}

#[test]
fn inactive_camera_controls_preserve_low_alpha_pixels_exactly() {
    let source = RgbaImage::from_fn(8, 4, |x, y| {
        Rgba([17 + x as u8, 81 + y as u8, 173, (x + y) as u8])
    });
    type Change = fn(&mut Settings);
    let cases: [Change; 10] = [
        |s| s.effects.grain_size = 70.,
        |s| s.effects.glow_spread = 80.,
        |s| s.curve.shadow_split = 30.,
        |s| s.mixer.points.push(PointColor::default()),
        |s| s.grading.blending = 90.,
        |s| s.grading.wheels[0].hue = 180.,
        |s| s.detail.sharpen_radius = 80.,
        |s| s.optics.profile_distortion = 30.,
        |s| s.process = Process::One,
        |s| s.guided = true,
    ];
    for (index, change) in cases.into_iter().enumerate() {
        let mut settings = Settings::default();
        change(&mut settings);
        assert_eq!(render(&source, &settings).unwrap(), source, "case {index}");
    }
}

#[test]
fn guided_geometry_ignores_short_lines_before_computing_corrections() {
    let source = RgbaImage::from_fn(24, 16, |x, y| Rgba([x as u8 * 9, y as u8 * 13, 90, 255]));
    let mut settings = Settings {
        guided: true,
        ..Default::default()
    };
    settings.guides.push(Guide {
        start: [0.1, 0.2],
        end: [0.9, 0.35],
    });
    let expected = render(&source, &settings).unwrap();
    assert_ne!(expected, source);
    settings.guides.insert(
        0,
        Guide {
            start: [0.5, 0.5],
            end: [0.501, 0.501],
        },
    );
    assert_eq!(render(&source, &settings).unwrap(), expected);
}
