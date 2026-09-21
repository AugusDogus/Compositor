use super::*;

fn view() -> Viewport {
    Viewport {
        size: [100., 100.],
        zoom: 1.,
        offset: [0., 0.],
    }
}

#[test]
fn dashes_follow_euclidean_distance_and_continue_around_corners() {
    let diagonal = Polyline {
        points: vec![[0., 0.], [30., 40.]],
        distance: 0.,
    };
    let ink = dashed(&[diagonal], 0).unwrap();
    assert_eq!(ink.len(), 7);
    for part in &ink[..6] {
        assert!((part.length() - 4.).abs() < 0.0001);
    }
    assert!((ink[0].points[1][0] - 2.4).abs() < 0.0001);
    assert!((ink[0].points[1][1] - 3.2).abs() < 0.0001);
    let corner = Polyline {
        points: vec![[0., 0.], [2., 0.], [2., 10.]],
        distance: 0.,
    };
    let ink = dashed(&[corner], 0).unwrap();
    assert_eq!(ink[0].points, vec![[0., 0.], [2., 0.], [2., 2.]]);
    assert_eq!(ink[1].points, vec![[2., 6.], [2., 10.]]);
}

#[test]
fn clipped_paths_keep_phase_while_panning_and_do_not_dash_offscreen_millions() {
    let points = [
        [-1_000_000., 20.],
        [1_000_000., 20.],
        [1_000_000., 200.],
        [-1_000_000., 200.],
    ];
    for pan in [0., 3.] {
        let parts = visible_parts(
            &points,
            [0., 0.],
            Viewport {
                offset: [pan, 0.],
                ..view()
            },
        );
        assert_eq!(parts.len(), 1);
        for (actual, expected) in parts[0].points.iter().zip([[-6., 20.], [106., 20.]]) {
            assert!(distance(*actual, expected) < 0.00001);
        }
        assert!((parts[0].distance - (999_994. - pan)).abs() < 0.00001);
        let ink = dashed(&parts, 0).unwrap();
        assert!(ink.len() <= 15);
        assert!(
            ink.iter()
                .any(|p| p.points == vec![[pan, 20.], [pan + 4., 20.]])
        );
        assert!(!gpu_paths(&ink).unwrap().is_empty());
    }
}

#[test]
fn sparse_large_ellipse_uses_contours_at_small_and_extreme_zoom() {
    let selection =
        Selection::marquee(30_000, 30_000, [0., 0.], [30_000., 30_000.], true, true).unwrap();
    assert!(selection.pixels.dense().is_none());
    for (zoom, offset) in [(0.02, [10., 10.]), (64., [-960_000., 10.])] {
        let mut outline = OutlinePaths::new(
            &selection,
            Viewport {
                size: [620., 620.],
                zoom,
                offset,
            },
        )
        .unwrap()
        .unwrap();
        for paths in [outline.white.clone(), outline.black(0).unwrap()] {
            assert!(!paths.is_empty());
            for path in paths.iter() {
                let b = path.bounds();
                assert!(b.x >= -7. && b.y >= -7.);
                assert!(b.x + b.width <= 627. && b.y + b.height <= 627.);
            }
        }
    }
}

#[test]
fn displaced_geometry_is_clipped_to_the_viewport_not_the_document() {
    let selection = Selection::rectangle(4, 4, [0., 0.], [4., 4.], false)
        .translated([-4., 0.])
        .unwrap();
    let viewport = Viewport {
        size: [30., 30.],
        zoom: 2.,
        offset: [12., 8.],
    };
    let outline = OutlinePaths::new(&selection, viewport).unwrap().unwrap();
    let bounds = outline.white[0].bounds();
    assert_eq!(
        [bounds.x, bounds.y, bounds.width, bounds.height],
        [3.5, 7.5, 9., 9.]
    );
    assert!(
        OutlinePaths::new(
            &selection,
            Viewport {
                offset: [-20., 8.],
                ..viewport
            }
        )
        .unwrap()
        .is_none()
    );
    let clipped = OutlinePaths::new(
        &selection,
        Viewport {
            size: [6., 10.],
            ..viewport
        },
    )
    .unwrap()
    .unwrap();
    assert!(!clipped.white.is_empty());
}

#[test]
fn closed_seams_remain_joined_and_short_gap_pieces_have_no_ink() {
    let closed = Polyline {
        points: vec![[0., 0.], [3., 0.], [3., 3.], [0., 3.], [0., 0.]],
        distance: 0.,
    };
    let ink = dashed(&[closed], 0).unwrap();
    assert!(ink.iter().any(|p| {
        p.points
            .windows(3)
            .any(|p| p == [[0., 3.], [0., 0.], [3., 0.]])
    }));
    let gap = Polyline {
        points: vec![[0., 0.], [1., 0.]],
        distance: 5.,
    };
    assert!(dashed(&[gap], 0).unwrap().is_empty());
    let mut parts = vec![
        Polyline {
            points: vec![[0., 0.], [3., 0.]],
            distance: 0.,
        },
        Polyline {
            points: vec![[0., 3.], [0., 0.]],
            distance: 9.,
        },
    ];
    join_seam(&mut parts);
    assert_eq!(parts[0].points, vec![[0., 3.], [0., 0.], [3., 0.]]);
}

#[test]
fn raster_mask_outlines_trace_holes_at_half_coverage() {
    let mask = image::GrayImage::from_fn(10, 10, |x, y| {
        image::Luma([if (3..7).contains(&x) && (3..7).contains(&y) {
            127
        } else {
            128
        }])
    });
    let selection = Selection::from_mask(mask);
    let mut outline = OutlinePaths::new(&selection, view()).unwrap().unwrap();
    assert_eq!(
        outline.contours.len(),
        2,
        "The unselected hole needs its own boundary"
    );
    assert!(!outline.black(0).unwrap().is_empty());
    assert!(
        OutlinePaths::new(&Selection::from_mask(image::GrayImage::new(10, 10)), view())
            .unwrap()
            .is_none()
    );
}
