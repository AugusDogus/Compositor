use super::*;
#[test]
fn all_bytes_match_integer_equal_bin_reference() {
    for levels in 2..=256 {
        let settings = Posterize::new(levels).unwrap();
        for value in 0..=255u16 {
            let expected = if levels == 256 {
                value
            } else {
                let bin = value * levels / 256;
                ((u32::from(bin) * 255 + u32::from(levels - 1) / 2) / u32::from(levels - 1)) as u16
            };
            let actual = settings.pixel([value as u8, value as u8, value as u8, 73]);
            assert_eq!(
                u16::from(actual[0]),
                expected,
                "levels={levels} value={value}"
            );
            let continuous = settings.apply_rgb([f32::from(value) / 255.; 3]);
            assert_eq!(
                (continuous[0] * 255.).round() as u16,
                expected,
                "layer levels={levels} value={value}"
            );
        }
    }
}
#[test]
fn four_levels_have_equal_bins_and_keep_alpha() {
    for (value, expected) in [
        (0, 0),
        (63, 0),
        (64, 85),
        (127, 85),
        (128, 170),
        (191, 170),
        (192, 255),
        (255, 255),
    ] {
        assert_eq!(
            Posterize::default().pixel([value, value, value, 80]),
            [expected, expected, expected, 80]
        );
    }
    assert_eq!(
        Posterize::default().pixel([80, 140, 220, 0]),
        [80, 140, 220, 0]
    );
}
#[test]
fn full_levels_preserve_continuous_color() {
    let input = [0.1234567, 0.55555, 0.8765432];
    assert_eq!(Posterize::new(256).unwrap().apply_rgb(input), input);
    let rgba = [
        0.1234567890123,
        0.5555555555555,
        0.8765432109876,
        0.3456789012345,
    ];
    assert_eq!(
        crate::adjustment::ExtendedAdjustment::Posterize(Posterize::new(256).unwrap())
            .apply_rgba(rgba),
        rgba
    );
}
#[test]
fn invalid_serialized_levels_are_rejected() {
    for value in ["0", "1", "257", "65536", "-1", "4.5", "null"] {
        assert!(serde_json::from_str::<Posterize>(&format!("{{\"levels\":{value}}}")).is_err());
    }
    assert!(serde_json::from_str::<Posterize>(r#"{"levels":4,"unknown":1}"#).is_err());
    let original = Posterize::new(256).unwrap();
    assert_eq!(
        serde_json::from_str::<Posterize>(&serde_json::to_string(&original).unwrap()).unwrap(),
        original
    );
}
#[test]
fn image_reference_uses_channel_bins_without_changing_dimensions() {
    let image = RgbaImage::from_pixel(3, 2, image::Rgba([90, 150, 240, 80]));
    let result = reference(&image, Posterize::default());
    assert_eq!(result.dimensions(), image.dimensions());
    assert!(result.pixels().all(|p| p.0 == [85, 170, 255, 80]));
}

#[test]
fn continuous_backdrops_are_not_quantized_before_binning() {
    for levels in [2, 4, 8, 16, 128, 255] {
        let settings = Posterize::new(levels).unwrap();
        let boundary = 256. / f32::from(levels);
        let input = [
            (boundary - 0.001) / 255.,
            boundary / 255.,
            (boundary + 0.001) / 255.,
        ];
        let actual = settings.apply_rgb(input);
        assert_eq!(actual[0], 0.);
        assert_eq!(actual[1], 1. / f32::from(levels - 1));
        assert_eq!(actual[2], actual[1]);
    }
}
