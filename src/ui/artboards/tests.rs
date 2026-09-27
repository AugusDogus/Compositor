use super::*;
use quickgui::{
    Application, MouseButton, Point, PointerEvent, PointerPhase, Size, Vector, WindowOptions,
};
fn editor() -> Editor {
    let mut e = Editor::with_test_document();
    e.tabs = vec![Session::new(Document::new(100, 100).unwrap(), None).into()];
    e.session_mut().fit = false;
    e.session_mut().zoom = 1.;
    e
}
#[test]
fn artboard_form_creates_named_rgba_board_and_invalid_resize_preserves_history() {
    let e = editor();
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Artboard settings").size(1400., 900.), e)
        .unwrap();
    cx.update(view, |e, cx| {
        e.action(Action::NewArtboard, cx);
    })
    .unwrap();
    for (i, value) in ["Card", "10", "20", "40", "30", "#F0804080"]
        .into_iter()
        .enumerate()
    {
        cx.update(view, |e, cx| {
            e.update_form_field(i, value);
            e.changed(cx);
        })
        .unwrap();
    }
    cx.click(view.window_handle(), "form-apply").unwrap();
    cx.read(view, |e| {
        let layer = e.session().document.active_layer().unwrap();
        assert_eq!(layer.name, "Card");
        assert_eq!(layer.transform.origin, [10., 20.]);
        assert!(matches!(
            layer.content,
            LayerContent::Artboard(artboard::Artboard {
                background: [240, 128, 64, 128]
            })
        ));
        assert_eq!(e.session().undo_label(), Some("New Artboard"));
    })
    .unwrap();
    cx.update(view, |e, cx| {
        e.action(Action::ArtboardSettings, cx);
        e.update_form_field(3, "NaN");
        e.submit_form(cx);
    })
    .unwrap();
    cx.read(view, |e| {
        assert!(matches!(&e.modal,Some(Form::Edit{error,..})if !error.is_empty()));
        assert_eq!(
            e.session().document.active_layer().unwrap().transform.size,
            [40., 30.]
        );
        assert_eq!(e.session().undo_label(), Some("New Artboard"));
    })
    .unwrap();
    cx.click(view.window_handle(), "form-cancel").unwrap();
    cx.update(view, |e, cx| e.action(Action::Undo, cx)).unwrap();
    cx.read(view, |e| {
        assert!(!e.session().document.layers.iter().any(|l| l.is_artboard()))
    })
    .unwrap();
}
fn pointer(x: f32, y: f32, phase: PointerPhase) -> PointerEvent {
    let p = Point::new(x, y);
    PointerEvent {
        tablet: None,
        phase,
        position: p,
        origin: p,
        local_position: p,
        local_origin: p,
        delta: Vector::ZERO,
        button: MouseButton::Left,
        modifiers: Modifiers::empty(),
        size: Size::new(400., 300.),
    }
}
#[test]
fn label_drag_preserves_screen_position_when_canvas_grows_and_cancel_restores_view() {
    let mut e = editor();
    let mut frame = Transform::new(40, 30);
    frame.origin = [10., 20.];
    let id = artboard::create(&mut e.session_mut().document, "Card", frame, [255; 4]).unwrap();
    let original = e.session().document.clone();
    let pan = e.session().pan;
    e.pointer(&pointer(170., 105., PointerPhase::Down)).unwrap();
    assert!(matches!(e.gesture, Some(Gesture::Artboard(_))));
    e.pointer(&pointer(500., 125., PointerPhase::Move)).unwrap();
    let current = e.session().document.layer(id).unwrap().transform;
    assert_eq!(current.origin, [340., 40.]);
    let (zoom, offset) = e.viewport(400., 300.);
    let label = super::super::artboard_overlay::label(current, zoom, offset);
    assert_eq!([label.x, label.y], [490., 116.]);
    e.pointer(&pointer(500., 125., PointerPhase::Cancel))
        .unwrap();
    assert_eq!(e.session().document, original);
    assert_eq!(e.session().pan, pan);
    e.pointer(&pointer(170., 105., PointerPhase::Down)).unwrap();
    e.pointer(&pointer(200., 125., PointerPhase::Up)).unwrap();
    assert_eq!(
        e.session().document.layer(id).unwrap().transform.origin,
        [40., 40.]
    );
    e.session_mut().undo();
    assert_eq!(e.session().document, original);
}
#[test]
fn from_layers_preserves_pixels_and_background_changes_invalidate_preview() {
    let mut e = editor();
    compositor::edits::fill(
        &mut e.session_mut().document,
        [30, 70, 90, 255],
        false,
        false,
    )
    .unwrap();
    e.artboard_from_layers().unwrap();
    let id = e.active_artboard().unwrap();
    let key = super::super::canvas_content::CanvasContent::new(&e.session().document);
    if let LayerContent::Artboard(board) = &mut e
        .session_mut()
        .document
        .layers
        .iter_mut()
        .find(|l| l.id == id)
        .unwrap()
        .content
    {
        board.background = [255; 4];
    }
    assert!(!key.matches(&e.session().document));
    assert_eq!(
        compositor::render::sample(&e.session().document, [50., 50.]).unwrap(),
        [30. / 255., 70. / 255., 90. / 255., 1.]
    );
}

#[test]
fn body_and_resize_drags_keep_a_stationary_pointer_stable_after_canvas_growth() {
    let mut e = editor();
    let mut frame = Transform::new(40, 30);
    frame.origin = [60., 20.];
    let id = artboard::create(&mut e.session_mut().document, "Card", frame, [255; 4]).unwrap();
    let mut child = compositor::document::Layer::blank("Child", 10, 10);
    child.parent = Some(id);
    child.transform.origin = [65., 25.];
    let childid = child.id;
    e.session_mut().document.add(child).unwrap();
    e.session_mut().document.select(id, false);
    for (start, end, expected) in [
        ([230., 135.], [260., 135.], [90., 20.]),
        ([280., 135.], [320., 135.], [90., 20.]),
    ] {
        // Recompute screen points for the current frame, so the second case
        // grabs the right-middle resize handle after moving the board.
        let current = e.session().document.layer(id).unwrap().transform;
        let (z, o) = e.viewport(400., 300.);
        let resize = start[0] == 280.;
        let actual = if resize {
            [
                (o[0] + (current.origin[0] + current.size[0]) * z) as f32,
                (o[1] + (current.origin[1] + current.size[1] / 2.) * z) as f32,
            ]
        } else {
            start
        };
        let target = if resize {
            [actual[0] + 40., actual[1]]
        } else {
            end
        };
        let child_before = e.session().document.layer(childid).unwrap().transform;
        e.pointer(&pointer(actual[0], actual[1], PointerPhase::Down))
            .unwrap();
        assert!(matches!(e.gesture, Some(Gesture::Artboard(_))));
        e.pointer(&pointer(target[0], target[1], PointerPhase::Move))
            .unwrap();
        let after = e.session().document.clone();
        e.pointer(&pointer(target[0], target[1], PointerPhase::Move))
            .unwrap();
        assert_eq!(e.session().document, after);
        e.pointer(&pointer(target[0], target[1], PointerPhase::Up))
            .unwrap();
        assert_eq!(
            e.session().document.layer(id).unwrap().transform.origin,
            expected
        );
        if resize {
            assert_eq!(
                e.session().document.layer(childid).unwrap().transform,
                child_before
            );
        }
    }
}

#[test]
fn auto_select_board_drag_stays_stable_and_hidden_boards_do_not_move() {
    let mut e = editor();
    let mut frame = Transform::new(40, 30);
    frame.origin = [60., 20.];
    let id = artboard::create(&mut e.session_mut().document, "Card", frame, [255; 4]).unwrap();
    e.tools.transform_auto_select = true;
    e.pointer(&pointer(230., 135., PointerPhase::Down)).unwrap();
    assert!(matches!(e.gesture, Some(Gesture::Artboard(_))));
    e.pointer(&pointer(300., 135., PointerPhase::Move)).unwrap();
    let moved = e.session().document.clone();
    e.pointer(&pointer(300., 135., PointerPhase::Up)).unwrap();
    assert_eq!(e.session().document, moved);
    e.session_mut()
        .document
        .layers
        .iter_mut()
        .find(|l| l.id == id)
        .unwrap()
        .visible = false;
    e.tools.transform_auto_select = false;
    let hidden = e.session().document.clone();
    e.pointer(&pointer(230., 135., PointerPhase::Down)).unwrap();
    e.pointer(&pointer(300., 135., PointerPhase::Up)).unwrap();
    assert_eq!(e.session().document, hidden);
}

#[test]
fn board_with_selected_child_edits_board_and_mixed_selection_is_rejected() {
    let mut e = editor();
    let board = artboard::create(
        &mut e.session_mut().document,
        "Card",
        Transform::new(100, 100),
        [0; 4],
    )
    .unwrap();
    let mut child = compositor::document::Layer::blank("Child", 10, 10);
    child.parent = Some(board);
    let child_id = child.id;
    e.session_mut().document.add(child).unwrap();
    e.session_mut().document.select(board, false);
    e.session_mut().document.select(child_id, true);
    assert_eq!(e.active_artboard(), Some(board));
    assert!(e.action_available(Action::ArtboardSettings));
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Artboard selection").size(1400., 900.),
            e,
        )
        .unwrap();
    cx.update(view, |e, cx| e.action(Action::Transform, cx))
        .unwrap();
    cx.read(view, |e| assert!(matches!(e.modal, Some(Form::Edit {action: Action::EditArtboard(id), ..}) if id == board))).unwrap();
    cx.update(view, |e, _| {
        e.modal = None;
        let other = compositor::document::Layer::blank("Other", 10, 10);
        let other_id = other.id;
        e.session_mut().document.add(other).unwrap();
        e.session_mut().document.select(board, false);
        e.session_mut().document.select(other_id, true);
        let original = e.session().document.clone();
        assert!(e.edit_selected_artboard().is_err());
        assert!(e.modal.is_none());
        assert_eq!(e.session().document, original);
    })
    .unwrap();
}

#[test]
fn maximum_artboard_count_can_draw_all_frame_outlines() {
    let mut e = editor();
    e.session_mut().document.layers = (0..10_000)
        .map(|i| {
            let mut layer = compositor::document::Layer::blank(format!("Board {i}"), 1, 1);
            layer.content = LayerContent::Artboard(artboard::Artboard { background: [0; 4] });
            layer.transform = Transform::new(20, 20);
            layer
        })
        .collect();
    assert!(e.artboard_overlay(1., [0., 30.], [100., 100.]).is_ok());
}
