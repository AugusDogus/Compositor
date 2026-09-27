use super::*;

const CORNERS: [Point; 4] = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]];

fn close(actual: Point, expected: Point) {
    for axis in 0..2 {
        assert!(
            (actual[axis] - expected[axis]).abs() < 1e-8,
            "{actual:?} != {expected:?}"
        );
    }
}

fn warp() -> Projective {
    Projective::new([[0.1, 0.2], [1.3, -0.1], [0.9, 1.2], [-0.2, 0.8]]).unwrap()
}

#[test]
fn maps_corners_and_recovers_interior_and_exterior_points() {
    let corners = [[10., 20.], [150., 10.], [130., 100.], [30., 120.]];
    let warp = Projective::new(corners).unwrap();
    for (point, expected) in CORNERS.into_iter().zip(corners) {
        close(warp.apply(point).unwrap(), expected);
    }
    for point in [[0., 0.], [0.25, 0.7], [1., 1.], [-0.1, 1.1]] {
        close(
            warp.inverse()
                .unwrap()
                .apply(warp.apply(point).unwrap())
                .unwrap(),
            point,
        );
    }
}

#[test]
fn preserves_affine_shear_and_reflections() {
    for corners in [
        [[0., 0.], [2., 0.], [3., 1.], [1., 1.]],
        [[1., 0.], [0., 0.], [-0.5, 1.], [0.5, 1.]],
    ] {
        let warp = Projective::new(corners).unwrap();
        for (point, expected) in CORNERS.into_iter().zip(corners) {
            close(warp.apply(point).unwrap(), expected);
        }
        assert_eq!(warp.mapping().matrix()[6], 0.);
        assert_eq!(warp.mapping().matrix()[7], 0.);
    }
}

#[test]
fn rejects_crossed_concave_collapsed_nonfinite_and_excessive_quads() {
    for corners in [
        [[0., 0.], [1., 1.], [1., 0.], [0., 1.]],
        [[0., 0.], [1., 0.], [0.2, 0.2], [0., 1.]],
        [[0., 0.], [1., 0.], [2., 0.], [0., 1.]],
        [[0., 0.], [f64::NAN, 0.], [1., 1.], [0., 1.]],
        [[0., 0.], [1_000_001., 0.], [1., 1.], [0., 1.]],
    ] {
        assert!(Projective::new(corners).is_err());
    }
}

#[test]
fn inverse_need_not_be_valid_on_unit_square() {
    let warp = Projective::new([[0., 0.], [0.25, 0.], [0.25, 0.25], [0., 1.]]).unwrap();
    let inverse = warp.inverse().unwrap();
    assert_eq!(inverse.map_rectangle([0., 0., 1., 1.]), Err(Error::Horizon));
    close(
        inverse.apply(warp.apply([0.3, 0.7]).unwrap()).unwrap(),
        [0.3, 0.7],
    );
}

#[test]
fn rejects_horizon_inside_rectangle_even_when_all_corners_are_finite() {
    let mapping = Homography::from_matrix([1., 0., 0., 0., 1., 0., 2., 0., -1.]).unwrap();
    for point in CORNERS {
        assert!(mapping.apply(point).is_ok());
    }
    assert_eq!(mapping.map_rectangle([0., 0., 1., 1.]), Err(Error::Horizon));
    assert_eq!(mapping.apply([0.5, 0.3]), Err(Error::Horizon));
    assert!(Projective::from_mapping(mapping).is_err());
}

#[test]
fn following_composes_all_three_mappings_without_losing_perspective() {
    let old = warp().mapping();
    let new = Projective::new([[1., 1.], [3., 0.], [2., 4.], [0., 2.]])
        .unwrap()
        .mapping();
    let current = Homography::from_matrix([0.6, 0.2, 0.1, -0.1, 0.7, 0.2, 0.1, 0.2, 1.]).unwrap();
    let following = new
        .compose(old.inverse().unwrap())
        .unwrap()
        .compose(current)
        .unwrap();
    for point in [[0., 0.], [1., 1.], [0.13, 0.78], [-0.1, 0.4]] {
        let expected = new
            .apply(
                old.inverse()
                    .unwrap()
                    .apply(current.apply(point).unwrap())
                    .unwrap(),
            )
            .unwrap();
        close(following.apply(point).unwrap(), expected);
    }
}

#[test]
fn padding_and_cropping_preserve_old_source_positions() {
    for (origin, size) in [([-0.1, -0.2], [1.2, 1.4]), ([0.2, 0.1], [0.6, 0.7])] {
        let original = warp();
        let rebound = original.rebind(origin, size).unwrap();
        for point in [[0., 0.], [0.3, 0.4], [1., 1.]] {
            close(
                rebound.apply(point).unwrap(),
                original
                    .apply([
                        origin[0] + point[0] * size[0],
                        origin[1] + point[1] * size[1],
                    ])
                    .unwrap(),
            );
        }
    }
}

#[test]
fn source_expansion_rejects_horizon_and_invalid_dimensions() {
    let warp = Projective::new([[0., 0.], [0.25, 0.], [0.25, 0.25], [0., 1.]]).unwrap();
    assert_eq!(warp.rebind([-1., 0.], [2., 1.]), Err(Error::Horizon));
    for size in [[0., 1.], [-1., 1.], [1., f64::INFINITY], [f64::NAN, 1.]] {
        assert!(warp.rebind([0., 0.], size).is_err());
    }
}

#[test]
fn rectangle_corner_bounds_contain_all_interior_samples() {
    let mapping = warp().mapping();
    let corners = mapping.map_rectangle([-0.1, -0.1, 1.1, 1.1]).unwrap();
    for x in 0..=24 {
        for y in 0..=24 {
            let mapped = mapping
                .apply([-0.1 + x as f64 * 0.05, -0.1 + y as f64 * 0.05])
                .unwrap();
            for axis in 0..2 {
                let minimum = corners
                    .iter()
                    .map(|point| point[axis])
                    .fold(f64::INFINITY, f64::min);
                let maximum = corners
                    .iter()
                    .map(|point| point[axis])
                    .fold(f64::NEG_INFINITY, f64::max);
                assert!((minimum - 1e-12..=maximum + 1e-12).contains(&mapped[axis]));
            }
        }
    }
}

#[test]
fn matrix_normalization_preserves_mapping_at_extreme_common_scales() {
    let expected = warp().mapping();
    for scale in [1e-200, -1e-200, 1e200, -1e200] {
        let mapping =
            Homography::from_matrix(expected.matrix().map(|value| value * scale)).unwrap();
        close(
            mapping.apply([0.37, 0.62]).unwrap(),
            expected.apply([0.37, 0.62]).unwrap(),
        );
    }
    assert!(Homography::from_matrix([0.; 9]).is_err());
    assert!(Homography::from_matrix([1.; 9]).is_err());
    assert!(Homography::from_matrix([f64::INFINITY; 9]).is_err());
}

#[test]
fn serde_preserves_matrix_and_rejects_invalid_placements() {
    let original = warp();
    let encoded = serde_json::to_string(&original).unwrap();
    let decoded: Projective = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, original);
    for invalid in [
        "[0,0,0,0,0,0,0,0,0]",
        "[1,0,0,0,1,0,2,0,-1]",
        "[1,0,2000000,0,1,0,0,0,1]",
    ] {
        assert!(serde_json::from_str::<Projective>(invalid).is_err());
    }
}

#[test]
fn tiny_valid_normalized_geometry_does_not_use_pixel_sized_threshold() {
    let corners = [[0., 0.], [1e-6, 0.], [0.8e-6, 1e-6], [0., 1e-6]];
    let mapping = Projective::new(corners).unwrap();
    for (point, expected) in CORNERS.into_iter().zip(corners) {
        close(mapping.apply(point).unwrap(), expected);
    }
}
