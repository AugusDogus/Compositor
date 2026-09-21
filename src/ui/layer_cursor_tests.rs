use super::*;
use compositor::layer_ops;
use quickgui::{Application, MouseMoveEvent, WindowOptions};

#[test]
fn layer_modifiers_follow_thumbnail_and_clipping_geometry_without_changing_selection() {
    let mut e = Editor::with_test_document();
    let mut doc = Document::new(8, 8).unwrap();
    compositor::edits::fill(&mut doc, [255; 4], false, false).unwrap();
    layer_ops::duplicate_active(&mut doc).unwrap();
    let top = doc.active.unwrap();
    compositor::edits::add_mask(&mut doc, false).unwrap();
    let original = doc.clone();
    e.tabs = vec![Session::new(doc, None).into()];
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Layer cursors").size(1500., 900.), e)
        .unwrap();
    let window = view.window_handle();
    cx.update(view, |_, cx| cx.focus_window(window)).unwrap();
    let row = cx
        .element_bounds(window, format!("layer-row-{top}"))
        .unwrap();
    let image = cx
        .element_bounds(window, format!("layer-thumbnail-frame-{top}-false"))
        .unwrap();
    let mask = cx
        .element_bounds(window, format!("layer-thumbnail-frame-{top}-true"))
        .unwrap();
    let name = Point::new(row.x + row.width - 30., row.y + 15.);
    let bottom = Point::new(name.x, row.y + row.height - 2.);
    let image_point = Point::new(image.x + image.width / 2., image.y + image.height / 2.);
    let mask_point = Point::new(mask.x + mask.width / 2., mask.y + mask.height / 2.);
    use cursor_art::Glyph;
    for (point, keys, glyph) in [
        (name, Modifiers::empty(), None),
        (name, Modifiers::ALT, Some(Glyph::Duplicate)),
        (bottom, Modifiers::ALT, Some(Glyph::CreateClipping)),
        (image_point, Modifiers::CONTROL, Some(Glyph::LoadSelection)),
        (
            mask_point,
            Modifiers::CONTROL | Modifiers::ALT,
            Some(Glyph::LoadSelection),
        ),
        (mask_point, Modifiers::ALT, Some(Glyph::Duplicate)),
        (name, Modifiers::CONTROL, None),
    ] {
        cx.visual(window).unwrap().move_pointer(point).unwrap();
        cx.simulate_mouse_move(
            window,
            "layer-list",
            MouseMoveEvent {
                position: point,
                pressed_button: None,
                modifiers: keys,
            },
        )
        .unwrap();
        let scale = cx.window_state(window).unwrap().scale_factor;
        let expected = cx
            .update(view, |e, _| {
                Some(glyph.map_or(
                    quickgui::CursorOverrideId::System(quickgui::CursorStyle::Arrow),
                    |g| {
                        quickgui::CursorOverrideId::Image(
                            e.cursor_art.image(g, scale).unwrap().id(),
                        )
                    },
                ))
            })
            .unwrap();
        assert_eq!(cx.window_state(window).unwrap().cursor_override, expected);
    }
    assert_eq!(
        cx.read(view, |e| e.session().document.clone()).unwrap(),
        original
    );
    assert!(
        cx.read(view, |e| e.session().undo_label().is_none())
            .unwrap()
    );
    // Modifier-only changes must refresh the native cursor at a stationary pointer.
    cx.update(view, |e, cx| {
        e.event(&Event::ModifiersChanged(Modifiers::ALT), cx)
    })
    .unwrap();
    assert!(cx.window_state(window).unwrap().cursor_override.is_some());
    cx.update(view, |e, cx| {
        e.pending = true;
        cx.invalidate();
    })
    .unwrap();
    assert_eq!(
        cx.window_state(window).unwrap().cursor_override,
        Some(quickgui::CursorOverrideId::System(
            quickgui::CursorStyle::Arrow
        ))
    );
    cx.update(view, |e, cx| {
        e.pending = false;
        e.modal = Some(Form::Close);
        cx.invalidate();
    })
    .unwrap();
    assert!(cx.window_state(window).unwrap().cursor_override.is_none());
    // Re-evaluate hover beneath the modal, then uncover the same stationary pointer.
    cx.visual(window).unwrap().move_pointer(name).unwrap();
    cx.update(view, |e, cx| {
        e.modal = None;
        cx.invalidate();
    })
    .unwrap();
    cx.visual(window).unwrap().move_pointer(name).unwrap();
    let scale = cx.window_state(window).unwrap().scale_factor;
    assert_eq!(
        cx.window_state(window).unwrap().cursor_override,
        Some(quickgui::CursorOverrideId::Image(
            cx.update(view, |e, _| {
                e.cursor_art.image(Glyph::Duplicate, scale).unwrap().id()
            })
            .unwrap()
        ))
    );
    cx.visual(window)
        .unwrap()
        .move_pointer(Point::new(1200., 50.))
        .unwrap();
    assert!(cx.window_state(window).unwrap().cursor_override.is_none());
}

#[test]
fn alt_click_clips_the_hit_row_without_selecting_it_and_undo_restores_the_stack() {
    let mut e = Editor::with_test_document();
    let mut doc = Document::new(8, 8).unwrap();
    compositor::edits::fill(&mut doc, [255; 4], false, false).unwrap();
    let base = doc.active.unwrap();
    layer_ops::duplicate_active(&mut doc).unwrap();
    let target = doc.active.unwrap();
    doc.select(base, false);
    let original = doc.clone();
    e.tabs = vec![Session::new(doc, None).into()];
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Alt clipping").size(1500., 900.), e)
        .unwrap();
    let window = view.window_handle();
    let row = cx
        .element_bounds(window, format!("layer-row-{target}"))
        .unwrap();
    let press = MouseDownEvent {
        button: MouseButton::Left,
        position: Point::new(row.x + row.width - 30., row.y + row.height - 2.),
        modifiers: Modifiers::ALT,
        click_count: 1,
        first_mouse: false,
    };
    assert!(
        cx.simulate_mouse_down(window, format!("layer-drop-below-{target}"), press)
            .unwrap()
    );
    cx.read(view, |e| {
        assert_eq!(
            e.session().document.layer(target).unwrap().clip_source,
            Some(base)
        );
        assert_eq!(e.session().document.active, Some(base));
        assert_eq!(e.session().document.selected, original.selected);
    })
    .unwrap();
    cx.update(view, |e, cx| e.action(Action::Undo, cx)).unwrap();
    assert_eq!(
        cx.read(view, |e| e.session().document.clone()).unwrap(),
        original
    );
    assert!(
        cx.simulate_mouse_down(window, format!("layer-drop-below-{target}"), press)
            .unwrap()
    );
    assert!(
        cx.simulate_mouse_down(window, format!("layer-drop-below-{target}"), press)
            .unwrap()
    );
    assert_eq!(
        cx.read(view, |e| e.session().document.clone()).unwrap(),
        original
    );
    // An unavailable bottom-row press and an Alt-click on a mask never clip.
    let row = cx
        .element_bounds(window, format!("layer-row-{base}"))
        .unwrap();
    cx.simulate_mouse_down(
        window,
        "layer-list",
        MouseDownEvent {
            position: Point::new(row.x + row.width - 30., row.y + row.height - 2.),
            ..press
        },
    )
    .unwrap();
    assert_eq!(
        cx.read(view, |e| e.session().document.clone()).unwrap(),
        original
    );
    cx.update(view, |e, cx| {
        compositor::edits::add_mask(&mut e.session_mut().document, false).unwrap();
        cx.invalidate();
    })
    .unwrap();
    let mask = cx
        .element_bounds(window, format!("layer-thumbnail-frame-{base}-true"))
        .unwrap();
    cx.simulate_mouse_down(
        window,
        "layer-list",
        MouseDownEvent {
            position: Point::new(mask.x + 5., mask.y + mask.height - 1.),
            ..press
        },
    )
    .unwrap();
    assert!(
        cx.read(view, |e| e
            .session()
            .document
            .layer(base)
            .unwrap()
            .clip_source
            .is_none())
            .unwrap()
    );
}
