//! Layer and floating-selection handles from TransformOverlay.swift.
use super::floating::Placement;
use super::*;
use compositor::geometry::Point;

pub(super) fn overlay(placement: Placement, zoom: f64, offset: Point) -> Element {
    let mut overlay = div().absolute().size_full().accessibility_hidden(true);
    let transform = placement.bounds();
    let mut handles = placement.handles();
    handles[8] = transform.geometry_point([0.5, -28. / zoom / transform.size[1]]);
    let points = handles.map(|point| {
        quickgui::Point::new(
            (offset[0] + point[0] * zoom) as f32,
            (offset[1] + point[1] * zoom) as f32,
        )
    });
    let rotation = matches!(placement, Placement::Affine(_));
    let accent = Color::rgb8(0, 122, 255);
    for (width, color) in [(3., Color::BLACK.with_alpha(0.7)), (1., accent)] {
        let mut builder = quickgui::PathBuilder::stroke(width);
        builder.move_to(points[0]);
        for index in [2, 4, 6] {
            builder.line_to(points[index]);
        }
        builder.close();
        if rotation {
            builder.move_to(points[1]);
            builder.line_to(points[8]);
        }
        if let Ok(path) = builder.build() {
            overlay = overlay.child(
                quickgui::canvas(move |_, painter| {
                    painter.paint_path(&path, color);
                })
                .absolute()
                .size_full(),
            );
        }
    }
    for (index, point) in points
        .into_iter()
        .take(if rotation { 9 } else { 8 })
        .enumerate()
    {
        // CoreGraphics centers its stroke on the 7-point square / 8-point circle.
        let size = if index == 8 { 9. } else { 8. };
        overlay = overlay.child(
            div()
                .absolute()
                .size(size, size)
                .rounded(if index == 8 { size / 2. } else { 0. })
                .bg(Color::WHITE)
                .border(1., accent)
                .translate(point.x - size / 2., point.y - size / 2.),
        );
    }
    overlay
}

#[cfg(test)]
mod tests {
    use super::*;
    use compositor::geometry::Transform;
    use quickgui::{Application, WindowOptions};

    struct Handles(Placement);
    impl View for Handles {
        fn render(&mut self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            div()
                .size_full()
                .relative()
                .bg(Color::rgb8(30, 30, 30))
                .child(overlay(self.0, 1., [0., 0.]))
        }
    }

    #[test]
    fn affine_handles_have_a_round_rotation_control_and_visible_stem() {
        let placement = Placement::Affine(Transform {
            origin: [40.5, 60.5],
            ..Transform::new(100, 100)
        });
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Transform guides")
                    .size(200., 200.)
                    .minimum_size(1., 1.),
                Handles(placement),
            )
            .unwrap();
        let frame = cx.capture_screenshot(view.window_handle()).unwrap();
        let scale = frame.width() as f32 / 200.;
        let pixel = |x: f32, y: f32| frame.pixel((x * scale) as u32, (y * scale) as u32).unwrap();
        assert_eq!(
            pixel(90.5, 46.5),
            [0, 122, 255, 255],
            "The rotation control must connect to the top edge"
        );
        assert_eq!(
            pixel(40.5, 60.5),
            [255; 4],
            "Resize handles must have white centers"
        );
        assert_eq!(pixel(90.5, 32.5), [255; 4]);
        assert_eq!(
            pixel(94.5, 36.5),
            [30, 30, 30, 255],
            "The rotation control must be round"
        );
        cx.update(view, |view, cx| {
            view.0 = Placement::Perspective(placement.corners());
            cx.invalidate();
        })
        .unwrap();
        let perspective = cx.capture_screenshot(view.window_handle()).unwrap();
        assert_eq!(
            perspective.pixel((90.5 * scale) as u32, (46.5 * scale) as u32),
            Some([30, 30, 30, 255])
        );
        assert_eq!(
            perspective.pixel((90.5 * scale) as u32, (32.5 * scale) as u32),
            Some([30, 30, 30, 255])
        );
    }
}
