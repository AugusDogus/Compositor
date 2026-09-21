//! Selection draft outlines from TransformOverlay.swift.
use super::*;
use compositor::{document::ShapeKind, geometry::Point};
use quickgui::{PathBuilder, Size};

enum Outline<'a> {
    Freehand(&'a [Point]),
    Polygon {
        points: &'a [Point],
        cursor: Option<Point>,
    },
    Marquee {
        start: Point,
        end: Point,
        kind: ShapeKind,
    },
}

impl Outline<'_> {
    fn path(&self, width: f32, zoom: f64, offset: Point) -> Result<quickgui::Path> {
        // CoreGraphics uses a miter limit of 10 for the source draft stroke.
        let mut path = PathBuilder::stroke(width).with_style(quickgui::PathStyle::Stroke(
            quickgui::StrokeOptions::default()
                .with_line_width(width)
                .with_miter_limit(10.),
        ));
        let view_point = |point: Point| {
            quickgui::Point::new(
                (offset[0] + point[0] * zoom) as f32,
                (offset[1] + point[1] * zoom) as f32,
            )
        };
        match self {
            Self::Freehand(points) | Self::Polygon { points, .. } => {
                if let Some(first) = points.first() {
                    path.move_to(view_point(*first));
                    for point in &points[1..] {
                        path.line_to(view_point(*point));
                    }
                    if let Self::Polygon {
                        cursor: Some(cursor),
                        ..
                    } = self
                    {
                        // Pointer coordinates are already local to the canvas.
                        path.line_to(quickgui::Point::new(cursor[0] as f32, cursor[1] as f32));
                    }
                }
            }
            Self::Marquee { start, end, kind } => {
                let min = view_point([start[0].min(end[0]), start[1].min(end[1])]);
                let max = view_point([start[0].max(end[0]), start[1].max(end[1])]);
                if *kind == ShapeKind::Ellipse {
                    let radii = Size::new((max.x - min.x) / 2., (max.y - min.y) / 2.);
                    let right = quickgui::Point::new(max.x, (min.y + max.y) / 2.);
                    let left = quickgui::Point::new(min.x, right.y);
                    if radii.width > 0. && radii.height > 0. {
                        path.move_to(right);
                        path.arc_to(radii, 0., false, true, left);
                        path.arc_to(radii, 0., false, true, right);
                        path.close();
                    }
                } else {
                    path.move_to(min);
                    path.line_to(quickgui::Point::new(max.x, min.y));
                    path.line_to(max);
                    path.line_to(quickgui::Point::new(min.x, max.y));
                    path.close();
                }
            }
        }
        path.build().map_err(|error| compositor::invalid(format!("Could not draw the selection draft: {error}. Cancel the draft and try a shorter outline; the prior selection is preserved.")))
    }
}

impl Editor {
    pub(super) fn selection_draft_overlay(&self, zoom: f64, offset: Point) -> Result<Element> {
        let draft = if let Some(draft) = &self.tools.polygon {
            Outline::Polygon {
                points: &draft.points,
                cursor: self.canvas_pointer,
            }
        } else {
            match &self.gesture {
                Some(Gesture::Lasso { points, .. }) => Outline::Freehand(points),
                Some(Gesture::Text { start, end, .. } | Gesture::Object { start, end, .. }) => {
                    Outline::Marquee {
                        start: *start,
                        end: *end,
                        kind: ShapeKind::Rectangle,
                    }
                }
                Some(Gesture::Region {
                    start, end, tool, ..
                }) => Outline::Marquee {
                    start: *start,
                    end: *end,
                    kind: if *tool == Tool::Ellipse {
                        ShapeKind::Ellipse
                    } else {
                        ShapeKind::Rectangle
                    },
                },
                _ => return Ok(div()),
            }
        };
        let dark = draft.path(2., zoom, offset)?;
        let light = draft.path(1., zoom, offset)?;
        let mut overlay = div()
            .absolute()
            .size_full()
            .accessibility_hidden(true)
            .child(
                quickgui::canvas(move |_, painter| {
                    painter.paint_path(&dark, Color::BLACK.with_alpha(0.8));
                    painter.paint_path(&light, Color::WHITE);
                })
                .absolute()
                .size_full(),
            );
        if let Outline::Polygon { points, .. } = draft
            && let Some(first) = points.first()
        {
            overlay = overlay.child(
                div()
                    .absolute()
                    .size(9., 9.)
                    .bg(Color::WHITE)
                    .border(1., Color::BLACK)
                    .translate(
                        (offset[0] + first[0] * zoom - 4.5) as f32,
                        (offset[1] + first[1] * zoom - 4.5) as f32,
                    ),
            );
        }
        Ok(overlay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compositor::selection::SelectionMode;
    use quickgui::{Application, WindowOptions};

    struct DraftView(Editor);
    impl View for DraftView {
        fn render(&mut self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            div()
                .size_full()
                .relative()
                .bg(Color::rgb8(100, 100, 100))
                .child(self.0.selection_draft_overlay(1., [0., 0.]).unwrap())
        }
    }

    #[test]
    fn polygon_draft_has_a_white_open_line_and_only_the_first_corner_handle() {
        let mut editor = Editor::with_test_document();
        editor.tools.polygon = Some(super::super::selection_tools::PolygonDraft {
            points: vec![[30.5, 30.5], [140.5, 30.5], [140.5, 110.5]],
            mode: SelectionMode::Replace,
        });
        editor.canvas_pointer = Some([50.5, 110.5]);
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Polygon draft")
                    .size(180., 150.)
                    .minimum_size(1., 1.),
                DraftView(editor),
            )
            .unwrap();
        let frame = cx.capture_screenshot(view.window_handle()).unwrap();
        let scale = frame.width() as f32 / 180.;
        let pixel = |x: f32, y: f32| frame.pixel((x * scale) as u32, (y * scale) as u32).unwrap();
        assert_eq!(
            pixel(80.5, 30.5),
            [255; 4],
            "The draft line must be white over black"
        );
        assert_eq!(
            pixel(33.5, 33.5),
            [255; 4],
            "The first corner must have an 8-point white handle"
        );
        assert_eq!(
            pixel(142.5, 28.5),
            [100, 100, 100, 255],
            "Subsequent vertices must not have handles"
        );
        assert_eq!(
            pixel(80.5, 110.5),
            [255; 4],
            "The last vertex must connect to the pointer"
        );
        assert_eq!(
            pixel(40.5, 70.5),
            [100, 100, 100, 255],
            "The draft must not close before commit"
        );
    }

    #[test]
    fn freehand_and_marquee_drafts_draw_their_own_outline() {
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Selection drafts")
                    .size(180., 150.)
                    .minimum_size(1., 1.),
                DraftView(Editor::with_test_document()),
            )
            .unwrap();
        for tool in [Tool::Lasso, Tool::Rectangle, Tool::Ellipse, Tool::Object] {
            cx.update(view, |view, cx| {
                view.0.gesture = Some(if tool == Tool::Lasso {
                    Gesture::Lasso {
                        points: vec![[30.5, 30.5], [140.5, 30.5], [140.5, 110.5]],
                        base: None,
                        mode: SelectionMode::Replace,
                    }
                } else if tool == Tool::Object {
                    Gesture::Object {
                        start: [30.5, 30.5],
                        end: [140.5, 110.5],
                        sample_all: true,
                        antialiased: true,
                        mode: SelectionMode::Replace,
                    }
                } else {
                    Gesture::Region {
                        anchor: [30.5, 30.5],
                        start: [30.5, 30.5],
                        end: [140.5, 110.5],
                        tool,
                        base: None,
                        mode: SelectionMode::Replace,
                        constrain_armed: true,
                    }
                });
                cx.invalidate();
            })
            .unwrap();
            let frame = cx.capture_screenshot(view.window_handle()).unwrap();
            let scale = frame.width() as f32 / 180.;
            let pixel =
                |x: f32, y: f32| frame.pixel((x * scale) as u32, (y * scale) as u32).unwrap();
            let top = pixel(85.5, 30.5);
            assert!(
                top[0] > 220 && top[0] == top[1] && top[1] == top[2],
                "{tool:?} needs a white draft outline: {top:?}"
            );
            if tool == Tool::Lasso {
                assert_eq!(
                    pixel(85.5, 70.5),
                    [100, 100, 100, 255],
                    "Freehand drafts must not connect back to the start"
                );
                assert_eq!(
                    pixel(33.5, 33.5),
                    [100, 100, 100, 255],
                    "Freehand drafts do not have corner handles"
                );
            } else if tool == Tool::Ellipse {
                assert_eq!(
                    pixel(30.5, 30.5),
                    [100, 100, 100, 255],
                    "An ellipse must not use its bounding rectangle"
                );
            }
        }
    }
}
