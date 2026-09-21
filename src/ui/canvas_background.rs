use super::*;
use compositor::geometry::Point;

/// Keep the transparency grid in view coordinates. A retained low-resolution
/// image preview must not magnify its checkerboard when zooming or panning.
pub(super) fn checkerboard(
    size: [f32; 2],
    document: [f64; 2],
    zoom: f64,
    offset: Point,
) -> Element {
    quickgui::canvas(move |_, painter| {
        let left = offset[0].max(0.) as f32;
        let top = offset[1].max(0.) as f32;
        let right = (offset[0] + document[0] * zoom).min(f64::from(size[0])) as f32;
        let bottom = (offset[1] + document[1] * zoom).min(f64::from(size[1])) as f32;
        if right <= left || bottom <= top {
            return;
        }
        painter.fill_rect(
            quickgui::Rect::new(left, top, right - left, bottom - top),
            Color::rgb8(77, 77, 77),
        );
        let x0 = ((f64::from(left) - offset[0]) / 10.).floor() as i64;
        let y0 = ((f64::from(top) - offset[1]) / 10.).floor() as i64;
        let x1 = ((f64::from(right) - offset[0]) / 10.).ceil() as i64;
        let y1 = ((f64::from(bottom) - offset[1]) / 10.).ceil() as i64;
        for y in y0..y1 {
            for x in x0..x1 {
                if (x + y).rem_euclid(2) != 0 {
                    continue;
                }
                let x = (offset[0] + x as f64 * 10.) as f32;
                let y = (offset[1] + y as f64 * 10.) as f32;
                painter.fill_rect(
                    quickgui::Rect::new(
                        x.max(left),
                        y.max(top),
                        (x + 10.).min(right) - x.max(left),
                        (y + 10.).min(bottom) - y.max(top),
                    ),
                    Color::rgb8(89, 89, 89),
                );
            }
        }
    })
    .absolute()
    .size_full()
    .into_element()
}

/// EditorCanvas.swift paints a 14-point shadow beneath the document.
pub(super) fn shadow(rect: quickgui::Rect) -> Element {
    div()
        .absolute()
        .left(rect.x)
        .top(rect.y)
        .size(rect.width, rect.height)
        .bg(Color::rgb8(66, 66, 66))
        .shadow(quickgui::BoxShadow::new(0., 3., Color::BLACK.with_alpha(0.35)).blur_radius(14.))
}

/// The source outline is centered on the document edge and stays one physical pixel wide.
pub(super) fn outline(rect: quickgui::Rect, backing_scale: f64) -> Element {
    let hairline = (1. / backing_scale) as f32;
    // Canvas coordinates bypass flex layout's whole-point rounding.
    quickgui::canvas(move |_, painter| {
        let left = rect.x - hairline / 2.;
        let top = rect.y - hairline / 2.;
        let width = rect.width + hairline;
        let height = rect.height + hairline;
        let horizontal = hairline.min(height / 2.);
        let vertical = hairline.min(width / 2.);
        let side_height = height - 2. * horizontal;
        for edge in [
            quickgui::Rect::new(left, top, width, horizontal),
            quickgui::Rect::new(left, top + height - horizontal, width, horizontal),
            quickgui::Rect::new(left, top + horizontal, vertical, side_height),
            quickgui::Rect::new(
                left + width - vertical,
                top + horizontal,
                vertical,
                side_height,
            ),
        ] {
            painter.fill_rect(edge, Color::WHITE.with_alpha(0.13));
        }
    })
    .absolute()
    .size_full()
    .into_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, WindowOptions};

    struct CanvasSample {
        zoom: f64,
        offset: Point,
    }

    impl View for CanvasSample {
        fn render(&mut self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            div()
                .size_full()
                .bg(Color::rgb8(27, 27, 27))
                .child(checkerboard(
                    [100., 100.],
                    [80., 80.],
                    self.zoom,
                    self.offset,
                ))
        }
    }

    #[test]
    fn canvas_checker_uses_source_tones_and_ten_point_tiles_at_every_zoom() {
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Canvas checker").size(100., 100.),
                CanvasSample {
                    zoom: 1.,
                    offset: [10., 10.],
                },
            )
            .unwrap();
        for (zoom, offset) in [(1., [10., 10.]), (4., [-190., -190.])] {
            cx.update(view, |sample, cx| {
                sample.zoom = zoom;
                sample.offset = offset;
                cx.invalidate();
            })
            .unwrap();
            let shot = cx.capture_screenshot(view.window_handle()).unwrap();
            let scale = shot.width() / 100;
            for (x, y, tone) in [(15, 15, 89), (25, 15, 77), (15, 25, 77), (25, 25, 89)] {
                assert_eq!(
                    shot.pixel(x * scale, y * scale),
                    Some([tone, tone, tone, 255])
                );
            }
            if zoom == 1. {
                assert_eq!(shot.pixel(5 * scale, 5 * scale), Some([27, 27, 27, 255]));
                assert_eq!(shot.pixel(95 * scale, 95 * scale), Some([27, 27, 27, 255]));
            }
        }
    }

    struct EdgeSample {
        outlined: bool,
    }

    impl View for EdgeSample {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let scale = cx.scale_factor();
            let rect = quickgui::Rect::new(20. + 0.5 / scale, 20. + 0.5 / scale, 60., 60.);
            let surface = div()
                .size_full()
                .bg(Color::rgb8(27, 27, 27))
                .child(shadow(rect))
                .child(
                    div()
                        .absolute()
                        .left(rect.x)
                        .top(rect.y)
                        .size(rect.width, rect.height)
                        .bg(Color::rgb8(40, 160, 230)),
                );
            if self.outlined {
                surface.child(outline(rect, f64::from(scale)))
            } else {
                surface
            }
        }
    }

    #[test]
    fn canvas_shadow_leaves_image_pixels_intact_and_outline_stays_one_physical_pixel() {
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Canvas edge").size(100., 100.),
                EdgeSample { outlined: false },
            )
            .unwrap();
        let window = view.window_handle();
        let base = cx.capture_screenshot(window).unwrap();
        let scale = base.width() / 100;
        assert_eq!(
            base.pixel(50 * scale, 50 * scale),
            Some([40, 160, 230, 255])
        );
        assert_eq!(base.pixel(2 * scale, 50 * scale), Some([27, 27, 27, 255]));
        let above = base.pixel(50 * scale, 16 * scale).unwrap()[0];
        let below = base.pixel(50 * scale, 85 * scale).unwrap()[0];
        assert!(
            below < above && above < 27,
            "Shadow must extend below the canvas: {above}, {below}"
        );
        cx.update(view, |sample, cx| {
            sample.outlined = true;
            cx.invalidate();
        })
        .unwrap();
        let outlined = cx.capture_screenshot(window).unwrap();
        let changed: Vec<_> = (0..base.width())
            .filter(|x| base.pixel(*x, 50 * scale) != outlined.pixel(*x, 50 * scale))
            .collect();
        assert_eq!(changed, [20 * scale, 80 * scale]);
        assert_eq!(
            outlined.pixel(50 * scale, 50 * scale),
            base.pixel(50 * scale, 50 * scale)
        );
    }
}
