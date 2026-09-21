use super::*;
use quickgui::{CustomShader, Rect, ShaderParameters};

pub(super) fn shader() -> Result<CustomShader> {
    CustomShader::new(include_str!("pixel_grid.wgsl"))
        .map_err(|error| compositor::invalid(format!("Could not prepare the pixel grid: {error}")))
}

/// Draw the union of both sets of lines once, so crossings retain the same
/// opacity. A single shader also avoids path tessellation limits on large screens.
pub(super) fn overlay(
    shader: &CustomShader,
    size: [f32; 2],
    document: [u32; 2],
    zoom: f64,
    offset: compositor::geometry::Point,
    backing_scale: f64,
) -> Element {
    let shader = shader.clone();
    quickgui::canvas(move |_, painter| {
        let left = offset[0].max(0.) as f32;
        let top = offset[1].max(0.) as f32;
        let right = (offset[0] + f64::from(document[0]) * zoom).min(f64::from(size[0])) as f32;
        let bottom = (offset[1] + f64::from(document[1]) * zoom).min(f64::from(size[1])) as f32;
        if right <= left || bottom <= top {
            return;
        }
        let color = Color::rgba8(140, 140, 140, 115);
        let parameters = ShaderParameters::new()
            .vector(
                0,
                [
                    (offset[0] - f64::from(left)).rem_euclid(zoom) as f32,
                    (offset[1] - f64::from(top)).rem_euclid(zoom) as f32,
                    zoom as f32,
                    backing_scale as f32,
                ],
            )
            .vector(1, [color.r, color.g, color.b, color.a]);
        painter.paint_shader(
            Rect::new(left, top, right - left, bottom - top),
            &shader,
            parameters,
        );
    })
    .id("pixel-grid")
    .absolute()
    .size_full()
    .into_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, WindowOptions};

    struct GridView {
        shader: CustomShader,
        pan: f64,
    }

    impl View for GridView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let scale = f64::from(cx.window_state().scale_factor);
            div()
                .size_full()
                .bg(Color::rgb8(240, 180, 20))
                .child(overlay(
                    &self.shader,
                    [64., 64.],
                    [30_000, 30_000],
                    8.,
                    [0.5 / scale + self.pan; 2],
                    scale,
                ))
        }
    }

    #[test]
    fn intersections_blend_once_and_leave_pixel_interiors_unchanged() {
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Grid")
                    .size(64., 64.)
                    .minimum_size(1., 1.),
                GridView {
                    shader: shader().unwrap(),
                    pan: 0.,
                },
            )
            .unwrap();
        let frame = cx.capture_screenshot(view.window_handle()).unwrap();
        let scale = frame.width() / 64;
        let line_at = 8 * scale;
        let inside = 4 * scale;
        assert_eq!(frame.pixel(inside, inside), Some([240, 180, 20, 255]));
        assert_eq!(frame.pixel(line_at, line_at), frame.pixel(line_at, inside));
        assert_eq!(frame.pixel(line_at, line_at), frame.pixel(inside, line_at));
        let line = frame.pixel(line_at, line_at).unwrap();
        for (actual, expected) in line.into_iter().zip([195_u8, 162, 74, 255]) {
            assert!(actual.abs_diff(expected) <= 1, "{line:?}");
        }
        // Moving to a distant portion of a sparse canvas must not drift the grid.
        cx.update(view, |view, cx| {
            view.pan = -80_000.;
            cx.invalidate();
        })
        .unwrap();
        let panned = cx.capture_screenshot(view.window_handle()).unwrap();
        for y in 2..frame.height() {
            for x in 2..frame.width() {
                assert_eq!(panned.pixel(x, y), frame.pixel(x, y), "at {x},{y}");
            }
        }
    }
}
