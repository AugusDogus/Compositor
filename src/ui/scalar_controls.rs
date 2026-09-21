//! Compact numeric controls shared by tool headers and adjustment panels.
use super::*;
use quickgui::{PointerPhase, Slider, SliderState};

const THUMB_SIZE: f32 = 20.;
const THUMB_HEIGHT: f32 = 16.;
const THUMB_TOP: f32 = (24. - THUMB_HEIGHT) / 2.;

pub(super) struct SliderDrag {
    control: &'static str,
    start_offset: f32,
    travel: f32,
}

#[derive(Clone, Copy)]
pub(super) enum Scalar {
    Effect(super::layer_effects::Parameter),
    BrushSize,
    BrushHardness,
    BrushSmoothing,
    BrushOpacity,
    LayerOpacity,
    GradientOpacity,
    ShapeRadius,
    ShapeLineWidth,
    WandTolerance,
    ObjectEdge,
    SelectionExpand,
    SelectionContract,
    SelectionFeather,
    Field(usize),
    Parameter(usize, super::parameter_controls::Scale),
    JpegQuality,
}
impl Scalar {
    fn value(self, editor: &Editor) -> f64 {
        match self {
            Self::Effect(p) => match &editor.modal {
                Some(Form::Effects(e)) => e.number(p),
                _ => 0.,
            },
            Self::BrushSize => editor.tools.brush.diameter,
            Self::BrushSmoothing => editor.tools.brush_smoothing,
            Self::BrushHardness => editor.tools.brush.hardness * 100.,
            Self::BrushOpacity => editor.tools.brush.opacity * 100.,
            Self::GradientOpacity => editor.tools.gradient.opacity * 100.,
            Self::ShapeRadius => editor.tools.shape_radius,
            Self::ShapeLineWidth => editor.tools.shape_line_width,
            Self::WandTolerance => f64::from(editor.tools.wand_tolerance),
            Self::ObjectEdge => f64::from(editor.tools.object_edge_offset),
            Self::SelectionExpand => f64::from(editor.tools.selection_expand_amount),
            Self::SelectionContract => f64::from(editor.tools.selection_contract_amount),
            Self::SelectionFeather => f64::from(editor.tools.selection_feather_amount),
            Self::LayerOpacity => editor
                .current_document()
                .and_then(Document::active_layer)
                .map_or(100., |l| l.opacity * 100.),
            Self::Parameter(index, scale) => scale.position(Self::Field(index).value(editor)),
            Self::JpegQuality => match &editor.modal {
                Some(Form::Edit { fields, .. }) => {
                    fields.first().and_then(|f| f.1.parse().ok()).unwrap_or(85.)
                }
                _ => 85.,
            },
            Self::Field(index) => match editor.tool_form() {
                Some(Form::Edit { fields, .. }) => fields
                    .get(index)
                    .and_then(|f| f.1.parse().ok())
                    .unwrap_or(0.),
                _ => 0.,
            },
        }
    }
    fn set(self, editor: &mut Editor, value: f64) -> Result<()> {
        match self {
            Self::Effect(p) => editor.change_effect(|e| e.set_number(p, value)),
            Self::BrushSize => editor.tools.brush.diameter = value,
            Self::BrushSmoothing => editor.tools.brush_smoothing = value.clamp(0., 100.),
            Self::BrushHardness => editor.tools.brush.hardness = value / 100.,
            Self::BrushOpacity => editor.tools.brush.opacity = value / 100.,
            Self::GradientOpacity => {
                editor.tools.gradient.opacity = value / 100.;
                let result = editor.refresh_gradient();
                if let Err(error) = &result {
                    editor.show_error(alerts::Operation::Paint, error.to_string());
                }
                result?;
            }
            Self::ShapeRadius => editor.tools.shape_radius = value.round(),
            Self::ShapeLineWidth => editor.tools.shape_line_width = value.round(),
            Self::WandTolerance => editor.tools.wand_tolerance = value.round() as u8,
            Self::ObjectEdge => {
                editor.tools.object_edge_offset = value.round().clamp(-10., 10.) as i8
            }
            Self::SelectionExpand => editor.tools.selection_expand_amount = value.round() as u16,
            Self::SelectionFeather => editor.tools.selection_feather_amount = value.round() as u16,
            Self::SelectionContract => {
                editor.tools.selection_contract_amount = value.round() as u16
            }
            Self::LayerOpacity => {
                if let Some(layer) = editor.session_mut().document.active_layer_mut() {
                    layer.opacity = value / 100.;
                }
            }
            Self::Parameter(index, scale) => Self::Field(index).set(editor, scale.value(value))?,
            Self::JpegQuality => editor.update_form_field(0, &value.round().to_string()),
            Self::Field(index) => {
                editor.update_dimension(index, &value.to_string());
                editor.update_transform_scale(index);
                editor.refresh_adjustment();
                editor.refresh_filter();
                editor.refresh_jpeg();
            }
        }
        Ok(())
    }
}
impl Editor {
    pub(super) fn unit_suffix(input: Element, unit: &'static str) -> Element {
        div()
            .flex_row()
            .items_center()
            .gap(2.)
            .flex_shrink_0()
            .child(input)
            .child(text(unit).text_size(12.).line_height(15.))
    }
    pub(super) fn check_control(label: &'static str, checked: bool) -> Element {
        quickgui::Checkbox::new(checked)
            .root()
            .text_size(12.)
            .line_height(15.)
            .flex_shrink_0()
            .h(26.)
            .p(0.)
            .gap(6.)
            .flex_row()
            .items_center()
            .accessibility_label(label)
            .group()
            .child(
                div()
                    .w(13.)
                    .h(13.)
                    .flex_shrink_0()
                    .rounded(3.)
                    .group_focus(super::controls::focus_outline)
                    .border(1., Color::rgb8(112, 112, 112))
                    .bg(if checked {
                        Color::rgb8(89, 126, 170)
                    } else {
                        Color::rgb8(44, 44, 44)
                    })
                    .child(if checked {
                        Icon::Check.element(11.)
                    } else {
                        div()
                    }),
            )
            .child(text(label).whitespace_nowrap())
    }

    pub(super) fn scalar_slider(
        &self,
        cx: &mut ViewContext<'_, Self>,
        id: &'static str,
        label: &'static str,
        scalar: Scalar,
        range: (f64, f64),
        width: f32,
    ) -> Element {
        let step = if matches!(scalar, Scalar::Parameter(..)) {
            0.
        } else {
            0.01
        };
        let mut state = SliderState::new(range.0, range.1, range.0).step(step);
        // SliderState::new snaps using its default integer step. Set the value only
        // after configuring precision, so fractional and logarithmic values survive.
        state.set_value(scalar.value(self));
        let slider = Slider::new(id, &state);
        let pointer = cx.pointer_listener(id, move |this, event, cx| {
            if event.phase == PointerPhase::Down && event.button != quickgui::MouseButton::Left {
                return;
            }
            let travel = event.size.width - THUMB_SIZE;
            if !travel.is_finite() || travel <= 0. {
                return;
            }
            let layer = matches!(scalar, Scalar::LayerOpacity);
            if layer && event.phase == PointerPhase::Down {
                if !this.can_edit_opacity() {
                    return;
                }
                let result = this
                    .finish_pending_edits()
                    .and_then(|()| this.session_mut().begin("Layer Opacity"));
                if let Err(error) = result {
                    this.result(Err(error), cx);
                    return;
                }
            }
            let mut state = SliderState::new(range.0, range.1, range.0).step(step);
            state.set_value(scalar.value(this));
            if event.phase == PointerPhase::Down {
                cx.focus(quickgui::FocusHandle::new(id));
                let center = THUMB_SIZE / 2. + travel * state.fraction(0);
                let offset = event.local_position.x - center;
                let on_thumb = offset.abs() <= THUMB_SIZE / 2.
                    && (THUMB_TOP..=THUMB_TOP + THUMB_HEIGHT).contains(&event.local_position.y);
                this.slider_drag = Some(SliderDrag {
                    control: id,
                    start_offset: event.local_position.x
                        - THUMB_SIZE / 2.
                        - if on_thumb { offset } else { 0. },
                    travel,
                });
            }
            let Some(drag) = this.slider_drag.as_ref().filter(|drag| drag.control == id) else {
                return;
            };
            // Map the pointer to the knob's travel, retaining the initial grab
            // offset just like NSSlider. Track clicks use the knob center.
            // Retain press geometry so relayout cannot change a stationary value.
            let mut adjusted = *event;
            adjusted.local_position.x = drag.start_offset + event.position.x - event.origin.x;
            let change = state.apply_pointer_change(
                &adjusted,
                quickgui::Size::new(drag.travel, event.size.height),
            );
            if change.committed {
                this.slider_drag = None;
            }
            if change.values_changed {
                let result = scalar.set(this, state.values()[0]);
                if let Err(error) = result {
                    this.status = error.to_string();
                }
            }
            if layer && change.committed {
                if event.phase == PointerPhase::Cancel {
                    this.session_mut().cancel();
                } else {
                    let result = this.session_mut().commit();
                    this.result(result, cx);
                }
            }
            this.changed(cx);
        });
        let key = cx.key_down_listener(id, move |this, event, cx| {
            if matches!(scalar, Scalar::LayerOpacity) && !this.can_edit_opacity() {
                return;
            }
            let forward = matches!(event.key, Key::ArrowUp | Key::ArrowRight);
            if !forward
                && !matches!(
                    event.key,
                    Key::ArrowDown | Key::ArrowLeft | Key::Home | Key::End
                )
            {
                cx.propagate();
                return;
            }
            let mut state = SliderState::new(range.0, range.1, range.0)
                .step(if matches!(scalar, Scalar::Parameter(..)) {
                    (range.1 - range.0) / 100.
                } else {
                    1.
                })
                .large_step(if matches!(scalar, Scalar::Parameter(..)) {
                    (range.1 - range.0) / 10.
                } else {
                    10.
                });
            state.set_value(scalar.value(this));
            match event.key {
                Key::Home => {
                    state.active_to_minimum();
                }
                Key::End => {
                    state.active_to_maximum();
                }
                _ if event.modifiers.contains(Modifiers::SHIFT) => {
                    state.large_step_active(forward);
                }
                _ => {
                    state.step_active(if forward { 1. } else { -1. });
                }
            }
            let layer = matches!(scalar, Scalar::LayerOpacity);
            if layer && let Err(error) = this.session_mut().begin("Layer Opacity") {
                this.result(Err(error), cx);
                return;
            }
            let result = scalar.set(this, state.values()[0]);
            if let Err(error) = result {
                this.status = error.to_string();
            }
            if layer {
                let result = this.session_mut().commit();
                this.result(result, cx);
            }
            cx.prevent_default();
            cx.stop_propagation();
            this.changed(cx);
        });
        slider
            .root_with(div().w(width).h(24.).relative().flex_shrink_0())
            .accessibility_label(label)
            .group()
            .on_pointer(pointer)
            .on_key_down(key)
            .child(
                div()
                    .absolute()
                    .left(0.)
                    .top(9.)
                    .w(width)
                    .h(6.)
                    .rounded(3.)
                    .bg(Color::rgb8(83, 83, 83)),
            )
            .child(
                div()
                    .absolute()
                    .left(0.)
                    .top(9.)
                    .w(THUMB_SIZE / 2. + (width - THUMB_SIZE) * state.fraction(0))
                    .h(6.)
                    .rounded(3.)
                    .bg(Color::rgb8(0, 122, 255)),
            )
            .child(
                div()
                    .id(slider.thumb_id(0))
                    .absolute()
                    .left((width - THUMB_SIZE) * state.fraction(0))
                    .top(THUMB_TOP)
                    .w(THUMB_SIZE)
                    .h(THUMB_HEIGHT)
                    .rounded(8.)
                    .group_focus(super::controls::focus_outline)
                    .bg(Color::rgb8(216, 216, 216)),
            )
    }
    pub(super) fn layer_opacity_slider(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        self.scalar_slider(
            cx,
            "layer-opacity-slider",
            "Layer opacity",
            Scalar::LayerOpacity,
            (0., 100.),
            (self.panel_layout.width - 132.).max(40.),
        )
    }
    pub(super) fn brush_value(
        &self,
        cx: &mut ViewContext<'_, Self>,
        id: &'static str,
        scalar: Scalar,
        range: (f64, f64),
    ) -> Element {
        Self::text_field(format!("{:.0}", scalar.value(self)))
            .w(44.)
            .h(24.)
            .text_input_padding(5.)
            .rounded(5.)
            .text_size(12.)
            .line_height(15.)
            .bg(Color::rgb8(29, 29, 29))
            .border(1., Color::rgb8(67, 67, 67))
            .on_input(cx.input_listener(id, move |this, input, cx| {
                if let Ok(value) = input.parse::<f64>()
                    && value.is_finite()
                {
                    let result = scalar.set(this, value.clamp(range.0, range.1));
                    if let Err(error) = result {
                        this.status = error.to_string();
                    }
                    cx.invalidate();
                }
            }))
            .on_key_down(cx.key_down_listener(id, move |this, event, cx| {
                if matches!(event.key, Key::ArrowUp | Key::ArrowDown) {
                    let amount = if event.modifiers.contains(Modifiers::SHIFT) {
                        10.
                    } else {
                        1.
                    };
                    let value = scalar.value(this)
                        + if event.key == Key::ArrowUp {
                            amount
                        } else {
                            -amount
                        };
                    let result = scalar.set(this, value.clamp(range.0, range.1));
                    if let Err(error) = result {
                        this.status = error.to_string();
                    }
                    cx.prevent_default();
                    cx.stop_propagation();
                    cx.invalidate();
                } else if matches!(event.key, Key::Enter | Key::Escape) {
                    cx.focus(quickgui::FocusHandle::new("workspace"));
                    cx.prevent_default();
                    cx.stop_propagation();
                } else {
                    cx.stop_propagation();
                }
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thumb_press_preserves_value_and_drag_retains_the_grab_offset() {
        let mut editor = Editor::with_test_document();
        editor.tools.tool = Tool::Brush;
        editor.tools.brush.hardness = 0.2;
        let (mut cx, view) = quickgui::Application::new()
            .into_test_context(
                quickgui::WindowOptions::new("Slider knob drag").size(1500., 900.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        let id = "brush-hardness-slider";
        let track = cx.element_bounds(window, id).unwrap();
        let slider = Slider::new(id, &SliderState::new(0., 100., 20.));
        let thumb = cx.element_bounds(window, slider.thumb_id(0)).unwrap();
        let start = quickgui::Point::new(thumb.x + thumb.width - 2., thumb.y + thumb.height / 2.);
        cx.simulate_pointer_drag(window, id, start, start).unwrap();
        assert_eq!(cx.read(view, |e| e.tools.brush.hardness).unwrap(), 0.2);
        assert_eq!(cx.element_bounds(window, id).unwrap(), track);
        assert_eq!(
            cx.element_bounds(window, slider.thumb_id(0)).unwrap(),
            thumb
        );
        let end = quickgui::Point::new(start.x + (track.width - thumb.width) * 0.2, start.y);
        cx.simulate_pointer_drag(window, id, start, end).unwrap();
        let hardness = cx.read(view, |e| e.tools.brush.hardness).unwrap();
        assert!(
            (hardness - 0.4).abs() < 0.001,
            "hardness: {hardness}, track: {track:?}, thumb: {thumb:?}"
        );
        assert!(
            cx.read(view, |e| e.session().undo_label().is_none())
                .unwrap()
        );
    }
    use quickgui::{Application, Keystroke, Point, WindowOptions};

    #[test]
    fn layer_slider_knob_click_is_inert_and_cancel_restores_a_track_preview() {
        let mut editor = Editor::with_test_document();
        editor.session_mut().document.layers[0].opacity = 0.2;
        let original = editor.session().document.clone();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Opacity capture").size(1500., 900.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        let id = "layer-opacity-slider";
        let track = cx.element_bounds(window, id).unwrap();
        let thumb = cx
            .element_bounds(
                window,
                Slider::new(id, &SliderState::new(0., 100., 20.)).thumb_id(0),
            )
            .unwrap();
        let edge = Point::new(thumb.x + 2., thumb.y + thumb.height / 2.);
        cx.simulate_pointer_drag(window, id, edge, edge).unwrap();
        cx.read(view, |e| {
            assert_eq!(e.session().document, original);
            assert!(e.session().undo_label().is_none());
            assert!(!e.session().has_pending_edit());
            assert!(e.slider_drag.is_none());
        })
        .unwrap();
        let target = Point::new(
            track.x + THUMB_SIZE / 2. + (track.width - THUMB_SIZE) * 0.75,
            track.y + 12.,
        );
        let down = quickgui::PointerEvent {
            phase: PointerPhase::Down,
            position: target,
            origin: target,
            local_position: target,
            local_origin: target,
            delta: quickgui::Vector::ZERO,
            button: quickgui::MouseButton::Left,
            modifiers: Modifiers::empty(),
            size: quickgui::Size::ZERO,
        };
        cx.simulate_pointer(window, id, down).unwrap();
        cx.read(view, |e| {
            assert!((e.session().document.layers[0].opacity - 0.75).abs() < 0.001);
            assert!(e.session().has_pending_edit());
            assert!(e.session().undo_label().is_none());
        })
        .unwrap();
        cx.simulate_pointer(
            window,
            id,
            quickgui::PointerEvent {
                phase: PointerPhase::Cancel,
                ..down
            },
        )
        .unwrap();
        cx.read(view, |e| {
            assert_eq!(e.session().document, original);
            assert!(e.session().undo_label().is_none());
            assert!(!e.session().has_pending_edit());
            assert!(e.slider_drag.is_none());
        })
        .unwrap();
    }

    #[test]
    fn failed_gradient_opacity_redraw_cancels_the_preview_and_alerts_from_every_control() {
        for input in [
            "text",
            "field-key",
            "slider-key",
            "slider-pointer",
            "opacity-shortcut",
            "palette-swap",
            "palette-apply",
        ] {
            let mut editor = Editor::with_test_document();
            let original = Document::new(30_000, 30_000).unwrap();
            editor.tabs = vec![Session::new(original.clone(), None).into()];
            editor.tools.tool = Tool::Gradient;
            editor.begin_gradient([0., 0.]).unwrap();
            // A pending line whose full-canvas raster exceeds the source budget.
            // Redraw must reject it before allocating pixels.
            editor.pending_gradient.as_mut().unwrap().end = [10., 10.];
            if input == "palette-apply" {
                editor.open_form(Action::Color);
            }
            let (mut cx, view) = Application::new()
                .into_test_context(
                    WindowOptions::new("Gradient redraw failure").size(1500., 900.),
                    editor,
                )
                .unwrap();
            let window = view.window_handle();
            match input {
                "opacity-shortcut" | "palette-swap" => {
                    cx.focus(window, "workspace").unwrap();
                    cx.simulate_keystrokes(window, if input == "palette-swap" { "x" } else { "5" })
                        .unwrap();
                }
                "palette-apply" => cx.click(window, "form-apply").unwrap(),
                "text" => {
                    cx.focus(window, "gradient-opacity").unwrap();
                    cx.simulate_keystrokes(window, "ctrl-a").unwrap();
                    cx.simulate_input(window, "50").unwrap();
                }
                "field-key" | "slider-key" => {
                    cx.focus(
                        window,
                        if input == "field-key" {
                            "gradient-opacity"
                        } else {
                            "gradient-opacity-slider"
                        },
                    )
                    .unwrap();
                    cx.simulate_keystrokes(window, "down").unwrap();
                }
                _ => {
                    let bounds = cx
                        .element_bounds(window, "gradient-opacity-slider")
                        .unwrap();
                    let point =
                        Point::new(bounds.x + bounds.width / 2., bounds.y + bounds.height / 2.);
                    cx.simulate_pointer_drag(window, "gradient-opacity-slider", point, point)
                        .unwrap();
                }
            }
            cx.read(view, |e| {
                assert_eq!(e.errors.len(), 1, "{input}: {}", e.status);
                assert_eq!(
                    e.errors.front().unwrap().operation,
                    alerts::Operation::Paint
                );
                assert!(e.pending_gradient.is_none());
                assert!(e.modal.is_none());
                assert!(!e.session().has_pending_edit());
                assert_eq!(e.session().document, original);
                assert!(e.session().undo_label().is_none());
            })
            .unwrap();
            cx.click(window, "error-ok").unwrap();
            assert!(cx.read(view, |e| e.errors.is_empty()).unwrap());
        }
    }
    #[test]
    fn opacity_slider_previews_then_records_one_undo_and_supports_keyboard() {
        let editor = Editor::with_test_document();
        let original = editor.session().document.clone();
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Opacity").size(1280., 900.), editor)
            .unwrap();
        let window = view.window_handle();
        let bounds = cx.element_bounds(window, "layer-opacity-slider").unwrap();
        cx.simulate_pointer_drag(
            window,
            "layer-opacity-slider",
            Point::new(bounds.x + bounds.width - THUMB_SIZE / 2., bounds.y + 12.),
            Point::new(
                bounds.x + THUMB_SIZE / 2. + (bounds.width - THUMB_SIZE) * 0.25,
                bounds.y + 12.,
            ),
        )
        .unwrap();
        let opacity = cx
            .read(view, |e| e.session().document.layers[0].opacity)
            .unwrap();
        assert!((opacity - 0.25).abs() < 0.001);
        cx.update(view, |e, cx| {
            e.session_mut().undo();
            e.changed(cx);
        })
        .unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            original
        );
        cx.focus(window, "layer-opacity-slider").unwrap();
        cx.simulate_keystroke(window, Keystroke::new(Key::ArrowLeft, Modifiers::SHIFT))
            .unwrap();
        assert!(
            (cx.read(view, |e| e.session().document.layers[0].opacity)
                .unwrap()
                - 0.9)
                .abs()
                < 0.001
        );
    }
}
