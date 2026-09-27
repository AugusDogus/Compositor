use super::*;
use compositor::{adjustment::PhotoFilter, document::Layer};

fn open() -> Editor {
    let mut e = Editor::with_test_document();
    e.open_extended_adjustment(Some(
        ExtendedAdjustment::PhotoFilter(PhotoFilter::default()),
    ))
    .unwrap();
    e
}
fn density(e: &mut Editor, value: &str) {
    let Some(Form::Edit { fields, .. }) = &mut e.modal else {
        panic!("Missing editor");
    };
    fields[0].1 = value.into();
    e.refresh_extended_adjustment();
}
#[test]
fn preview_apply_and_creation_have_independent_undo_steps() {
    let mut e = open();
    let created = e.session().document.clone();
    density(&mut e, "75");
    let preview = e.session().document.clone();
    assert_ne!(created, preview);
    e.extended_edit.as_mut().unwrap().preview = false;
    e.refresh_extended_adjustment();
    assert_eq!(e.session().document, created);
    e.finish_extended_adjustment().unwrap();
    assert_eq!(e.session().document, preview);
    assert_eq!(
        e.session().undo_label(),
        Some("Edit Photo Filter Adjustment")
    );
    e.session_mut().undo();
    assert_eq!(e.session().document, created);
    assert_eq!(
        e.session().undo_label(),
        Some("New Photo Filter Adjustment")
    );
    e.session_mut().undo();
    assert_eq!(e.session().document.layers.len(), 1);
    e.session_mut().redo();
    e.session_mut().redo();
    assert_eq!(e.session().document, preview);
}
#[test]
fn cancel_and_invalid_fields_preserve_created_layer_and_committed_recovery() {
    let mut e = open();
    let created = e.session().document.clone();
    density(&mut e, "85");
    assert_eq!(e.session().committed_document(), &created);
    density(&mut e, "NaN");
    assert!(e.finish_extended_adjustment().is_err());
    assert!(e.extended_edit.is_some());
    e.cancel_adjustment();
    assert_eq!(e.session().document, created);
    assert!(e.extended_edit.is_none());
}
#[test]
fn unchanged_fractional_color_survives_reopening_and_layer_commands_are_gated() {
    let mut e = open();
    e.cancel_adjustment();
    let s = PhotoFilter {
        color: [0.12345, 0.56789, 0.99999],
        density: 31.2345,
        preserve_luminosity: true,
    };
    e.session_mut().document.active_layer_mut().unwrap().content =
        LayerContent::ExtendedAdjustment(Box::new(ExtendedAdjustment::PhotoFilter(s)));
    let original = e.session().document.clone();
    e.open_adjustment(None).unwrap();
    for action in [
        Action::Save,
        Action::Undo,
        Action::Redo,
        Action::DeleteLayer,
        Action::New,
        Action::CloseTab,
    ] {
        assert!(!e.action_available(action));
    }
    e.finish_extended_adjustment().unwrap();
    assert_eq!(e.session().document, original);
}
#[test]
fn creation_inside_folder_expands_it_and_preserves_parent() {
    let mut e = Editor::with_test_document();
    let mut group = Layer::blank("Folder", 10, 10);
    group.content = LayerContent::Group;
    let id = group.id;
    e.session_mut().document.add(group).unwrap();
    e.session_mut().collapsed.insert(id);
    e.open_extended_adjustment(Some(ExtendedAdjustment::PhotoFilter(Default::default())))
        .unwrap();
    assert_eq!(
        e.session().document.active_layer().unwrap().parent,
        Some(id)
    );
    assert!(!e.session().collapsed.contains(&id));
}

#[test]
fn density_slider_updates_layer_preview_immediately() {
    let mut e = open();
    let before = e.session().document.clone();
    super::super::scalar_controls::Scalar::Field(0)
        .set(&mut e, 80.)
        .unwrap();
    assert_ne!(e.session().document, before);
    assert!(
        matches!(&e.session().document.active_layer().unwrap().content, LayerContent::ExtendedAdjustment(a) if matches!(**a, ExtendedAdjustment::PhotoFilter(s) if s.density == 80.))
    );
}

#[test]
fn extended_photo_filter_controls_restore_picker_and_cancel_preview() {
    use quickgui::{Application, WindowOptions};
    let editor = open();
    let original = editor.session().document.clone();
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(
            WindowOptions::new("Editable Photo Filter").size(1280., 900.),
            editor,
        )
        .unwrap();
    let window = view.window_handle();
    cx.click(window, "photo-filter-Cooling").unwrap();
    cx.read(view, |e| assert_ne!(e.session().document, original))
        .unwrap();
    cx.click(window, "photo-filter-color").unwrap();
    cx.focus(window, 51_003_u64).unwrap();
    cx.simulate_keystroke(
        window,
        quickgui::Keystroke::new(Key::Character("a".into()), Modifiers::CONTROL),
    )
    .unwrap();
    cx.simulate_input(window, "#12ab34").unwrap();
    cx.click(window, "color-picker-close").unwrap();
    cx.read(view, |e| {
        assert!(matches!(&e.modal, Some(Form::Edit {fields,..}) if fields[2].1 == "#007ECC"))
    })
    .unwrap();
    cx.click(window, "extended-adjustment-preview").unwrap();
    cx.read(view, |e| assert_eq!(e.session().document, original))
        .unwrap();
    cx.click(window, "extended-adjustment-preview").unwrap();
    cx.read(view, |e| assert_ne!(e.session().document, original))
        .unwrap();
    let frame = cx.capture_screenshot(window).unwrap();
    if let Some(path) = std::env::var_os("COMPOSITOR_EXTENDED_SCREENSHOT") {
        frame.write_png(path).unwrap();
    }
    cx.click(window, "form-cancel").unwrap();
    cx.read(view, |e| {
        assert!(e.extended_edit.is_none());
        assert_eq!(e.session().document, original);
    })
    .unwrap();
}
