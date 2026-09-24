//! FilterSheet.swift's slider mapping and displayed precision.
use super::scalar_controls::Scalar;
use super::*;
use compositor::filters::Filter;

#[derive(Clone, Copy)]
pub(super) enum Scale {
    Linear(u8),
    Logarithmic(u8),
}
impl Scale {
    fn decimal_places(self) -> usize {
        usize::from(match self {
            Self::Linear(places) | Self::Logarithmic(places) => places,
        })
    }
    pub fn position(self, value: f64) -> f64 {
        match self {
            Self::Linear(_) => value,
            Self::Logarithmic(_) => value.max(f64::MIN_POSITIVE).ln(),
        }
    }
    pub fn value(self, position: f64) -> f64 {
        let (value, decimals) = match self {
            Self::Linear(decimals) => (position, decimals),
            Self::Logarithmic(decimals) => (position.exp(), decimals),
        };
        let precision = 10_f64.powi(i32::from(decimals));
        (value * precision).round() / precision
    }
}
struct Parameter {
    label: &'static str,
    range: (f64, f64),
    unit: &'static str,
    scale: Scale,
}
impl Parameter {
    // Keep label builder temporaries out of the frame that builds the slider.
    fn label_view(&self, width: f32) -> Element {
        text(self.label)
            .text_size(13.)
            .line_height(16.)
            .w(width)
            .flex_shrink_0()
            .whitespace_nowrap()
    }

    fn for_field(action: Action, kind: Option<Kind>, index: usize) -> Option<Self> {
        use Scale::{Linear, Logarithmic};
        let (label, range, unit, scale) = match (action, kind, index) {
            (Action::Filter(Filter::Gaussian { .. }), _, 0) => {
                ("Radius", (0.1, 250.), "px", Logarithmic(1))
            }
            (Action::Filter(Filter::Motion { .. }), _, 0) => {
                ("Distance", (1., 2000.), "px", Logarithmic(0))
            }
            (Action::Filter(Filter::Motion { .. }), _, 1) => ("Angle", (-90., 90.), "°", Linear(0)),
            (Action::Filter(Filter::Noise { .. }), _, 0) => {
                ("Amount", (0.1, 400.), "%", Logarithmic(1))
            }
            (Action::Filter(Filter::Lens { .. }), _, 0) => {
                ("Remove Distortion", (-100., 100.), "", Linear(0))
            }
            (Action::Filter(Filter::Vignette(_)), _, 0)
            | (Action::Filter(Filter::Bloom { .. } | Filter::TonalContrast { .. }), _, 0) => {
                ("Amount", (0., 100.), "%", Linear(0))
            }
            (Action::Filter(Filter::Vignette(_)), _, 1) => ("Midpoint", (0., 100.), "%", Linear(0)),
            (Action::Filter(Filter::Vignette(_)), _, 2) => {
                ("Roundness", (-100., 100.), "%", Linear(0))
            }
            (Action::Filter(Filter::Vignette(_)), _, 3) => ("Feather", (0., 100.), "%", Linear(0)),
            (Action::Filter(Filter::Vignette(_)), _, 4) => {
                ("Highlights", (0., 100.), "%", Linear(0))
            }
            (Action::Filter(Filter::Bloom { .. }), _, 1) => {
                ("Radius", (1., 150.), "px", Logarithmic(0))
            }
            (Action::Filter(Filter::TonalContrast { .. }), _, 1) => {
                ("Radius", (1., 100.), "px", Logarithmic(0))
            }
            (Action::Filter(Filter::TonalContrast { .. }), _, 2) => {
                ("Shadows", (-100., 100.), "%", Linear(0))
            }
            (Action::Filter(Filter::TonalContrast { .. }), _, 3) => {
                ("Midtones", (-100., 100.), "%", Linear(0))
            }
            (Action::Filter(Filter::TonalContrast { .. }), _, 4) => {
                ("Highlights", (-100., 100.), "%", Linear(0))
            }
            (Action::RemoveBackground, _, 0) => ("Refine", (0., 40.), "px", Linear(0)),
            (Action::RemoveBackground, _, 1) => ("Contrast", (0., 100.), "%", Linear(0)),
            (Action::RemoveBackground, _, 2) => ("Shift Edge", (-10., 10.), "px", Linear(0)),
            (_, Some(Kind::GaussianBlur), 0) => ("Radius", (0.1, 250.), "px", Logarithmic(1)),
            (_, Some(Kind::MotionBlur), 0) => ("Distance", (1., 2000.), "px", Logarithmic(0)),
            (_, Some(Kind::MotionBlur), 1) => ("Angle", (-90., 90.), "°", Linear(0)),
            (_, Some(Kind::AddNoise), 0) => ("Amount", (0.1, 400.), "%", Logarithmic(1)),
            (_, Some(Kind::Exposure), 0) => ("Exposure", (-20., 20.), "", Linear(2)),
            (_, Some(Kind::Exposure), 1) => ("Offset", (-0.5, 0.5), "", Linear(4)),
            (_, Some(Kind::Exposure), 2) => ("Gamma", (0.01, 9.99), "", Logarithmic(2)),
            (_, Some(Kind::BlackWhite), i @ 0..=5) => (
                ["Reds", "Yellows", "Greens", "Cyans", "Blues", "Magentas"][i],
                (-200., 300.),
                "%",
                Linear(0),
            ),
            (_, Some(Kind::BlackWhite), 7) => ("Tint hue", (0., 360.), "°", Linear(0)),
            (_, Some(Kind::BlackWhite), 8) => ("Tint saturation", (0., 100.), "%", Linear(0)),
            (_, Some(Kind::ColorBalance), i @ 0..=8) => (
                [
                    "Shadow Cyan / Red",
                    "Shadow Magenta / Green",
                    "Shadow Yellow / Blue",
                    "Midtone Cyan / Red",
                    "Midtone Magenta / Green",
                    "Midtone Yellow / Blue",
                    "Highlight Cyan / Red",
                    "Highlight Magenta / Green",
                    "Highlight Yellow / Blue",
                ][i],
                (-100., 100.),
                "",
                Linear(0),
            ),
            (_, Some(Kind::Grain), 0) => ("Amount", (0., 100.), "", Linear(0)),
            (_, Some(Kind::Grain), 1) => ("Size", (0.5, 20.), "px", Logarithmic(1)),
            (_, Some(Kind::Grain), 2) => ("Roughness", (0., 100.), "", Linear(0)),
            _ => return None,
        };
        Some(Self {
            label,
            range,
            unit,
            scale,
        })
    }
}
impl Editor {
    pub(super) fn parameter_control(
        &self,
        cx: &mut ViewContext<'_, Self>,
        action: Action,
        index: usize,
        value: &str,
    ) -> Option<Element> {
        let kind = self.adjustment_edit.as_ref().map(|edit| edit.settings.kind);
        let parameter = Parameter::for_field(action, kind, index)?;
        let label_width = match (action, kind, index) {
            (Action::Filter(Filter::Lens { .. }), _, _) => 116.,
            (_, Some(Kind::Grain), 2) => 72.,
            (_, Some(Kind::ColorBalance), _) => 172.,
            (_, Some(Kind::BlackWhite), _) => 100.,
            _ => 60.,
        };
        let unit_width = if parameter.unit.is_empty() { 0. } else { 18. };
        let id = [
            "parameter-0",
            "parameter-1",
            "parameter-2",
            "parameter-3",
            "parameter-4",
            "parameter-5",
            "parameter-6",
            "parameter-7",
            "parameter-8",
            "parameter-9",
        ]
        .get(index)
        .copied()?;
        let range = (
            parameter.scale.position(parameter.range.0),
            parameter.scale.position(parameter.range.1),
        );
        let mut row = div()
            .flex_row()
            .items_center()
            .gap(10.)
            .child(parameter.label_view(label_width))
            .child(self.scalar_slider(
                cx,
                id,
                parameter.label,
                Scalar::Parameter(index, parameter.scale),
                range,
                330. - label_width - 56. - 20. - 2. - unit_width,
            ))
            .child(
                div()
                    .flex_row()
                    .items_center()
                    .flex_shrink_0()
                    .gap(2.)
                    .child(
                        self.form_number_input(cx, index, value, parameter.scale.decimal_places())
                            .w(56.),
                    )
                    .child(
                        text(parameter.unit)
                            .text_size(13.)
                            .line_height(16.)
                            .w(unit_width),
                    ),
            );
        if matches!(action, Action::RemoveBackground) {
            row = row.tooltip(match index {
                0 => "Pull the mask onto the image's own edges, which recovers hair and fur",
                1 => "Clear the haze that leaves background showing through thin areas",
                _ => "Shrink the mask to drop the rim of background color around the subject, or grow it",
            });
        }
        Some(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, Point, WindowOptions};

    #[test]
    fn exposure_fractional_values_position_the_thumb_and_arrow_keys_start_at_the_displayed_value() {
        let mut editor = Editor::with_test_document();
        editor.open_adjustment(Some(Kind::Exposure)).unwrap();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Exposure slider positions").size(1500., 900.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        let control = cx.element_bounds(window, "parameter-1").unwrap();
        let slider =
            quickgui::Slider::new("parameter-1", &quickgui::SliderState::new(-0.5, 0.5, 0.));
        let thumb = cx.element_bounds(window, slider.thumb_id(0)).unwrap();
        assert!((thumb.x + thumb.width / 2. - control.x - control.width / 2.).abs() < 1.);
        let center = Point::new(
            control.x + control.width / 2.,
            control.y + control.height / 2.,
        );
        cx.simulate_pointer_drag(window, "parameter-1", center, center)
            .unwrap();
        cx.simulate_keystrokes(window, "right").unwrap();
        cx.read(view, |e| {
            let Some(Form::Edit { fields, .. }) = &e.modal else {
                panic!("Panel closed");
            };
            assert_eq!(fields[1].1, "0.01");
        })
        .unwrap();
        cx.update(view, |e, cx| {
            e.update_form_field(0, "1.25");
            e.changed(cx);
        })
        .unwrap();
        let control = cx.element_bounds(window, "parameter-0").unwrap();
        let slider =
            quickgui::Slider::new("parameter-0", &quickgui::SliderState::new(-20., 20., 0.));
        let thumb = cx.element_bounds(window, slider.thumb_id(0)).unwrap();
        let expected = control.x + (control.width - thumb.width) * (21.25 / 40.);
        assert!(
            (thumb.x - expected).abs() <= 0.5,
            "Pixel-aligned thumb at {}, expected {expected}",
            thumb.x
        );
    }

    #[test]
    fn logarithmic_radius_drag_maps_to_geometric_midpoint_and_cancel_preserves_pixels() {
        let mut editor = Editor::with_test_document();
        let mut doc = Document::new(8, 8).unwrap();
        compositor::edits::fill(&mut doc, [120, 60, 40, 255], false, false).unwrap();
        editor.tabs = vec![Session::new(doc.clone(), None).into()];
        editor.open_filter(Filter::Gaussian { radius: 1. }).unwrap();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Gaussian Blur").size(1280., 900.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        let bounds = cx.element_bounds(window, "parameter-0").unwrap();
        let center = Point::new(bounds.x + bounds.width / 2., bounds.y + bounds.height / 2.);
        cx.simulate_pointer_drag(window, "parameter-0", center, center)
            .unwrap();
        cx.read(view, |e| {
            let Some(Form::Edit { fields, .. }) = &e.modal else {
                panic!("Filter sheet closed")
            };
            assert_eq!(fields[0].1, "5"); // sqrt(0.1 * 250), rounded to one decimal.
        })
        .unwrap();
        cx.click(window, "form-cancel").unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            doc
        );
    }
    #[test]
    fn grain_size_slider_uses_the_original_half_to_twenty_pixel_range() {
        let mut editor = Editor::with_test_document();
        editor.open_adjustment(Some(Kind::Grain)).unwrap();
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Grain").size(1280., 900.), editor)
            .unwrap();
        let window = view.window_handle();
        cx.focus(window, "parameter-1").unwrap();
        cx.simulate_keystrokes(window, "end").unwrap();
        cx.read(view, |e| {
            let Some(Form::Edit { fields, .. }) = &e.modal else {
                panic!("Grain sheet closed")
            };
            assert_eq!(fields[1].1, "20");
        })
        .unwrap();
        cx.simulate_keystrokes(window, "home").unwrap();
        cx.read(view, |e| {
            let Some(Form::Edit { fields, .. }) = &e.modal else {
                panic!("Grain sheet closed")
            };
            assert_eq!(fields[1].1, "0.5");
        })
        .unwrap();
    }
}
