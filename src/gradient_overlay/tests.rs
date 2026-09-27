use super::*;
#[test]
fn independent_opacity_knots_never_change_color_interpolation() {
    let mut overlay = Overlay {
        stops: Stops::endpoints([255, 0, 0, 255], [0, 0, 255, 255]),
        ..Default::default()
    };
    assert_eq!(overlay.sample(0.25), [0.75, 0., 0.25, 1.]);
    overlay.opacity_stops = OpacityStops::new(vec![
        OpacityStop {
            position: 0.,
            opacity: 0.,
        },
        OpacityStop {
            position: 0.37,
            opacity: 0.123456789,
        },
        OpacityStop {
            position: 1.,
            opacity: 1.,
        },
    ])
    .unwrap();
    assert_eq!(overlay.sample(0.37), [0.63, 0., 0.37, 0.123456789]);
    let sample = overlay.sample(0.25);
    assert_eq!(&sample[..3], &[0.75, 0., 0.25]);
    assert!((sample[3] - 0.25 / 0.37 * 0.123456789).abs() < 1e-12);
    let color_count = overlay.stops.as_slice().len();
    overlay.opacity_stops.insert(0.3).unwrap();
    assert_eq!(overlay.stops.as_slice().len(), color_count);
    overlay.stops = Stops::endpoints([255, 0, 0, 0], [0, 0, 255, 255]);
    assert!(overlay.validate().is_err());
}
#[test]
fn physical_angles_and_radial_distance_use_stable_source_bounds() {
    let mut overlay = Overlay {
        angle: 0.,
        ..Default::default()
    };
    let geometry = Geometry::new(&overlay, [200, 100]).unwrap();
    assert_eq!(geometry.position([0., 50.]), 0.);
    assert_eq!(geometry.position([200., 50.]), 1.);
    overlay.angle = 90.;
    let geometry = Geometry::new(&overlay, [200, 100]).unwrap();
    assert!((geometry.position([100., 0.]) - 1.).abs() < 1e-12);
    assert!(geometry.position([100., 100.]).abs() < 1e-12);
    overlay.angle = 45.;
    let geometry = Geometry::new(&overlay, [200, 100]).unwrap();
    assert!((geometry.position([120., 70.]) - 0.5).abs() < 1e-12);
    overlay.style = Style::Radial;
    let geometry = Geometry::new(&overlay, [200, 100]).unwrap();
    assert_eq!(geometry.position([100., 50.]), 0.);
    assert_eq!(geometry.position([150., 50.]), 1.);
    assert_eq!(geometry.position([100., 100.]), 1.);
    overlay.reverse = true;
    let geometry = Geometry::new(&overlay, [200, 100]).unwrap();
    assert_eq!(geometry.position([100., 50.]), 1.);
    assert_eq!(geometry.position([150., 50.]), 0.);
    assert!(Geometry::new(&overlay, [0, 1]).is_err());
}
#[test]
fn gradient_overlay_stops_roundtrip_with_alpha_hard_edges_and_disabled_state() {
    let overlay = Overlay {
        enabled: false,
        reverse: true,
        stops: Stops::new(vec![
            Stop {
                position: 0.,
                color: [255, 0, 0, 255],
            },
            Stop {
                position: 0.5,
                color: [0, 255, 0, 255],
            },
            Stop {
                position: 0.5,
                color: [0, 0, 255, 255],
            },
            Stop {
                position: 1.,
                color: [255; 4],
            },
        ])
        .unwrap(),
        ..Default::default()
    };
    let json = serde_json::to_vec(&overlay).unwrap();
    assert_eq!(serde_json::from_slice::<Overlay>(&json).unwrap(), overlay);
    assert_eq!(overlay.sample(0.5), [0., 0., 1., 1.]);
}
#[test]
fn gradient_overlay_rejects_invalid_stops_without_normalizing_input() {
    for stops in [
        serde_json::json!([]),
        serde_json::json!([
            {"position":-0.1,"color":[0,0,0,255]},
            {"position":1.,"color":[255,255,255,255]},
        ]),
        serde_json::json!([
            {"position":0.,"color":[256,0,0,255]},
            {"position":1.,"color":[255,255,255,255]},
        ]),
    ] {
        let mut json = serde_json::to_value(Overlay::default()).unwrap();
        json["stops"] = stops;
        assert!(serde_json::from_value::<Overlay>(json).is_err());
    }
    let mut overlay = Overlay::default();
    for value in [f64::NAN, f64::INFINITY, -361., 361.] {
        overlay.angle = value;
        assert!(overlay.validate().is_err());
    }
    overlay.angle = 0.;
    for value in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
        overlay.opacity = value;
        assert!(overlay.validate().is_err());
    }
}

#[test]
fn integer_render_keys_are_bounded_and_independent_of_effect_padding() {
    for size in [[1, 1], [30_000, 30_000], [30_000, 1], [1, 30_000]] {
        for angle in [-360., -135., 0., 30., 45., 90., 135., 180., 270., 360.] {
            for style in [Style::Linear, Style::Radial] {
                for reverse in [false, true] {
                    let overlay = Overlay {
                        angle,
                        style,
                        reverse,
                        ..Default::default()
                    };
                    let geometry = Geometry::new(&overlay, size).unwrap();
                    let low = geometry.stop_key(0.);
                    let high = geometry.stop_key(1.);
                    for point in [
                        [0., 0.],
                        [0.5, 0.5],
                        [f64::from(size[0]) - 0.5, f64::from(size[1]) - 0.5],
                        [f64::from(size[0]), f64::from(size[1])],
                    ] {
                        let key = geometry.key(point);
                        assert!(
                            (low..=high).contains(&key),
                            "{size:?} {angle} {style:?} {reverse} {key} {low} {high}"
                        );
                        for inset in [2, 123, 6502] {
                            let padded = Geometry::padded(
                                &overlay,
                                size.map(|axis| axis + 2 * inset),
                                inset,
                            )
                            .unwrap();
                            assert_eq!(padded.key(point), key);
                        }
                    }
                }
            }
        }
    }
}
#[test]
fn reversed_radial_clamping_respects_coincident_endpoint_color_and_opacity_stops() {
    let overlay = Overlay {
        style: Style::Radial,
        reverse: true,
        stops: Stops::new(vec![
            Stop {
                position: 0.,
                color: [0, 0, 0, 255],
            },
            Stop {
                position: 0.,
                color: [255; 4],
            },
        ])
        .unwrap(),
        opacity_stops: OpacityStops::new(vec![
            OpacityStop {
                position: 0.,
                opacity: 0.,
            },
            OpacityStop {
                position: 0.,
                opacity: 1.,
            },
        ])
        .unwrap(),
        ..Default::default()
    };
    let prepared = Prepared::new(&overlay, [11, 11], 2).unwrap();
    assert_eq!(prepared.sample([0.5, 0.5]), [1.; 4]);
}
