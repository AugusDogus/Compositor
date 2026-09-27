use super::*;
use compositor::{adjustment::PhotoFilter, filters::Filter};

#[test]
fn photo_filter_fields_round_trip_settings_and_reject_invalid_drafts() {
    let filter = Filter::PhotoFilter(PhotoFilter::default());
    let (_, fields) = Editor::filter_fields(filter);
    let mut values: Vec<_> = fields.into_iter().map(|(_, value)| value).collect();
    assert_eq!(Editor::filter_values(filter, &values).unwrap(), filter);
    for (index, value) in [(0, "101"), (0, "NaN"), (1, "2"), (2, "banana")] {
        let original = std::mem::replace(&mut values[index], value.into());
        assert!(Editor::filter_values(filter, &values).is_err());
        values[index] = original;
    }
}

#[test]
fn photo_filter_presets_color_picker_and_cancel_keep_original_document() {
    use quickgui::{Application, WindowOptions};
    let mut editor = Editor::with_test_document();
    compositor::edits::fill(
        &mut editor.session_mut().document,
        [100, 150, 200, 255],
        false,
        false,
    )
    .unwrap();
    let original = editor.session().document.clone();
    editor
        .open_filter(Filter::PhotoFilter(Default::default()))
        .unwrap();
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(WindowOptions::new("Photo Filter").size(1280., 900.), editor)
        .unwrap();
    let window = view.window_handle();
    cx.click(window, "photo-filter-Cooling").unwrap();
    cx.read(view, |e| {
        assert!(matches!(&e.modal, Some(Form::Edit { fields, .. }) if fields[2].1 == "#007ECC"))
    })
    .unwrap();
    for (button, expected) in [("color-picker-close", "#007ECC"), ("form-apply", "#12AB34")] {
        cx.click(window, "photo-filter-color").unwrap();
        cx.read(view, |e| assert!(e.picking_color())).unwrap();
        cx.focus(window, 51_003_u64).unwrap();
        cx.simulate_keystroke(
            window,
            quickgui::Keystroke::new(Key::Character("a".into()), Modifiers::CONTROL),
        )
        .unwrap();
        cx.simulate_input(window, "#12ab34").unwrap();
        cx.click(window, button).unwrap();
        cx.read(view, |e| {
            assert!(matches!(&e.modal, Some(Form::Edit { fields, .. }) if fields[2].1 == expected))
        })
        .unwrap();
    }
    cx.click(window, 50_001_u64).unwrap();
    cx.read(view, |e| {
        assert!(matches!(&e.modal, Some(Form::Edit { fields, .. }) if fields[1].1 == "0"))
    })
    .unwrap();
    cx.click(window, "form-cancel").unwrap();
    cx.read(view, |e| {
        assert_eq!(e.session().document, original);
        assert!(e.session().undo_label().is_none());
    })
    .unwrap();
}
