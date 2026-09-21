use super::*;
use quickgui::{Application, WindowOptions};

fn values(editor: &Editor) -> Vec<String> {
    let Some(Form::Edit { fields, .. }) = &editor.modal else {
        panic!("Size sheet closed unexpectedly")
    };
    fields.iter().map(|field| field.1.clone()).collect()
}

#[test]
fn size_dropdown_paints_above_overlapping_dimension_fields() {
    for action in [Action::CanvasSize, Action::ImageSize] {
        let (mut cx, view) = Application::new()
            .font(crate::UI_FONT)
            .into_test_context(
                WindowOptions::new("Size dropdown stacking").size(1000., 850.),
                editor(action),
            )
            .unwrap();
        let window = view.window_handle();
        cx.click(window, "size-unit").unwrap();
        let popup = cx
            .element_bounds(window, quickgui::SelectState::<()>::surface_id("size-unit"))
            .unwrap();
        let field = cx.element_bounds(window, 50_000_u64).unwrap();
        let point = quickgui::Point::new(popup.x + popup.width - 10., field.y + field.height / 2.);
        assert!(point.y > popup.y && point.y < popup.y + 28.);
        let frame = cx.capture_screenshot(window).unwrap();
        let scale = frame.width() as f32 / 1000.;
        assert_eq!(
            frame.pixel((point.x * scale) as u32, (point.y * scale) as u32),
            Some([0, 106, 216, 255]),
            "The selected menu row must cover the width field"
        );
    }
}
fn editor(action: Action) -> Editor {
    let mut editor = Editor::with_test_document();
    let mut doc = Document::new(200, 100).unwrap();
    doc.resolution = 100.;
    editor.tabs = vec![Session::new(doc, None).into()];
    editor.open_form(action);
    editor
}

#[test]
fn canvas_dropdown_keyboard_changes_units_without_applying_or_resizing() {
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .bind_keys(quickgui::select_key_bindings())
        .into_test_context(
            WindowOptions::new("Canvas Size").size(1000., 850.),
            editor(Action::CanvasSize),
        )
        .unwrap();
    let window = view.window_handle();
    cx.click(window, "size-unit").unwrap();
    assert!(cx.read(view, |e| e.size_menus.units.is_open()).unwrap());
    let trigger = cx.element_bounds(window, "size-unit").unwrap();
    let popup = cx
        .element_bounds(window, quickgui::SelectState::<()>::surface_id("size-unit"))
        .unwrap();
    assert!((popup.x - trigger.x).abs() <= 1.);
    assert!(popup.y >= trigger.y + trigger.height);
    assert!(popup.y <= trigger.y + trigger.height + 8.);
    cx.simulate_keystrokes(window, "down enter").unwrap();
    assert!(!cx.read(view, |e| e.size_menus.units.is_open()).unwrap());
    cx.read(view, |e| {
        assert_eq!(&values(e)[..2], &["100", "100"]);
        assert_eq!(values(e)[4], "percent");
        assert_eq!(
            (e.session().document.width, e.session().document.height),
            (200, 100)
        );
        assert!(e.session().undo_label().is_none());
    })
    .unwrap();
    cx.click(window, 50_005_u64).unwrap();
    assert_eq!(&cx.read(view, values).unwrap()[..2], &["0", "0"]);
    cx.update(view, |e, cx| {
        e.update_form_field(0, "25");
        e.changed(cx);
    })
    .unwrap();
    cx.click(window, "dimension-link").unwrap();
    assert_eq!(&cx.read(view, values).unwrap()[..2], &["25", "25"]);
    cx.click(window, "anchor-bottom-right").unwrap();
    cx.click(window, "form-apply").unwrap();
    cx.read(view, |e| {
        assert!(e.modal.is_none());
        assert_eq!(
            (e.session().document.width, e.session().document.height),
            (250, 125)
        );
        assert_eq!(e.session().undo_label(), Some("Canvas Size"));
    })
    .unwrap();
}

#[test]
fn canvas_black_and_white_presets_hide_the_color_well_and_fill_new_pixels() {
    for (keys, name, expected) in [
        ("home down down down enter", "black", [0., 0., 0., 1.]),
        ("home down down down down enter", "white", [1.; 4]),
    ] {
        let mut editor = editor(Action::CanvasSize);
        let original = editor.session().document.clone();
        for (index, value) in [(0, "201"), (1, "101"), (2, "left"), (3, "top")] {
            editor.update_form_field(index, value);
        }
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Canvas fill preset").size(1000., 850.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        cx.click(window, "size-fill").unwrap();
        cx.simulate_keystrokes(window, keys).unwrap();
        assert_eq!(cx.read(view, values).unwrap()[6], name);
        assert!(
            cx.element_bounds(window, "canvas-extension-custom")
                .is_err()
        );
        cx.click(window, "form-apply").unwrap();
        cx.read(view, |e| {
            assert!(e.modal.is_none());
            assert_eq!(
                compositor::render::sample(&e.session().document, [200.5, 100.5]),
                expected
            );
        })
        .unwrap();
        cx.update(view, |e, _| e.session_mut().undo()).unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            original
        );
    }
}

#[test]
fn print_size_picker_restricts_units_and_cancelled_selection_keeps_drafts() {
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .bind_keys(quickgui::select_key_bindings())
        .into_test_context(
            WindowOptions::new("Image Size").size(1000., 850.),
            editor(Action::ImageSize),
        )
        .unwrap();
    let window = view.window_handle();
    cx.click(window, "size-unit").unwrap();
    assert!(cx.read(view, |e| e.size_menus.units.is_open()).unwrap());
    cx.simulate_keystrokes(window, "down escape").unwrap();
    assert_eq!(&cx.read(view, values).unwrap()[..2], &["200", "100"]);
    cx.click(window, "image-size-resample").unwrap();
    cx.click(window, "size-unit").unwrap();
    assert!(
        cx.read(view, |e| e.size_menus.print_units.is_open())
            .unwrap()
    );
    cx.simulate_keystrokes(window, "down enter").unwrap();
    cx.read(view, |e| {
        assert_eq!(e.size_menus.print_units.items().len(), 2);
        let draft = values(e);
        assert!((draft[0].parse::<f64>().unwrap() - 5.08).abs() < 1e-8);
        assert!((draft[1].parse::<f64>().unwrap() - 2.54).abs() < 1e-8);
        assert!(!e.image_sizing.resamples());
    })
    .unwrap();
    cx.click(window, "form-apply").unwrap();
    cx.read(view, |e| {
        assert!(e.modal.is_none());
        assert_eq!(
            (
                e.session().document.width,
                e.session().document.height,
                e.session().document.resolution
            ),
            (200, 100, 100.)
        );
    })
    .unwrap();
}

#[test]
fn invalid_size_disables_apply_until_dimensions_are_repaired() {
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(
            WindowOptions::new("Canvas Size").size(1000., 850.),
            editor(Action::CanvasSize),
        )
        .unwrap();
    let window = view.window_handle();
    cx.update(view, |e, cx| {
        e.update_form_field(0, "0");
        e.changed(cx);
    })
    .unwrap();
    assert!(matches!(
        cx.click(window, "form-apply"),
        Err(quickgui::TestAppError::NotClickable { .. })
    ));
    assert!(cx.read(view, |e| e.modal.is_some()).unwrap());
    cx.update(view, |e, cx| {
        e.update_form_field(0, "300");
        e.changed(cx);
    })
    .unwrap();
    cx.click(window, "form-apply").unwrap();
    assert_eq!(cx.read(view, |e| e.session().document.width).unwrap(), 300);
}

#[test]
fn changing_sheets_closes_old_dropdown_and_prevents_stale_choices() {
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .bind_keys(quickgui::select_key_bindings())
        .into_test_context(
            WindowOptions::new("Size sheets").size(1000., 850.),
            editor(Action::CanvasSize),
        )
        .unwrap();
    let window = view.window_handle();
    cx.click(window, "size-unit").unwrap();
    assert!(cx.read(view, |e| e.size_menus.units.is_open()).unwrap());
    cx.update(view, |e, cx| e.action(Action::ImageSize, cx))
        .unwrap();
    assert!(!cx.read(view, |e| e.size_menus.units.is_open()).unwrap());
    assert_eq!(&cx.read(view, values).unwrap()[..2], &["200", "100"]);
    cx.click(window, "size-unit").unwrap();
    assert!(cx.read(view, |e| e.size_menus.units.is_open()).unwrap());
    cx.click(window, "form-cancel").unwrap();
    assert!(!cx.read(view, |e| e.size_menus.units.is_open()).unwrap());
    assert!(cx.read(view, |e| e.modal.is_none()).unwrap());
}
