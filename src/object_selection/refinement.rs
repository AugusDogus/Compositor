//! Match the object tool's binary edge adjustment and closed-contour smoothing.
use crate::{
    Result, geometry::Point, invalid, selection::Selection, selection_geometry::SelectionGeometry,
};
use image::{GrayImage, Luma};

pub(super) fn selection(mask: &GrayImage, edge: i8, smooth: bool) -> Result<Option<Selection>> {
    let mut mask = GrayImage::from_fn(mask.width(), mask.height(), |x, y| {
        Luma([if mask[(x, y)][0] >= 128 { 255 } else { 0 }])
    });
    if edge != 0 {
        // A radius-r square is equivalent to r repeated 3x3 operations. Two
        // sliding-count passes keep refinement linear even at the 10 px limit.
        let radius = u32::from(edge.unsigned_abs());
        mask = binary_axis(&mask, radius, false, edge > 0);
        mask = binary_axis(&mask, radius, true, edge > 0);
    }
    let mut geometry = SelectionGeometry::trace(&mask)?;
    if geometry.contours.is_empty() {
        return Ok(None);
    }
    geometry.antialiased = smooth;
    if smooth {
        let mut remaining_points = 1_000_000;
        for contour in &mut geometry.contours {
            *contour = smooth_contour(contour, &mut remaining_points)?;
        }
    }
    Selection::from_geometry(
        geometry,
        [0., 0., f64::from(mask.width()), f64::from(mask.height())],
    )
    .map(Some)
}

/// Clipped windows match upstream: pixels beyond the canvas do not erode edges.
fn binary_axis(mask: &GrayImage, radius: u32, vertical: bool, erode: bool) -> GrayImage {
    let (length, lanes) = if vertical {
        (mask.height(), mask.width())
    } else {
        (mask.width(), mask.height())
    };
    let mut result = GrayImage::new(mask.width(), mask.height());
    for lane in 0..lanes {
        let position = |offset| {
            if vertical {
                (lane, offset)
            } else {
                (offset, lane)
            }
        };
        let mut selected = (0..=radius.min(length - 1))
            .filter(|&offset| mask[position(offset)][0] != 0)
            .count() as u32;
        for offset in 0..length {
            let start = offset.saturating_sub(radius);
            let end = (offset + radius).min(length - 1);
            let inside = if erode {
                selected == end - start + 1
            } else {
                selected > 0
            };
            result[position(offset)] = Luma([if inside { 255 } else { 0 }]);
            if offset >= radius {
                selected -= u32::from(mask[position(offset - radius)][0] != 0);
            }
            if offset + radius + 1 < length {
                selected += u32::from(mask[position(offset + radius + 1)][0] != 0);
            }
        }
    }
    result
}

fn smooth_contour(input: &[Point], remaining_points: &mut usize) -> Result<Vec<Point>> {
    let mut points = simplify_closed(input);
    if points.len() < 3 {
        return Ok(points);
    }
    let output_points = points.len().checked_mul(8).filter(|count| *count <= *remaining_points)
        .ok_or_else(|| invalid("This object outline is too complex to smooth. Turn off Anti-alias or select a smaller region. The current selection is unchanged."))?;
    *remaining_points -= output_points;
    for _ in 0..3 {
        let mut next = Vec::with_capacity(points.len() * 2);
        for (index, a) in points.iter().enumerate() {
            let b = points[(index + 1) % points.len()];
            next.push([a[0] * 0.75 + b[0] * 0.25, a[1] * 0.75 + b[1] * 0.25]);
            next.push([a[0] * 0.25 + b[0] * 0.75, a[1] * 0.25 + b[1] * 0.75]);
        }
        points = next;
    }
    Ok(points)
}

fn simplify_closed(input: &[Point]) -> Vec<Point> {
    let input = if input.len() > 1 && input.first() == input.last() {
        &input[..input.len() - 1]
    } else {
        input
    };
    if input.len() < 4 {
        return input.to_vec();
    }
    let start = (0..input.len())
        .min_by(|&a, &b| {
            input[a][0]
                .total_cmp(&input[b][0])
                .then(input[a][1].total_cmp(&input[b][1]))
        })
        .unwrap_or(0);
    let mut rotated: Vec<_> = input[start..]
        .iter()
        .chain(&input[..start])
        .copied()
        .collect();
    rotated.push(rotated[0]);
    // Iterative RDP avoids stack overflow on long, intricate model boundaries.
    let mut keep = vec![false; rotated.len()];
    keep[0] = true;
    keep[rotated.len() - 1] = true;
    let mut pending = vec![(0, rotated.len() - 1)];
    while let Some((first, last)) = pending.pop() {
        let mut greatest = 1.6;
        let mut farthest = None;
        for index in first + 1..last {
            let distance = line_distance(rotated[index], rotated[first], rotated[last]);
            if distance > greatest {
                greatest = distance;
                farthest = Some(index);
            }
        }
        if let Some(index) = farthest {
            keep[index] = true;
            pending.push((first, index));
            pending.push((index, last));
        }
    }
    let result: Vec<_> = rotated[..rotated.len() - 1]
        .iter()
        .zip(keep)
        .filter_map(|(p, keep)| keep.then_some(*p))
        .collect();
    if result.len() >= 3 {
        result
    } else {
        input.to_vec()
    }
}

fn line_distance(point: Point, a: Point, b: Point) -> f64 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let length = dx.hypot(dy);
    if length == 0. {
        (point[0] - a[0]).hypot(point[1] - a[1])
    } else {
        (dy * point[0] - dx * point[1] + b[0] * a[1] - b[1] * a[0]).abs() / length
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sliding_edge_filter_matches_repeated_square_morphology() {
        let original = GrayImage::from_fn(19, 13, |x, y| {
            Luma([
                if (3..16).contains(&x) && (2..11).contains(&y) && (x * 7 + y * 3) % 29 != 0 {
                    255
                } else {
                    0
                },
            ])
        });
        for erode in [true, false] {
            let mut repeated = original.clone();
            for radius in 1..=10 {
                repeated = GrayImage::from_fn(original.width(), original.height(), |x, y| {
                    let mut value = if erode { 255 } else { 0 };
                    for ny in y.saturating_sub(1)..=(y + 1).min(original.height() - 1) {
                        for nx in x.saturating_sub(1)..=(x + 1).min(original.width() - 1) {
                            value = if erode {
                                value.min(repeated[(nx, ny)][0])
                            } else {
                                value.max(repeated[(nx, ny)][0])
                            };
                        }
                    }
                    Luma([value])
                });
                let horizontal = binary_axis(&original, radius, false, erode);
                assert_eq!(
                    binary_axis(&horizontal, radius, true, erode),
                    repeated,
                    "radius {radius}, erode {erode}"
                );
            }
        }
    }
    #[test]
    fn smoothing_checks_output_budget_before_expanding_contours() {
        let contour = [[0., 0.], [100., 0.], [100., 100.], [0., 100.]];
        let mut budget = 31;
        assert!(smooth_contour(&contour, &mut budget).is_err());
        assert_eq!(budget, 31);
        budget = 32;
        assert_eq!(smooth_contour(&contour, &mut budget).unwrap().len(), 32);
        assert_eq!(budget, 0);
    }
}
