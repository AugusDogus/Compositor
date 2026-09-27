//! Bound rendering independently from the document-space geometry budget.
use super::*;
use compositor::{
    geometry::Point,
    invalid,
    vector_path::{BezierPath, FlattenOptions},
};

fn prepare(
    geometry: &BezierPath,
    placement: Option<(compositor::geometry::Transform, [u32; 2])>,
    selected: Option<usize>,
    zoom: f64,
    offset: Point,
    size: [f32; 2],
) -> Result<Vec<quickgui::Path>> {
    let project = |p: Point| [p[0] * zoom + offset[0], p[1] * zoom + offset[1]];
    let options = FlattenOptions {
        tolerance: (0.25 / zoom).clamp(0.0001, 1000.),
        ..Default::default()
    };
    let points = if let Some((transform, size)) = placement {
        compositor::path_shape::projected_points(geometry, size, transform, options)?
    } else {
        geometry.flatten(options)?
    };
    let markers = super::path_target::markers(geometry, placement)?;
    let mut segments: Vec<_> = points
        .windows(2)
        .map(|p| [project(p[0]), project(p[1])])
        .collect();
    if let Some(anchor) = selected.and_then(|i| markers.get(i)) {
        for handle in [anchor.incoming, anchor.outgoing].into_iter().flatten() {
            segments.push([project(anchor.point), project(handle)]);
        }
    }
    for (i, anchor) in markers.iter().enumerate() {
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
        let placement = self
            .tools
            .paths
            .active
            .and_then(|a| a.target.placement(&self.session().document));
        let paths = prepare(&path.geometry, placement, selected, zoom, offset, size)?;
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
    fn perspective_handles_on_horizon_keep_outline_and_anchor_selection() {
        use super::super::{
            path_target::Target,
            paths::{Active, Mode},
        };
        use compositor::geometry::projective::Projective;
        let geometry = BezierPath {
            anchors: vec![
                Anchor {
                    point: [0., 0.],
                    incoming: None,
                    outgoing: Some([40., 100.]),
                },
                Anchor {
                    point: [100., 0.],
                    incoming: Some([60., 100.]),
                    outgoing: None,
                },
            ],
            ..Default::default()
        };
        let mut document = Document::new(300, 300).unwrap();
        let id = compositor::path_shape::create(
            &mut document,
            "Curve",
            geometry,
            compositor::path_shape::Style {
                fill: Some([255; 4]),
                stroke: None,
            },
        )
        .unwrap();
        let layer = document
            .layers
            .iter_mut()
            .find(|layer| layer.id == id)
            .unwrap();
        layer.transform.warp =
            Some(Projective::try_from([1., 0., 0., 0., 1., 0., 0., -77. / 101., 1.]).unwrap());
        let transform = layer.transform;
        let source = layer.path_shape().unwrap().source();
        assert_eq!(source.size, [102, 77]);
        assert!(transform.try_point([41. / 102., 101. / 77.]).is_err());
        let geometry = source.geometry.clone();
        let paths = prepare(
            &geometry,
            Some((transform, source.size)),
            Some(0),
            1.,
            [0.; 2],
            [300.; 2],
        )
        .unwrap();
        assert!(paths.iter().any(|path| !path.is_empty()));
        let mut editor = Editor::with_test_document();
        editor.tabs = vec![Session::new(document, None).into()];
        editor.tools.tool = Tool::Pen;
        editor.tools.paths.active = Some(Active {
            target: Target::Shape(id),
            selected: Some(0),
            mode: Mode::Editing,
        });
        for index in 0..2 {
            let point = geometry.anchors[index].point;
            let point = transform.point([point[0] / 102., point[1] / 77.]);
            editor.path_down(point, 1., Modifiers::empty()).unwrap();
            assert_eq!(editor.tools.paths.active.unwrap().selected, Some(index));
            editor.finish_path_drag(false).unwrap();
        }
    }
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
        let paths = prepare(&geometry, None, Some(0), 1., [0., 0.], [1000., 1000.]).unwrap();
        assert!(paths.len() > 1);
        assert!(paths.iter().any(|p| !p.is_empty()));
        assert!(
            prepare(
                &geometry,
                None,
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
