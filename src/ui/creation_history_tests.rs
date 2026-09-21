use super::*;
use quickgui::{Application, WindowOptions};

#[test]
fn new_canvas_undo_restores_welcome_and_redo_restores_the_same_document() {
    let e = Editor::new(Vec::new()).unwrap();
    let id = e.tabs[0].id;
    let (mut cx, view) = Application::new()
        .bind_keys(quickgui::menubar_key_bindings())
        .bind_keys(quickgui::popover_menu_key_bindings())
        .bind_keys(menus::key_bindings())
        .into_test_context(
            WindowOptions::new("Canvas creation history").size(1500., 900.),
            e,
        )
        .unwrap();
    let window = view.window_handle();
    cx.update(view, |e, cx| {
        e.apply_form(Action::New, vec!["108".into(), "64".into()])
            .unwrap();
        assert!(e.can_undo());
        assert_eq!(
            e.build_menu(1).items()[0].label().as_ref(),
            "Undo New Canvas"
        );
        cx.invalidate();
    })
    .unwrap();
    let document = cx.read(view, |e| e.session().document.clone()).unwrap();
    cx.focus(window, "workspace").unwrap();
    cx.simulate_keystrokes(window, "ctrl-z").unwrap();
    cx.read(view, |e| {
        assert!(!e.has_document());
        assert_eq!(e.tabs[0].id, id);
        assert!(!e.can_undo());
        assert!(e.can_redo());
        assert_eq!(
            e.build_menu(1).items()[1].label().as_ref(),
            "Redo New Canvas"
        );
        assert!(!e.build_menu(1).items()[1].is_disabled());
        assert!(e.build_menu(1).items()[0].is_disabled());
        assert!(!e.tabs[0].dirty());
    })
    .unwrap();
    assert!(cx.contains_element(window, "welcome-create").unwrap());
    cx.simulate_keystrokes(window, "ctrl-shift-z").unwrap();
    cx.read(view, |e| {
        assert_eq!(e.session().document, document);
        assert_eq!(e.session().id, id);
        assert!(e.can_undo());
        assert!(!e.can_redo());
        assert!(e.tabs[0].dirty());
    })
    .unwrap();
    assert!(!cx.contains_element(window, "welcome-create").unwrap());
    cx.simulate_keystrokes(window, "alt-e home enter").unwrap();
    assert!(cx.contains_element(window, "welcome-create").unwrap());
    cx.focus(window, "workspace").unwrap();
    cx.simulate_keystrokes(window, "alt-e home enter").unwrap();
    cx.read(view, |e| assert_eq!(e.session().document, document))
        .unwrap();
}

fn pasted_editor() -> Editor {
    let mut editor = Editor::new(Vec::new()).unwrap();
    editor
        .paste_pixels(
            editor.tabs[0].id,
            image::RgbaImage::from_pixel(8, 6, image::Rgba([180, 60, 20, 255])),
        )
        .unwrap();
    editor
}

#[test]
fn first_paste_and_later_edits_round_trip_through_welcome_in_order() {
    let mut e = pasted_editor();
    let pasted = e.session().document.clone();
    assert_eq!(e.tabs[0].undo_label(), Some("Paste"));
    e.session_mut()
        .edit("Fill", |doc| {
            compositor::edits::fill(doc, [20, 180, 60, 255], false, false)
        })
        .unwrap();
    let filled = e.session().document.clone();
    e.undo_document();
    assert_eq!(e.session().document, pasted);
    e.undo_document();
    assert!(!e.has_document());
    e.redo_document();
    assert_eq!(e.session().document, pasted);
    assert_eq!(e.tabs[0].redo_label(), Some("Fill"));
    e.redo_document();
    assert_eq!(e.session().document, filled);
}

#[test]
fn failed_creation_preserves_redo_and_successful_creation_branches_saved_history() {
    let mut e = pasted_editor();
    e.session_mut().mark_saved("Saved.comp".into());
    let saved = e.session().document.clone();
    e.undo_document();
    assert!(e.tabs[0].dirty());
    assert!(!e.tabs[0].needs_save());
    assert_eq!(e.tabs[0].title(), "Saved");
    assert!(
        e.tabs[0]
            .edit_or_create(
                "Import Images",
                || Document::new(3, 4),
                |_| Err(compositor::invalid("Decode failed"))
            )
            .is_err()
    );
    assert_eq!(e.tabs[0].redo_label(), Some("Paste"));
    e.redo_document();
    assert_eq!(e.session().document, saved);
    assert!(!e.tabs[0].dirty());
    e.undo_document();
    e.paste_pixels(e.tabs[0].id, image::RgbaImage::new(4, 3))
        .unwrap();
    assert_eq!(
        e.session().path.as_deref(),
        Some(std::path::Path::new("Saved.comp"))
    );
    assert!(e.tabs[0].dirty());
    assert!(e.tabs[0].redo_label().is_none());
    e.undo_document();
    e.apply_form(Action::New, vec!["6".into(), "7".into()])
        .unwrap();
    assert!(e.session().path.is_none());
    assert_eq!(e.tabs[0].undo_label(), Some("New Canvas"));
    e.undo_document();
    assert!(!e.tabs[0].dirty());
}

#[test]
fn cross_project_copy_has_creation_history_and_keeps_source_unchanged() {
    let mut e = pasted_editor();
    let source = e.session().document.clone();
    let drag = layer_drag::LayerDrag {
        session: e.tabs[0].id,
        layer: source.active.unwrap(),
        operation: layer_drag::Transfer::Copy,
    };
    e.copy_drag_to_new_tab(&drag).unwrap();
    let copied = e.session().document.clone();
    assert_eq!(e.tabs[1].undo_label(), Some("Copy Layers from Project"));
    e.undo_document();
    assert!(!e.has_document());
    e.activate_tab(0);
    assert_eq!(e.session().document, source);
    e.activate_tab(1);
    assert!(e.can_redo());
    e.redo_document();
    assert_eq!(e.session().document, copied);
}

#[test]
fn evicted_creation_cannot_undo_past_retained_history_to_welcome() {
    let mut e = pasted_editor();
    for n in 0..101 {
        e.session_mut()
            .edit("Rename", |doc| {
                doc.layers[0].name = format!("Layer {n}");
                Ok(())
            })
            .unwrap();
    }
    for _ in 0..100 {
        e.undo_document();
    }
    assert!(!e.can_undo());
    assert!(e.has_document());
    assert_eq!(e.session().document.layers[0].name, "Layer 0");
}

#[test]
fn saved_creation_undone_to_welcome_closes_without_save_prompt() {
    let mut e = pasted_editor();
    e.session_mut().mark_saved("Saved.comp".into());
    let id = e.tabs[0].id;
    e.undo_document();
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Close welcome"), e)
        .unwrap();
    cx.update(view, |e, cx| {
        e.request_close(CloseIntent::Tab(id), cx);
        assert!(e.close_intent.is_none());
        assert!(e.modal.is_none());
        assert!(e.tabs.iter().all(|tab| tab.id != id));
    })
    .unwrap();
}

#[test]
fn hue_panel_survives_welcome_and_cancel_clears_the_retained_transaction() {
    let mut e = pasted_editor();
    let original = e.session().document.clone();
    e.open_pixel_adjustment(Kind::HueSaturation).unwrap();
    if let Some(Form::Edit { fields, .. }) = &mut e.modal {
        fields[0].1 = "90".into();
    }
    e.preview_adjustment().unwrap();
    let preview = e.session().document.clone();
    assert_ne!(preview, original);
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Creation with Hue").size(1500., 900.), e)
        .unwrap();
    let window = view.window_handle();
    cx.update(view, |e, cx| e.action(Action::Undo, cx)).unwrap();
    assert!(cx.contains_element(window, "welcome-create").unwrap());
    assert!(cx.contains_element(window, "floating-panel-close").unwrap());
    cx.read(view, |e| {
        assert!(!e.has_document());
        assert!(!e.adjustment_source_is_current());
        assert!(e.adjustment_edit.is_some());
    })
    .unwrap();
    cx.focus(window, "welcome-dimension-0").unwrap();
    cx.simulate_keystrokes(window, "ctrl-y").unwrap();
    cx.read(view, |e| assert_eq!(e.session().document, preview))
        .unwrap();
    cx.update(view, |e, cx| e.action(Action::Undo, cx)).unwrap();
    cx.click(window, "form-cancel").unwrap();
    cx.simulate_keystrokes(window, "ctrl-shift-z").unwrap();
    cx.read(view, |e| {
        assert_eq!(e.session().document, original);
        assert!(!e.session().has_pending_edit());
        assert!(e.adjustment_edit.is_none());
    })
    .unwrap();
}

#[test]
fn filter_panel_survives_creation_undo_and_cancel_leaves_clean_redo() {
    let mut e = pasted_editor();
    let original = e.session().document.clone();
    e.open_filter(compositor::filters::Filter::Gaussian { radius: 2. })
        .unwrap();
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Creation with filter").size(1500., 900.),
            e,
        )
        .unwrap();
    let window = view.window_handle();
    cx.update(view, |e, cx| e.action(Action::Undo, cx)).unwrap();
    assert!(cx.contains_element(window, "welcome-create").unwrap());
    assert!(cx.contains_element(window, "floating-panel-close").unwrap());
    cx.read(view, |e| {
        assert!(!e.filter_source_is_current());
        assert!(e.filter_edit.is_some());
    })
    .unwrap();
    cx.update(view, |e, cx| e.action(Action::Redo, cx)).unwrap();
    cx.read(view, |e| assert!(e.filter_source_is_current()))
        .unwrap();
    cx.update(view, |e, cx| e.action(Action::Undo, cx)).unwrap();
    cx.click(window, "form-cancel").unwrap();
    cx.simulate_keystrokes(window, "ctrl-y").unwrap();
    cx.read(view, |e| {
        assert_eq!(e.session().document, original);
        assert!(!e.session().has_pending_edit());
        assert!(e.filter_edit.is_none());
    })
    .unwrap();
}

#[test]
fn undo_creation_under_the_pointer_restores_the_native_arrow() {
    let mut e = pasted_editor();
    e.tools.tool = Tool::Zoom;
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Creation cursor").size(1500., 900.), e)
        .unwrap();
    let window = view.window_handle();
    let bounds = cx.element_bounds(window, "canvas").unwrap();
    cx.update(view, |_, cx| cx.focus_window(window)).unwrap();
    cx.simulate_mouse_move(
        window,
        "canvas",
        quickgui::MouseMoveEvent {
            position: quickgui::Point::new(bounds.x + 100., bounds.y + 100.),
            pressed_button: None,
            modifiers: Modifiers::empty(),
        },
    )
    .unwrap();
    assert!(cx.window_state(window).unwrap().cursor_override.is_some());
    cx.update(view, |e, cx| e.action(Action::Undo, cx)).unwrap();
    assert!(cx.contains_element(window, "welcome-create").unwrap());
    assert!(cx.window_state(window).unwrap().cursor_override.is_none());
    cx.update(view, |e, cx| e.action(Action::Redo, cx)).unwrap();
    assert!(cx.contains_element(window, "canvas").unwrap());
}

#[test]
fn oversized_creation_redo_is_evicted_without_losing_saved_project_identity() {
    let mut tab = ProjectTab::empty("Untitled".into());
    let mut document = Document::new(8193, 8192).unwrap();
    let pixels = Arc::new(image::RgbaImage::new(8193, 8192));
    let retained = Arc::downgrade(&pixels);
    document.layers[0].content = compositor::document::LayerContent::Raster(Some(pixels));
    tab.create_document(document).unwrap();
    tab.session_mut().unwrap().mark_saved("Saved.comp".into());
    tab.undo();
    assert!(tab.session().is_none());
    assert!(tab.redo_label().is_none());
    assert!(
        retained.upgrade().is_none(),
        "Evicted history must release its image"
    );
    assert_eq!(tab.title(), "Saved");
    assert!(tab.dirty());
    assert!(!tab.needs_save());
    tab.edit_or_create("Paste", || Document::new(4, 3), |_| Ok(()))
        .unwrap();
    assert_eq!(
        tab.session().unwrap().path.as_deref(),
        Some(std::path::Path::new("Saved.comp"))
    );
    assert!(tab.dirty());
    tab.undo();
    assert_eq!(tab.redo_label(), Some("Paste"));
    tab.create_document(Document::new(5, 5).unwrap()).unwrap();
    assert!(tab.session().unwrap().path.is_none());
    tab.undo();
    assert!(!tab.dirty());
}
