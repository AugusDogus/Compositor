use super::*;

fn pixel(gray: f64, alpha: f64) -> [f64; 4] {
    [gray / 255., gray / 255., gray / 255., alpha]
}

// Test values use byte-scale gray; production keys keep three decimal places.
fn weight(range: Range, gray: f64) -> f64 {
    range.weight_key((gray * 1000.).round() as u32)
}

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-12, "{actual} != {expected}");
}

#[test]
fn default_includes_every_byte_tone() {
    let range = Range::default();
    let settings = Settings::default();
    assert!(range.is_identity());
    assert!(settings.is_identity());
    for gray in 0..=255 {
        assert_eq!(weight(range, f64::from(gray)), 1.);
        assert_eq!(settings.weight(pixel(f64::from(gray), 1.), [0.; 4]), 1.);
    }
}

#[test]
fn hard_cutoffs_include_both_endpoints() {
    let range = Range::new([40, 40], [200, 200]).unwrap();
    for (gray, expected) in [(39., 0.), (40., 1.), (120., 1.), (200., 1.), (201., 0.)] {
        assert_eq!(weight(range, gray), expected);
    }
    let single = Range::new([128, 128], [128, 128]).unwrap();
    assert_eq!(weight(single, 128.), 1.);
    assert_eq!(weight(single, 127.999), 0.);
    assert_eq!(weight(single, 128.001), 0.);
}

#[test]
fn independent_split_handles_control_each_fade() {
    let range = Range::new([20, 60], [180, 240]).unwrap();
    for (gray, expected) in [
        (0., 0.),
        (20., 0.),
        (30., 0.25),
        (40., 0.5),
        (60., 1.),
        (180., 1.),
        (195., 0.75),
        (210., 0.5),
        (240., 0.),
        (255., 0.),
    ] {
        near(weight(range, gray), expected);
    }
}

#[test]
fn overlapping_fades_multiply_and_crossed_hard_ranges_hide_everything() {
    let overlap = Range::new([0, 200], [50, 250]).unwrap();
    near(weight(overlap, 100.), 0.5 * 0.75);
    near(weight(overlap, 150.), 0.75 * 0.5);
    let crossed = Range::new([200, 200], [50, 50]).unwrap();
    for gray in 0..=255 {
        assert_eq!(weight(crossed, f64::from(gray)), 0.);
    }
}

#[test]
fn invalid_pair_order_and_out_of_bounds_serialized_values_are_rejected() {
    assert!(Range::new([90, 40], [200, 200]).is_err());
    assert!(Range::new([40, 40], [220, 200]).is_err());
    for json in [
        r#"{"black":[90,40],"white":[200,200]}"#,
        r#"{"black":[40,40],"white":[220,200]}"#,
        r#"{"black":[-1,0],"white":[255,255]}"#,
        r#"{"black":[0,0],"white":[255,256]}"#,
        r#"{"black":[0,0.5],"white":[255,255]}"#,
        r#"{"black":[0],"white":[255,255]}"#,
        r#"{"black":[0,0],"white":[255,255],"feather":20}"#,
    ] {
        assert!(serde_json::from_str::<Range>(json).is_err(), "{json}");
    }
}

#[test]
fn disabled_settings_roundtrip_without_losing_independent_handles() {
    let settings = Settings {
        enabled: false,
        source: Range::from_endpoints([10, 20, 210, 220]).unwrap(),
        underlying: Range::from_endpoints([30, 40, 230, 240]).unwrap(),
    };
    let json = serde_json::to_string(&settings).unwrap();
    assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), settings);
    assert_eq!(settings.source.endpoints(), [10, 20, 210, 220]);
    assert!(settings.is_identity());
    assert_eq!(settings.weight([0.; 4], [1.; 4]), 1.);
    let invalid = json.replace("[10,20]", "[20,10]");
    assert!(serde_json::from_str::<Settings>(&invalid).is_err());
}

#[test]
fn source_and_backdrop_weights_multiply_without_changing_source_coverage() {
    let settings = Settings {
        enabled: true,
        source: Range::new([0, 100], [255, 255]).unwrap(),
        underlying: Range::new([0, 200], [255, 255]).unwrap(),
    };
    for source_alpha in [0., 0.25, 0.5, 1.] {
        near(
            settings.weight(pixel(50., source_alpha), pixel(50., 1.)),
            0.125,
        );
    }
}

#[test]
fn backdrop_alpha_controls_exclusion_and_hidden_rgb_is_irrelevant() {
    let settings = Settings {
        enabled: true,
        source: Range::default(),
        underlying: Range::new([100, 100], [200, 200]).unwrap(),
    };
    for hidden in [[0., 0., 0., 0.], [1., 1., 1., 0.], [1., 0., 0., 0.]] {
        assert_eq!(settings.weight(pixel(128., 1.), hidden), 1.);
    }
    for alpha in [0., 0.25, 0.5, 1.] {
        near(
            settings.weight(pixel(128., 1.), pixel(0., alpha)),
            1. - alpha,
        );
        assert_eq!(settings.weight(pixel(128., 1.), pixel(128., alpha)), 1.);
    }
}

#[test]
fn gray_uses_encoded_rgb_and_keeps_neutral_byte_boundaries_exact() {
    assert_eq!(gray([1., 0., 0., 1.]), 299 * 255);
    assert_eq!(gray([0., 1., 0., 1.]), 587 * 255);
    assert_eq!(gray([0., 0., 1., 1.]), 114 * 255);
    for tone in 0..=255u8 {
        let settings = Settings {
            enabled: true,
            source: Range::new([tone, tone], [tone, tone]).unwrap(),
            underlying: Range::default(),
        };
        assert_eq!(
            settings.weight(pixel(f64::from(tone), 1.), [0.; 4]),
            1.,
            "{tone}"
        );
    }
}

#[test]
fn all_pair_extremes_produce_bounded_weights() {
    let pairs = [[0, 0], [0, 255], [255, 255], [64, 192], [127, 128]];
    for black in pairs {
        for white in pairs {
            let range = Range::new(black, white).unwrap();
            for quarter in 0..=1020 {
                let weight = weight(range, f64::from(quarter) / 4.);
                assert!((0. ..=1.).contains(&weight), "{range:?}: {weight}");
            }
        }
    }
}
