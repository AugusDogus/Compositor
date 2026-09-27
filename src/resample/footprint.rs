use crate::geometry::{Point, Transform, projective::Homography};

/// Whole-image reduction must retain the resolution required by the most
/// magnified part of a projective source. Bound both forward-Jacobian columns
/// over the full source square in output pixel coordinates. The homogeneous
/// numerator derivatives are linear; the denominator has a positive minimum
/// on that square, so this bound also covers interior extrema.
///
/// A strongly varying projection may therefore retain more resolution than a
/// local mip filter would. An unrepresentable bound retains the full source;
/// uncertainty must never discard source detail through an excessive reduction.
pub(crate) fn projective_size(
    transform: Transform,
    source: (u32, u32),
    output_scale: Point,
) -> (u32, u32) {
    let bound = || {
        // Translation does not affect a derivative. Remove it before matrix
        // composition to avoid cancellation for small layers far from zero.
        let centered = Transform {
            origin: [-transform.size[0] * 0.5, -transform.size[1] * 0.5],
            ..transform
        };
        let scale = Homography::from_matrix([
            output_scale[0],
            0.,
            0.,
            0.,
            output_scale[1],
            0.,
            0.,
            0.,
            1.,
        ])?;
        scale
            .compose(centered.mapping()?)?
            .derivative_bounds([0., 0., 1., 1.])
    };
    let Ok(bound) = bound() else {
        return source;
    };
    let dimension = |bound: f64, source: u32| {
        // Matrix normalization can move an exact integer by a few ulps. Keep
        // identity warps on the same reduction grid as their affine placement.
        let rounded = bound.round();
        let pixels = if (bound - rounded).abs() <= 32. * f64::EPSILON * bound.abs().max(1.) {
            rounded
        } else {
            bound.ceil()
        };
        (pixels.max(1.) as u32).min(source)
    };
    (dimension(bound[0], source.0), dimension(bound[1], source.1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::projective::Projective;

    #[test]
    fn projective_prefilter_bound_retains_every_sampled_local_footprint() {
        for warp in [
            [[0., 0.], [1., 0.], [0.99, 1.], [0.01, 1.]],
            [[0.1, 0.], [1.2, 0.2], [0.7, 1.], [0., 0.8]],
            [[0., 0.], [0.4, 0.], [0.4, 0.4], [0., 1.]],
        ] {
            for rotation in [0., 31., 92.] {
                for flips in [[false, false], [true, false], [true, true]] {
                    let transform = Transform {
                        origin: [100_000., -100_000.],
                        rotation,
                        flip_x: flips[0],
                        flip_y: flips[1],
                        warp: Some(Projective::new(warp).unwrap()),
                        ..Transform::new(255, 255)
                    };
                    let scale = [0.07, 0.11];
                    let reduced = projective_size(transform, (4096, 4096), scale);
                    let mapping = transform.mapping().unwrap();
                    for y in 0..=20 {
                        for x in 0..=20 {
                            let derivative = mapping
                                .derivative([f64::from(x) / 20., f64::from(y) / 20.])
                                .unwrap();
                            for (axis, column) in derivative.into_iter().enumerate() {
                                let footprint = (column[0] * scale[0]).hypot(column[1] * scale[1]);
                                let resolution = if axis == 0 { reduced.0 } else { reduced.1 };
                                assert!(
                                    footprint <= f64::from(resolution) + 1e-7,
                                    "{footprint} exceeds {resolution}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
