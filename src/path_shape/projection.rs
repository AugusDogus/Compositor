//! Flatten source cubics under a homography, retaining rational-curve geometry.
use crate::{
    Result,
    geometry::{Point, Transform},
    invalid,
    vector_path::{BezierPath, FlattenOptions, MAX_FLATTENED_SEGMENTS},
};
use kurbo::{CubicBez, ParamCurve};

/// Positive homogeneous weights keep a rational curve inside its projected
/// control hull. Subdivide source curves until that hull fits the output chord.
pub fn projected_points(
    path: &BezierPath,
    size: [u32; 2],
    transform: Transform,
    options: FlattenOptions,
) -> Result<Vec<Point>> {
    path.validate()?;
    if size.contains(&0)
        || !options.tolerance.is_finite()
        || !(0.0001..=1000.).contains(&options.tolerance)
        || !(1..=MAX_FLATTENED_SEGMENTS).contains(&options.max_segments)
    {
        return Err(invalid(
            "Perspective path rendering needs valid source dimensions, tolerance and segment budget.",
        ));
    }
    let mapping = transform.mapping().map_err(|e| invalid(e.to_string()))?;
    let unit = |p: Point| [p[0] / f64::from(size[0]), p[1] / f64::from(size[1])];
    let project = |p: Point| mapping.apply(unit(p)).map_err(|e| invalid(e.to_string()));
    let Some(first) = path.anchors.first() else {
        return Ok(Vec::new());
    };
    let mut output = vec![project(first.point)?];
    let matrix = mapping.matrix();
    for segment in path.segments() {
        let points = segment.map(|p| kurbo::Point::new(p[0], p[1]));
        let mut stack = vec![(
            CubicBez::new(points[0], points[1], points[2], points[3]),
            0_u8,
        )];
        while let Some((curve, depth)) = stack.pop() {
            let points = [curve.p0, curve.p1, curve.p2, curve.p3].map(|p| [p.x, p.y]);
            let weights = points.map(|p| {
                let p = unit(p);
                matrix[6] * p[0] + matrix[7] * p[1] + matrix[8]
            });
            let same_side = weights.iter().all(|w| *w > 0.) || weights.iter().all(|w| *w < 0.);
            let projected = if same_side {
                Some(
                    points
                        .map(project)
                        .into_iter()
                        .collect::<Result<Vec<_>>>()?,
                )
            } else {
                None
            };
            if let Some(p) = projected.as_ref().filter(|p| {
                distance(p[1], p[0], p[3]).max(distance(p[2], p[0], p[3])) <= options.tolerance
            }) {
                if output.len() > options.max_segments {
                    return Err(invalid(
                        "The perspective path exceeds its outline segment budget. Zoom out or simplify the source path; its geometry is preserved.",
                    ));
                }
                output.push(p[3]);
            } else {
                if depth >= 32 {
                    return Err(invalid(
                        "The perspective path crosses infinity or exceeds its subdivision limit. Reduce the distortion; the source path is preserved.",
                    ));
                }
                let (left, right) = curve.subdivide();
                stack.push((right, depth + 1));
                stack.push((left, depth + 1));
            }
        }
    }
    Ok(output)
}
fn distance(p: Point, a: Point, b: Point) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let length = d[0] * d[0] + d[1] * d[1];
    let t = if length == 0. {
        0.
    } else {
        (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / length).clamp(0., 1.)
    };
    (p[0] - a[0] - t * d[0]).hypot(p[1] - a[1] - t * d[1])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{geometry::projective::Projective, vector_path::Anchor};
    #[test]
    fn perspective_curve_outline_tracks_actual_rational_curve_not_projected_cubic() {
        let path = BezierPath {
            anchors: vec![
                Anchor {
                    point: [0., 0.],
                    incoming: None,
                    outgoing: Some([100., 0.]),
                },
                Anchor {
                    point: [100., 100.],
                    incoming: Some([0., 100.]),
                    outgoing: None,
                },
            ],
            ..Default::default()
        };
        let transform = Transform {
            warp: Some(Projective::new([[0., 0.], [1., 0.], [0.55, 1.], [0.45, 1.]]).unwrap()),
            ..Transform::new(100, 100)
        };
        let points = projected_points(
            &path,
            [100, 100],
            transform,
            FlattenOptions {
                tolerance: 0.02,
                ..Default::default()
            },
        )
        .unwrap();
        let curve = CubicBez::new((0., 0.), (100., 0.), (0., 100.), (100., 100.));
        for i in 0..=1000 {
            let p = curve.eval(f64::from(i) / 1000.);
            let actual = transform.try_point([p.x / 100., p.y / 100.]).unwrap();
            let closest = points
                .windows(2)
                .map(|s| distance(actual, s[0], s[1]))
                .fold(f64::INFINITY, f64::min);
            assert!(closest <= 0.020001, "{closest}");
        }
        let mapped = path
            .mapped(|p| transform.point([p[0] / 100., p[1] / 100.]))
            .unwrap();
        let wrong = mapped.flatten(FlattenOptions::default()).unwrap();
        let middle = curve.eval(0.5);
        let actual = transform.point([middle.x / 100., middle.y / 100.]);
        assert!(
            wrong
                .windows(2)
                .map(|s| distance(actual, s[0], s[1]))
                .fold(f64::INFINITY, f64::min)
                > 1.
        );
    }
    #[test]
    fn projective_outline_enforces_segment_budget() {
        let path = BezierPath {
            anchors: vec![
                Anchor {
                    point: [0., 0.],
                    incoming: None,
                    outgoing: Some([100., 100.]),
                },
                Anchor::corner([100., 0.]),
            ],
            ..Default::default()
        };
        assert!(
            projected_points(
                &path,
                [100, 100],
                Transform::new(100, 100),
                FlattenOptions {
                    tolerance: 0.01,
                    max_segments: 1
                }
            )
            .is_err()
        );
    }
}
