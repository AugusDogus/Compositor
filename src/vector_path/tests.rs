use super::*;

fn arch() -> BezierPath {
    BezierPath {
        anchors: vec![
            Anchor {
                outgoing: Some([0., 100.]),
                ..Anchor::corner([0., 0.])
            },
            Anchor {
                incoming: Some([100., 100.]),
                ..Anchor::corner([100., 0.])
            },
        ],
        closure: Closure::Open,
    }
}
fn polygon(points: &[Point], closure: Closure) -> BezierPath {
    BezierPath {
        anchors: points.iter().copied().map(Anchor::corner).collect(),
        closure,
    }
}

#[test]
fn tight_bounds_exclude_handle_extents() {
    assert_eq!(arch().bounds().unwrap(), Some([0., 0., 100., 75.]));
    assert_eq!(BezierPath::default().bounds().unwrap(), None);
    assert_eq!(
        polygon(&[[3., 4.]], Closure::Open).bounds().unwrap(),
        Some([3., 4., 3., 4.])
    );
}

#[test]
fn adaptive_flatten_preserves_endpoints_and_error_tolerance() {
    let path = arch();
    let tolerance = 0.05;
    let points = path
        .flatten(FlattenOptions {
            tolerance,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(points.first(), Some(&[0., 0.]));
    assert_eq!(points.last(), Some(&[100., 0.]));
    assert!(points.len() > 8);
    let cubic = curve(path.segments().next().unwrap());
    for index in 0..=1000 {
        let on_curve = point(cubic.eval(f64::from(index) / 1000.));
        let distance = points
            .windows(2)
            .map(|line| distance_to_segment(on_curve, line[0], line[1]))
            .fold(f64::INFINITY, f64::min);
        assert!(distance <= tolerance, "distance={distance}");
    }
    assert!(
        path.flatten(FlattenOptions {
            tolerance: 1.,
            ..Default::default()
        })
        .unwrap()
        .len()
            < points.len()
    );
}

#[test]
fn straight_segments_remain_single_segments_and_closed_paths_return_to_start() {
    let open = polygon(&[[0., 0.], [10., 0.], [10., 10.]], Closure::Open);
    assert_eq!(
        open.flatten(FlattenOptions::default()).unwrap(),
        vec![[0., 0.], [10., 0.], [10., 10.]]
    );
    let closed = BezierPath {
        closure: Closure::Closed,
        ..open
    };
    assert_eq!(
        closed.flatten(FlattenOptions::default()).unwrap(),
        vec![[0., 0.], [10., 0.], [10., 10.], [0., 0.]]
    );
}

#[test]
fn collinear_reversals_and_looping_curves_do_not_collapse_to_their_chord() {
    let reversed = BezierPath {
        anchors: vec![
            Anchor {
                outgoing: Some([100., 0.]),
                ..Anchor::corner([0., 0.])
            },
            Anchor {
                incoming: Some([-100., 0.]),
                ..Anchor::corner([1., 0.])
            },
        ],
        closure: Closure::Open,
    };
    let points = reversed.flatten(FlattenOptions::default()).unwrap();
    assert!(points.iter().any(|point| point[0] > 20.));
    assert!(points.iter().any(|point| point[0] < -20.));
    let looped = BezierPath {
        anchors: vec![
            Anchor {
                outgoing: Some([100., 100.]),
                ..Anchor::corner([0., 0.])
            },
            Anchor {
                incoming: Some([-100., 100.]),
                ..Anchor::corner([0., 0.])
            },
        ],
        closure: Closure::Open,
    };
    let points = looped.flatten(FlattenOptions::default()).unwrap();
    assert!(points.len() > 3);
    assert!(points.iter().any(|point| point[1] > 70.));
}

#[test]
fn segment_budget_is_global_and_overflow_returns_no_partial_result() {
    let path = polygon(&[[0., 0.], [1., 0.], [2., 0.]], Closure::Open);
    assert!(
        path.flatten(FlattenOptions {
            max_segments: 1,
            ..Default::default()
        })
        .is_err()
    );
    assert_eq!(
        path.flatten(FlattenOptions {
            max_segments: 2,
            ..Default::default()
        })
        .unwrap()
        .len(),
        3
    );
    assert!(
        arch()
            .flatten(FlattenOptions {
                max_segments: 2,
                ..Default::default()
            })
            .is_err()
    );
    assert_eq!(path.anchors.len(), 3);
}

#[test]
fn invalid_inputs_and_options_are_rejected() {
    for coordinate in [f64::NAN, f64::INFINITY, 1_000_001.] {
        let path = polygon(&[[coordinate, 0.]], Closure::Open);
        assert!(path.validate().is_err());
        assert!(path.bounds().is_err());
        assert!(path.flatten(FlattenOptions::default()).is_err());
    }
    let mut path = arch();
    path.anchors[0].incoming = Some([0., f64::NAN]);
    assert!(path.validate().is_err());
    for tolerance in [0., -1., f64::NAN, f64::INFINITY, 1001.] {
        assert!(
            arch()
                .flatten(FlattenOptions {
                    tolerance,
                    ..Default::default()
                })
                .is_err()
        );
    }
    for max_segments in [0, MAX_FLATTENED_SEGMENTS + 1] {
        assert!(
            arch()
                .flatten(FlattenOptions {
                    max_segments,
                    ..Default::default()
                })
                .is_err()
        );
    }
    assert!(polygon(&[[0., 0.]], Closure::Closed).validate().is_err());
    assert!(
        polygon(&vec![[0., 0.]; MAX_ANCHORS + 1], Closure::Open)
            .validate()
            .is_err()
    );
}

#[test]
fn handles_take_hit_priority_and_the_closest_handle_wins() {
    let path = BezierPath {
        anchors: vec![
            Anchor {
                incoming: Some([1., 0.]),
                outgoing: Some([3., 0.]),
                ..Anchor::corner([0., 0.])
            },
            Anchor::corner([20., 0.]),
        ],
        closure: Closure::Open,
    };
    assert_eq!(path.hit([0., 0.], 5.).unwrap(), Some(Hit::Incoming(0)));
    assert_eq!(path.hit([2.8, 0.], 5.).unwrap(), Some(Hit::Outgoing(0)));
    assert_eq!(path.hit([19.5, 0.], 1.).unwrap(), Some(Hit::Anchor(1)));
    assert_eq!(path.hit([40., 0.], 1.).unwrap(), None);
    assert_eq!(path.hit([20., 0.], 0.).unwrap(), Some(Hit::Anchor(1)));
    assert!(path.hit([0., 0.], -1.).is_err());
}

#[test]
fn mapping_moves_handles_and_roundtrips_rotated_flipped_layer_coordinates() {
    let original = arch();
    let transform = crate::geometry::Transform {
        origin: [50., 80.],
        rotation: 37.,
        flip_x: true,
        flip_y: true,
        ..crate::geometry::Transform::new(300, 150)
    };
    let placed = original
        .mapped(|point| transform.point([point[0] / 100., point[1] / 100.]))
        .unwrap();
    let local = placed
        .mapped(|point| transform.unit(point).map(|value| value * 100.))
        .unwrap();
    for (a, b) in original.anchors.iter().zip(local.anchors) {
        for (a, b) in [Some(a.point), a.incoming, a.outgoing]
            .into_iter()
            .flatten()
            .zip(
                [Some(b.point), b.incoming, b.outgoing]
                    .into_iter()
                    .flatten(),
            )
        {
            assert!(distance_squared(a, b) < 1e-20);
        }
    }
    assert!(original.mapped(|_| [f64::NAN, 0.]).is_err());
    assert_eq!(original, arch());
}

#[test]
fn open_curves_make_closed_selections_without_mutating_the_path() {
    let path = arch()
        .mapped(|point| [point[0] + 10., point[1] + 10.])
        .unwrap();
    let selection = path.selection(140, 120, true).unwrap();
    assert_eq!(selection.coverage([60., 40.]), 1.);
    assert_eq!(selection.coverage([60., 100.]), 0.);
    assert_eq!(path.closure, Closure::Open);
    assert!(
        polygon(&[[1., 1.], [5., 5.]], Closure::Open)
            .selection(10, 10, true)
            .is_err()
    );
}

#[test]
fn full_large_canvas_selection_remains_sparse() {
    let path = polygon(
        &[[0., 0.], [30_000., 0.], [30_000., 30_000.], [0., 30_000.]],
        Closure::Closed,
    );
    let selection = path.selection(30_000, 30_000, true).unwrap();
    assert!(selection.pixels.dense().is_none());
    assert_eq!(selection.coverage([15_000., 15_000.]), 1.);
    assert_eq!(selection.coverage([30_001., 15_000.]), 0.);
}

#[test]
fn serialized_geometry_roundtrips_and_unknown_structure_is_rejected() {
    let path = arch();
    let bytes = serde_json::to_vec(&path).unwrap();
    let parsed: BezierPath = serde_json::from_slice(&bytes).unwrap();
    parsed.validate().unwrap();
    assert_eq!(parsed, path);
    assert!(
        serde_json::from_str::<BezierPath>(r#"{"anchors": [], "closure":"closed", "extra":true}"#)
            .is_err()
    );
}

#[test]
fn saved_path_identity_names_and_collection_budgets_are_validated() {
    let original = SavedPath::new("Outline", arch()).unwrap();
    validate(std::slice::from_ref(&original)).unwrap();
    assert!(validate(&[original.clone(), original.clone()]).is_err());
    for name in ["", "  ", "bad\nname", &"x".repeat(257)] {
        assert!(SavedPath::new(name, arch()).is_err());
    }
    let mut invalid_id = original.clone();
    invalid_id.id = uuid::Uuid::nil();
    assert!(invalid_id.validate().is_err());
    let many = (0..=MAX_PATHS)
        .map(|_| SavedPath::new("Path", BezierPath::default()).unwrap())
        .collect::<Vec<_>>();
    assert!(validate(&many).is_err());
    let dense = BezierPath {
        anchors: vec![Anchor::corner([0., 0.]); MAX_ANCHORS],
        closure: Closure::Open,
    };
    let many = (0..=MAX_TOTAL_ANCHORS / MAX_ANCHORS)
        .map(|_| SavedPath::new("Path", dense.clone()).unwrap())
        .collect::<Vec<_>>();
    assert!(validate(&many).is_err());
}

#[test]
fn canvas_operations_map_anchors_and_handles_with_the_image() {
    use crate::{
        canvas_rotation::{self, QuarterTurn},
        document::Document,
        edits,
        geometry::Sampling,
        image_resize,
    };
    let mut document = Document::new(100, 80).unwrap();
    document
        .paths
        .push(SavedPath::new("Curve", arch()).unwrap());
    let original = document.paths.clone();
    edits::canvas_size(&mut document, 120, 100, [0.5, 0.5]).unwrap();
    assert_eq!(
        document.paths,
        mapped(&original, |[x, y]| [x + 10., y + 10.]).unwrap()
    );
    edits::crop(&mut document, [10., 10.], [110., 90.]).unwrap();
    assert_eq!(document.paths, original);
    image_resize::resize(&mut document, 200, 240, 72., Sampling::Nearest).unwrap();
    assert_eq!(
        document.paths,
        mapped(&original, |[x, y]| [x * 2., y * 3.]).unwrap()
    );
    let scaled = document.paths.clone();
    canvas_rotation::rotate(&mut document, QuarterTurn::Clockwise).unwrap();
    assert_eq!(
        document.paths,
        mapped(&scaled, |[x, y]| [240. - y, x]).unwrap()
    );
    canvas_rotation::rotate(&mut document, QuarterTurn::CounterClockwise).unwrap();
    assert_eq!(document.paths, scaled);
    edits::flip_canvas(&mut document, true).unwrap();
    assert_eq!(
        document.paths,
        mapped(&scaled, |[x, y]| [200. - x, y]).unwrap()
    );
    edits::flip_canvas(&mut document, true).unwrap();
    edits::flip_canvas(&mut document, false).unwrap();
    assert_eq!(
        document.paths,
        mapped(&scaled, |[x, y]| [x, 240. - y]).unwrap()
    );
}

#[test]
fn overflowing_canvas_operations_preserve_document_and_paths() {
    use crate::{
        canvas_rotation::{self, QuarterTurn},
        document::Document,
        edits,
        geometry::Sampling,
        image_resize,
    };
    let mut document = Document::new(10, 10).unwrap();
    document.paths.push(
        SavedPath::new(
            "Far away",
            polygon(&[[-1_000_000., -1_000_000.]], Closure::Open),
        )
        .unwrap(),
    );
    let original = document.clone();
    assert!(edits::flip_canvas(&mut document, true).is_err());
    assert_eq!(document, original);
    assert!(edits::crop(&mut document, [1., 1.], [9., 9.]).is_err());
    assert_eq!(document, original);
    assert!(edits::canvas_size(&mut document, 8, 8, [1., 1.]).is_err());
    assert_eq!(document, original);
    assert!(canvas_rotation::rotate(&mut document, QuarterTurn::Clockwise).is_err());
    assert_eq!(document, original);
    assert!(image_resize::resize(&mut document, 20, 20, 72., Sampling::Nearest).is_err());
    assert_eq!(document, original);
}
