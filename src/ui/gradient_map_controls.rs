use super::*;
use compositor::adjustment::Color as AdjustmentColor;

fn color(value: AdjustmentColor) -> Color {
    Color::rgb8(
        (value.red * 255.).round() as u8,
        (value.green * 255.).round() as u8,
        (value.blue * 255.).round() as u8,
    )
}
impl Editor {
    pub(super) fn gradient_map_controls(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let Some(edit) = &self.adjustment_edit else {
            return div();
        };
        let map = edit.settings.gradient_map_settings.unwrap_or_default();
        let ends = if map.reversed {
            [map.highlights, map.shadows]
        } else {
            [map.shadows, map.highlights]
        };
        let mut swatches = div().flex_row().items_center().gap(20.);
        for (label, value, highlights) in [
            ("Shadows", map.shadows, false),
            ("Highlights", map.highlights, true),
        ] {
            swatches = swatches.child(
                div()
                    .flex_row()
                    .items_center()
                    .gap(8.)
                    .child(
                        super::palette_controls::swatch(color(value), format!("{label} color"))
                            .tooltip(format!("Choose the {} color", label.to_lowercase()))
                            .on_click(cx.listener(
                                if highlights {
                                    "gradient-map-highlights"
                                } else {
                                    "gradient-map-shadows"
                                },
                                move |this, cx| {
                                    let result = this.open_gradient_map_picker().map(|()| {
                                        if highlights
                                            && let Some(Form::Color(picker)) = &mut this.modal
                                        {
                                            picker.select_background();
                                        }
                                    });
                                    this.result(result, cx);
                                },
                            )),
                    )
                    .child(text(label).text_size(13.).line_height(16.)),
            );
        }
        div()
            .flex_col()
            .gap(12.)
            .child(
                div()
                    .id("gradient-map-ramp")
                    .h(20.)
                    .relative()
                    .rounded(4.)
                    .bg_linear_gradient(quickgui::GradientDirection::ToRight, ends.map(color))
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .rounded(4.)
                            .border(1., Color::BLACK.with_alpha(0.35)),
                    )
                    .accessibility_hidden(true),
            )
            .child(swatches)
            .child(self.form_toggle(cx, 2, "Reverse", map.reversed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, WindowOptions};

    #[test]
    fn gradient_map_uses_the_palette_swatch_outline_and_a_translucent_ramp_border() {
        let mut editor = Editor::with_test_document();
        compositor::edits::fill(
            &mut editor.session_mut().document,
            [128, 128, 128, 255],
            false,
            false,
        )
        .unwrap();
        editor.tools.brush.color = [0, 0, 0, 255];
        editor.open_pixel_adjustment(Kind::GradientMap).unwrap();
        let (mut cx, view) = Application::new()
            .font(crate::UI_FONT)
            .into_test_context(
                WindowOptions::new("Gradient Map swatches").size(1280., 900.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        let rail = cx.element_bounds(window, 320_u64).unwrap();
        let shadows = cx.element_bounds(window, "gradient-map-shadows").unwrap();
        let ramp = cx.element_bounds(window, "gradient-map-ramp").unwrap();
        let frame = cx.capture_screenshot(window).unwrap();
        let scale = frame.width() as f32 / 1280.;
        let pixel = |bounds: quickgui::Rect, x: f32, y: f32| {
            frame
                .pixel(
                    ((bounds.x + x) * scale) as u32,
                    ((bounds.y + y) * scale) as u32,
                )
                .unwrap()
        };
        assert_eq!([shadows.width, shadows.height], [24., 24.]);
        // Both source swatches have the same black rim and inset white stroke.
        // Compare the top half, away from the rail's overlapping background well.
        for y in 0..10 {
            let actual = pixel(shadows, 12., y as f32);
            let expected = pixel(rail, 12., y as f32);
            assert!(
                actual
                    .into_iter()
                    .zip(expected)
                    .all(|(a, b)| a.abs_diff(b) <= 3),
                "Gradient Map and palette outlines differ at row {y}: {actual:?}/{expected:?}, {shadows:?}/{rail:?}"
            );
        }
        let dark = pixel(ramp, ramp.width * 0.25, 0.5)[0];
        let light = pixel(ramp, ramp.width * 0.75, 0.5)[0];
        assert!(
            light > dark + 40,
            "Border must retain the gradient: {dark}, {light}"
        );
        for fraction in [0.25, 0.75] {
            let edge = pixel(ramp, ramp.width * fraction, 0.5)[0] as f32;
            let fill = pixel(ramp, ramp.width * fraction, 10.)[0] as f32;
            assert!(
                edge > fill * 0.6 && edge < fill * 0.9,
                "Border must darken its local color without becoming opaque: {edge}/{fill}"
            );
        }
    }
}
