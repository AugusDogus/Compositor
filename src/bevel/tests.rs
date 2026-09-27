use super::*;

#[test]
fn defaults_round_trip_and_all_styles_persist() {
    let defaults = Settings::default();
    assert_eq!(defaults.style, Style::Inner);
    assert_eq!(defaults.depth, 100.);
    assert_eq!(defaults.size, 5.);
    assert_eq!(defaults.angle, 120.);
    assert_eq!(defaults.altitude, 30.);
    assert_eq!(defaults.highlight_opacity, 0.75);
    assert_eq!(defaults.shadow_opacity, 0.75);
    for style in [Style::Inner, Style::Outer, Style::Emboss] {
        let settings = Settings { style, ..defaults };
        let encoded = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<Settings>(&encoded).unwrap(),
            settings
        );
    }
}

#[test]
fn validation_rejects_nonfinite_and_out_of_range_values() {
    for field in 0..6 {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1001., 1001.] {
            let mut settings = Settings::default();
            let fields = [
                &mut settings.depth,
                &mut settings.size,
                &mut settings.angle,
                &mut settings.altitude,
                &mut settings.highlight_opacity,
                &mut settings.shadow_opacity,
            ];
            *fields.into_iter().nth(field).unwrap() = value;
            assert!(settings.validate().is_err(), "field {field}: {value}");
            assert!(Lighting::new(&settings).is_err());
        }
    }
}

#[test]
fn deserialization_rejects_invalid_fields_at_the_boundary() {
    for (field, values) in [
        ("depth", [0., 1000.01]),
        ("size", [-0.01, 250.01]),
        ("angle", [-360.01, 360.01]),
        ("altitude", [-0.01, 90.01]),
        ("highlightOpacity", [-0.01, 1.01]),
        ("shadowOpacity", [-0.01, 1.01]),
    ] {
        for value in values {
            let mut saved = serde_json::to_value(Settings::default()).unwrap();
            saved[field] = serde_json::json!(value);
            assert!(serde_json::from_value::<Settings>(saved).is_err());
        }
    }
    let mut saved = serde_json::to_value(Settings::default()).unwrap();
    saved["style"] = serde_json::json!("unknown");
    assert!(serde_json::from_value::<Settings>(saved).is_err());
}

#[test]
fn inclusive_numeric_endpoints_are_valid() {
    for settings in [
        Settings {
            depth: 1.,
            size: 0.,
            angle: -360.,
            altitude: 0.,
            highlight_opacity: 0.,
            shadow_opacity: 0.,
            ..Default::default()
        },
        Settings {
            depth: 1000.,
            size: 250.,
            angle: 360.,
            altitude: 90.,
            highlight_opacity: 1.,
            shadow_opacity: 1.,
            ..Default::default()
        },
    ] {
        settings.validate().unwrap();
        let saved = serde_json::to_string(&settings).unwrap();
        serde_json::from_str::<Settings>(&saved).unwrap();
    }
}

#[test]
fn flat_interiors_have_no_lighting_change() {
    for altitude in [0., 30., 90.] {
        let light = Lighting::new(&Settings {
            altitude,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(light.sample([0., 0.]), Shading::default());
    }
}

#[test]
fn light_from_right_brightens_right_edges_and_shades_left_edges() {
    let light = Lighting::new(&Settings {
        angle: 0.,
        altitude: 0.,
        ..Default::default()
    })
    .unwrap();
    let right = light.sample([-0.1, 0.]);
    let left = light.sample([0.1, 0.]);
    assert!(right.highlight > 0.);
    assert_eq!(right.shadow, 0.);
    assert_eq!(left.highlight, 0.);
    assert_eq!(right.highlight, left.shadow);
}

#[test]
fn positive_ninety_degrees_lights_top_edges_in_image_coordinates() {
    let light = Lighting::new(&Settings {
        angle: 90.,
        altitude: 0.,
        ..Default::default()
    })
    .unwrap();
    assert!(light.sample([0., 0.1]).highlight > 0.);
    assert_eq!(light.sample([0., 0.1]).shadow, 0.);
    assert!(light.sample([0., -0.1]).shadow > 0.);
    assert_eq!(light.sample([0., -0.1]).highlight, 0.);
}

#[test]
fn overhead_light_has_no_directional_bias_and_zero_altitude_is_supported() {
    for angle in [-360., -10., 120., 360.] {
        let light = Lighting::new(&Settings {
            angle,
            altitude: 90.,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(light.light, [0., 0., 1.]);
        let edge = light.sample([0.2, 0.]);
        assert_eq!(edge.highlight, 0.);
        assert!(edge.shadow > 0.);
        assert_eq!(edge, light.sample([-0.2, 0.]));
        assert_eq!(edge, light.sample([0., 0.2]));
    }
    let horizontal = Lighting::new(&Settings {
        angle: 0.,
        altitude: 0.,
        ..Default::default()
    })
    .unwrap();
    assert_eq!(horizontal.light, [1., 0., 0.]);
}

#[test]
fn zero_size_disabled_and_zero_opacity_leave_pixels_unchanged() {
    for settings in [
        Settings {
            size: 0.,
            ..Default::default()
        },
        Settings {
            enabled: false,
            ..Default::default()
        },
        Settings {
            highlight_opacity: 0.,
            shadow_opacity: 0.,
            ..Default::default()
        },
    ] {
        let light = Lighting::new(&settings).unwrap();
        for gradient in [[1., -1.], [-1., 1.], [0., 0.]] {
            assert_eq!(light.sample(gradient), Shading::default());
        }
    }
}

#[test]
fn fractional_height_changes_are_not_quantized_to_alpha_bytes() {
    let light = Lighting::new(&Settings {
        angle: 0.,
        altitude: 0.,
        ..Default::default()
    })
    .unwrap();
    let faint = light.sample([-0.0001, 0.]);
    let stronger = light.sample([-0.0002, 0.]);
    assert!(faint.highlight > 0.);
    assert!(stronger.highlight > faint.highlight);
}

#[test]
fn extreme_relief_produces_finite_shading_bounded_by_each_opacity() {
    for altitude in [0., 30., 90.] {
        let light = Lighting::new(&Settings {
            depth: 1000.,
            size: 250.,
            altitude,
            highlight_opacity: 0.4,
            shadow_opacity: 0.8,
            ..Default::default()
        })
        .unwrap();
        for x in [-1., -0.001, 0., 0.001, 1.] {
            for y in [-1., -0.001, 0., 0.001, 1.] {
                let shade = light.sample([x, y]);
                assert!((0. ..=0.4).contains(&shade.highlight));
                assert!((0. ..=0.8).contains(&shade.shadow));
                assert!(shade.highlight == 0. || shade.shadow == 0.);
            }
        }
    }
}
