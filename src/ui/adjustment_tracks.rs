//! Colored adjustment tracks and parameter-local resets.
use super::{scalar_controls::Scalar, *};
use compositor::adjustment::{BlackWhite, ColorRange, hsl_to_rgb};

pub(super) struct Track {
    pub colors: Vec<Color>,
    pub reset: f64,
    pub index: usize,
}

fn hue(degrees: f64, saturation: f64, light: f64) -> Color {
    let [r, g, b] = hsl_to_rgb([degrees.rem_euclid(360.), saturation, light]);
    Color::rgb8(
        (r * 255.).round() as u8,
        (g * 255.).round() as u8,
        (b * 255.).round() as u8,
    )
}

impl Editor {
    pub(super) fn adjustment_slider_track(&self, scalar: Scalar) -> Option<Track> {
        let index = match scalar {
            Scalar::Field(index) | Scalar::Parameter(index, _) => index,
            _ => return None,
        };
        let settings = &self.adjustment_edit.as_ref()?.settings;
        let gray = Color::rgb8(128, 128, 128);
        let (colors, reset) = match settings.kind {
            Kind::BlackWhite => {
                let defaults = BlackWhite::default();
                match index {
                    0..=5 => (
                        vec![
                            hue(index as f64 * 60., 0.65, 0.12),
                            hue(index as f64 * 60., 0.9, 0.5),
                            hue(index as f64 * 60., 0.3, 0.9),
                        ],
                        defaults.weights()[index],
                    ),
                    7 => (
                        (0..=6).map(|i| hue(i as f64 * 60., 1., 0.5)).collect(),
                        defaults.tint_hue,
                    ),
                    8 => (
                        vec![
                            gray,
                            hue(
                                settings.black_white_settings.unwrap_or_default().tint_hue,
                                1.,
                                0.5,
                            ),
                        ],
                        defaults.tint_saturation,
                    ),
                    _ => return None,
                }
            }
            Kind::ColorBalance if index < 9 => {
                let (a, b) = match index % 3 {
                    0 => ([26, 184, 204], [219, 46, 51]),
                    1 => ([204, 56, 179], [61, 179, 77]),
                    _ => ([242, 209, 46], [56, 102, 235]),
                };
                (
                    vec![
                        Color::rgb8(a[0], a[1], a[2]),
                        gray,
                        Color::rgb8(b[0], b[1], b[2]),
                    ],
                    0.,
                )
            }
            Kind::HueSaturation if index < 3 => {
                let s = settings.hsv_settings.clone().unwrap_or_default();
                let center = match s.range {
                    ColorRange::Master | ColorRange::Reds => 0.,
                    ColorRange::Yellows => 60.,
                    ColorRange::Greens => 120.,
                    ColorRange::Cyans => 180.,
                    ColorRange::Blues => 240.,
                    ColorRange::Magentas => 300.,
                };
                match index {
                    0 => (
                        (0..=6)
                            .map(|i| {
                                hue(
                                    i as f64 * 60. + if s.colorize { 0. } else { center - 180. },
                                    0.8,
                                    0.5,
                                )
                            })
                            .collect(),
                        0.,
                    ),
                    1 => {
                        let tint = if s.colorize {
                            s.adjustments
                                .iter()
                                .find(|(range, _)| *range == ColorRange::Master)
                                .map_or(0., |(_, a)| a.hue)
                        } else {
                            center
                        };
                        (
                            vec![gray, hue(tint, 1., 0.5)],
                            if s.colorize { 25. } else { 0. },
                        )
                    }
                    _ => (
                        vec![Color::rgb8(0, 0, 0), gray, Color::rgb8(255, 255, 255)],
                        0.,
                    ),
                }
            }
            _ => return None,
        };
        Some(Track {
            colors,
            reset,
            index,
        })
    }

    pub(super) fn resettable_adjustment_label(
        &self,
        cx: &mut ViewContext<'_, Self>,
        index: usize,
        label: Element,
    ) -> Element {
        let Some(track) = self.adjustment_slider_track(Scalar::Field(index)) else {
            return label;
        };
        label
            .tooltip("Drag to adjust. Double-click to reset.")
            .on_mouse_down(
                quickgui::MouseButton::Left,
                cx.mouse_down_listener(
                    format!("adjustment-reset-label-{index}"),
                    move |this, event, cx| {
                        if event.click_count == 2 {
                            this.update_form_field(index, &track.reset.to_string());
                            cx.prevent_default();
                            this.changed(cx);
                        }
                    },
                ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn colored_controls_reset_only_the_selected_parameter() {
        let mut editor = Editor::with_test_document();
        editor.open_adjustment(Some(Kind::BlackWhite)).unwrap();
        editor.update_form_field(0, "100");
        editor.update_form_field(1, "120");
        let track = editor.adjustment_slider_track(Scalar::Field(0)).unwrap();
        assert_eq!(track.reset, 40.);
        let (mut cx, view) = quickgui::Application::new()
            .into_test_context(
                quickgui::WindowOptions::new("Adjustment tracks").size(1400., 1000.),
                editor,
            )
            .unwrap();
        for id in ["adjustment-reset-label-0", "parameter-0"] {
            cx.update(view, |editor, cx| {
                editor.update_form_field(0, "100");
                cx.invalidate();
            })
            .unwrap();
            cx.simulate_mouse_down(
                view.window_handle(),
                id,
                quickgui::MouseDownEvent {
                    button: quickgui::MouseButton::Left,
                    click_count: 2,
                    ..Default::default()
                },
            )
            .unwrap();
            cx.read(view, |editor| {
                let s = editor
                    .adjustment_edit
                    .as_ref()
                    .unwrap()
                    .settings
                    .black_white_settings
                    .unwrap();
                assert_eq!((s.reds, s.yellows), (40., 120.));
                assert!(editor.adjustment_slider_track(Scalar::BrushSize).is_none());
            })
            .unwrap();
        }
    }
}
