use super::*;
use crate::{geometry::Point, vector_path::Anchor};

fn path(points: &[Point], closure: Closure) -> BezierPath {
    BezierPath {
        anchors: points.iter().copied().map(Anchor::corner).collect(),
        closure,
    }
}

fn rectangle() -> BezierPath {
    path(
        &[[10., 20.], [30., 20.], [30., 40.], [10., 40.]],
        Closure::Closed,
    )
}

fn fill() -> Style {
    Style {
        fill: Some([255, 0, 0, 128]),
        stroke: None,
    }
}

fn assert_path_near(a: &BezierPath, b: &BezierPath) {
    assert_eq!(a.closure, b.closure);
    assert_eq!(a.anchors.len(), b.anchors.len());
    for (a, b) in a.anchors.iter().zip(&b.anchors) {
        for (a, b) in [Some(a.point), a.incoming, a.outgoing].into_iter().zip([
            Some(b.point),
            b.incoming,
            b.outgoing,
        ]) {
            match (a, b) {
                (Some(a), Some(b)) => {
                    assert!((a[0] - b[0]).hypot(a[1] - b[1]) < 1e-8, "{a:?} != {b:?}")
                }
                (None, None) => (),
                _ => panic!("Handle presence changed"),
            }
        }
    }
}

#[test]
fn fill_keeps_straight_alpha_and_document_geometry() {
    let geometry = rectangle();
    let (content, transform) = Content::from_document_path(geometry.clone(), fill()).unwrap();
    assert_eq!(transform.origin, [9., 19.]);
    assert_eq!(content.source.size, [22, 22]);
    assert_eq!(content.pixels.get_pixel(5, 5).0, [255, 0, 0, 128]);
    assert_eq!(content.pixels.get_pixel(0, 0).0, [0; 4]);
    assert_path_near(&geometry, &content.document_path(transform).unwrap());
}

#[test]
fn open_stroke_has_round_caps_and_padding() {
    let geometry = path(&[[-30., -10.], [10., -10.]], Closure::Open);
    let style = Style {
        fill: None,
        stroke: Some(Stroke {
            width: 10.,
            color: [0, 255, 0, 255],
        }),
    };
    let (content, transform) = Content::from_document_path(geometry.clone(), style).unwrap();
    assert_eq!(content.source.size, [52, 12]);
    assert_eq!(transform.origin, [-36., -16.]);
    assert_eq!(content.pixels.get_pixel(2, 6).0, [0, 255, 0, 255]);
    assert_eq!(content.pixels.get_pixel(0, 6).0, [0; 4]);
    assert_eq!(content.pixels.get_pixel(1, 1).0, [0; 4]);
    assert_path_near(&geometry, &content.document_path(transform).unwrap());
}

#[test]
fn open_fill_closes_without_closing_the_stroke_or_source() {
    let style = Style {
        stroke: Some(Stroke {
            width: 2.,
            color: [0, 0, 255, 255],
        }),
        ..fill()
    };
    let geometry = path(&[[0., 0.], [20., 0.], [20., 20.]], Closure::Open);
    let (content, _) = Content::from_document_path(geometry, style).unwrap();
    assert_eq!(content.source.geometry.closure, Closure::Open);
    assert_eq!(content.pixels.get_pixel(15, 7).0, [255, 0, 0, 128]);
    assert_eq!(content.pixels.get_pixel(15, 2).0, [0, 0, 255, 255]);
    // The diagonal closing edge belongs to the fill, with no blue stroke.
    assert_eq!(content.pixels.get_pixel(12, 12).0[2], 0);
}

#[test]
fn transparent_stroke_composites_over_fill() {
    let style = Style {
        fill: Some([255, 0, 0, 255]),
        stroke: Some(Stroke {
            width: 6.,
            color: [0, 0, 255, 128],
        }),
    };
    let (content, _) = Content::from_document_path(rectangle(), style).unwrap();
    assert_eq!(content.pixels.get_pixel(4, 12).0, [127, 0, 128, 255]);
    assert_eq!(content.pixels.get_pixel(12, 12).0, [255, 0, 0, 255]);
}

#[test]
fn transformed_editing_preserves_orientation_and_unchanged_anchor_positions() {
    let mut original = rectangle();
    original.anchors[0].outgoing = Some([20., 15.]);
    original.anchors[1].incoming = Some([25., 15.]);
    let (content, base) = Content::from_document_path(original, fill()).unwrap();
    for flip_x in [false, true] {
        for flip_y in [false, true] {
            for rotation in [0., 37., -132.] {
                let transform = Transform {
                    size: [base.size[0] * 2.3, base.size[1] * 0.7],
                    rotation,
                    flip_x,
                    flip_y,
                    ..base
                };
                let before = content.document_path(transform).unwrap();
                for displacement in [[-15., 9.], [4., -3.]] {
                    let mut edited = before.clone();
                    edited.anchors[2].point[0] += displacement[0];
                    edited.anchors[2].point[1] += displacement[1];
                    let (next, next_transform) =
                        content.edited(edited.clone(), fill(), transform).unwrap();
                    assert_path_near(&edited, &next.document_path(next_transform).unwrap());
                    assert_eq!(next_transform.rotation, rotation);
                    assert_eq!(next_transform.flip_x, flip_x);
                    assert_eq!(next_transform.flip_y, flip_y);
                    assert_eq!(next_transform.sampling, transform.sampling);
                }
            }
        }
    }
}

#[test]
fn shrinking_geometry_and_changing_stroke_preserves_placement() {
    let (content, transform) = Content::from_document_path(rectangle(), fill()).unwrap();
    let smaller = path(
        &[[18., 28.], [22., 28.], [22., 32.], [18., 32.]],
        Closure::Closed,
    );
    let (next, next_transform) = content.edited(smaller.clone(), fill(), transform).unwrap();
    assert_eq!(next.source.size, [6, 6]);
    assert_path_near(&smaller, &next.document_path(next_transform).unwrap());
    let style = Style {
        stroke: Some(Stroke {
            width: 30.,
            color: [0, 0, 0, 255],
        }),
        ..fill()
    };
    let (next, next_transform) = next.edited(smaller.clone(), style, next_transform).unwrap();
    assert_eq!(next.source.size, [36, 36]);
    assert_path_near(&smaller, &next.document_path(next_transform).unwrap());
}

#[test]
fn rerasterizing_preserves_source_and_scales_stroke_anisotropically() {
    let style = Style {
        fill: None,
        stroke: Some(Stroke {
            width: 4.,
            color: [10, 20, 30, 255],
        }),
    };
    let (content, _) =
        Content::from_document_path(path(&[[0., 0.], [20., 0.]], Closure::Open), style).unwrap();
    let next = content.with_resolution([52, 18]).unwrap();
    assert_eq!(next.source(), content.source());
    assert!(Arc::ptr_eq(&next.source, &content.source));
    assert_eq!(next.pixels.dimensions(), (52, 18));
    assert_eq!(next.pixels.get_pixel(26, 5).0, [10, 20, 30, 255]);
    assert_eq!(next.pixels.get_pixel(26, 2).0, [0; 4]);
}

#[test]
fn curved_geometry_survives_source_roundtrip_and_high_resolution_cache() {
    let geometry = BezierPath {
        anchors: vec![
            Anchor {
                outgoing: Some([0., 80.]),
                ..Anchor::corner([0., 0.])
            },
            Anchor {
                incoming: Some([80., 80.]),
                ..Anchor::corner([80., 0.])
            },
        ],
        closure: Closure::Open,
    };
    let (content, transform) = Content::from_document_path(geometry.clone(), fill()).unwrap();
    let source: Source =
        serde_json::from_str(&serde_json::to_string(content.source()).unwrap()).unwrap();
    let restored = Content::from_source(source).unwrap();
    assert_eq!(content, restored);
    let preview = restored.with_resolution([820, 620]).unwrap();
    assert_path_near(&geometry, &preview.document_path(transform).unwrap());
    assert_eq!(preview.pixels.get_pixel(410, 200).0, [255, 0, 0, 128]);
}

#[test]
fn invalid_style_geometry_size_and_transform_are_rejected() {
    assert!(
        Content::from_document_path(
            rectangle(),
            Style {
                fill: None,
                stroke: None
            }
        )
        .is_err()
    );
    for width in [0., -1., f64::NAN, f64::INFINITY, 30_001.] {
        assert!(
            Content::from_document_path(
                rectangle(),
                Style {
                    fill: None,
                    stroke: Some(Stroke {
                        width,
                        color: [0; 4]
                    })
                }
            )
            .is_err()
        );
    }
    for geometry in [
        BezierPath::default(),
        path(&[[1., 1.]], Closure::Open),
        path(&[[0., 0.], [f64::NAN, 1.]], Closure::Open),
        path(&[[0., 0.], [40_000., 1.]], Closure::Open),
    ] {
        assert!(Content::from_document_path(geometry, fill()).is_err());
    }
    let (content, mut transform) = Content::from_document_path(rectangle(), fill()).unwrap();
    for size in [[0, 22], [30_001, 22], [30_000, 30_000]] {
        assert!(content.with_resolution(size).is_err());
    }
    let mut source = content.source().clone();
    source.size = [1, 1];
    assert!(Content::from_source(source).is_err());
    transform.size[0] = 0.;
    assert!(content.document_path(transform).is_err());
    assert!(content.edited(rectangle(), fill(), transform).is_err());
}

#[test]
fn document_geometry_supports_existing_path_selection() {
    let (content, transform) = Content::from_document_path(rectangle(), fill()).unwrap();
    let geometry = content.document_path(transform).unwrap();
    assert_eq!(
        geometry.selection(50, 50, true).unwrap(),
        rectangle().selection(50, 50, true).unwrap()
    );
}

#[test]
fn excessive_curve_complexity_is_rejected_before_raster_allocation() {
    let geometry = BezierPath {
        anchors: (0..crate::vector_path::MAX_ANCHORS)
            .map(|index| Anchor {
                incoming: Some([50., -100.]),
                outgoing: Some([50., 100.]),
                ..Anchor::corner([if index % 2 == 0 { 0. } else { 100. }, 0.])
            })
            .collect(),
        closure: Closure::Open,
    };
    let error = Content::from_document_path(geometry, fill()).unwrap_err();
    assert!(error.to_string().contains("segment budget"), "{error}");
}

#[test]
fn deserialized_sources_validate_style_and_unclipped_geometry() {
    let (content, _) = Content::from_document_path(rectangle(), fill()).unwrap();
    let original = serde_json::to_value(content.source()).unwrap();
    let mut clipped = original.clone();
    clipped["geometry"]["anchors"][0]["point"] = serde_json::json!([-1., 1.]);
    let source: Source = serde_json::from_value(clipped).unwrap();
    assert!(Content::from_source(source).is_err());
    let mut empty_style = original.clone();
    empty_style["style"]["fill"] = serde_json::Value::Null;
    let source: Source = serde_json::from_value(empty_style).unwrap();
    assert!(Content::from_source(source).is_err());
    let mut unknown = original;
    unknown["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<Source>(unknown).is_err());
}
