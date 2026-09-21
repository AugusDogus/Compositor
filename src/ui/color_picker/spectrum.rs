//! Keep the displayed spectrum in the same encoded sRGB space as the HSB picker math.
use super::*;
use quickgui::{CustomShader, Rect, ShaderParameters};

pub(in crate::ui) fn shader() -> Result<CustomShader> {
    CustomShader::new(include_str!("spectrum.wgsl")).map_err(|error| {
        compositor::invalid(format!("Could not prepare the color picker: {error}"))
    })
}

pub(super) fn field(shader: &CustomShader, hue: f64, strip: bool, width: f32) -> Element {
    let shader = shader.clone();
    quickgui::canvas(move |bounds, painter| {
        painter.paint_shader(
            Rect::new(0., 0., bounds.width, bounds.height),
            &shader,
            ShaderParameters::new().vector(0, [hue as f32, if strip { 1. } else { 0. }, 0., 0.]),
        );
    })
    .w(width)
    .h(256.)
    .into_element()
}

pub(super) fn marker(hsb: Hsb) -> Element {
    quickgui::canvas(move |bounds, painter| {
        let center = quickgui::Point::new(
            hsb.saturation as f32 * bounds.width,
            (1. - hsb.brightness) as f32 * bounds.height,
        );
        // Paint the source's inset circle strokes without rounding the selected
        // color position, 12-point white diameter, or outer 0.75-point border.
        for (radius, width, color) in [(6.375, 0.75, Color::BLACK), (5.25, 1.5, Color::WHITE)] {
            let mut path = quickgui::PathBuilder::stroke(width);
            let right = quickgui::Point::new(center.x + radius, center.y);
            let left = quickgui::Point::new(center.x - radius, center.y);
            let radii = quickgui::Size::new(radius, radius);
            path.move_to(right);
            path.arc_to(radii, 0., false, true, left);
            path.arc_to(radii, 0., false, true, right);
            path.close();
            painter.paint_path(path.build().expect("Validated picker marker"), color);
        }
    })
    .into_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, WindowOptions};
    struct Spectrum {
        shader: CustomShader,
        hue: f64,
        strip: bool,
    }
    impl View for Spectrum {
        fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            field(&self.shader, self.hue, self.strip, 256.)
        }
    }

    #[test]
    fn rendered_picker_pixels_match_hsb_numbers_for_field_and_hue_strip() {
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Spectrum")
                    .size(256., 256.)
                    .minimum_size(1., 1.),
                Spectrum {
                    shader: shader().unwrap(),
                    hue: 0.,
                    strip: false,
                },
            )
            .unwrap();
        for strip in [false, true] {
            for hue in [0., 61., 149., 210., 298., 360.] {
                cx.update(view, |view, cx| {
                    view.hue = hue;
                    view.strip = strip;
                    cx.invalidate();
                })
                .unwrap();
                let frame = cx.capture_screenshot(view.window_handle()).unwrap();
                for y in [0, 19, 63, 127, 200, 255] {
                    for x in [0, 31, 127, 200, 255] {
                        let scale = frame.width() / 256;
                        let (px, py) = (x * scale, y * scale);
                        let u = (f64::from(px) + 0.5) / f64::from(frame.width());
                        let v = (f64::from(py) + 0.5) / f64::from(frame.height());
                        let expected = Hsb {
                            hue: if strip { (1. - v) * 360. } else { hue },
                            saturation: if strip { 1. } else { u },
                            brightness: if strip { 1. } else { 1. - v },
                        }
                        .rgb();
                        let actual = frame.pixel(px, py).unwrap();
                        assert!(
                            actual
                                .into_iter()
                                .zip(expected)
                                .all(|(a, b)| a.abs_diff(b) <= 1),
                            "strip={strip}, hue={hue}, ({x},{y}): {actual:?}, expected {expected:?}"
                        );
                    }
                }
            }
        }
    }
}
