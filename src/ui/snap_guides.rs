//! Alignment guides sit above the editor's other canvas overlays.
use super::*;
use compositor::geometry::Point;

impl Editor {
    pub(super) fn snap_guides_overlay(&self, zoom: f64, offset: Point) -> Element {
        let guides = match &self.gesture {
            Some(Gesture::Move { guides, .. }) => *guides,
            Some(Gesture::HeaderTransform(drag)) => drag.guides,
            _ if self.tools.tool == Tool::Crop => self
                .tools
                .pending_crop
                .as_ref()
                .map_or([None; 2], |crop| crop.guides),
            _ => [None; 2],
        };
        let doc = &self.session().document;
        let size = [f64::from(doc.width) * zoom, f64::from(doc.height) * zoom];
        quickgui::canvas(move |bounds, painter| {
            let limits = [bounds.width as f64, bounds.height as f64];
            let mut path = quickgui::PathBuilder::stroke(1.);
            for (axis, position) in guides.into_iter().enumerate() {
                let Some(position) = position else { continue };
                let location = offset[axis] + position * zoom;
                let along = 1 - axis;
                let low = offset[along].max(0.);
                let high = (offset[along] + size[along]).min(limits[along]);
                if location < -0.5 || location > limits[axis] + 0.5 || low >= high {
                    continue;
                }
                // Clip before tessellation so distant panning and zoomed sparse
                // documents never exceed QuickGUI's path-coordinate budget.
                let point = |value: f64| {
                    if axis == 0 {
                        quickgui::Point::new(location as f32, value as f32)
                    } else {
                        quickgui::Point::new(value as f32, location as f32)
                    }
                };
                path.move_to(point(low));
                path.line_to(point(high));
            }
            painter.paint_path(
                path.build().expect("Viewport-clipped guide coordinates"),
                Color::rgb8(0, 122, 255),
            );
        })
        .absolute()
        .size_full()
        .accessibility_hidden(true)
        .into_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compositor::geometry::Transform;
    use quickgui::{Application, WindowOptions};

    struct Guides(Editor, Point);
    impl View for Guides {
        fn render(&mut self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            div()
                .size_full()
                .relative()
                .bg(Color::BLACK)
                .child(self.0.snap_guides_overlay(0.805, self.1))
        }
    }

    #[test]
    fn fractional_zoom_guides_end_at_the_canvas_edges() {
        let mut editor = Editor::with_test_document();
        editor.tabs[0].set_document(Document::new(100, 100).unwrap(), None);
        editor.tools.tool = Tool::Crop;
        editor.tools.pending_crop = Some(crop::CropPreview {
            frame: Transform::new(100, 100),
            guides: [Some(50.), Some(50.)],
        });
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Snap guide endpoints")
                    .size(130., 130.)
                    .minimum_size(1., 1.),
                Guides(editor, [20.25, 20.25]),
            )
            .unwrap();
        let shot = cx.capture_screenshot(view.window_handle()).unwrap();
        let scale = shot.width() as f32 / 130.;
        for vertical in [false, true] {
            let cross = (60.5 * scale) as u32;
            for coordinate in [19, 20, 21, 99, 100, 101, 102] {
                let at = (coordinate as f32 * scale) as u32;
                let actual = if vertical {
                    shot.pixel(cross, at)
                } else {
                    shot.pixel(at, cross)
                }
                .unwrap()[2];
                let low = 20.25 * scale;
                let high = 100.75 * scale;
                let expected =
                    ((at as f32 + 1.).min(high) - (at as f32).max(low)).clamp(0., 1.) * 255.;
                assert!(
                    (f32::from(actual) - expected).abs() <= 2.,
                    "Guide vertical={vertical} at {coordinate}: expected {expected}, got {actual}"
                );
            }
        }
        for offset in [[2_000_000., 2_000_000.], [-2_000_000., -2_000_000.]] {
            cx.update(view, |view, cx| {
                view.1 = offset;
                cx.invalidate();
            })
            .unwrap();
            let offscreen = cx.capture_screenshot(view.window_handle()).unwrap();
            assert_eq!(
                offscreen.pixel(100, 100),
                Some([0, 0, 0, 255]),
                "Distant offscreen guides must clip without exceeding the path budget"
            );
        }
    }
}
