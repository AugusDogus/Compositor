use super::*;
use quickgui::{Application, Keystroke, Point as UiPoint, WindowOptions};

fn selected_document() -> Document {
    let mut doc = Document::new(100, 80).unwrap();
    compositor::edits::fill(&mut doc, [80, 120, 200, 128], false, false).unwrap();
    doc.selection = Some(compositor::selection::Selection::rectangle(
        100,
        80,
        [20., 20.],
        [60., 60.],
        false,
    ));
    doc
}

#[test]
fn arrow_nudges_keep_floating_transform_drafts_pending() {
    for perspective in [false, true] {
        for apply in [false, true] {
            let original = selected_document();
            let mut editor = Editor::with_test_document();
            editor.tabs = vec![Session::new(original.clone(), None).into()];
            editor.begin_pixel_transform().unwrap();
            editor.header_transform_input(0, "25").unwrap();
            if perspective {
                editor.finish_header_transform(true).unwrap();
                editor
                    .preview_pixels(floating::Placement::Perspective([
                        [25., 20.],
                        [65., 25.],
                        [60., 60.],
                        [25., 60.],
                    ]))
                    .unwrap();
            }
            let before = editor.session().document.clone();
            let corners = editor.pending_pixels.as_ref().unwrap().placement.corners();
            editor.nudge(&Key::ArrowRight, Modifiers::empty()).unwrap();
            editor.nudge(&Key::ArrowDown, Modifiers::SHIFT).unwrap();
            assert!(editor.pending_pixels.is_some());
            assert!(editor.session().undo_label().is_none());
            assert_eq!(
                editor.pending_pixels.as_ref().unwrap().placement.corners(),
                corners.map(|p| [p[0] + 1., p[1] + 10.])
            );
            editor.nudge(&Key::ArrowLeft, Modifiers::empty()).unwrap();
            editor.nudge(&Key::ArrowUp, Modifiers::SHIFT).unwrap();
            assert_eq!(editor.session().document, before);
            editor.finish_toolbar_transform(apply).unwrap();
            if apply {
                assert_eq!(editor.session().undo_label(), Some("Transform Selection"));
                editor.session_mut().undo();
            }
            assert_eq!(editor.session().document, original);
            assert!(editor.session().undo_label().is_none());
        }
    }
}

#[test]
fn floating_numbers_then_handles_keep_original_source_and_one_undo_step() {
    for apply in [false, true] {
        let original = selected_document();
        let mut editor = Editor::with_test_document();
        editor.tabs = vec![Session::new(original.clone(), None).into()];
        editor.begin_pixel_transform().unwrap();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Floating inspector").size(1500., 900.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        cx.focus(window, "transform-value-0").unwrap();
        cx.simulate_keystroke(
            window,
            Keystroke::new(Key::Character("a".into()), Modifiers::CONTROL),
        )
        .unwrap();
        cx.simulate_input(window, "25").unwrap();
        cx.read(view, |e| {
            assert_eq!(
                e.pending_pixels.as_ref().unwrap().placement.bounds().origin,
                [25., 20.]
            );
            assert!(e.session().undo_label().is_none());
        })
        .unwrap();
        let canvas = cx.element_bounds(window, "canvas").unwrap();
        let (zoom, offset) = cx
            .update(view, |e, _| e.viewport(canvas.width, canvas.height))
            .unwrap();
        let point = |x, y| {
            UiPoint::new(
                canvas.x + (offset[0] + x * zoom) as f32,
                canvas.y + (offset[1] + y * zoom) as f32,
            )
        };
        cx.simulate_pointer_drag(window, "canvas", point(45., 40.), point(50., 40.))
            .unwrap();
        cx.read(view, |e| {
            assert!(e.transform_edit.is_none());
            assert!(e.pending_pixels.is_some());
            assert!(e.session().undo_label().is_none());
            assert_eq!(e.header_transform_bounds().unwrap().origin, [30., 20.]);
        })
        .unwrap();
        cx.click(window, "transform-flip-h").unwrap();
        cx.click(
            window,
            if apply {
                "transform-apply"
            } else {
                "transform-cancel"
            },
        )
        .unwrap();
        if apply {
            assert_eq!(
                cx.read(view, |e| e.session().undo_label().map(str::to_owned))
                    .unwrap()
                    .as_deref(),
                Some("Transform Selection")
            );
            cx.update(view, |e, cx| e.action(Action::Undo, cx)).unwrap();
        }
        cx.read(view, |e| {
            assert!(e.pending_pixels.is_none() && e.transform_edit.is_none());
            assert!(e.session().undo_label().is_none());
            assert_eq!(e.session().document, original);
        })
        .unwrap();
    }
}

#[test]
fn perspective_keeps_toolbar_but_disables_affine_fields_and_cancel_restores_pixels() {
    let original = selected_document();
    let mut editor = Editor::with_test_document();
    editor.tabs = vec![Session::new(original.clone(), None).into()];
    editor.begin_pixel_transform().unwrap();
    editor
        .preview_pixels(floating::Placement::Perspective([
            [10., 10.],
            [70., 20.],
            [60., 70.],
            [20., 60.],
        ]))
        .unwrap();
    let preview = editor.session().document.clone();
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Perspective inspector").size(1500., 900.),
            editor,
        )
        .unwrap();
    let window = view.window_handle();
    assert!(cx.element_bounds(window, "transform-value-0").is_ok());
    let flip = cx.click(window, "transform-flip-h");
    assert!(
        matches!(flip, Err(quickgui::TestAppError::NotClickable { .. })),
        "{flip:?}"
    );
    assert!(matches!(
        cx.click(window, "transform-sampling"),
        Err(quickgui::TestAppError::NotClickable { .. })
    ));
    cx.update(view, |e, _| e.header_transform_input(0, "100").unwrap())
        .unwrap();
    assert_eq!(
        cx.read(view, |e| e.session().document.clone()).unwrap(),
        preview
    );
    cx.click(window, "transform-cancel").unwrap();
    assert_eq!(
        cx.read(view, |e| e.session().document.clone()).unwrap(),
        original
    );
    assert!(
        cx.read(view, |e| e.session().undo_label().is_none())
            .unwrap()
    );
}

#[test]
fn cancelling_a_floating_drag_preserves_the_previous_draft_and_transaction() {
    use super::layer_tests::pointer;
    use quickgui::PointerPhase;
    for perspective in [false, true] {
        let original = selected_document();
        let mut editor = Editor::with_test_document();
        editor.tabs = vec![Session::new(original.clone(), None).into()];
        editor.begin_pixel_transform().unwrap();
        editor.header_transform_input(0, "25").unwrap();
        if perspective {
            editor.finish_header_transform(true).unwrap();
            editor
                .preview_pixels(floating::Placement::Perspective([
                    [25., 20.],
                    [65., 25.],
                    [60., 60.],
                    [25., 60.],
                ]))
                .unwrap();
        }
        let before = editor.session().document.clone();
        let corner = editor.pending_pixels.as_ref().unwrap().placement.corners()[0];
        pointer(&mut editor, PointerPhase::Down, corner, Modifiers::CONTROL);
        pointer(
            &mut editor,
            PointerPhase::Move,
            [35., 30.],
            Modifiers::CONTROL,
        );
        assert_ne!(editor.session().document, before);
        pointer(
            &mut editor,
            PointerPhase::Cancel,
            [35., 30.],
            Modifiers::CONTROL,
        );
        assert!(editor.pending_pixels.is_some());
        assert_eq!(editor.session().document, before);
        assert!(editor.session().undo_label().is_none());
        let collapsed = editor.pending_pixels.as_ref().unwrap().placement.corners()[2];
        pointer(&mut editor, PointerPhase::Down, corner, Modifiers::CONTROL);
        pointer(
            &mut editor,
            PointerPhase::Move,
            collapsed,
            Modifiers::CONTROL,
        );
        assert_eq!(editor.session().document, before);
        pointer(&mut editor, PointerPhase::Up, collapsed, Modifiers::CONTROL);
        editor.finish_toolbar_transform(true).unwrap();
        assert_eq!(editor.session().undo_label(), Some("Transform Selection"));
        editor.session_mut().undo();
        assert_eq!(editor.session().document, original);
    }
}

#[test]
fn ordinary_transform_fields_with_a_selection_target_the_layer_until_ctrl_t() {
    let original = selected_document();
    let mut editor = Editor::with_test_document();
    editor.tabs = vec![Session::new(original.clone(), None).into()];
    assert_eq!(editor.header_transform_bounds().unwrap().size, [100., 80.]);
    editor.header_transform_input(0, "5").unwrap();
    assert_eq!(
        editor.session().document.layers[0].transform.origin,
        [5., 0.]
    );
    assert_eq!(editor.session().document.selection, original.selection);
    assert_eq!(
        editor.session().document.layers[0].raster(),
        original.layers[0].raster()
    );
    editor.finish_toolbar_transform(false).unwrap();
    assert_eq!(editor.session().document, original);
    editor.begin_pixel_transform().unwrap();
    assert_eq!(editor.header_transform_bounds().unwrap().size, [40., 40.]);
    editor.finish_toolbar_transform(false).unwrap();
    assert_eq!(editor.session().document, original);
}
