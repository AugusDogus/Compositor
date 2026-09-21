//! Live shape geometry and appearance from ShapeTool.swift and TransformOverlay.swift.
use super::*;
use compositor::{
    document::{Shape, ShapeKind},
    geometry::Point,
};
use quickgui::{FillOptions, PathBuilder, PathStyle, Size, StrokeOptions};

pub(super) struct ShapeDraft {
    anchor: Point,
    start: Point,
    end: Point,
    kind: ShapeKind,
    radius: f64,
}

impl ShapeDraft {
    fn new(anchor: Point, ellipse: bool, radius: f64) -> Self {
        Self {
            anchor,
            start: anchor,
            end: anchor,
            kind: if ellipse {
                ShapeKind::Ellipse
            } else {
                ShapeKind::Rectangle
            },
            radius: if ellipse { 0. } else { radius },
        }
    }

    pub(super) fn drag(&mut self, point: Point, modifiers: Modifiers) {
        self.start = self.anchor;
        self.end = point;
        if modifiers.contains(Modifiers::SHIFT) {
            let delta = [point[0] - self.anchor[0], point[1] - self.anchor[1]];
            let side = delta[0].abs().max(delta[1].abs());
            self.end = std::array::from_fn(|axis| {
                self.anchor[axis] + if delta[axis] < 0. { -side } else { side }
            });
        }
        if modifiers.contains(Modifiers::ALT) {
            self.start = std::array::from_fn(|axis| 2. * self.anchor[axis] - self.end[axis]);
        }
    }
}

impl Editor {
    pub(super) fn begin_shape(&mut self, point: Point) -> Result<()> {
        let draft = ShapeDraft::new(point, self.tools.shape_ellipse, self.tools.shape_radius);
        self.session_mut().begin(match draft.kind {
            ShapeKind::Rectangle => "Rectangle",
            ShapeKind::Ellipse => "Ellipse",
        })?;
        self.gesture = Some(Gesture::Shape(draft));
        Ok(())
    }

    pub(super) fn finish_shape(&mut self, draft: ShapeDraft) -> Result<()> {
        let [r, g, b, _] = self.tools.brush.color;
        let shape = Shape {
            kind: draft.kind,
            red: f64::from(r) / 255.,
            green: f64::from(g) / 255.,
            blue: f64::from(b) / 255.,
            corner_radius: draft.radius,
        };
        let active = self.session().document.active;
        compositor::edits::shape(
            &mut self.session_mut().document,
            draft.start,
            draft.end,
            shape,
        )?;
        self.retain_mask_target(active);
        Ok(())
    }

    pub(super) fn shape_draft_overlay(&self, zoom: f64, offset: Point) -> Result<Element> {
        let Some(Gesture::Shape(draft)) = &self.gesture else {
            return Ok(div());
        };
        let width = ((draft.end[0] - draft.start[0]).abs() * zoom) as f32;
        let height = ((draft.end[1] - draft.start[1]).abs() * zoom) as f32;
        if width <= 0. || height <= 0. {
            return Ok(div());
        }
        let left = (offset[0] + draft.start[0].min(draft.end[0]) * zoom) as f32;
        let top = (offset[1] + draft.start[1].min(draft.end[1]) * zoom) as f32;
        let radius = (draft.radius * zoom) as f32;
        let fill = outline(
            draft.kind,
            width,
            height,
            radius,
            PathStyle::Fill(FillOptions::default()),
        )?;
        let stroke = outline(
            draft.kind,
            width,
            height,
            radius,
            PathStyle::Stroke(StrokeOptions::default()),
        )?;
        let [r, g, b, _] = self.tools.brush.color;
        Ok(quickgui::canvas(move |_, painter| {
            painter.paint_path(&fill, Color::rgb8(r, g, b));
            painter.paint_path(&stroke, Color::BLACK.with_alpha(0.6));
        })
        .absolute()
        .size(width + 2., height + 2.)
        .translate(left - 1., top - 1.)
        .accessibility_hidden(true))
    }
}

/// Local coordinates leave a one-point inset for the centered stroke and its antialiasing fringe.
fn outline(
    kind: ShapeKind,
    width: f32,
    height: f32,
    radius: f32,
    style: PathStyle,
) -> Result<quickgui::Path> {
    let mut path = PathBuilder::fill().with_style(style);
    let point = |x, y| quickgui::Point::new(x + 1., y + 1.);
    match kind {
        ShapeKind::Ellipse => {
            let radii = Size::new(width / 2., height / 2.);
            path.move_to(point(width, height / 2.));
            path.arc_to(radii, 0., false, true, point(0., height / 2.));
            path.arc_to(radii, 0., false, true, point(width, height / 2.));
        }
        ShapeKind::Rectangle => {
            let r = radius.clamp(0., width.min(height) / 2.);
            let radii = Size::new(r, r);
            path.move_to(point(r, 0.));
            path.line_to(point(width - r, 0.));
            path.arc_to(radii, 0., false, true, point(width, r));
            path.line_to(point(width, height - r));
            path.arc_to(radii, 0., false, true, point(width - r, height));
            path.line_to(point(r, height));
            path.arc_to(radii, 0., false, true, point(0., height - r));
            path.line_to(point(0., r));
            path.arc_to(radii, 0., false, true, point(r, 0.));
        }
    }
    path.close();
    path.build().map_err(|error| compositor::invalid(format!("Could not draw the shape preview: {error}. Cancel the drag and try a smaller shape; existing layers are unchanged.")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, WindowOptions};

    struct Preview(Editor);
    impl View for Preview {
        fn render(&mut self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            div()
                .size_full()
                .relative()
                .bg(Color::rgb8(100, 100, 100))
                .child(self.0.shape_draft_overlay(1., [0., 0.]).unwrap())
        }
    }

    #[test]
    fn shape_drafts_show_the_fill_and_the_actual_rounded_or_elliptical_boundary() {
        let mut editor = Editor::with_test_document();
        editor.tools.tool = Tool::Shape;
        editor.tools.brush.color = [210, 90, 40, 255];
        let original = editor.session().document.clone();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Shape drafts")
                    .size(200., 160.)
                    .minimum_size(1., 1.),
                Preview(editor),
            )
            .unwrap();
        for (ellipse, radius, rounded) in [
            (false, 0., false),
            (false, 20., true),
            (false, 5000., true),
            (true, 0., true),
        ] {
            cx.update(view, |view, cx| {
                view.0.session_mut().cancel();
                view.0.tools.shape_ellipse = ellipse;
                view.0.tools.shape_radius = radius;
                view.0.begin_shape([40.5, 60.5]).unwrap();
                if let Some(Gesture::Shape(draft)) = &mut view.0.gesture {
                    draft.drag([160.5, 120.5], Modifiers::empty());
                }
                cx.invalidate();
            })
            .unwrap();
            let frame = cx.capture_screenshot(view.window_handle()).unwrap();
            let scale = frame.width() as f32 / 200.;
            let pixel =
                |x: f32, y: f32| frame.pixel((x * scale) as u32, (y * scale) as u32).unwrap();
            assert_eq!(
                pixel(100., 90.),
                [210, 90, 40, 255],
                "The dragged shape must show its fill before release"
            );
            assert_eq!(
                pixel(43., 63.),
                if rounded {
                    [100, 100, 100, 255]
                } else {
                    [210, 90, 40, 255]
                }
            );
            assert_eq!(pixel(100., 50.), [100, 100, 100, 255]);
            assert_eq!(
                pixel(57., 68.),
                if ellipse {
                    [100, 100, 100, 255]
                } else {
                    [210, 90, 40, 255]
                },
                "An ellipse must not use a capsule boundary"
            );
            let rim = pixel(100., 60.5);
            assert!(
                rim[0] < 110 && rim[1] < 60,
                "The source's dark outline must remain visible: {rim:?}"
            );
            cx.read(view, |view| {
                assert_eq!(view.0.session().document, original);
                assert!(view.0.session().undo_label().is_none());
            })
            .unwrap();
        }
    }
}
