//! Horizontal label drags edit the adjacent number without moving focus into its text field.
use super::{scalar_controls::Scalar, *};
use quickgui::{ElementId, MouseButton, PointerPhase};

pub(super) struct Drag {
    id: ElementId,
    start: f64,
    active: bool,
}
impl Editor {
    pub(super) fn scrub_unit_suffix(
        &self,
        input: Element,
        unit: &'static str,
        cx: &mut ViewContext<'_, Self>,
        id: impl Into<ElementId>,
        scalar: Scalar,
        range: (f64, f64),
    ) -> Element {
        div()
            .flex_row()
            .items_center()
            .gap(2.)
            .flex_shrink_0()
            .child(input)
            .child(self.scrub_label(
                cx,
                id,
                scalar,
                range,
                1.,
                text(unit).text_size(12.).line_height(15.),
            ))
    }
    pub(super) fn scrub_label(
        &self,
        cx: &mut ViewContext<'_, Self>,
        id: impl Into<ElementId>,
        scalar: Scalar,
        range: (f64, f64),
        step: f64,
        label: Element,
    ) -> Element {
        let id = id.into();
        label
            .id(id)
            .cursor(quickgui::CursorStyle::ResizeLeftRight)
            .on_pointer(cx.pointer_listener(id, move |this, event, cx| {
                if event.phase == PointerPhase::Down {
                    if event.button != MouseButton::Left
                        || this.filter_applying()
                        || this.pending
                        || !scalar.can_edit(this)
                    {
                        return;
                    }
                    let start = scalar.value(this);
                    if !start.is_finite() {
                        return;
                    }
                    this.camera_slider_preview(scalar, event);
                    this.numeric_scrub = Some(Drag {
                        id,
                        start,
                        active: false,
                    });
                    return;
                }
                let Some(drag) = this.numeric_scrub.as_ref().filter(|drag| drag.id == id) else {
                    return;
                };
                let (start, active) = (drag.start, drag.active);
                let delta = f64::from(event.position.x - event.origin.x);
                if !active && delta.abs() >= 1. && event.phase != PointerPhase::Cancel {
                    if let Err(error) = scalar.begin_edit(this) {
                        this.numeric_scrub = None;
                        this.result(Err(error), cx);
                        return;
                    }
                    if let Some(drag) = &mut this.numeric_scrub {
                        drag.active = true;
                    }
                }
                let active = this.numeric_scrub.as_ref().is_some_and(|drag| drag.active);
                this.camera_slider_preview(scalar, event);
                if active {
                    let next = if event.phase == PointerPhase::Cancel {
                        start
                    } else {
                        (((start + delta * step) / step).round() * step).clamp(range.0, range.1)
                    };
                    let result = scalar.set(this, next);
                    if let Err(error) = result {
                        this.result(Err(error), cx);
                    }
                }
                if matches!(event.phase, PointerPhase::Up | PointerPhase::Cancel) {
                    this.numeric_scrub = None;
                    if active
                        && let Err(error) =
                            scalar.end_edit(this, event.phase == PointerPhase::Cancel)
                    {
                        this.status = error.to_string();
                    }
                }
                if active {
                    cx.prevent_default();
                    cx.stop_propagation();
                    this.changed(cx);
                }
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, Point, WindowOptions};

    #[test]
    fn label_click_is_inert_and_brush_drag_changes_one_unit_per_pixel() {
        let mut editor = Editor::with_test_document();
        editor.tools.tool = Tool::Brush;
        editor.tools.brush.diameter = 100.;
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Numeric scrub").size(1600., 1000.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        let bounds = cx.element_bounds(window, "brush-size-label").unwrap();
        let start = Point::new(bounds.x + 3., bounds.y + 5.);
        cx.simulate_pointer_drag(window, "brush-size-label", start, start)
            .unwrap();
        assert_eq!(cx.read(view, |e| e.tools.brush.diameter).unwrap(), 100.);
        cx.simulate_pointer_drag(
            window,
            "brush-size-label",
            start,
            Point::new(start.x + 25., start.y),
        )
        .unwrap();
        assert_eq!(cx.read(view, |e| e.tools.brush.diameter).unwrap(), 125.);
        assert!(
            cx.read(view, |e| e.session().undo_label().is_none())
                .unwrap()
        );
        cx.simulate_pointer_drag(
            window,
            "brush-size-label",
            start,
            Point::new(start.x - 1000., start.y),
        )
        .unwrap();
        assert_eq!(cx.read(view, |e| e.tools.brush.diameter).unwrap(), 1.);
    }

    #[test]
    fn layer_drag_is_one_undo_step_and_cancel_restores_the_original() {
        let mut editor = Editor::with_test_document();
        editor.session_mut().document.layers[0].opacity = 0.5;
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Opacity scrub").size(1500., 900.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        let bounds = cx.element_bounds(window, "layer-opacity-label").unwrap();
        let start = Point::new(bounds.x + 3., bounds.y + 5.);
        cx.simulate_pointer_drag(
            window,
            "layer-opacity-label",
            start,
            Point::new(start.x + 20., start.y),
        )
        .unwrap();
        cx.update(view, |e, _| {
            assert_eq!(e.session().document.layers[0].opacity, 0.7);
            assert_eq!(e.session().undo_label(), Some("Layer Opacity"));
            e.session_mut().undo();
        })
        .unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.layers[0].opacity)
                .unwrap(),
            0.5
        );
        let down = quickgui::PointerEvent {
            tablet: None,
            phase: PointerPhase::Down,
            position: start,
            origin: start,
            local_position: start,
            local_origin: start,
            delta: quickgui::Vector::ZERO,
            button: MouseButton::Left,
            modifiers: Modifiers::empty(),
            size: quickgui::Size::ZERO,
        };
        cx.simulate_pointer(window, "layer-opacity-label", down)
            .unwrap();
        cx.simulate_pointer(
            window,
            "layer-opacity-label",
            quickgui::PointerEvent {
                phase: PointerPhase::Move,
                position: Point::new(start.x + 30., start.y),
                ..down
            },
        )
        .unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.layers[0].opacity)
                .unwrap(),
            0.8
        );
        cx.simulate_pointer(
            window,
            "layer-opacity-label",
            quickgui::PointerEvent {
                phase: PointerPhase::Cancel,
                ..down
            },
        )
        .unwrap();
        cx.read(view, |e| {
            assert_eq!(e.session().document.layers[0].opacity, 0.5);
            assert!(!e.session().has_pending_edit());
            assert!(e.numeric_scrub.is_none());
        })
        .unwrap();
    }

    #[test]
    fn logarithmic_filter_slider_label_scrubs_in_linear_display_units() {
        let mut editor = Editor::with_test_document();
        compositor::edits::fill(
            &mut editor.session_mut().document,
            [100, 100, 100, 255],
            false,
            false,
        )
        .unwrap();
        editor
            .open_filter(compositor::filters::Filter::Gaussian { radius: 1. })
            .unwrap();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Radius scrub").size(1500., 1000.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        let bounds = cx
            .element_bounds(window, "adjustment-reset-label-0")
            .unwrap();
        let start = Point::new(bounds.x + 3., bounds.y + 5.);
        cx.simulate_pointer_drag(
            window,
            "adjustment-reset-label-0",
            start,
            Point::new(start.x + 10., start.y),
        )
        .unwrap();
        cx.read(view, |e| {
            let Some(Form::Edit { fields, .. }) = &e.modal else {
                panic!("Filter panel closed");
            };
            assert_eq!(fields[0].1, "2");
        })
        .unwrap();
    }
    #[test]
    fn image_size_scrubbing_keeps_aspect_ratio_and_print_units_change_resolution_only() {
        let mut editor = Editor::with_test_document();
        let mut doc = Document::new(200, 100).unwrap();
        doc.resolution = 100.;
        editor.tabs = vec![Session::new(doc.clone(), None).into()];
        editor.open_form(Action::ImageSize);
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Size scrub").size(1500., 1000.), editor)
            .unwrap();
        let window = view.window_handle();
        let bounds = cx.element_bounds(window, "size-label-0").unwrap();
        let start = Point::new(bounds.x + 3., bounds.y + 5.);
        cx.simulate_pointer_drag(
            window,
            "size-label-0",
            start,
            Point::new(start.x + 10., start.y),
        )
        .unwrap();
        cx.read(view, |e| {
            let Some(Form::Edit { fields, .. }) = &e.modal else {
                panic!("Size sheet closed");
            };
            assert_eq!(fields[0].1, "210");
            assert_eq!(fields[1].1, "105");
            assert_eq!(e.session().document, doc);
        })
        .unwrap();
        cx.click(window, "image-size-resample").unwrap();
        let before = cx
            .read(view, |e| {
                let Some(Form::Edit { fields, .. }) = &e.modal else {
                    panic!("Size sheet closed");
                };
                fields[2].1.parse::<f64>().unwrap()
            })
            .unwrap();
        let bounds = cx.element_bounds(window, "size-label-0").unwrap();
        let start = Point::new(bounds.x + 3., bounds.y + 5.);
        cx.simulate_pointer_drag(
            window,
            "size-label-0",
            start,
            Point::new(start.x + 10., start.y),
        )
        .unwrap();
        cx.read(view, |e| {
            let Some(Form::Edit { fields, .. }) = &e.modal else {
                panic!("Size sheet closed");
            };
            assert!(!e.image_sizing.resamples());
            assert!(fields[2].1.parse::<f64>().unwrap() < before);
            assert_eq!(e.session().document, doc);
        })
        .unwrap();
    }
}
