use super::*;
fn editor() -> Editor {
    let mut editor = Editor::with_test_document();
    let mut doc = Document::new(12, 12).unwrap();
    compositor::edits::fill(&mut doc, [10, 20, 30, 128], false, false).unwrap();
    editor.tabs = vec![Session::new(doc, None).into()];
    editor
}
#[test]
fn effects_preview_is_nondestructive_and_cancel_restores_document() {
    let mut editor = editor();
    let original = editor.session().document.clone();
    editor.open_effects().unwrap();
    editor.change_effect(|e| {
        e.effects.color_overlay = Some(ColorOverlayEffect {
            red: 1.,
            ..Default::default()
        })
    });
    assert_eq!(
        editor.session().document.layers[0].raster(),
        original.layers[0].raster()
    );
    assert_eq!(
        compositor::render::render(&editor.session().document, 12, 12).unwrap()[(4, 4)],
        image::Rgba([255, 0, 0, 128])
    );
    editor.finish_effects(false).unwrap();
    assert_eq!(editor.session().document, original);
}
#[test]
fn effects_commit_and_copy_are_one_undoable_edit() {
    let mut editor = editor();
    let mut other = editor.session().document.layers[0].clone();
    other.id = Uuid::new_v4();
    let target = other.id;
    editor.session_mut().document.layers.push(other);
    let original = editor.session().document.clone();
    editor.open_effects().unwrap();
    editor.change_effect(|e| {
        e.kind = EffectKind::Stroke;
        e.effects.stroke = Some(StrokeEffect {
            size: 1.,
            inside: true,
            ..Default::default()
        });
    });
    editor.copy_effect_to(target).unwrap();
    editor.finish_effects(true).unwrap();
    assert_eq!(
        editor.session().document.layers[0].effects,
        editor.session().document.layer(target).unwrap().effects
    );
    let changed = editor.session().document.clone();
    editor.session_mut().undo();
    assert_eq!(editor.session().document, original);
    editor.session_mut().redo();
    assert_eq!(editor.session().document, changed);
}
#[test]
fn invalid_effect_draft_cannot_replace_source_or_commit() {
    let mut editor = editor();
    let original = editor.session().document.clone();
    editor.open_effects().unwrap();
    editor.change_effect(|e| {
        e.effects.stroke = Some(StrokeEffect {
            size: 501.,
            ..Default::default()
        })
    });
    assert_eq!(editor.session().document, original);
    assert!(editor.finish_effects(true).is_err());
    editor.finish_effects(false).unwrap();
    assert_eq!(editor.session().document, original);
}

#[test]
fn effects_sheet_buttons_open_color_picker_and_cancel_the_whole_edit() {
    use quickgui::{Application, WindowOptions};
    let editor = editor();
    let original = editor.session().document.clone();
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Layer effects").size(1200., 900.),
            editor,
        )
        .unwrap();
    let window = view.window_handle();
    cx.click(window, "layer-effects").unwrap();
    cx.click(window, "effect-toggle").unwrap();
    assert!(cx.element_bounds(window, "effect-size").is_ok());
    cx.update(view, |e, cx| {
        e.resolve_test_canvas_preview().unwrap();
        cx.invalidate();
    })
    .unwrap();
    cx.click(window, "effect-color-picker").unwrap();
    assert!(
        cx.read(view, |e| matches!(e.modal, Some(Form::Color(_))))
            .unwrap()
    );
    cx.update(view, |e, cx| {
        e.finish_color(false).unwrap();
        cx.invalidate();
    })
    .unwrap();
    cx.click(window, "effects-cancel").unwrap();
    assert_eq!(
        cx.read(view, |e| e.session().document.clone()).unwrap(),
        original
    );
}

#[test]
fn effects_changes_invalidate_the_canvas_without_replacing_source_assets() {
    let mut editor = editor();
    let key = canvas_content::CanvasContent::new(&editor.session().document);
    editor.open_effects().unwrap();
    editor.change_effect(|e| e.effects.stroke = Some(StrokeEffect::default()));
    assert!(!key.matches(&editor.session().document));
}

#[test]
fn outer_glow_controls_edit_copy_hide_remove_cancel_and_undo() {
    use compositor::effects::OuterGlowEffect;
    use quickgui::{Application, WindowOptions};
    let mut editor = editor();
    let mut other = editor.session().document.layers[0].clone();
    other.id = Uuid::new_v4();
    let target = other.id;
    editor.session_mut().document.layers.push(other);
    let original = editor.session().document.clone();
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Outer glow").size(1024., 768.), editor)
        .unwrap();
    let window = view.window_handle();
    cx.click(window, "layer-effects").unwrap();
    cx.click(window, "effect-tab-Outer Glow").unwrap();
    cx.click(window, "effect-toggle").unwrap();
    assert!(cx.element_bounds(window, "effect-size").is_ok());
    assert!(cx.element_bounds(window, "effect-opacity").is_ok());
    assert!(cx.element_bounds(window, "effect-angle").is_err());
    let glow = cx
        .read(view, |e| {
            e.session().document.layers[0]
                .effects
                .as_ref()
                .unwrap()
                .outer_glow
                .clone()
                .unwrap()
        })
        .unwrap();
    assert_eq!(
        glow,
        OuterGlowEffect {
            enabled: Some(true),
            ..Default::default()
        }
    );
    cx.update(view, |e, cx| {
        e.change_effect(|edit| {
            edit.set_number(Parameter::Size, 7.5);
            edit.set_number(Parameter::Opacity, 40.);
            edit.set_color([255, 128, 0, 255]);
        });
        e.copy_effect_to(target).unwrap();
        cx.invalidate();
    })
    .unwrap();
    if let Some(path) = std::env::var_os("COMPOSITOR_TEST_OUTER_GLOW_SCREENSHOT") {
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
    cx.click(window, "effect-toggle").unwrap();
    assert!(
        !cx.read(view, |e| e.session().document.layers[0]
            .effects
            .as_ref()
            .unwrap()
            .outer_glow
            .as_ref()
            .unwrap()
            .enabled
            .unwrap())
            .unwrap()
    );
    cx.click(window, "effect-toggle").unwrap();
    cx.click(window, "effects-apply").unwrap();
    let committed = cx.read(view, |e| e.session().document.clone()).unwrap();
    assert_eq!(committed.layers[0].effects, committed.layers[1].effects);
    let glow = committed.layers[0]
        .effects
        .as_ref()
        .unwrap()
        .outer_glow
        .as_ref()
        .unwrap();
    assert_eq!(
        (glow.size, glow.opacity, glow.red, glow.blue),
        (7.5, 0.4, 1., 0.)
    );
    cx.click(window, "layer-effects").unwrap();
    cx.click(window, "effect-remove").unwrap();
    assert!(
        cx.read(view, |e| e.session().document.layers[0].effects.is_none())
            .unwrap()
    );
    cx.click(window, "effects-cancel").unwrap();
    assert_eq!(
        cx.read(view, |e| e.session().document.clone()).unwrap(),
        committed
    );
    cx.update(view, |e, _| {
        e.session_mut().undo();
    })
    .unwrap();
    assert_eq!(
        cx.read(view, |e| e.session().document.clone()).unwrap(),
        original
    );
    cx.update(view, |e, _| {
        e.session_mut().redo();
    })
    .unwrap();
    assert_eq!(
        cx.read(view, |e| e.session().document.clone()).unwrap(),
        committed
    );
}

#[test]
fn inner_glow_is_editable_and_cancel_restores_source() {
    let original = editor();
    let doc = original.session().document.clone();
    let (mut cx, view) = quickgui::Application::new()
        .into_test_context(
            quickgui::WindowOptions::new("Inner glow").size(1200., 900.),
            original,
        )
        .unwrap();
    let window = view.window_handle();
    cx.click(window, "layer-effects").unwrap();
    cx.click(window, "effect-tab-Inner Glow").unwrap();
    cx.click(window, "effect-toggle").unwrap();
    assert!(cx.element_bounds(window, "effect-size").is_ok());
    assert!(
        cx.read(view, |e| e.session().document.layers[0]
            .effects
            .as_ref()
            .unwrap()
            .inner_glow
            .is_some())
            .unwrap()
    );
    cx.click(window, "effects-cancel").unwrap();
    assert_eq!(
        cx.read(view, |e| e.session().document.clone()).unwrap(),
        doc
    );
}
