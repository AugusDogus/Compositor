//! Bound rendering independently from the document-space geometry budget.
use super::*;
use compositor::{
    geometry::Point,
    invalid,
    vector_path::{BezierPath, FlattenOptions},
};

fn prepare(
    geometry: &BezierPath,
    selected: Option<usize>,
    zoom: f64,
    offset: Point,
    size: [f32; 2],
) -> Result<Vec<quickgui::Path>> {
    let project = |p: Point| [p[0] * zoom + offset[0], p[1] * zoom + offset[1]];
    let points = geometry.flatten(FlattenOptions {
        tolerance: (0.25 / zoom).clamp(0.0001, 1000.),
        ..Default::default()
    })?;
    let mut segments: Vec<_> = points
        .windows(2)
        .map(|p| [project(p[0]), project(p[1])])
        .collect();
    if let Some(anchor) = selected.and_then(|i| geometry.anchors.get(i)) {
        for handle in [anchor.incoming, anchor.outgoing].into_iter().flatten() {
            segments.push([project(anchor.point), project(handle)]);
        }
    }
    for (i, anchor) in geometry.anchors.iter().enumerate() {
        let mut square = |point: Point, radius: f64| {
            let [x, y] = project(point);
            let corners = [
                [x - radius, y - radius],
                [x + radius, y - radius],
                [x + radius, y + radius],
                [x - radius, y + radius],
            ];
            for j in 0..4 {
                segments.push([corners[j], corners[(j + 1) % 4]]);
            }
        };
        square(anchor.point, if Some(i) == selected { 4. } else { 3. });
        if Some(i) == selected {
            for handle in [anchor.incoming, anchor.outgoing].into_iter().flatten() {
                square(handle, 2.5);
            }
        }
    }
    // Two commands per segment. Stay well below QuickGUI's 65536-command cap.
    segments.chunks(8192).map(|chunk| {
        let mut path = quickgui::PathBuilder::stroke(1.5);
        for &[a,b] in chunk {
            if let Some((start,end)) = super::selection_paths::clip_segment(a,b,size) {
                let at = |t: f64| quickgui::Point::new((a[0]+(b[0]-a[0])*t) as f32,(a[1]+(b[1]-a[1])*t) as f32);
                path.move_to(at(start)); path.line_to(at(end));
            }
        }
        path.build().map_err(|error| invalid(format!("The path outline could not be drawn: {error}. The saved geometry is intact; zoom out and retry.")))
    }).collect()
}
impl Editor {
    pub(super) fn path_overlay(&self, zoom: f64, offset: Point, size: [f32; 2]) -> Result<Element> {
        if self.tools.tool != Tool::Pen {
            return Ok(div());
        }
        let Some(path) = self.path_snapshot()? else {
            return Ok(div());
        };
        let selected = self.tools.paths.active.and_then(|a| a.selected);
        let paths = prepare(&path.geometry, selected, zoom, offset, size)?;
        let color = self.colors.accent();
        Ok(quickgui::canvas(move |_, painter| {
            for path in &paths {
                painter.paint_path(path, color);
            }
        })
        .absolute()
        .size_full()
        .accessibility_hidden(true)
        .into_element())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use compositor::vector_path::Anchor;
    #[test]
    fn many_curves_and_distant_handles_stay_within_render_budgets() {
        let geometry = BezierPath {
            anchors: (0..1800)
                .map(|i| Anchor {
                    point: [f64::from(i % 100) * 10., f64::from(i / 100) * 10.],
                    incoming: Some([0., 1000.]),
                    outgoing: Some([1000., 0.]),
                })
                .collect(),
            ..Default::default()
        };
        // Use a less extreme path for the geometry flattening budget while
        // still exceeding the renderer's single-path command budget.
        let geometry = geometry.mapped(|p| [p[0] * 0.1, p[1] * 0.1]).unwrap();
        let paths = prepare(&geometry, Some(0), 1., [0., 0.], [1000., 1000.]).unwrap();
        assert!(paths.len() > 1);
        assert!(paths.iter().any(|p| !p.is_empty()));
        assert!(
            prepare(
                &geometry,
                None,
                1.,
                [2_000_000., 2_000_000.],
                [1000., 1000.]
            )
            .unwrap()
            .iter()
            .all(|p| p.is_empty())
        );
    }
}
