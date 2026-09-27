use super::*;

/// Replay authored segments exactly, without the mouse stroke's Catmull-Rom
/// interpolation. All segments share one coverage plane and opacity budget.
/// Failure leaves the document unchanged.
pub fn paint_polyline(
    document: &mut Document,
    points: &[Point],
    brush: Brush,
    shape: sampled::Shape,
    mask: bool,
) -> Result<()> {
    if !(2..=crate::vector_path::MAX_FLATTENED_SEGMENTS + 1).contains(&points.len()) {
        return Err(invalid(
            "A brush path needs 2 to 262145 points. Add an anchor or simplify the path; the image is unchanged.",
        ));
    }
    for point in points {
        validate_point(*point)?;
    }
    let mut next = document.clone();
    let mut stroke = Stroke::start_shaped_input(
        &mut next,
        Input {
            point: points[0],
            tip: None,
        },
        brush,
        PaintMode::Paint,
        mask,
        false,
        shape,
    )?;
    for point in &points[1..] {
        stroke.walk(&mut next, *point)?;
    }
    // No samples are appended, so finish cannot generate an interpolated tail.
    next.validate()?;
    stroke.finish(&mut next)?;
    *document = next;
    Ok(())
}
