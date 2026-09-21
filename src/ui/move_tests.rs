use super::*;
use compositor::document::LayerContent;
use quickgui::{MouseButton, Point, PointerEvent, PointerPhase, Size, Vector};
use std::sync::Arc;

fn editor() -> Editor {
    let mut editor = Editor::with_test_document();
    let mut doc = Document::new(400, 300).unwrap();
    doc.layers[0].content = LayerContent::Raster(Some(Arc::new(image::RgbaImage::from_pixel(
        100,
        100,
        image::Rgba([255, 0, 0, 255]),
    ))));
    doc.layers[0].transform = compositor::geometry::Transform::new(100, 100);
    doc.layers[0].transform.origin = [150., 100.];
    editor.tabs = vec![Session::new(doc, None).into()];
    editor.session_mut().fit = false;
    editor.tools.tool = Tool::Move;
    editor
}

#[test]
fn zoom_click_keeps_the_clicked_document_pixel_fixed() {
    let mut e = editor();
    e.tools.tool = Tool::Zoom;
    e.session_mut().pan = [35., -27.];
    let point = [80., 65.];
    let (zoom, offset) = e.viewport(400., 300.);
    let before = [(point[0] - offset[0]) / zoom, (point[1] - offset[1]) / zoom];
    pointer(
        &mut e,
        PointerPhase::Down,
        point.map(|v| v as f32),
        Modifiers::empty(),
    );
    pointer(
        &mut e,
        PointerPhase::Up,
        point.map(|v| v as f32),
        Modifiers::empty(),
    );
    let (zoom, offset) = e.viewport(400., 300.);
    let after = [(point[0] - offset[0]) / zoom, (point[1] - offset[1]) / zoom];
    assert_eq!(zoom, 2.);
    assert_eq!(before, after);
    pointer(
        &mut e,
        PointerPhase::Down,
        point.map(|v| v as f32),
        Modifiers::ALT,
    );
    pointer(
        &mut e,
        PointerPhase::Up,
        point.map(|v| v as f32),
        Modifiers::ALT,
    );
    assert_eq!(e.session().zoom, 1.);
    assert_eq!(e.session().pan, [35., -27.]);
}

fn pointer(editor: &mut Editor, phase: PointerPhase, at: [f32; 2], modifiers: Modifiers) {
    let point = Point::new(at[0], at[1]);
    editor
        .pointer(&PointerEvent {
            phase,
            position: point,
            origin: point,
            local_position: point,
            local_origin: point,
            delta: Vector::ZERO,
            button: MouseButton::Left,
            modifiers,
            size: Size::new(400., 300.),
        })
        .unwrap();
}

#[test]
fn alt_drag_from_a_mask_targets_the_duplicated_layers_pixels() {
    let mut e = editor();
    compositor::edits::add_mask(&mut e.session_mut().document, false).unwrap();
    e.tools.mask_target = true;
    let original = e.session().document.clone();
    pointer(&mut e, PointerPhase::Down, [20., 20.], Modifiers::ALT);
    assert!(e.tools.mask_target);
    pointer(&mut e, PointerPhase::Move, [40., 30.], Modifiers::CONTROL);
    pointer(&mut e, PointerPhase::Up, [40., 30.], Modifiers::CONTROL);
    let doc = &e.session().document;
    assert_eq!(doc.layers.len(), 2);
    assert_eq!(doc.layers[0], original.layers[0]);
    assert_eq!(doc.active_layer().unwrap().transform.origin, [170., 110.]);
    assert_eq!(doc.active_layer().unwrap().mask, original.layers[0].mask);
    assert!(
        !e.tools.mask_target,
        "Selecting a new duplicate targets its pixels"
    );
    assert_eq!(e.session().undo_label(), Some("Duplicate Layer"));
    e.session_mut().undo();
    assert_eq!(e.session().document, original);
}

#[test]
fn alt_drag_outside_layer_duplicates_once_and_undo_restores_original() {
    let mut e = editor();
    let original = e.session().document.clone();
    pointer(&mut e, PointerPhase::Down, [20., 20.], Modifiers::ALT);
    assert_eq!(
        e.session().document,
        original,
        "a press alone must not duplicate"
    );
    pointer(&mut e, PointerPhase::Move, [30., 25.], Modifiers::CONTROL);
    pointer(&mut e, PointerPhase::Move, [40., 30.], Modifiers::CONTROL);
    pointer(&mut e, PointerPhase::Up, [40., 30.], Modifiers::CONTROL);
    let doc = &e.session().document;
    assert_eq!(doc.layers.len(), 2);
    assert_eq!(doc.layers[0], original.layers[0]);
    assert_eq!(doc.active_layer().unwrap().transform.origin, [170., 110.]);
    assert!(Arc::ptr_eq(
        doc.layers[0].raster().unwrap(),
        doc.layers[1].raster().unwrap()
    ));
    let moved = doc.clone();
    e.session_mut().undo();
    assert_eq!(e.session().document, original);
    assert!(e.session().undo_label().is_none());
    e.session_mut().redo();
    assert_eq!(e.session().document, moved);
}

#[test]
fn alt_click_and_cancelled_alt_drag_preserve_layers_and_history() {
    let mut e = editor();
    let original = e.session().document.clone();
    pointer(&mut e, PointerPhase::Down, [20., 20.], Modifiers::ALT);
    pointer(&mut e, PointerPhase::Up, [20., 20.], Modifiers::ALT);
    assert_eq!(e.session().document, original);
    pointer(&mut e, PointerPhase::Down, [20., 20.], Modifiers::ALT);
    pointer(&mut e, PointerPhase::Move, [40., 30.], Modifiers::CONTROL);
    assert_eq!(e.session().document.layers.len(), 2);
    pointer(&mut e, PointerPhase::Cancel, [40., 30.], Modifiers::empty());
    assert_eq!(e.session().document, original);
    assert!(e.session().undo_label().is_none());
}

#[test]
fn alt_drag_moves_multiple_layers_and_groups_without_duplicating() {
    for group in [false, true] {
        let mut e = editor();
        let mut second = e.session().document.layers[0].clone();
        second.id = uuid::Uuid::new_v4();
        second.transform.origin = [250., 100.];
        let id = second.id;
        e.session_mut().document.layers.push(second);
        e.session_mut().document.select(id, true);
        if group {
            compositor::layer_ops::group(&mut e.session_mut().document).unwrap();
        }
        let original = e.session().document.clone();
        pointer(&mut e, PointerPhase::Down, [20., 20.], Modifiers::ALT);
        pointer(&mut e, PointerPhase::Move, [40., 30.], Modifiers::CONTROL);
        pointer(&mut e, PointerPhase::Up, [40., 30.], Modifiers::CONTROL);
        assert_eq!(e.session().document.layers.len(), original.layers.len());
        for layer in original.layers.iter().filter(|layer| !layer.is_group()) {
            assert_eq!(
                e.session()
                    .document
                    .layer(layer.id)
                    .unwrap()
                    .transform
                    .origin,
                [
                    layer.transform.origin[0] + 20.,
                    layer.transform.origin[1] + 10.
                ]
            );
        }
    }
}

#[test]
fn temporary_eyedropper_samples_through_a_drag_without_editing_or_changing_tools() {
    for tool in [
        Tool::Brush,
        Tool::Erase,
        Tool::Heal,
        Tool::Gradient,
        Tool::Eyedropper,
    ] {
        let mut e = editor();
        e.tools.tool = tool;
        e.tools.brush.color = [10, 20, 30, 255];
        let original = e.session().document.clone();
        pointer(&mut e, PointerPhase::Down, [-10., 20.], Modifiers::ALT);
        assert_eq!(e.tools.brush.color, [10, 20, 30, 255]);
        pointer(&mut e, PointerPhase::Move, [20., 20.], Modifiers::ALT);
        assert_eq!(
            e.tools.brush.color,
            [10, 20, 30, 255],
            "transparent samples preserve color"
        );
        pointer(&mut e, PointerPhase::Move, [175., 125.], Modifiers::empty());
        assert_eq!(e.tools.brush.color, [255, 0, 0, 255]);
        pointer(&mut e, PointerPhase::Up, [175., 125.], Modifiers::empty());
        assert_eq!(e.tools.tool, tool);
        assert_eq!(e.session().document, original);
        assert!(e.session().undo_label().is_none());
        assert!(e.gesture.is_none());
    }
}

#[test]
fn mask_brush_uses_mask_palette_without_changing_pixel_palette() {
    let mut e = editor();
    e.tools.tool = Tool::Brush;
    e.tools.mask_target = true;
    e.tools.brush.color = [255, 0, 0, 255];
    e.tools.brush.hardness = 1.;
    e.tools.brush.diameter = 10.;
    e.session_mut().document.layers[0].mask = Some(compositor::document::Mask {
        pixels: Arc::new(image::GrayImage::from_pixel(100, 100, image::Luma([255]))),
        enabled: true,
        linked: true,
        placement: None,
    });
    let original = e.session().document.clone();
    pointer(&mut e, PointerPhase::Down, [175., 125.], Modifiers::empty());
    pointer(&mut e, PointerPhase::Up, [175., 125.], Modifiers::empty());
    assert_eq!(
        e.session().document.layers[0].mask.as_ref().unwrap().pixels[(25, 25)][0],
        0
    );
    assert_eq!(e.tools.brush.color, [255, 0, 0, 255]);
    let hidden = e.session().document.clone();
    e.tools.tool = Tool::Erase;
    e.tools.mask_paint_white = true;
    pointer(&mut e, PointerPhase::Down, [175., 125.], Modifiers::empty());
    pointer(&mut e, PointerPhase::Up, [175., 125.], Modifiers::empty());
    assert_eq!(
        e.session().document.layers[0].mask.as_ref().unwrap().pixels[(25, 25)][0],
        255
    );
    e.session_mut().undo();
    assert_eq!(e.session().document, hidden);
    e.session_mut().undo();
    assert_eq!(e.session().document, original);
}

#[test]
fn scrub_zoom_uses_initial_zoom_and_anchor_and_cancels_without_document_history() {
    let mut e = editor();
    e.tools.tool = Tool::Zoom;
    e.session_mut().pan = [35., -27.];
    let original = e.session().document.clone();
    pointer(&mut e, PointerPhase::Down, [80., 65.], Modifiers::empty());
    assert_eq!(e.session().zoom, 1.);
    pointer(&mut e, PointerPhase::Move, [82., 100.], Modifiers::empty());
    assert_eq!(e.session().zoom, 1., "vertical motion does not zoom");
    pointer(&mut e, PointerPhase::Move, [180., 65.], Modifiers::empty());
    assert_eq!(e.session().zoom, 2.);
    assert_eq!(e.session().pan, [190., 31.]);
    pointer(&mut e, PointerPhase::Move, [280., 65.], Modifiers::empty());
    assert_eq!(e.session().zoom, 4.);
    pointer(&mut e, PointerPhase::Up, [280., 65.], Modifiers::empty());
    assert_eq!(e.session().zoom, 4., "release must not add a click zoom");
    assert_eq!(e.session().document, original);
    assert!(e.session().undo_label().is_none());
    let pan = e.session().pan;
    pointer(&mut e, PointerPhase::Down, [80., 65.], Modifiers::empty());
    pointer(&mut e, PointerPhase::Move, [-20., 65.], Modifiers::empty());
    assert_eq!(e.session().zoom, 2.);
    pointer(
        &mut e,
        PointerPhase::Cancel,
        [-20., 65.],
        Modifiers::empty(),
    );
    assert_eq!(e.session().zoom, 4.);
    assert_eq!(e.session().pan, pan);
}

#[test]
fn numeric_zoom_changes_view_without_resizing_the_document() {
    use quickgui::{Application, Keystroke, WindowOptions};
    let mut e = editor();
    e.tools.tool = Tool::Zoom;
    let original = e.session().document.clone();
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Zoom").size(1280., 900.), e)
        .unwrap();
    let window = view.window_handle();
    cx.focus(window, "zoom-percent").unwrap();
    cx.simulate_keystroke(
        window,
        Keystroke::new(Key::Character("a".into()), Modifiers::CONTROL),
    )
    .unwrap();
    cx.simulate_input(window, "150").unwrap();
    cx.simulate_keystroke(window, Keystroke::new(Key::Enter, Modifiers::empty()))
        .unwrap();
    assert_eq!(cx.read(view, |e| e.session().zoom).unwrap(), 1.5);
    assert_eq!(
        cx.read(view, |e| e.session().document.clone()).unwrap(),
        original
    );
    assert!(
        cx.read(view, |e| e.session().undo_label().is_none())
            .unwrap()
    );
}
