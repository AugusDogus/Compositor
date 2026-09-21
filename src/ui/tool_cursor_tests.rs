use super::*;
use compositor::selection::Selection;
use quickgui::{Application, MouseMoveEvent, WindowOptions};

#[test]
fn every_cursor_rasterizes_at_both_scales_with_reusable_native_identity() {
    let mut atlas = cursor_art::Atlas::default();
    let mut glyphs = vec![
        Glyph::Eyedropper,
        Glyph::ZoomIn,
        Glyph::ZoomOut,
        Glyph::Rotate,
        Glyph::Move,
        Glyph::Duplicate,
        Glyph::Distort,
        Glyph::MoveSelection,
        Glyph::MovePixels,
        Glyph::LoadSelection,
        Glyph::CreateClipping,
        Glyph::ReleaseClipping,
    ];
    for badge in [Badge::New, Badge::Add, Badge::Subtract] {
        glyphs.extend([
            Glyph::Rectangle(badge),
            Glyph::Ellipse(badge),
            Glyph::Lasso(badge),
            Glyph::Polygon(badge),
            Glyph::Wand(badge),
        ]);
    }
    for scale in [1., 2.] {
        for glyph in &glyphs {
            let image = atlas.image(*glyph, scale).unwrap();
            assert_eq!(image.id(), atlas.image(*glyph, scale).unwrap().id());
            let pixels = image.image().rgba();
            // Thin black strokes can antialias over their white outline at 1×.
            let black = pixels
                .chunks_exact(4)
                .filter(|p| p[3] > 200 && p[0] < 130)
                .count();
            let white = pixels
                .chunks_exact(4)
                .filter(|p| p[3] > 100 && p[0] > 200)
                .count();
            assert!(
                black > 10 && white > 10,
                "{glyph:?} {scale}: {black} black, {white} white"
            );
            assert!(pixels.chunks_exact(4).filter(|p| p[3] == 0).count() > 100);
            assert!(u32::from(image.hotspot()[0]) < image.image().width());
            assert!(u32::from(image.hotspot()[1]) < image.image().height());
        }
    }
    let movement = atlas.image(Glyph::Move, 1.).unwrap();
    let shaft = &movement.image().rgba()[(17 * 36 + 18) * 4..(17 * 36 + 18) * 4 + 4];
    assert!(
        shaft[0] < 20 && shaft[3] > 240,
        "The Move badge shaft must remain black above its white outline: {shaft:?}"
    );
    let plus = atlas.image(Glyph::ZoomIn, 1.).unwrap();
    let minus = atlas.image(Glyph::ZoomOut, 1.).unwrap();
    let pixel =
        |image: &quickgui::CursorImage, x: usize, y: usize| image.image().rgba()[(y * 24 + x) * 4];
    assert!(pixel(&plus, 10, 8) < 150 && pixel(&minus, 10, 8) > 240);
    assert_eq!(plus.hotspot(), [10, 10]);
    assert_eq!(
        atlas.image(Glyph::Eyedropper, 2.).unwrap().hotspot(),
        [6, 42]
    );
}

#[test]
fn selection_cursors_follow_modes_hover_and_captured_pixel_moves() {
    let mut e = Editor::with_test_document();
    e.canvas_pointer = Some([25., 25.]);
    let doc = &mut e.session_mut().document;
    doc.selection = Some(Selection::rectangle(
        1280,
        800,
        [20., 20.],
        [80., 80.],
        false,
    ));
    for tool in [
        Tool::Rectangle,
        Tool::Ellipse,
        Tool::Lasso,
        Tool::Polygon,
        Tool::Wand,
    ] {
        e.tools.tool = tool;
        e.keyboard_modifiers = Modifiers::empty();
        assert_eq!(
            e.canvas_cursor(1., [0.; 2]),
            Cursor::Image(Glyph::MoveSelection)
        );
        e.keyboard_modifiers = Modifiers::CONTROL;
        assert_eq!(
            e.canvas_cursor(1., [0.; 2]),
            Cursor::Image(Glyph::MovePixels)
        );
        e.keyboard_modifiers |= Modifiers::ALT;
        assert_eq!(
            e.canvas_cursor(1., [0.; 2]),
            Cursor::Image(Glyph::Duplicate)
        );
    }
    e.tools.tool = Tool::Rectangle;
    e.canvas_pointer = Some([100., 100.]);
    for (keys, badge) in [
        (Modifiers::empty(), Badge::New),
        (Modifiers::SHIFT, Badge::Add),
        (Modifiers::ALT, Badge::Subtract),
    ] {
        e.keyboard_modifiers = keys;
        assert_eq!(
            e.canvas_cursor(1., [0.; 2]),
            Cursor::Image(Glyph::Rectangle(badge))
        );
    }
    e.gesture = Some(Gesture::SelectionMove {
        selection: e.session().document.selection.clone().unwrap(),
        start: [25.; 2],
    });
    assert_eq!(
        e.canvas_cursor(1., [0.; 2]),
        Cursor::Image(Glyph::MoveSelection)
    );
    assert!(e.session().undo_label().is_none());
}

#[test]
fn transform_and_crop_cursors_follow_handles_rotation_distortion_and_drag_lock() {
    let mut e = Editor::with_test_document();
    compositor::edits::fill(&mut e.session_mut().document, [255; 4], false, false).unwrap();
    let bounds = Transform {
        origin: [100.; 2],
        size: [200., 100.],
        ..Transform::new(200, 100)
    };
    e.session_mut()
        .document
        .active_layer_mut()
        .unwrap()
        .transform = bounds;
    e.tools.tool = Tool::Move;
    e.tools.show_transform_controls = true;
    for (point, expected) in [
        (
            [100., 100.],
            Cursor::System(CursorStyle::ResizeUpLeftDownRight),
        ),
        ([200., 100.], Cursor::System(CursorStyle::ResizeUpDown)),
        ([200., 72.], Cursor::Image(Glyph::Rotate)),
        ([200., 150.], Cursor::Image(Glyph::Move)),
    ] {
        e.canvas_pointer = Some(point);
        assert_eq!(e.canvas_cursor(1., [0.; 2]), expected);
    }
    e.canvas_pointer = Some([100.; 2]);
    e.keyboard_modifiers = Modifiers::CONTROL;
    assert_eq!(e.canvas_cursor(1., [0.; 2]), Cursor::Image(Glyph::Distort));
    e.keyboard_modifiers = Modifiers::ALT;
    e.canvas_pointer = Some([200., 150.]);
    assert_eq!(
        e.canvas_cursor(1., [0.; 2]),
        Cursor::Image(Glyph::Duplicate)
    );
    e.gesture = Some(Gesture::Transform {
        original: Box::new(e.session().document.clone()),
        bounds,
        start: [100.; 2],
        handle: Handle::Resize(0),
    });
    e.canvas_pointer = Some([800., 700.]);
    assert_eq!(
        e.canvas_cursor(1., [0.; 2]),
        Cursor::System(CursorStyle::ResizeUpLeftDownRight)
    );
    e.gesture = None;
    e.keyboard_modifiers = Modifiers::empty();
    let rotated = Transform {
        rotation: 45.,
        ..bounds
    };
    e.session_mut()
        .document
        .active_layer_mut()
        .unwrap()
        .transform = rotated;
    e.canvas_pointer = Some(rotated.geometry_point([0., 0.]));
    assert_eq!(
        e.canvas_cursor(1., [0.; 2]),
        Cursor::System(CursorStyle::ResizeUpDown)
    );
    e.tools.tool = Tool::Crop;
    e.tools.pending_crop = Some(crop::CropPreview {
        frame: bounds,
        guides: [None; 2],
    });
    e.canvas_pointer = Some([300., 150.]);
    assert_eq!(
        e.canvas_cursor(1., [0.; 2]),
        Cursor::System(CursorStyle::ResizeLeftRight)
    );
}

#[test]
fn cursor_bitmaps_switch_on_entry_modifiers_tools_and_restore_on_exit_and_modal() {
    let mut e = Editor::with_test_document();
    e.tools.tool = Tool::Zoom;
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Native cursors").size(1280., 850.), e)
        .unwrap();
    let window = view.window_handle();
    let bounds = cx.element_bounds(window, "canvas").unwrap();
    cx.update(view, |_, cx| cx.focus_window(window)).unwrap();
    let motion = MouseMoveEvent {
        position: quickgui::Point::new(bounds.x + 100., bounds.y + 100.),
        pressed_button: None,
        modifiers: Modifiers::empty(),
    };
    cx.visual(window)
        .unwrap()
        .move_pointer(motion.position)
        .unwrap();
    cx.simulate_mouse_move(window, "canvas", motion).unwrap();
    let plus = cx
        .window_state(window)
        .unwrap()
        .cursor_override
        .expect("Entering canvas installs the zoom cursor");
    cx.update(view, |e, cx| {
        e.event(
            &Event::FilesHovered(quickgui::DroppedFiles::new([std::path::PathBuf::from(
                "photo.png",
            )])),
            cx,
        );
    })
    .unwrap();
    assert!(cx.window_state(window).unwrap().cursor_override.is_none());
    cx.update(view, |e, cx| e.event(&Event::FilesHoverCancelled, cx))
        .unwrap();
    assert_eq!(cx.window_state(window).unwrap().cursor_override, Some(plus));
    cx.update(view, |e, cx| {
        e.event(&Event::ModifiersChanged(Modifiers::ALT), cx)
    })
    .unwrap();
    let minus = cx.window_state(window).unwrap().cursor_override.unwrap();
    assert_ne!(plus, minus);
    cx.update(view, |e, cx| {
        e.event(&Event::ModifiersChanged(Modifiers::empty()), cx)
    })
    .unwrap();
    assert_eq!(cx.window_state(window).unwrap().cursor_override, Some(plus));
    cx.update(view, |e, cx| e.select_tool(Tool::Eyedropper, cx))
        .unwrap();
    assert_ne!(cx.window_state(window).unwrap().cursor_override, Some(plus));
    cx.update(view, |e, cx| {
        e.space_pan = true;
        cx.invalidate();
    })
    .unwrap();
    assert_eq!(
        cx.window_state(window).unwrap().cursor_override,
        Some(quickgui::CursorOverrideId::System(CursorStyle::OpenHand))
    );
    cx.update(view, |e, cx| {
        e.space_pan = false;
        e.modal = Some(Form::Close);
        cx.invalidate();
    })
    .unwrap();
    assert!(cx.window_state(window).unwrap().cursor_override.is_none());
    cx.update(view, |e, cx| {
        e.modal = None;
        e.select_tool(Tool::Clone, cx);
        e.tools.clone_source = Some([20.; 2]);
        cx.invalidate();
    })
    .unwrap();
    assert!(!cx.window_state(window).unwrap().cursor_visible);
    cx.update(view, |e, cx| {
        e.tab_scrolling.dragging = true;
        cx.invalidate();
    })
    .unwrap();
    assert!(cx.window_state(window).unwrap().cursor_visible);
    assert!(cx.window_state(window).unwrap().cursor_override.is_none());
    cx.update(view, |e, cx| e.event(&Event::FilesHoverCancelled, cx))
        .unwrap();
    assert!(!cx.window_state(window).unwrap().cursor_visible);
    cx.visual(window)
        .unwrap()
        .move_pointer(quickgui::Point::new(10., 10.))
        .unwrap();
    assert!(cx.window_state(window).unwrap().cursor_visible);
    assert!(cx.window_state(window).unwrap().cursor_override.is_none());
    assert!(
        cx.read(view, |e| e.session().undo_label().is_none())
            .unwrap()
    );
}

#[test]
fn project_drag_completion_discards_cached_canvas_hover_outside_the_canvas() {
    let mut e = Editor::with_test_document();
    e.tools.tool = Tool::Zoom;
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Drag cursor exit").size(1280., 850.), e)
        .unwrap();
    let window = view.window_handle();
    cx.update(view, |_, cx| cx.focus_window(window)).unwrap();
    cx.visual(window)
        .unwrap()
        .move_pointer(quickgui::Point::new(28., 51.))
        .unwrap();
    cx.update(view, |e, cx| {
        // Native drags can leave the last canvas hover cached while crossing the toolbar.
        e.canvas_pointer = Some([100., 100.]);
        e.layer_list.cursors.pointer = Some(quickgui::Point::new(1100., 275.));
        e.tab_scrolling.dragging = true;
        e.event(&Event::FilesHoverCancelled, cx);
    })
    .unwrap();
    assert!(cx.read(view, |e| e.canvas_pointer.is_none()).unwrap());
    assert!(
        cx.read(view, |e| e.layer_list.cursors.pointer.is_none())
            .unwrap()
    );
    assert!(cx.window_state(window).unwrap().cursor_override.is_none());
    assert!(cx.window_state(window).unwrap().cursor_visible);
    assert!(
        cx.read(view, |e| e.session().undo_label().is_none())
            .unwrap()
    );
}

#[test]
fn captured_pan_keeps_closed_hand_outside_canvas_until_release() {
    let mut e = Editor::with_test_document();
    e.tools.tool = Tool::Hand;
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Captured cursor").size(1280., 850.), e)
        .unwrap();
    let window = view.window_handle();
    let bounds = cx.element_bounds(window, "canvas").unwrap();
    cx.update(view, |_, cx| cx.focus_window(window)).unwrap();
    let inside = quickgui::Point::new(bounds.x + 100., bounds.y + 100.);
    cx.visual(window).unwrap().move_pointer(inside).unwrap();
    cx.simulate_mouse_move(
        window,
        "canvas",
        MouseMoveEvent {
            position: inside,
            pressed_button: None,
            modifiers: Modifiers::empty(),
        },
    )
    .unwrap();
    let event = quickgui::PointerEvent {
        phase: quickgui::PointerPhase::Down,
        position: inside,
        origin: inside,
        local_position: quickgui::Point::new(100., 100.),
        local_origin: quickgui::Point::new(100., 100.),
        delta: quickgui::Vector::ZERO,
        button: quickgui::MouseButton::Left,
        modifiers: Modifiers::empty(),
        size: quickgui::Size::new(bounds.width, bounds.height),
    };
    cx.simulate_pointer(window, "canvas", event).unwrap();
    let closed = Some(quickgui::CursorOverrideId::System(CursorStyle::ClosedHand));
    assert_eq!(cx.window_state(window).unwrap().cursor_override, closed);
    let outside = quickgui::Point::new(10., 10.);
    cx.visual(window).unwrap().move_pointer(outside).unwrap();
    assert_eq!(cx.window_state(window).unwrap().cursor_override, closed);
    cx.simulate_mouse_move(
        window,
        "canvas",
        MouseMoveEvent {
            position: outside,
            pressed_button: Some(quickgui::MouseButton::Left),
            modifiers: Modifiers::empty(),
        },
    )
    .unwrap();
    assert!(cx.read(view, |e| e.canvas_pointer.is_none()).unwrap());
    cx.simulate_pointer(
        window,
        "canvas",
        quickgui::PointerEvent {
            phase: quickgui::PointerPhase::Up,
            position: outside,
            local_position: quickgui::Point::new(outside.x - bounds.x, outside.y - bounds.y),
            ..event
        },
    )
    .unwrap();
    assert!(cx.window_state(window).unwrap().cursor_override.is_none());
    assert!(
        cx.read(view, |e| e.session().undo_label().is_none())
            .unwrap()
    );
}

#[test]
fn object_cursor_keeps_prompt_badges_inside_selection_and_retains_ctrl_pixel_moves() {
    let mut editor = Editor::with_test_document();
    editor.tools.tool = Tool::Object;
    editor.canvas_pointer = Some([25., 25.]);
    editor.session_mut().document.selection = Some(Selection::rectangle(
        1280,
        800,
        [20., 20.],
        [80., 80.],
        false,
    ));
    for (modifiers, glyph) in [
        (Modifiers::empty(), Glyph::Object(Badge::New)),
        (Modifiers::SHIFT, Glyph::Object(Badge::Add)),
        (Modifiers::ALT, Glyph::Object(Badge::Subtract)),
        (Modifiers::CONTROL, Glyph::MovePixels),
        (Modifiers::CONTROL | Modifiers::ALT, Glyph::Duplicate),
    ] {
        editor.keyboard_modifiers = modifiers;
        assert_eq!(editor.canvas_cursor(1., [0.; 2]), Cursor::Image(glyph));
    }
}
