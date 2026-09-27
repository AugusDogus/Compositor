use super::*;
use compositor::vector_path::{Anchor, BezierPath, Closure, SavedPath};
use quickgui::{MouseButton, Point, PointerEvent, PointerPhase, Size, Vector};

fn editor() -> Editor {
    let mut e = Editor::with_test_document();
    let mut doc = Document::new(128, 128).unwrap();
    let mut path = BezierPath {
        anchors: vec![
            Anchor::corner([20., 20.]),
            Anchor::corner([80., 20.]),
            Anchor::corner([50., 80.]),
        ],
        closure: Closure::Closed,
    };
    path.anchors[1].outgoing = Some([100., 50.]);
    let saved = SavedPath::new("Triangle", path).unwrap();
    let id = saved.id;
    doc.paths.push(saved);
    e.tabs = vec![Session::new(doc, None).into()];
    e.tools.tool = Tool::Pen;
    e.tools.paths.active = Some(Active {
        target: Target::Saved(id),
        selected: None,
        mode: Mode::Editing,
    });
    e
}
fn pointer(p: [f64; 2], phase: PointerPhase) -> PointerEvent {
    let point = Point::new(p[0] as f32, p[1] as f32);
    PointerEvent {
        tablet: None,
        phase,
        position: point,
        origin: point,
        local_position: point,
        local_origin: point,
        delta: Vector::ZERO,
        button: MouseButton::Left,
        modifiers: Modifiers::empty(),
        size: Size::new(128., 128.),
    }
}
#[test]
fn creation_keeps_saved_path_and_style_and_rasterization_undo_independently() {
    let mut e = editor();
    let saved = e.session().document.paths.clone();
    e.create_path_shape().unwrap();
    let id = e.session().document.active.unwrap();
    assert_eq!(e.session().document.paths, saved);
    assert!(e.session().document.layer(id).unwrap().is_path_shape());
    assert!(!e.can_edit_pixels());
    let created = e.session().document.clone();
    e.apply_path_shape(id, &["none".into(), "#30609080".into(), "4".into()])
        .unwrap();
    let style = e
        .session()
        .document
        .layer(id)
        .unwrap()
        .path_shape()
        .unwrap()
        .source()
        .style;
    assert_eq!(style.fill, None);
    assert_eq!(
        style.stroke,
        Some(Stroke {
            width: 4.,
            color: [48, 96, 144, 128]
        })
    );
    let styled = e.session().document.clone();
    e.rasterize_path_shape().unwrap();
    assert!(!e.session().document.layer(id).unwrap().is_path_shape());
    assert!(e.can_edit_pixels());
    e.session_mut().undo();
    assert_eq!(e.session().document, styled);
    e.session_mut().undo();
    assert_eq!(e.session().document, created);
    e.session_mut().undo();
    assert!(e.session().document.layer(id).is_none());
    assert_eq!(e.session().document.paths, saved);
}
#[test]
fn invalid_style_keeps_shape_and_history() {
    let mut e = editor();
    e.create_path_shape().unwrap();
    let before = e.session().document.clone();
    let id = before.active.unwrap();
    for values in [
        ["none", "none", "1"],
        ["#000000", "#ffffff", "NaN"],
        ["#oops", "none", "1"],
        ["none", "#ffffff", "0"],
    ] {
        assert!(e.apply_path_shape(id, &values.map(str::to_owned)).is_err());
        assert_eq!(e.session().document, before);
        assert_eq!(e.session().undo_label(), Some("Create Path Shape"));
    }
}
#[test]
fn transformed_anchor_edits_repeat_stably_cancel_and_undo() {
    let mut e = editor();
    e.create_path_shape().unwrap();
    let id = e.session().document.active.unwrap();
    let layer = e
        .session_mut()
        .document
        .layers
        .iter_mut()
        .find(|l| l.id == id)
        .unwrap();
    layer.transform.rotation = 28.;
    layer.transform.size[0] *= 1.5;
    layer.transform.size[1] *= 0.75;
    layer.transform.flip_x = true;
    let before = e.session().document.clone();
    let geometry = path_shape::layer_path(&before, id).unwrap();
    let start = geometry.anchors[0].point;
    let end = [start[0] + 8., start[1] + 6.];
    e.path_down(start, 1., Modifiers::empty()).unwrap();
    e.path_pointer(&pointer(end, PointerPhase::Move), end)
        .unwrap();
    let moved = path_shape::layer_path(&e.session().document, id).unwrap();
    for _ in 0..10 {
        e.path_pointer(&pointer(end, PointerPhase::Move), end)
            .unwrap();
    }
    let repeated = path_shape::layer_path(&e.session().document, id).unwrap();
    for (a, b) in moved.anchors.iter().zip(repeated.anchors.iter()) {
        assert!((a.point[0] - b.point[0]).abs() < 1e-8);
        assert!((a.point[1] - b.point[1]).abs() < 1e-8);
    }
    e.path_pointer(&pointer(end, PointerPhase::Cancel), end)
        .unwrap();
    assert_eq!(e.session().document, before);
    e.path_down(start, 1., Modifiers::empty()).unwrap();
    e.path_pointer(&pointer(end, PointerPhase::Up), end)
        .unwrap();
    let after = path_shape::layer_path(&e.session().document, id).unwrap();
    assert!((after.anchors[0].point[0] - end[0]).abs() < 1e-8);
    assert!((after.anchors[0].point[1] - end[1]).abs() < 1e-8);
    e.session_mut().undo();
    assert_eq!(e.session().document, before);
}
#[test]
fn shape_selection_job_uses_shape_geometry() {
    let mut e = editor();
    e.create_path_shape().unwrap();
    e.path_command(super::super::path_header::Command::Select)
        .unwrap();
    let job = e.job.take().unwrap();
    let jobs::Job::Path { path, operation } = job else {
        panic!("Expected path operation")
    };
    assert!(matches!(path, Target::Shape(_)));
    let mut doc = e.session().document.clone();
    let geometry = path.snapshot(&doc).unwrap().geometry;
    operation.apply_geometry(&mut doc, &geometry).unwrap();
    assert!(doc.selection.is_some());
}

#[test]
fn pen_toolbar_creates_shape_and_opens_style_dialog() {
    use quickgui::{Application, WindowOptions};
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Path shape").size(1280., 800.), editor())
        .unwrap();
    let window = view.window_handle();
    cx.click(window, "path-shape").unwrap();
    cx.read(view, |e| {
        assert!(e.session().document.active_layer().unwrap().is_path_shape())
    })
    .unwrap();
    cx.click(window, "path-shape").unwrap();
    cx.read(view, |e| {
        assert!(matches!(
            e.modal,
            Some(Form::Edit {
                action: Action::EditPathShape(_),
                ..
            })
        ))
    })
    .unwrap();
    cx.capture_screenshot(window).unwrap();
}
