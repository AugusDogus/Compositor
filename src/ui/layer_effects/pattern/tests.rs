use super::*;
use image::{Rgba, RgbaImage};
use quickgui::{Application, WindowOptions};
fn pattern() -> Pattern {
    Pattern::from_pixels(
        "Red and blue",
        RgbaImage::from_fn(2, 2, |x, _| {
            if x == 0 {
                Rgba([255, 0, 0, 255])
            } else {
                Rgba([0, 0, 255, 255])
            }
        }),
    )
    .unwrap()
}
fn editor() -> Editor {
    let mut e = Editor::with_test_document();
    let mut doc = Document::new(32, 32).unwrap();
    compositor::edits::fill(&mut doc, [255; 4], false, false).unwrap();
    e.tabs = vec![Session::new(doc, None).into()];
    e
}
#[test]
fn imported_pattern_previews_and_cancel_preserves_pixels_and_history() {
    let mut e = editor();
    let before = e.session().document.clone();
    e.open_effects().unwrap();
    let request = e.pattern_request().unwrap();
    e.receive_pattern_pack(request, Imported::from_patterns(&[pattern()]).unwrap())
        .unwrap();
    assert_eq!(e.session().document, before);
    e.choose_pattern(0);
    assert_eq!(
        compositor::render::render(&e.session().document, 32, 32).unwrap()[(0, 0)],
        image::Rgba([255, 0, 0, 255])
    );
    assert_eq!(
        e.session().document.active_layer().unwrap().raster(),
        before.active_layer().unwrap().raster()
    );
    e.finish_effects(false).unwrap();
    assert_eq!(e.session().document, before);
    assert!(e.session().undo_label().is_none());
}
#[test]
fn stale_import_cannot_change_another_effects_draft() {
    let mut e = editor();
    e.open_effects().unwrap();
    let request = e.pattern_request().unwrap();
    e.finish_effects(false).unwrap();
    e.open_effects().unwrap();
    let before = e.session().document.clone();
    assert!(
        e.receive_pattern_pack(request, Imported::from_patterns(&[pattern()]).unwrap())
            .is_err()
    );
    assert_eq!(e.session().document, before);
    let Some(Form::Effects(edit)) = &e.modal else {
        panic!("Missing effects draft")
    };
    assert!(edit.patterns.entries.is_empty());
}
#[test]
fn pattern_ui_selects_scales_applies_and_reopens_without_source_changes() {
    let mut e = editor();
    let original = e.session().document.clone();
    e.open_effects().unwrap();
    let request = e.pattern_request().unwrap();
    e.receive_pattern_pack(request, Imported::from_patterns(&[pattern()]).unwrap())
        .unwrap();
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(WindowOptions::new("Patterns").size(1180., 780.), e)
        .unwrap();
    let window = view.window_handle();
    cx.click(window, "pattern-choice-0").unwrap();
    cx.update(view, |e, _| {
        e.change_effect(|draft| {
            draft.set_number(Parameter::Scale, 150.);
            draft.set_number(Parameter::Opacity, 60.);
        })
    })
    .unwrap();
    for id in [
        "pattern-import",
        "pattern-choice-0",
        "effect-scale",
        "effects-apply",
    ] {
        let bounds = cx.element_bounds(window, id).unwrap();
        assert!(bounds.x >= 0. && bounds.x + bounds.width <= 1180.);
        assert!(bounds.y >= 0. && bounds.y + bounds.height <= 780.);
    }
    if let Some(path) = std::env::var_os("COMPOSITOR_PATTERN_SCREENSHOT") {
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
    cx.update(view, |e, cx| {
        let overlay = e
            .session()
            .document
            .active_layer()
            .unwrap()
            .effects
            .as_ref()
            .unwrap()
            .pattern_overlay
            .as_ref()
            .unwrap();
        assert_eq!(overlay.settings.scale, 1.5);
        assert_eq!(overlay.settings.opacity, 0.6);
        assert_eq!(
            e.session().document.active_layer().unwrap().raster(),
            original.active_layer().unwrap().raster()
        );
        assert_eq!(e.session().undo_label(), Some("Layer Effects"));
        e.open_effects().unwrap();
        let Some(Form::Effects(edit)) = &e.modal else {
            panic!("Missing effects draft")
        };
        assert!(edit.kind == EffectKind::Pattern);
        cx.invalidate();
    })
    .unwrap();
    cx.click(window, "effect-toggle").unwrap();
    cx.read(view, |e| {
        assert!(
            !e.session()
                .document
                .active_layer()
                .unwrap()
                .effects
                .as_ref()
                .unwrap()
                .pattern_overlay
                .as_ref()
                .unwrap()
                .settings
                .enabled
        )
    })
    .unwrap();
    cx.click(window, "effects-cancel").unwrap();
    cx.update(view, |e, _| {
        e.session_mut().undo();
        assert_eq!(e.session().document, original);
    })
    .unwrap();
}
#[test]
fn pattern_copy_and_removal_are_part_of_the_same_undo_transaction() {
    let mut e = editor();
    let target = e.session().document.active.unwrap();
    e.session_mut()
        .document
        .add(compositor::document::Layer::blank("Second", 32, 32))
        .unwrap();
    compositor::edits::fill(&mut e.session_mut().document, [255; 4], false, false).unwrap();
    e.open_effects().unwrap();
    let request = e.pattern_request().unwrap();
    e.receive_pattern_pack(request, Imported::from_patterns(&[pattern()]).unwrap())
        .unwrap();
    e.choose_pattern(0);
    e.copy_effect_to(target).unwrap();
    assert!(
        e.session()
            .document
            .layer(target)
            .unwrap()
            .effects
            .as_ref()
            .unwrap()
            .pattern_overlay
            .is_some()
    );
    e.finish_effects(false).unwrap();
    assert!(
        e.session()
            .document
            .layers
            .iter()
            .all(|l| l.effects.is_none())
    );
}

#[test]
fn oversized_pattern_metadata_keeps_effects_draft_cancelable() {
    let mut e = editor();
    for _ in 1..260 {
        e.session_mut()
            .document
            .add(compositor::document::Layer::blank("Tile", 1, 1))
            .unwrap();
        compositor::edits::fill(&mut e.session_mut().document, [255; 4], false, false).unwrap();
    }
    let before = e.session().document.clone();
    e.open_effects().unwrap();
    let request = e.pattern_request().unwrap();
    let tile = Pattern::from_pixels(
        &"x".repeat(16_384),
        RgbaImage::from_pixel(1, 1, Rgba([255; 4])),
    )
    .unwrap();
    e.receive_pattern_pack(request, Imported::from_patterns(&[tile]).unwrap())
        .unwrap();
    e.choose_pattern(0);
    let targets: Vec<_> = e.session().document.layers.iter().map(|l| l.id).collect();
    for target in targets {
        e.copy_effect_to(target).unwrap();
    }
    e.session().document.validate().unwrap();
    let preview = e.session().document.clone();
    assert!(e.finish_effects(true).is_err());
    assert!(matches!(e.modal, Some(Form::Effects(_))));
    assert_eq!(e.session().document, preview);
    assert!(e.session().undo_label().is_none());
    e.finish_effects(false).unwrap();
    assert_eq!(e.session().document, before);
}
