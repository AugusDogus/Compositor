use super::*;
use quickgui::{Application, WindowOptions};

fn editor() -> Editor {
    let mut editor = Editor::with_test_document();
    editor.tabs = vec![Session::new(Document::new(9, 1).unwrap(), None).into()];
    editor.tools.tool = Tool::Gradient;
    editor.tools.brush.color = [255, 0, 0, 255];
    editor.tools.background = [0, 0, 255, 255];
    editor.tools.gradient.style = Style::ForegroundToBackground;
    editor.begin_gradient([0.5, 0.5]).unwrap();
    editor
        .move_gradient([8.5, 0.5], super::super::gradient::Endpoint::End, false)
        .unwrap();
    editor
}

#[test]
fn stop_editor_previews_colors_and_alpha_then_applies_as_one_undo_step() {
    let editor = editor();
    let original = editor.session().committed_document().clone();
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Gradient stops").size(1280., 900.),
            editor,
        )
        .unwrap();
    let window = view.window_handle();
    cx.click(window, "gradient-swatch").unwrap();
    cx.click(window, "gradient-stop-add").unwrap();
    cx.click(window, "gradient-stop-color").unwrap();
    cx.focus(window, 51_003_u64).unwrap();
    cx.simulate_keystrokes(window, "ctrl-a").unwrap();
    cx.simulate_input(window, "#00ff00").unwrap();
    cx.simulate_keystrokes(window, "enter").unwrap();
    cx.click(window, "form-apply").unwrap();
    cx.focus(window, "gradient-stop-opacity").unwrap();
    cx.simulate_keystrokes(window, "ctrl-a").unwrap();
    cx.simulate_input(window, "50").unwrap();
    cx.read(view, |e| {
        assert_eq!(e.tools.gradient.stops.as_slice().len(), 3);
        assert_eq!(
            e.session().document.layers[0].raster().unwrap()[(4, 0)].0,
            [0, 255, 0, 128]
        );
        assert_eq!(e.session().committed_document(), &original);
        assert!(e.session().undo_label().is_none());
    })
    .unwrap();
    cx.click(window, "gradient-stops-apply").unwrap();
    cx.click(window, "gradient-apply").unwrap();
    cx.update(view, |e, _| {
        assert_eq!(e.session().undo_label(), Some("Gradient"));
        let after = e.session().document.clone();
        e.session_mut().undo();
        assert_eq!(e.session().document, original);
        e.session_mut().redo();
        assert_eq!(e.session().document, after);
    })
    .unwrap();
}

#[test]
fn stop_cancel_restores_settings_and_preview_and_invalid_input_does_not_apply() {
    let mut e = editor();
    let initial = e.session().document.clone();
    let settings = e.tools.gradient.clone();
    e.open_gradient_stops().unwrap();
    e.add_gradient_stop().unwrap();
    e.gradient_stop_input(true, "25").unwrap();
    e.set_gradient_stop_color(1, [0, 255, 0, 255]).unwrap();
    let last_valid = e.session().document.clone();
    e.gradient_stop_input(true, "NaN").unwrap();
    assert!(e.finish_gradient_stops(true).is_err());
    assert_eq!(e.session().document, last_valid);
    e.finish_gradient_stops(false).unwrap();
    assert_eq!(e.tools.gradient, settings);
    assert_eq!(e.session().document, initial);
    assert!(e.session().has_pending_edit());
    assert!(e.session().undo_label().is_none());
}

#[test]
fn stop_drag_remove_and_escape_preserve_the_existing_gradient_preview() {
    let editor = editor();
    let initial = editor.session().document.clone();
    let settings = editor.tools.gradient.clone();
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Gradient drag").size(900., 700.), editor)
        .unwrap();
    let window = view.window_handle();
    cx.click(window, "gradient-swatch").unwrap();
    cx.click(window, "gradient-stop-add").unwrap();
    let bounds = cx.element_bounds(window, "gradient-stop-track").unwrap();
    cx.simulate_pointer_drag(
        window,
        "gradient-stop-track",
        quickgui::Point::new(bounds.x + bounds.width * 0.5, bounds.y + 10.),
        quickgui::Point::new(bounds.x + bounds.width * 0.75, bounds.y + 10.),
    )
    .unwrap();
    cx.read(view, |e| {
        assert!((e.tools.gradient.stops.as_slice()[1].position - 0.75).abs() < 0.02)
    })
    .unwrap();
    cx.click(window, "gradient-stop-remove").unwrap();
    cx.read(view, |e| {
        assert_eq!(e.tools.gradient.stops.as_slice().len(), 2)
    })
    .unwrap();
    cx.simulate_keystrokes(window, "escape").unwrap();
    cx.read(view, |e| {
        assert!(e.modal.is_none());
        assert_eq!(e.tools.gradient, settings);
        assert_eq!(e.session().document, initial);
        assert!(e.pending_gradient.is_some());
    })
    .unwrap();
}

#[test]
fn mask_previews_show_luminance_without_replacing_custom_rgb_stops() {
    let mut editor = editor();
    editor.pending_gradient = None;
    editor.session_mut().cancel();
    compositor::edits::add_mask(&mut editor.session_mut().document, false).unwrap();
    editor.tools.mask_target = true;
    editor.tools.gradient.style = Style::Custom;
    editor.tools.gradient.stops = Stops::endpoints([0, 255, 0, 255], [0, 255, 0, 255]);
    let original = editor.tools.gradient.clone();
    assert_eq!(editor.gradient_preview_colors(), [[182, 182, 182, 255]; 2]);
    editor.open_gradient_stops().unwrap();
    editor.finish_gradient_stops(true).unwrap();
    assert_eq!(editor.tools.gradient, original);
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Mask stop preview").size(1280., 900.),
            editor,
        )
        .unwrap();
    let window = view.window_handle();
    let bounds = cx.element_bounds(window, "gradient-swatch").unwrap();
    let frame = cx.capture_screenshot(window).unwrap();
    let scale = frame.width() as f32 / 1280.;
    let pixel = frame
        .pixel(
            ((bounds.x + bounds.width / 2.) * scale) as u32,
            ((bounds.y + bounds.height / 2.) * scale) as u32,
        )
        .unwrap();
    assert_eq!(pixel[0], pixel[1]);
    assert_eq!(pixel[1], pixel[2]);
    assert!(
        (180..=184).contains(&pixel[0]),
        "Mask swatch should show green luminance: {pixel:?}"
    );
    cx.read(view, |e| {
        assert_eq!(e.tools.gradient.stops.as_slice()[0].color, [0, 255, 0, 255])
    })
    .unwrap();
}

#[test]
fn choosing_custom_restores_saved_stops_and_cancel_restores_the_previous_style() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Saved custom gradient").size(1280., 900.),
            editor(),
        )
        .unwrap();
    let window = view.window_handle();
    cx.click(window, "gradient-swatch").unwrap();
    cx.click(window, "gradient-stop-add").unwrap();
    cx.update(view, |e, cx| {
        e.gradient_stop_input(true, "37.5").unwrap();
        e.gradient_stop_input(false, "25").unwrap();
        e.set_gradient_stop_color(1, [15, 220, 80, 64]).unwrap();
        cx.invalidate();
    })
    .unwrap();
    cx.click(window, "gradient-stops-apply").unwrap();
    let saved = cx.read(view, |e| e.tools.gradient.stops.clone()).unwrap();
    assert_eq!(saved.as_slice().len(), 3);
    cx.click(window, "gradient-style").unwrap();
    cx.simulate_keystrokes(window, "home enter").unwrap();
    cx.read(view, |e| {
        assert_eq!(e.tools.gradient.style, Style::ForegroundToBackground)
    })
    .unwrap();
    cx.click(window, "gradient-style").unwrap();
    cx.simulate_keystrokes(window, "end enter").unwrap();
    cx.read(view, |e| {
        assert!(matches!(e.modal, Some(Form::GradientStops(_))));
        assert_eq!(e.tools.gradient.stops, saved);
    })
    .unwrap();
    cx.click(window, "gradient-stop-add").unwrap();
    cx.click(window, "gradient-stops-cancel").unwrap();
    cx.read(view, |e| {
        assert_eq!(e.tools.gradient.style, Style::ForegroundToBackground);
        assert_eq!(e.tools.gradient.stops, saved);
    })
    .unwrap();
    // Editing a displayed preset explicitly seeds a new ramp, without losing
    // the saved custom ramp if the user cancels that editor.
    cx.click(window, "gradient-swatch").unwrap();
    cx.read(view, |e| {
        assert_eq!(e.tools.gradient.stops.as_slice().len(), 2)
    })
    .unwrap();
    cx.click(window, "gradient-stops-cancel").unwrap();
    cx.click(window, "gradient-style").unwrap();
    cx.simulate_keystrokes(window, "end enter").unwrap();
    cx.click(window, "gradient-stops-apply").unwrap();
    cx.read(view, |e| {
        assert_eq!(e.tools.gradient.style, Style::Custom);
        assert_eq!(e.tools.gradient.stops, saved);
    })
    .unwrap();
}
