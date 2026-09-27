use super::*;
use image::Rgba;

fn settings(range: Range, cmyk: [f32; 4], mode: Mode) -> SelectiveColor {
    let mut settings = SelectiveColor {
        mode,
        ..Default::default()
    };
    settings.adjustments[range.index()] = cmyk;
    settings
}

fn close(actual: [f32; 3], expected: [f32; 3]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
    }
}

#[test]
fn all_six_chromatic_ranges_match_their_color_and_leave_its_complement() {
    for (range, input) in [
        (Range::Reds, [1., 0., 0.]),
        (Range::Yellows, [1., 1., 0.]),
        (Range::Greens, [0., 1., 0.]),
        (Range::Cyans, [0., 1., 1.]),
        (Range::Blues, [0., 0., 1.]),
        (Range::Magentas, [1., 0., 1.]),
    ] {
        assert_eq!(membership(input)[range.index()], 1.);
        let settings = settings(range, [0., 0., 0., 50.], Mode::Absolute);
        close(settings.apply_rgb(input), input.map(|value| value * 0.5));
        let complement = input.map(|value| 1. - value);
        close(settings.apply_rgb(complement), complement);
        close(settings.apply_rgb([0.5; 3]), [0.5; 3]);
    }
}

#[test]
fn whites_neutrals_and_blacks_have_continuous_overlapping_membership() {
    for (input, weights) in [
        ([0.; 3], [0., 0., 1.]),
        ([0.25; 3], [0., 0.5, 0.5]),
        ([0.5; 3], [0., 1., 0.]),
        ([0.75; 3], [0.5, 0.5, 0.]),
        ([1.; 3], [1., 0., 0.]),
        ([0.8, 0.4, 0.2], [0., 0.4, 0.]),
    ] {
        close(membership(input)[6..].try_into().unwrap(), weights);
    }
    let neutral = settings(Range::Neutrals, [40., -20., 0., 0.], Mode::Absolute);
    close(neutral.apply_rgb([0.5; 3]), [0.1, 0.7, 0.5]);
    close(neutral.apply_rgb([0.; 3]), [0.; 3]);
    close(neutral.apply_rgb([1.; 3]), [1.; 3]);
    let black = settings(Range::Blacks, [-50., 0., 0., 0.], Mode::Absolute);
    close(black.apply_rgb([0.; 3]), [0.5, 0., 0.]);
    let white = settings(Range::Whites, [50., 0., 0., 0.], Mode::Absolute);
    close(white.apply_rgb([1.; 3]), [0.5, 1., 1.]);
}

#[test]
fn relative_adjusts_existing_ink_and_black_modulates_color_ink() {
    let absolute = settings(Range::Neutrals, [20., 0., 0., 25.], Mode::Absolute);
    let relative = SelectiveColor {
        mode: Mode::Relative,
        ..absolute
    };
    // At 75% gray the neutral weight is 1/2. Cyan's combined correction is
    // -((1 + 0.2) * 0.25 + 0.2) = -0.5, before relative ink scaling.
    close(absolute.apply_rgb([0.75; 3]), [0.5, 0.625, 0.625]);
    close(relative.apply_rgb([0.75; 3]), [0.6875, 0.71875, 0.71875]);
    let relative_white = settings(Range::Whites, [100.; 4], Mode::Relative);
    close(relative_white.apply_rgb([1.; 3]), [1.; 3]);
}

#[test]
fn range_corrections_use_original_color_and_accumulate_before_final_clipping() {
    let mut combined = settings(Range::Reds, [100., 0., 0., 0.], Mode::Absolute);
    combined.adjustments[Range::Yellows.index()] = [-100., 0., 0., 0.];
    combined.adjustments[Range::Neutrals.index()] = [50., 0., 0., 0.];
    // Red weight=.25, yellow=.25, neutral=.5. Red deltas are -.1875,
    // +.0625 and -.25. All three use the original .75 red channel.
    close(combined.apply_rgb([0.75, 0.5, 0.25]), [0.375, 0.5, 0.25]);
    combined.adjustments[Range::Yellows.index()] = [100., 0., 0., 0.];
    combined.adjustments[Range::Neutrals.index()] = [100., 0., 0., 0.];
    close(combined.apply_rgb([0.75, 0.5, 0.25]), [0., 0.5, 0.25]);
}

#[test]
fn extremes_clip_each_range_before_weighting_and_output_stays_finite() {
    let darken = settings(Range::Neutrals, [100.; 4], Mode::Absolute);
    // A maximal adjustment on 75% gray clips to -.75 before neutral weight .5.
    close(darken.apply_rgb([0.75; 3]), [0.375; 3]);
    for mode in [Mode::Relative, Mode::Absolute] {
        for value in [-100., 100.] {
            let settings = SelectiveColor {
                adjustments: [[value; 4]; 9],
                mode,
            };
            for input in [[0.; 3], [1.; 3], [0.5; 3], [0.1, 0.8, 0.4]] {
                assert!(
                    settings
                        .apply_rgb(input)
                        .iter()
                        .all(|v| (0. ..=1.).contains(v))
                );
            }
        }
    }
}

#[test]
fn alpha_invisible_rgb_and_identity_are_preserved() {
    let settings = settings(Range::Neutrals, [50., 0., 0., 0.], Mode::Absolute);
    let input = RgbaImage::from_fn(3, 1, |x, _| Rgba([128, 128, 128, [0, 73, 255][x as usize]]));
    let output = reference(&input, settings);
    assert_eq!(output[(0, 0)], input[(0, 0)]);
    assert_eq!(output[(1, 0)], Rgba([1, 128, 128, 73]));
    assert_eq!(output[(2, 0)], Rgba([1, 128, 128, 255]));
    for mode in [Mode::Relative, Mode::Absolute] {
        let identity = SelectiveColor {
            mode,
            ..Default::default()
        };
        assert!(identity.identity());
        assert_eq!(reference(&input, identity), input);
    }
}

#[test]
fn all_coefficients_validate_and_serialization_keeps_every_range() {
    for value in [-100.1, 100.1, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for range in Range::ALL {
            for channel in 0..4 {
                let mut settings = SelectiveColor::default();
                settings.adjustments[range.index()][channel] = value;
                assert!(settings.validate().is_err());
            }
        }
    }
    let original = SelectiveColor {
        adjustments: std::array::from_fn(|row| [row as f32 * 10., -100., 100., 12.5]),
        mode: Mode::Absolute,
    };
    original.validate().unwrap();
    let json = serde_json::to_string(&original).unwrap();
    let restored: SelectiveColor = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, original);
    assert!(
        serde_json::from_str::<SelectiveColor>(r#"{"adjustments":[],"mode":"relative"}"#).is_err()
    );
}

#[test]
fn combined_ranges_match_independent_ffmpeg_reference_within_byte_rounding() {
    // FFmpeg's selectivecolor filter rounds each range to an integer before
    // summing. Our continuous calculation rounds only the final output.
    let mut settings = SelectiveColor {
        adjustments: [
            [40., -20., 10., 15.],
            [-25., 10., 50., -20.],
            [10., 30., -10., 25.],
            [-30., 10., 20., -10.],
            [20., -40., 30., 10.],
            [10., 20., -20., 10.],
            [20., -10., 30., 10.],
            [-10., 10., 20., -10.],
            [-30., -20., -10., -20.],
        ],
        mode: Mode::Absolute,
    };
    let sources = [
        [0, 0, 0, 255],
        [255, 255, 255, 255],
        [128, 128, 128, 255],
        [204, 102, 51, 255],
        [26, 204, 153, 255],
        [153, 77, 230, 255],
    ];
    for (mode, expected) in [
        (
            Mode::Absolute,
            [
                [112, 92, 71],
                [173, 255, 145],
                [176, 131, 108],
                [171, 117, 13],
                [83, 174, 131],
                [131, 81, 196],
            ],
        ),
        (
            Mode::Relative,
            [
                [112, 92, 71],
                [255, 255, 255],
                [152, 129, 118],
                [200, 112, 16],
                [76, 198, 144],
                [145, 79, 227],
            ],
        ),
    ] {
        settings.mode = mode;
        for (source, expected) in sources.into_iter().zip(expected) {
            let actual = settings.pixel(source);
            for channel in 0..3 {
                assert!(
                    actual[channel].abs_diff(expected[channel]) <= 2,
                    "{mode:?} {source:?}: {actual:?} != {expected:?}"
                );
            }
        }
    }
}
