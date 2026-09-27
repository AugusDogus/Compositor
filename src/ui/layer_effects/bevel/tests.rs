use super::*;
use quickgui::{Application, WindowOptions};
fn editor() -> Editor {
    let mut e = Editor::with_test_document();
    let mut doc = Document::new(32, 32).unwrap();
    doc.layers[0].content = compositor::document::LayerContent::Raster(Some(Arc::new(
        image::RgbaImage::from_fn(32, 32, |x, y| {
            if (6..26).contains(&x) && (6..26).contains(&y) {
                image::Rgba([120, 160, 200, 255])
            } else {
                image::Rgba([0; 4])
            }
        }),
    )));
    e.tabs = vec![Session::new(doc, None).into()];
    e
}
#[test]
fn bevel_ui_edits_styles_preview_copy_cancel_apply_and_undo() {
    let mut e = editor();
    let mut second = e.session().document.layers[0].clone();
    second.id = Uuid::new_v4();
    let target = second.id;
    e.session_mut().document.layers.push(second);
    let original = e.session().document.clone();
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(WindowOptions::new("Bevel").size(1180., 780.), e)
        .unwrap();
    let window = view.window_handle();
    cx.click(window, "layer-effects").unwrap();
    cx.click(window, "effect-tab-Bevel/Emboss").unwrap();
    cx.click(window, "effect-toggle").unwrap();
    cx.click(window, "bevel-style-Outer Bevel").unwrap();
    cx.update(view, |e, cx| {
        e.change_effect(|edit| {
            edit.set_number(Parameter::BevelSize, 8.);
            edit.set_number(Parameter::Depth, 200.);
            edit.set_number(Parameter::Altitude, 45.);
            edit.set_number(Parameter::Highlight, 60.);
            edit.set_number(Parameter::Shading, 40.);
        });
        e.copy_effect_to(target).unwrap();
        e.changed(cx);
    })
    .unwrap();
    if let Some(path) = std::env::var_os("COMPOSITOR_BEVEL_SCREENSHOT") {
        cx.update(view, |e, cx| {
            e.resolve_test_canvas_preview().unwrap();
            cx.invalidate();
        })
        .unwrap();
        assert!(cx.element_bounds(window, "canvas-retry").is_err());
        cx.capture_screenshot(window)
            .unwrap()
            .write_png(path)
            .unwrap();
    }
    cx.click(window, "effects-apply").unwrap();
    cx.update(view, |e, _| {
        let layer = e.session().document.active_layer().unwrap();
        let bevel = layer.effects.as_ref().unwrap().bevel.as_ref().unwrap();
        assert_eq!(bevel.style, Style::Outer);
        assert_eq!(bevel.size, 8.);
        assert_eq!(bevel.depth, 200.);
        assert_eq!(bevel.altitude, 45.);
        assert_eq!(bevel.highlight_opacity, 0.6);
        assert_eq!(bevel.shadow_opacity, 0.4);
        assert_eq!(
            layer.effects,
            e.session().document.layer(target).unwrap().effects
        );
        assert_eq!(layer.raster(), original.active_layer().unwrap().raster());
        e.open_effects().unwrap();
        e.change_effect(|edit| edit.effects.bevel.as_mut().unwrap().style = Style::Emboss);
        e.finish_effects(false).unwrap();
        assert_eq!(
            e.session()
                .document
                .active_layer()
                .unwrap()
                .effects
                .as_ref()
                .unwrap()
                .bevel
                .as_ref()
                .unwrap()
                .style,
            Style::Outer
        );
        e.session_mut().undo();
        assert_eq!(e.session().document, original);
    })
    .unwrap();
}
#[test]
fn invalid_bevel_draft_cannot_replace_preview_or_commit() {
    let mut e = editor();
    let original = e.session().document.clone();
    e.open_effects().unwrap();
    e.change_effect(|edit| {
        edit.kind = EffectKind::Bevel;
        edit.effects.bevel = Some(Box::default());
        edit.set_number(Parameter::BevelSize, 251.);
    });
    assert_eq!(e.session().document, original);
    assert!(e.finish_effects(true).is_err());
    e.finish_effects(false).unwrap();
    assert_eq!(e.session().document, original);
}
