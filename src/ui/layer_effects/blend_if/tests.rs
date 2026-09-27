use super::*;

fn editor() -> Editor {
    let mut editor = Editor::with_test_document();
    let mut doc = Document::new(12, 12).unwrap();
    compositor::edits::fill(&mut doc, [100, 100, 100, 255], false, false).unwrap();
    editor.tabs = vec![Session::new(doc, None).into()];
    editor
}

#[test]
fn blend_if_preview_copy_cancel_and_undo_preserve_pixels() {
    let mut editor = editor();
    let mut other = editor.session().document.layers[0].clone();
    other.id = Uuid::new_v4();
    let target = other.id;
    editor.session_mut().document.layers.push(other);
    let original = editor.session().document.clone();
    let key = canvas_content::CanvasContent::new(&original);
    editor.open_effects().unwrap();
    editor.change_effect(|e| {
        e.kind = EffectKind::BlendIf;
        e.blend_if = Some(Settings::default());
        Handle::Source(1).set(e, 200.);
        Handle::Underlying(2).set(e, 220.);
    });
    assert!(!key.matches(&editor.session().document));
    assert_eq!(
        editor.session().document.layers[0].raster(),
        original.layers[0].raster()
    );
    editor.copy_effect_to(target).unwrap();
    assert_eq!(
        editor.session().document.layers[0].blend_if,
        editor.session().document.layer(target).unwrap().blend_if
    );
    editor.finish_effects(false).unwrap();
    assert_eq!(editor.session().document, original);
    editor.open_effects().unwrap();
    editor.change_effect(|e| {
        e.kind = EffectKind::BlendIf;
        e.blend_if = Some(Settings {
            source: Range::new([0, 200], [255, 255]).unwrap(),
            ..Default::default()
        });
    });
    editor.finish_effects(true).unwrap();
    let changed = editor.session().document.clone();
    editor.session_mut().undo();
    assert_eq!(editor.session().document, original);
    editor.session_mut().redo();
    assert_eq!(editor.session().document, changed);
}

#[test]
fn blend_if_sheet_exposes_split_handles_hide_reset_and_remove() {
    use quickgui::{Application, WindowOptions};
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Blend If").size(1024., 768.), editor())
        .unwrap();
    let window = view.window_handle();
    cx.click(window, "layer-effects").unwrap();
    cx.click(window, "effect-tab-Blend If").unwrap();
    cx.click(window, "effect-toggle").unwrap();
    for source in ["source", "underlying"] {
        for index in 0..4 {
            assert!(
                cx.element_bounds(window, format!("blend-if-{source}-{index}"))
                    .is_ok()
            );
        }
    }
    cx.update(view, |e, cx| {
        e.change_effect(|edit| Handle::Source(1).set(edit, 200.));
        e.resolve_test_canvas_preview().unwrap();
        cx.invalidate();
    })
    .unwrap();
    let footer = cx.element_bounds(window, "effects-apply").unwrap();
    assert!(footer.y >= 0. && footer.y + footer.height <= 768.);
    if let Some(path) = std::env::var_os("COMPOSITOR_BLEND_IF_SCREENSHOT") {
        cx.capture_screenshot(window)
            .unwrap()
            .write_png(path)
            .unwrap();
    }
    cx.click(window, "effect-toggle").unwrap();
    assert!(
        !cx.read(view, |e| e.session().document.layers[0]
            .blend_if
            .unwrap()
            .enabled)
            .unwrap()
    );
    cx.click(window, "blend-if-reset").unwrap();
    assert_eq!(
        cx.read(view, |e| e.session().document.layers[0]
            .blend_if
            .unwrap()
            .source)
            .unwrap(),
        Range::default()
    );
    cx.click(window, "effect-remove").unwrap();
    assert!(
        cx.read(view, |e| e.session().document.layers[0].blend_if.is_none())
            .unwrap()
    );
    cx.click(window, "effects-cancel").unwrap();
}
