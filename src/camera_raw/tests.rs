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
