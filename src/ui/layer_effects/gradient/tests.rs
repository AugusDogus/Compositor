use super::*;
use compositor::gradient_overlay::OpacityStop;
use quickgui::{Application, WindowOptions};
fn editor() -> Editor {
    let mut e = Editor::with_test_document();
    compositor::edits::fill(&mut e.session_mut().document, [255; 4], false, false).unwrap();
    e.open_effects().unwrap();
    e.change_effect(|edit| {
        edit.kind = EffectKind::Gradient;
        edit.effects.gradient_overlay = Some(Box::default());
        edit.sync_overlay_stop();
    });
    e
}
#[test]
fn gradient_stop_errors_preserve_preview_and_cancel_restores_source() {
    let mut e = editor();
    let preview = e.session().document.clone();
    e.overlay_stop_input(false, "not a color");
    assert_eq!(e.session().document, preview);
    assert!(e.finish_effects(true).is_err());
    e.change_effect(|edit| edit.set_number(Parameter::Angle, 45.));
    assert!(e.finish_effects(true).is_err());
    e.overlay_stop_input(false, "#ff0000");
    let Some(Form::Effects(edit)) = &e.modal else {
        panic!("Missing effects editor");
    };
    assert!(edit.error.is_empty());
    assert_eq!(
        edit.effects
            .gradient_overlay
            .as_ref()
            .unwrap()
            .stops
            .as_slice()[0]
            .color,
        [255, 0, 0, 255]
    );
    e.finish_effects(false).unwrap();
    assert!(
        e.session()
            .document
            .active_layer()
            .unwrap()
            .effects
            .is_none()
    );
    assert!(e.session().undo_label().is_none());
}
#[test]
fn gradient_overlay_ui_edits_independent_stops_and_preserves_undo() {
    let e = editor();
    let pixels = e
        .session()
        .document
        .active_layer()
        .unwrap()
        .raster()
        .unwrap()
        .clone();
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(WindowOptions::new("Gradient Overlay").size(1180., 780.), e)
        .unwrap();
    let window = view.window_handle();
    cx.click(window, "overlay-stop-add").unwrap();
    cx.update(view, |e, _| {
        e.overlay_stop_input(false, "#ff0000");
        e.overlay_stop_input(true, "30");
    })
    .unwrap();
    let colors = cx
        .read(view, |e| {
            e.session()
                .document
                .active_layer()
                .unwrap()
                .effects
                .as_ref()
                .unwrap()
                .gradient_overlay
                .as_ref()
                .unwrap()
                .stops
                .clone()
        })
        .unwrap();
    cx.click(window, "overlay-channel-Opacity stops").unwrap();
    cx.click(window, "overlay-stop-add").unwrap();
    cx.update(view, |e, _| {
        e.overlay_stop_input(false, "25");
        e.overlay_stop_input(true, "70");
    })
    .unwrap();
    cx.click(window, "overlay-style-Radial").unwrap();
    cx.click(window, "overlay-reverse").unwrap();
    cx.read(view, |e| {
        let layer = e.session().document.active_layer().unwrap();
        let overlay = layer
            .effects
            .as_ref()
            .unwrap()
            .gradient_overlay
            .as_ref()
            .unwrap();
        assert_eq!(overlay.stops, colors);
        assert_eq!(
            overlay.opacity_stops.as_slice()[1],
            OpacityStop {
                position: 0.7,
                opacity: 0.25
            }
        );
        assert_eq!(overlay.style, Style::Radial);
        assert!(overlay.reverse);
        assert_eq!(layer.raster().unwrap(), &pixels);
    })
    .unwrap();
    if let Some(path) = std::env::var_os("COMPOSITOR_GRADIENT_OVERLAY_SCREENSHOT") {
        cx.update(view, |e, cx| {
            e.resolve_test_canvas_preview().unwrap();
            cx.invalidate();
        })
        .unwrap();
        cx.capture_screenshot(window)
            .unwrap()
            .write_png(path)
            .unwrap();
    }
    cx.click(window, "effects-apply").unwrap();
    cx.update(view, |e, _| {
        assert_eq!(e.session().undo_label(), Some("Layer Effects"));
        e.open_effects().unwrap();
        let Some(Form::Effects(edit)) = &e.modal else {
            panic!("Missing effects editor");
        };
        assert!(edit.kind == EffectKind::Gradient);
        assert_eq!(
            edit.effects.gradient_overlay.as_ref().unwrap().stops,
            colors
        );
        e.finish_effects(false).unwrap();
        e.session_mut().undo();
        assert!(
            e.session()
                .document
                .active_layer()
                .unwrap()
                .effects
                .is_none()
        );
    })
    .unwrap();
}

#[test]
fn gradient_color_picker_previews_cancels_and_commits_selected_stop() {
    let mut e = editor();
    let original = e.session().document.clone();
    e.open_effect_color_picker();
    e.picker_input(Some(0), "255");
    e.preview_picker().unwrap();
    assert_ne!(e.session().document, original);
    e.finish_color(false).unwrap();
    assert_eq!(e.session().document, original);
    e.open_effect_color_picker();
    e.picker_input(Some(1), "200");
    e.preview_picker().unwrap();
    e.finish_color(true).unwrap();
    let Some(Form::Effects(edit)) = &e.modal else {
        panic!("Missing effects editor");
    };
    assert_eq!(edit.overlay_color(), [0, 200, 0, 255]);
    assert_eq!(edit.gradient.value, "#00C800");
    e.finish_effects(true).unwrap();
    e.session_mut().undo();
    assert!(
        e.session()
            .document
            .active_layer()
            .unwrap()
            .effects
            .is_none()
    );
}

#[test]
fn editing_one_stop_field_preserves_other_fields_precision() {
    let mut e = editor();
    e.change_effect(|edit| {
        let overlay = edit.effects.gradient_overlay.as_mut().unwrap();
        overlay
            .stops
            .update(
                0,
                compositor::gradient::stops::Stop {
                    position: 0.123456789,
                    color: [0, 0, 0, 255],
                },
            )
            .unwrap();
        overlay
            .opacity_stops
            .update(
                0,
                OpacityStop {
                    position: 0.234567891,
                    opacity: 0.987654321,
                },
            )
            .unwrap();
        edit.sync_overlay_stop();
    });
    e.overlay_stop_input(false, "#112233");
    e.overlay_stop_select(Channel::Opacity, 0);
    e.overlay_stop_input(true, "40");
    let overlay = e
        .session()
        .document
        .active_layer()
        .unwrap()
        .effects
        .as_ref()
        .unwrap()
        .gradient_overlay
        .as_ref()
        .unwrap();
    assert_eq!(overlay.stops.as_slice()[0].position, 0.123456789);
    assert_eq!(overlay.opacity_stops.as_slice()[0].opacity, 0.987654321);
    e.overlay_stop_input(false, "50");
    let overlay = e
        .session()
        .document
        .active_layer()
        .unwrap()
        .effects
        .as_ref()
        .unwrap()
        .gradient_overlay
        .as_ref()
        .unwrap();
    assert_eq!(overlay.opacity_stops.as_slice()[0].position, 0.4);
}
#[test]
fn canceling_picker_preserves_invalid_draft_and_last_valid_preview() {
    let mut e = editor();
    e.overlay_stop_input(false, "invalid");
    let preview = e.session().document.clone();
    e.open_effect_color_picker();
    e.picker_input(Some(0), "255");
    e.preview_picker().unwrap();
    assert_ne!(e.session().document, preview);
    e.finish_color(false).unwrap();
    assert_eq!(e.session().document, preview);
    assert!(e.finish_effects(true).is_err());
    let Some(Form::Effects(edit)) = &e.modal else {
        panic!("Missing effects draft");
    };
    assert_eq!(edit.gradient.value, "invalid");
    e.finish_effects(false).unwrap();
}

#[test]
fn repairing_invalid_input_keeps_valid_edits_to_other_stop_fields() {
    let mut e = editor();
    let before = e.session().document.clone();
    e.overlay_stop_input(false, "bad hex");
    e.overlay_stop_input(true, "30");
    assert_eq!(e.session().document, before);
    e.overlay_stop_input(false, "#112233");
    let overlay = e
        .session()
        .document
        .active_layer()
        .unwrap()
        .effects
        .as_ref()
        .unwrap()
        .gradient_overlay
        .as_ref()
        .unwrap();
    assert_eq!(
        overlay.stops.as_slice()[0],
        compositor::gradient::stops::Stop {
            position: 0.3,
            color: [17, 34, 51, 255]
        }
    );
    e.overlay_stop_select(Channel::Opacity, 0);
    e.overlay_stop_input(true, "invalid");
    e.overlay_stop_input(false, "25");
    e.overlay_stop_input(true, "40");
    let overlay = e
        .session()
        .document
        .active_layer()
        .unwrap()
        .effects
        .as_ref()
        .unwrap()
        .gradient_overlay
        .as_ref()
        .unwrap();
    assert_eq!(
        overlay.opacity_stops.as_slice()[0],
        OpacityStop {
            position: 0.4,
            opacity: 0.25
        }
    );
}
