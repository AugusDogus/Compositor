use super::*;
use quickgui::{Application, MouseButton, Size, Vector, WindowOptions};
fn editor() -> Editor {
    let mut e = Editor::with_test_document();
    e.tabs = vec![Session::new(Document::new(100, 100).unwrap(), None).into()];
    e.session_mut().fit = false;
    e.session_mut().zoom = 1.;
    e.tools.tool = Tool::Pen;
    e
}
fn pointer(p: Point, phase: PointerPhase, modifiers: Modifiers) -> PointerEvent {
    let point = quickgui::Point::new(p[0] as f32, p[1] as f32);
    PointerEvent {
        tablet: None,
        phase,
        position: point,
        origin: point,
        local_position: point,
        local_origin: point,
        delta: Vector::ZERO,
        button: MouseButton::Left,
        modifiers,
        size: Size::new(100., 100.),
    }
}
fn click(e: &mut Editor, p: Point) {
    e.pointer(&pointer(p, PointerPhase::Down, Modifiers::empty()))
        .unwrap();
    e.pointer(&pointer(p, PointerPhase::Up, Modifiers::empty()))
        .unwrap();
}
#[test]
fn anchors_curves_close_and_undo_are_distinct_transactions() {
    let mut e = editor();
    click(&mut e, [10., 10.]);
    e.pointer(&pointer([70., 10.], PointerPhase::Down, Modifiers::empty()))
        .unwrap();
    e.pointer(&pointer([80., 25.], PointerPhase::Up, Modifiers::empty()))
        .unwrap();
    let a = e.active_path().unwrap().geometry.anchors[1];
    assert_eq!(a.incoming, Some([60., -5.]));
    assert_eq!(a.outgoing, Some([80., 25.]));
    click(&mut e, [70., 70.]);
    click(&mut e, [10., 10.]);
    assert_eq!(e.active_path().unwrap().geometry.closure, Closure::Closed);
    e.session_mut().undo();
    assert_eq!(e.active_path().unwrap().geometry.closure, Closure::Open);
    e.session_mut().undo();
    assert_eq!(e.active_path().unwrap().geometry.anchors.len(), 2);
}
#[test]
fn cancelling_new_anchor_and_handle_drag_preserves_committed_geometry() {
    let mut e = editor();
    click(&mut e, [20., 20.]);
    let committed = e.session().document.clone();
    e.pointer(&pointer([80., 80.], PointerPhase::Down, Modifiers::empty()))
        .unwrap();
    e.pointer(&pointer([95., 85.], PointerPhase::Move, Modifiers::empty()))
        .unwrap();
    e.pointer(&pointer(
        [95., 85.],
        PointerPhase::Cancel,
        Modifiers::empty(),
    ))
    .unwrap();
    assert_eq!(e.session().document, committed);
    e.pointer(&pointer([20., 20.], PointerPhase::Down, Modifiers::ALT))
        .unwrap();
    e.pointer(&pointer([40., 30.], PointerPhase::Up, Modifiers::ALT))
        .unwrap();
    let anchor = e.active_path().unwrap().geometry.anchors[0];
    e.pointer(&pointer([40., 30.], PointerPhase::Down, Modifiers::ALT))
        .unwrap();
    e.pointer(&pointer([50., 45.], PointerPhase::Up, Modifiers::ALT))
        .unwrap();
    let edited = e.active_path().unwrap().geometry.anchors[0];
    assert_eq!(edited.incoming, anchor.incoming);
    assert_eq!(edited.outgoing, Some([50., 45.]));
    e.session_mut().undo();
    assert_eq!(e.active_path().unwrap().geometry.anchors[0], anchor);
}
#[test]
fn empty_documents_can_draw_and_finishing_edits_commits_the_gesture() {
    let mut e = editor();
    e.session_mut().document.layers.clear();
    e.session_mut().document.active = None;
    e.session_mut().document.selected.clear();
    e.pointer(&pointer([10., 10.], PointerPhase::Down, Modifiers::empty()))
        .unwrap();
    e.finish_pending_edits().unwrap();
    assert!(e.gesture.is_none());
    assert_eq!(e.session().document.paths.len(), 1);
    e.session_mut().undo();
    e.sync_paths();
    assert!(e.tools.paths.active.is_none());
    assert!(e.session().document.paths.is_empty());
}
#[test]
fn pen_header_commands_keyboard_and_selection_job_work_together() {
    let mut e = editor();
    for p in [[10., 10.], [80., 10.], [80., 80.]] {
        click(&mut e, p);
    }
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Pen tool").size(1600., 900.), e)
        .unwrap();
    cx.update(view, |e, cx| e.key(&Key::Enter, Modifiers::empty(), cx))
        .unwrap();
    cx.click(view.window_handle(), "path-close").unwrap();
    cx.read(view, |e| {
        assert_eq!(e.active_path().unwrap().geometry.closure, Closure::Closed)
    })
    .unwrap();
    cx.update(view, |e, _| {
        e.path_command(super::super::path_header::Command::Select)
            .unwrap();
        let job = e.job.take().unwrap();
        let doc = job.run(e.session().document.clone()).unwrap();
        assert!(doc.selection.unwrap().coverage([60., 30.]) > 0.9);
        e.pending = false;
    })
    .unwrap();
    cx.click(view.window_handle(), "path-new").unwrap();
    cx.read(view, |e| {
        assert!(e.tools.paths.active.is_none());
        assert_eq!(e.session().document.paths.len(), 1);
    })
    .unwrap();
    cx.click(view.window_handle(), "path-picker").unwrap();
    cx.capture_screenshot(view.window_handle()).unwrap();
}

#[test]
fn hidden_handles_cannot_intercept_new_anchors() {
    let mut e = editor();
    click(&mut e, [10., 10.]);
    e.pointer(&pointer([40., 10.], PointerPhase::Down, Modifiers::empty()))
        .unwrap();
    e.pointer(&pointer([50., 20.], PointerPhase::Up, Modifiers::empty()))
        .unwrap();
    click(&mut e, [80., 80.]);
    click(&mut e, [50., 20.]);
    assert_eq!(e.active_path().unwrap().geometry.anchors.len(), 4);
    assert_eq!(
        e.active_path().unwrap().geometry.anchors[3].point,
        [50., 20.]
    );
}

#[test]
fn pending_jobs_block_anchor_deletion_and_mask_jobs_use_the_mask_palette() {
    let mut e = editor();
    for p in [[10., 10.], [80., 10.], [80., 80.]] {
        click(&mut e, p);
    }
    compositor::edits::add_mask(&mut e.session_mut().document, false).unwrap();
    e.tools.mask_target = true;
    e.tools.mask_paint_white = false;
    e.tools.brush.color = [200, 30, 70, 255];
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Path mask").size(1600., 900.), e)
        .unwrap();
    for button in ["path-fill", "path-stroke"] {
        cx.update(view, |e, cx| {
            e.path_command(if button == "path-fill" {
                super::super::path_header::Command::Fill
            } else {
                super::super::path_header::Command::Stroke
            })
            .unwrap();
            let original = e.session().document.clone();
            e.key(&Key::Delete, Modifiers::empty(), cx);
            assert_eq!(e.session().document, original);
            match e.job.take().unwrap() {
                jobs::Job::Path {
                    operation: compositor::path_operations::Operation::Fill { color, mask, .. },
                    ..
                } => {
                    assert!(mask);
                    assert_eq!(color, [0, 0, 0, 255]);
                }
                jobs::Job::Path {
                    operation: compositor::path_operations::Operation::Stroke { brush, mask, .. },
                    ..
                } => {
                    assert!(mask);
                    assert_eq!(brush.color, [0, 0, 0, 255]);
                }
                _ => panic!("Expected path paint job"),
            }
            e.pending = false;
            e.changed(cx);
        })
        .unwrap();
    }
}
