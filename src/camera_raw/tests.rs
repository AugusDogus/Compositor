use super::*;
use image::Rgba;
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
