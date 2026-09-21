use super::*;
use quickgui::{Application, Keystroke, Point, WindowOptions};

#[test]
fn empty_layer_message_fills_the_panel_and_undo_restores_the_list() {
    let editor = Editor::with_test_document();
    let original = editor.session().document.clone();
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Empty layers").size(1500., 900.), editor)
        .unwrap();
    let window = view.window_handle();
    cx.update(view, |e, cx| e.action(Action::DeleteLayer, cx))
        .unwrap();
    assert!(
        cx.read(view, |e| e.session().document.layers.is_empty())
            .unwrap()
    );
    let list = cx.element_bounds(window, "layer-list").unwrap();
    let title = cx.element_bounds(window, "empty-layer-title").unwrap();
    assert!(list.height > 300.);
    assert!((title.y + title.height / 2. - (list.y + list.height / 2.)).abs() < 30.);
    assert!(title.x > list.x && title.x + title.width < list.x + list.width);
    cx.update(view, |e, cx| e.action(Action::Undo, cx)).unwrap();
    assert_eq!(
        cx.read(view, |e| e.session().document.clone()).unwrap(),
        original
    );
    assert!(!cx.contains_element(window, "empty-layer-title").unwrap());
}

#[test]
fn eye_swipes_set_one_state_across_rows_autoscroll_and_undo_once() {
    let mut editor = Editor::with_test_document();
    for index in 1..24 {
        let mut layer = compositor::document::Layer::blank(format!("Layer {index}"), 20, 20);
        layer.visible = index % 2 != 0;
        editor.session_mut().document.add(layer).unwrap();
    }
    let original = editor.session().document.clone();
    let top = original.layers.last().unwrap().id;
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Visibility swipe").size(1280., 650.),
            editor,
        )
        .unwrap();
    let window = view.window_handle();
    let eye = format!("layer-eye-{top}");
    let bounds = cx.element_bounds(window, eye.as_str()).unwrap();
    let from = Point::new(bounds.x + 10., bounds.y + 10.);
    let mut event = PointerEvent {
        phase: PointerPhase::Down,
        position: from,
        origin: from,
        local_position: from,
        local_origin: from,
        delta: quickgui::Vector::ZERO,
        button: MouseButton::Left,
        modifiers: Modifiers::empty(),
        size: quickgui::Size::ZERO,
    };
    cx.simulate_pointer(window, eye.as_str(), event).unwrap();
    cx.read(view, |e| {
        assert!(!e.session().document.layer(top).unwrap().visible);
        assert!(e.session().undo_label().is_none());
    })
    .unwrap();
    let viewport = cx.element_bounds(window, "layer-list").unwrap();
    event.phase = PointerPhase::Move;
    event.position = Point::new(from.x, viewport.y + viewport.height + 20.);
    for _ in 0..30 {
        cx.simulate_pointer(window, eye.as_str(), event).unwrap();
    }
    cx.read(view, |e| {
        assert!(e.layer_list.scroll.offset().y > 0.);
        assert!(
            e.session()
                .document
                .layers
                .iter()
                .all(|layer| !layer.visible)
        );
        assert_eq!(e.session().document.active, original.active);
        assert_eq!(e.session().document.selected, original.selected);
        assert!(e.session().undo_label().is_none());
    })
    .unwrap();
    // Retracing does not toggle any row back on.
    event.position = from;
    cx.simulate_pointer(window, eye.as_str(), event).unwrap();
    event.phase = PointerPhase::Up;
    cx.simulate_pointer(window, eye.as_str(), event).unwrap();
    cx.update(view, |e, cx| {
        assert_eq!(e.session().undo_label(), Some("Hide Layer"));
        assert!(
            e.session()
                .document
                .layers
                .iter()
                .all(|layer| !layer.visible)
        );
        e.session_mut().undo();
        assert_eq!(e.session().document, original);
        assert!(e.session().undo_label().is_none());
        e.layer_list.scroll.set_offset(quickgui::Vector::ZERO);
        cx.invalidate();
    })
    .unwrap();
    // Space activates the focused eye without selecting its layer.
    cx.focus(window, eye.as_str()).unwrap();
    cx.simulate_keystroke(window, Keystroke::new(Key::Space, Modifiers::empty()))
        .unwrap();
    cx.read(view, |e| {
        assert_eq!(e.session().undo_label(), Some("Hide Layer"));
        assert!(!e.session().document.layer(top).unwrap().visible);
        assert_eq!(e.session().document.active, original.active);
    })
    .unwrap();
    cx.click(window, eye.as_str()).unwrap();
    cx.read(view, |e| {
        assert_eq!(e.session().undo_label(), Some("Show Layer"));
        assert!(e.session().document.layer(top).unwrap().visible);
        assert_eq!(e.session().document.active, original.active);
    })
    .unwrap();
}
