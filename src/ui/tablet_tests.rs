use super::*;
use compositor::{document::Document, session::Session};
use quickgui::{MouseButton, PointerEvent, PointerPhase, Size, TabletInfo, Vector};

fn pointer(editor: &mut Editor, phase: PointerPhase, point: [f64; 2], pressure: f32, eraser: bool) {
    let (zoom, offset) = editor.viewport(200., 100.);
    let point = quickgui::Point::new(
        (point[0] * zoom + offset[0]) as f32,
        (point[1] * zoom + offset[1]) as f32,
    );
    editor
        .pointer(&PointerEvent {
            tablet: Some(TabletInfo {
                pressure: Some(pressure),
                tilt: Some([0.; 2]),
                eraser,
            }),
            phase,
            position: point,
            origin: point,
            local_position: point,
            local_origin: point,
            delta: Vector::ZERO,
            button: MouseButton::Left,
            modifiers: Modifiers::empty(),
            size: Size::new(200., 100.),
        })
        .unwrap();
}
fn editor() -> Editor {
    let mut editor = Editor::with_test_document();
    editor.tabs = vec![Session::new(Document::new(200, 100).unwrap(), None).into()];
    editor.session_mut().fit = false;
    editor.session_mut().zoom = 1.;
    editor.tools.brush.diameter = 40.;
    editor.tools.tool = Tool::Brush;
    editor
}
#[test]
fn tablet_pressure_and_eraser_edit_in_one_undo_without_changing_selected_tool() {
    let mut editor = editor();
    pointer(&mut editor, PointerPhase::Down, [40., 50.], 0.25, false);
    pointer(&mut editor, PointerPhase::Move, [80., 50.], 1., false);
    pointer(&mut editor, PointerPhase::Up, [80., 50.], 1., false);
    let painted = editor.session().document.clone();
    let pixels = painted.layers[0].raster().unwrap();
    assert_eq!(pixels[(40, 65)][3], 0);
    assert!(pixels[(80, 65)][3] > 200);
    editor.tools.tool = Tool::Move;
    pointer(&mut editor, PointerPhase::Down, [80., 50.], 0.5, true);
    pointer(&mut editor, PointerPhase::Up, [80., 50.], 0.5, true);
    assert_eq!(editor.tools.tool, Tool::Move);
    assert_eq!(
        editor.session().document.layers[0].raster().unwrap()[(80, 50)][3],
        0
    );
    editor.session_mut().undo();
    assert_eq!(editor.session().document, painted);
    editor.session_mut().undo();
    assert!(editor.session().document.layers[0].raster().is_none());
}
#[test]
fn cancel_discards_tablet_stroke_and_disabled_pressure_keeps_configured_size() {
    let mut editor = editor();
    let before = editor.session().document.clone();
    pointer(&mut editor, PointerPhase::Down, [40., 50.], 1., false);
    pointer(&mut editor, PointerPhase::Cancel, [40., 50.], 1., false);
    assert_eq!(editor.session().document, before);
    editor.tools.pen_pressure = false;
    pointer(&mut editor, PointerPhase::Down, [80., 50.], 0.1, false);
    pointer(&mut editor, PointerPhase::Up, [80., 50.], 0.1, false);
    assert!(editor.session().document.layers[0].raster().unwrap()[(80, 65)][3] > 200);
}

#[test]
fn pressure_changes_update_the_tip_while_the_smoothing_rope_is_slack() {
    let mut editor = editor();
    editor.tools.brush_smoothing = 20.;
    pointer(&mut editor, PointerPhase::Down, [40., 50.], 0.25, false);
    assert_eq!(
        editor.session().document.layers[0].raster().unwrap()[(40, 65)][3],
        0
    );
    pointer(&mut editor, PointerPhase::Move, [42., 50.], 1., false);
    assert!(editor.session().document.layers[0].raster().unwrap()[(40, 65)][3] > 200);
    assert_eq!(
        editor.session().document.layers[0].raster().unwrap()[(61, 50)][3],
        0
    );
}
