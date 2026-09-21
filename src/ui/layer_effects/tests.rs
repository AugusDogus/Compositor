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
