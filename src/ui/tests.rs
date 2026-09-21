use super::*;
use quickgui::{Application, WindowOptions};

#[test]
fn canvas_copy_shortcuts_keep_image_clipboard_instead_of_copying_interface_text() {
    let mut view = Editor::with_test_document();
    let mut doc = Document::new(2, 2).unwrap();
    compositor::edits::fill(&mut doc, [80, 120, 160, 255], false, false).unwrap();
    view.tabs = vec![Session::new(doc, None).into()];
    let (mut cx, editor) = Application::new()
        .into_test_context(
            WindowOptions::new("Canvas clipboard").size(1280., 850.),
            view,
        )
        .unwrap();
    let window = editor.window_handle();
    cx.focus(window, "workspace").unwrap();
    for key in ["a", "c"] {
        cx.simulate_keystroke(
            window,
            quickgui::Keystroke::new(Key::Character(key.into()), Modifiers::CONTROL),
        )
        .unwrap();
    }
    let clipboard = cx.read_from_clipboard().unwrap().unwrap();
    assert!(
        clipboard
            .entries()
            .iter()
            .any(|entry| matches!(entry, quickgui::ClipboardEntry::Image(_))),
        "{clipboard:?}"
    );
}

#[test]
fn hue_target_drag_selects_the_sampled_range_and_eyedropper_recenters_it() {
    use compositor::adjustment::ColorRange;
    let mut view = Editor::with_test_document();
    let mut doc = Document::new(100, 40).unwrap();
    doc.layers[0].content = compositor::document::LayerContent::Raster(Some(std::sync::Arc::new(
        image::RgbaImage::from_fn(100, 40, |x, _| {
            image::Rgba(if x < 50 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            })
        }),
    )));
    view.tabs = vec![Session::new(doc.clone(), None).into()];
    view.open_pixel_adjustment(Kind::HueSaturation).unwrap();
    let (mut cx, editor) = Application::new()
        .into_test_context(WindowOptions::new("Hue sampling").size(1280., 900.), view)
        .unwrap();
    let window = editor.window_handle();
    cx.click(window, "hue-Target color").unwrap();
    let bounds = cx.element_bounds(window, "canvas").unwrap();
    let (zoom, offset) = cx
        .update(editor, |e, _| e.viewport(bounds.width, bounds.height))
        .unwrap();
    let point = |x| {
        quickgui::Point::new(
            bounds.x + (offset[0] + x * zoom) as f32,
            bounds.y + (offset[1] + 20. * zoom) as f32,
        )
    };
    let start = point(75.);
    cx.simulate_pointer_drag(
        window,
        "canvas",
        start,
        quickgui::Point::new(start.x + 20., start.y),
    )
    .unwrap();
    cx.read(editor, |e| {
        let settings = e
            .adjustment_edit
            .as_ref()
            .unwrap()
            .settings
            .hsv_settings
            .as_ref()
            .unwrap();
        assert_eq!(settings.range, ColorRange::Blues);
        assert_eq!(
            settings
                .adjustments
                .iter()
                .find(|(r, _)| *r == ColorRange::Blues)
                .unwrap()
                .1
                .saturation,
            10.
        );
    })
    .unwrap();
    cx.click(window, "hue-Sample range").unwrap();
    cx.simulate_pointer_drag(window, "canvas", point(25.), point(25.))
        .unwrap();
    cx.read(editor, |e| {
        let settings = e
            .adjustment_edit
            .as_ref()
            .unwrap()
            .settings
            .hsv_settings
            .as_ref()
            .unwrap();
        let band = settings
            .bands
            .iter()
            .find(|(r, _)| *r == ColorRange::Blues)
            .unwrap()
            .1;
        assert_eq!(band.range_start, 345.);
        assert_eq!(band.range_end, 15.);
    })
    .unwrap();
    cx.click(window, "hue-Sample range").unwrap();
    assert!(!cx.read(editor, |e| e.adjustment_sampling()).unwrap());
    assert!(cx.element_bounds(window, 50_000_u64).unwrap().height >= 26.);
    // Non-master ranges use the source spectrum editor, without extra numeric rows.
    assert_eq!(
        cx.element_bounds(window, "hue-band-handles")
            .unwrap()
            .height,
        12.
    );
    assert!(cx.element_bounds(window, 50_006_u64).is_err());
    cx.click(window, "form-cancel").unwrap();
    assert_eq!(
        cx.read(editor, |e| e.session().document.clone()).unwrap(),
        doc
    );
}

#[test]
fn levels_eyedropper_samples_canvas_and_returns_to_the_same_edit() {
    let mut view = Editor::with_test_document();
    let mut doc = Document::new(20, 20).unwrap();
    compositor::edits::fill(&mut doc, [51, 102, 153, 255], false, false).unwrap();
    view.tabs = vec![Session::new(doc.clone(), None).into()];
    view.open_pixel_adjustment(Kind::Levels).unwrap();
    let (mut cx, editor) = Application::new()
        .into_test_context(
            WindowOptions::new("Levels sampling").size(1280., 900.),
            view,
        )
        .unwrap();
    let window = editor.window_handle();
    cx.click(window, "levels-sample-Black").unwrap();
    assert_eq!(
        cx.read(editor, |e| e.levels_sample_mode()).unwrap(),
        Some(compositor::levels_sample::LevelsSample::Black)
    );
    let bounds = cx.element_bounds(window, "canvas").unwrap();
    let point = quickgui::Point::new(bounds.x + bounds.width / 2., bounds.y + bounds.height / 2.);
    cx.simulate_pointer_drag(window, "canvas", point, point)
        .unwrap();
    cx.read(editor, |e| {
        assert_eq!(
            e.adjustment_edit.as_ref().unwrap().settings.levels.ranges[2].black,
            102.
        );
        assert!(e.session().document.selection.is_none());
    })
    .unwrap();
    cx.click(window, "levels-sample-Black").unwrap();
    assert!(
        cx.read(editor, |e| e.levels_sample_mode().is_none())
            .unwrap()
    );
    cx.click(window, "levels-sample-Gray").unwrap();
    cx.simulate_keystroke(
        window,
        quickgui::Keystroke::new(Key::Escape, Modifiers::empty()),
    )
    .unwrap();
    assert!(
        cx.read(editor, |e| e.levels_sample_mode().is_none())
            .unwrap()
    );
    assert!(cx.read(editor, |e| e.adjustment_edit.is_none()).unwrap());
    assert_eq!(
        cx.read(editor, |e| e.session().document.clone()).unwrap(),
        doc
    );
}

#[test]
fn selection_edges_follow_the_control_and_marquee_click_deselects() {
    let mut view = Editor::with_test_document();
    view.tabs = vec![Session::new(Document::new(100, 100).unwrap(), None).into()];
    view.tools.tool = Tool::Ellipse;
    let (mut cx, editor) = Application::new()
        .into_test_context(
            WindowOptions::new("Selection edges").size(1280., 900.),
            view,
        )
        .unwrap();
    let window = editor.window_handle();
    let bounds = cx.element_bounds(window, "canvas").unwrap();
    let (zoom, offset) = cx
        .update(editor, |e, _| e.viewport(bounds.width, bounds.height))
        .unwrap();
    let p = |x, y| {
        quickgui::Point::new(
            bounds.x + (offset[0] + x * zoom) as f32,
            bounds.y + (offset[1] + y * zoom) as f32,
        )
    };
    cx.simulate_pointer_drag(window, "canvas", p(10.2, 10.3), p(60.2, 60.3))
        .unwrap();
    cx.read(editor, |e| {
        let selection = e.session().document.selection.as_ref().unwrap();
        assert_eq!(selection.bounds(), Some([10., 10., 60., 60.]));
        assert!(
            selection
                .pixels
                .dense()
                .unwrap()
                .pixels()
                .any(|p| (1..255).contains(&p[0]))
        );
    })
    .unwrap();
    cx.focus(window, "workspace").unwrap();
    cx.simulate_keystroke(
        window,
        quickgui::Keystroke::new(Key::Character("z".into()), Modifiers::CONTROL),
    )
    .unwrap();
    cx.click(window, "selection-antialias").unwrap();
    cx.simulate_pointer_drag(window, "canvas", p(10., 10.), p(60., 60.))
        .unwrap();
    cx.read(editor, |e| {
        assert!(
            e.session()
                .document
                .selection
                .as_ref()
                .unwrap()
                .pixels
                .dense()
                .unwrap()
                .pixels()
                .all(|p| p[0] == 0 || p[0] == 255)
        )
    })
    .unwrap();
    cx.simulate_pointer_drag(window, "canvas", p(80., 80.), p(80., 80.))
        .unwrap();
    assert!(
        cx.read(editor, |e| e.session().document.selection.is_none())
            .unwrap()
    );
    cx.focus(window, "workspace").unwrap();
    cx.simulate_keystroke(
        window,
        quickgui::Keystroke::new(Key::Character("z".into()), Modifiers::CONTROL),
    )
    .unwrap();
    assert!(
        cx.read(editor, |e| e.session().document.selection.is_some())
            .unwrap()
    );
    cx.update(editor, |e, cx| {
        e.tools.tool = Tool::Polygon;
        e.tools.polygon = Some(super::selection_tools::PolygonDraft {
            points: vec![[0., 0.], [100., 0.], [0., 100.]],
            mode: compositor::selection::SelectionMode::Replace,
        });
        e.finish_polygon(cx);
    })
    .unwrap();
    cx.read(editor, |e| {
        assert!(
            e.session()
                .document
                .selection
                .as_ref()
                .unwrap()
                .pixels
                .dense()
                .unwrap()
                .pixels()
                .all(|p| p[0] == 0 || p[0] == 255)
        )
    })
    .unwrap();
}

#[test]
fn pressing_a_selected_row_keeps_the_drag_set_and_cross_tab_copy_copies_all_roots() {
    use super::layer_drag::{LayerDrag, Transfer};
    let mut view = Editor::with_test_document();
    let mut doc = Document::new(4, 4).unwrap();
    let first = doc.active.unwrap();
    doc.add(compositor::document::Layer::blank("Second", 4, 4))
        .unwrap();
    let second = doc.active.unwrap();
    doc.select(first, true);
    let selected = doc.selected.clone();
    view.tabs = vec![
        Session::new(doc, None).into(),
        Session::new(Document::new(4, 4).unwrap(), None).into(),
    ];
    let drag = LayerDrag {
        session: view.tabs[0].id,
        layer: first,
        operation: Transfer::Copy,
    };
    let (mut cx, editor) = Application::new()
        .into_test_context(
            WindowOptions::new("Multiple layer drag").size(1280., 900.),
            view,
        )
        .unwrap();
    cx.simulate_mouse_down(
        editor.window_handle(),
        1001_u64,
        quickgui::MouseDownEvent {
            button: quickgui::MouseButton::Left,
            position: quickgui::Point::new(1100., 250.),
            modifiers: Modifiers::empty(),
            click_count: 1,
            first_mouse: false,
        },
    )
    .unwrap();
    assert_eq!(
        cx.read(editor, |e| e.session().document.selected.clone())
            .unwrap(),
        selected
    );
    cx.click(editor.window_handle(), 1001_u64).unwrap();
    assert_eq!(
        cx.read(editor, |e| e.session().document.selected.clone())
            .unwrap(),
        std::collections::HashSet::from([first])
    );
    cx.update(editor, |e, _| e.session_mut().document.select(second, true))
        .unwrap();
    cx.update(editor, |e, _| e.copy_drag_to_tab(&drag, 1).unwrap())
        .unwrap();
    cx.read(editor, |e| {
        assert_eq!(e.tabs[0].session().unwrap().document.layers.len(), 2);
        assert_eq!(e.tabs[1].session().unwrap().document.layers.len(), 3);
        assert_eq!(e.tabs[1].session().unwrap().document.selected.len(), 2);
        assert!(
            !e.tabs[1]
                .session()
                .unwrap()
                .document
                .selected
                .contains(&first)
        );
        assert!(
            !e.tabs[1]
                .session()
                .unwrap()
                .document
                .selected
                .contains(&second)
        );
    })
    .unwrap();
}

#[test]
fn relative_layer_drops_reorder_siblings_and_undo_restores_order() {
    use super::layer_drag::{DropPlacement, LayerDrag, Transfer};
    let mut view = Editor::with_test_document();
    let mut doc = Document::new(4, 4).unwrap();
    let first = doc.layers[0].id;
    doc.add(compositor::document::Layer::blank("Middle", 4, 4))
        .unwrap();
    let middle = doc.active.unwrap();
    doc.add(compositor::document::Layer::blank("Top", 4, 4))
        .unwrap();
    let top = doc.active.unwrap();
    view.tabs = vec![Session::new(doc.clone(), None).into()];
    let drag = LayerDrag {
        session: view.session().id,
        layer: first,
        operation: Transfer::Move,
    };
    let (mut cx, editor) = Application::new()
        .into_test_context(WindowOptions::new("Layer drops").size(1280., 900.), view)
        .unwrap();
    let above = cx
        .element_bounds(editor.window_handle(), format!("layer-drop-above-{top}"))
        .unwrap();
    let below = cx
        .element_bounds(editor.window_handle(), format!("layer-drop-below-{top}"))
        .unwrap();
    assert_eq!(below.y - above.y, 44.);
    let row = cx.element_bounds(editor.window_handle(), 1007_u64).unwrap();
    assert_eq!(above.y, row.y);
    assert_eq!(below.y + below.height, row.y + row.height);

    cx.update(editor, |e, cx| {
        e.drop_layer(&drag, DropPlacement::Below(top), cx)
    })
    .unwrap();
    assert_eq!(
        cx.read(editor, |e| e
            .session()
            .document
            .layers
            .iter()
            .map(|l| l.id)
            .collect::<Vec<_>>())
            .unwrap(),
        vec![middle, first, top]
    );
    cx.update(editor, |e, cx| {
        e.drop_layer(&drag, DropPlacement::Above(top), cx)
    })
    .unwrap();
    assert_eq!(
        cx.read(editor, |e| e
            .session()
            .document
            .layers
            .iter()
            .map(|l| l.id)
            .collect::<Vec<_>>())
            .unwrap(),
        vec![middle, top, first]
    );
    cx.update(editor, |e, _| {
        e.session_mut().undo();
        e.session_mut().undo();
    })
    .unwrap();
    assert_eq!(
        cx.read(editor, |e| e.session().document.clone()).unwrap(),
        doc
    );
}

#[test]
fn hidden_transform_handles_do_not_capture_resize_drags() {
    let mut view = Editor::with_test_document();
    view.tabs = vec![Session::new(Document::new(100, 80).unwrap(), None).into()];
    view.session_mut().document.layers[0].content =
        compositor::document::LayerContent::Raster(Some(std::sync::Arc::new(
            image::RgbaImage::from_pixel(100, 80, image::Rgba([255; 4])),
        )));
    let (mut cx, editor) = Application::new()
        .into_test_context(WindowOptions::new("Hidden handles").size(1280., 900.), view)
        .unwrap();
    let window = editor.window_handle();
    cx.click(window, "transform-show-controls").unwrap();
    assert!(
        !cx.read(editor, |e| e.tools.show_transform_controls)
            .unwrap()
    );
    let bounds = cx.element_bounds(window, "canvas").unwrap();
    let (zoom, offset) = cx
        .update(editor, |e, _| e.viewport(bounds.width, bounds.height))
        .unwrap();
    let point = |x, y| {
        quickgui::Point::new(
            bounds.x + (offset[0] + x * zoom) as f32,
            bounds.y + (offset[1] + y * zoom) as f32,
        )
    };
    cx.simulate_pointer_drag(window, "canvas", point(0., 0.), point(30., 20.))
        .unwrap();
    let transform = cx
        .read(editor, |e| e.session().document.layers[0].transform)
        .unwrap();
    assert_eq!(transform.size, [100., 80.]);
    assert_eq!(transform.origin, [30., 20.]);
    cx.simulate_keystroke(
        window,
        quickgui::Keystroke::new(Key::Character("h".into()), Modifiers::CONTROL),
    )
    .unwrap();
    assert!(
        cx.read(editor, |e| e.tools.show_transform_controls)
            .unwrap()
    );
    cx.click(window, "transform-auto-select").unwrap();
    assert!(cx.read(editor, |e| e.tools.transform_auto_select).unwrap());
}

#[test]
fn brush_cursor_motion_keeps_the_cached_image_and_clone_source_tracks_alignment() {
    let mut view = Editor::with_test_document();
    view.tabs = vec![Session::new(Document::new(10, 10).unwrap(), None).into()];
    view.tools.tool = Tool::Clone;
    view.tools.clone_source = Some([2., 3.]);
    view.tools.clone_offset = Some([-4., 2.]);
    view.tools.clone_aligned = true;
    assert_eq!(view.clone_cursor_source([10., 12.]), Some([6., 14.]));
    view.tools.clone_aligned = false;
    assert_eq!(view.clone_cursor_source([10., 12.]), Some([2., 3.]));
    let (mut cx, editor) = Application::new()
        .into_test_context(WindowOptions::new("Brush cursor").size(1280., 850.), view)
        .unwrap();
    let window = editor.window_handle();
    let bounds = cx.element_bounds(window, "canvas").unwrap();
    let revision = cx.read(editor, |e| e.revision).unwrap();
    cx.simulate_mouse_move(
        window,
        "canvas",
        quickgui::MouseMoveEvent {
            position: quickgui::Point::new(bounds.x + 35., bounds.y + 50.),
            pressed_button: None,
            modifiers: Modifiers::empty(),
        },
    )
    .unwrap();
    assert_eq!(
        cx.read(editor, |e| e.canvas_pointer).unwrap(),
        Some([35., 50.])
    );
    assert_eq!(cx.read(editor, |e| e.revision).unwrap(), revision);
}

#[test]
fn floating_handles_keep_original_pixels_and_commit_as_one_edit() {
    let mut view = Editor::with_test_document();
    let mut doc = Document::new(100, 80).unwrap();
    doc.selection = Some(compositor::selection::Selection::rectangle(
        100,
        80,
        [20., 20.],
        [60., 60.],
        false,
    ));
    compositor::edits::fill(&mut doc, [80, 120, 200, 128], false, false).unwrap();
    view.tabs = vec![Session::new(doc.clone(), None).into()];
    let (mut cx, editor) = Application::new()
        .into_test_context(
            WindowOptions::new("Floating pixels").size(1280., 900.),
            view,
        )
        .unwrap();
    let window = editor.window_handle();
    cx.focus(window, "workspace").unwrap();
    cx.simulate_keystroke(
        window,
        quickgui::Keystroke::new(Key::Character("t".into()), Modifiers::CONTROL),
    )
    .unwrap();
    assert!(
        cx.read(editor, |e| e.pending_pixels.is_some() && e.modal.is_none())
            .unwrap()
    );
    let bounds = cx.element_bounds(window, "canvas").unwrap();
    let (zoom, offset) = cx
        .update(editor, |e, _| e.viewport(bounds.width, bounds.height))
        .unwrap();
    let p = |x, y| {
        quickgui::Point::new(
            bounds.x + (offset[0] + x * zoom) as f32,
            bounds.y + (offset[1] + y * zoom) as f32,
        )
    };
    cx.simulate_pointer_drag(window, "canvas", p(60., 60.), p(80., 70.))
        .unwrap();
    assert!(
        cx.read(editor, |e| e.pending_pixels.is_some()
            && e.session().undo_label().is_none())
            .unwrap()
    );
    assert_ne!(
        cx.read(editor, |e| e.session().document.clone()).unwrap(),
        doc
    );
    cx.click(window, "transform-cancel").unwrap();
    assert_eq!(
        cx.read(editor, |e| e.session().document.clone()).unwrap(),
        doc
    );
    cx.focus(window, "workspace").unwrap();
    cx.simulate_keystroke(
        window,
        quickgui::Keystroke::new(Key::Character("t".into()), Modifiers::CONTROL),
    )
    .unwrap();
    cx.update(editor, |e, _| {
        use super::floating::Placement;
        let original = e.pending_pixels.as_ref().unwrap().placement;
        e.preview_pixels(Placement::Perspective([
            [10., 10.],
            [70., 20.],
            [60., 70.],
            [20., 60.],
        ]))
        .unwrap();
        e.preview_pixels(original).unwrap();
        assert_eq!(e.session().document, doc);
    })
    .unwrap();
    cx.simulate_pointer_drag(window, "canvas", p(40., 40.), p(50., 50.))
        .unwrap();
    cx.click(window, "transform-apply").unwrap();
    assert_eq!(
        cx.read(editor, |e| e.session().undo_label().map(str::to_owned))
            .unwrap()
            .as_deref(),
        Some("Transform Selection")
    );
    cx.focus(window, "workspace").unwrap();
    cx.simulate_keystroke(
        window,
        quickgui::Keystroke::new(Key::Character("z".into()), Modifiers::CONTROL),
    )
    .unwrap();
    assert_eq!(
        cx.read(editor, |e| e.session().document.clone()).unwrap(),
        doc
    );
}

#[test]
fn inline_rename_commits_on_enter_and_escape_restores_the_name() {
    use quickgui::Keystroke;
    let mut view = Editor::with_test_document();
    view.tabs = vec![Session::new(Document::new(2, 2).unwrap(), None).into()];
    compositor::edits::add_mask(&mut view.session_mut().document, false).unwrap();
    let layer = view.session().document.layers[0].id;
    let (mut cx, editor) = Application::new()
        .into_test_context(WindowOptions::new("Rename").size(1280., 850.), view)
        .unwrap();
    let window = editor.window_handle();
    let image_id = format!("layer-thumbnail-frame-{layer}-false");
    let mask_id = format!("layer-thumbnail-frame-{layer}-true");
    let image_bounds = cx.element_bounds(window, image_id.as_str()).unwrap();
    let mask_bounds = cx.element_bounds(window, mask_id.as_str()).unwrap();
    cx.click(
        window,
        quickgui::Menubar::new("application-menu").item_id(6),
    )
    .unwrap();
    let rename = cx
        .read(editor, |e| e.menus.command_id("Rename Layer…"))
        .unwrap();
    cx.click(window, rename).unwrap();
    assert!(cx.read(editor, |e| e.modal.is_none()).unwrap());
    assert_eq!(
        cx.element_bounds(window, image_id.as_str()).unwrap(),
        image_bounds
    );
    assert_eq!(
        cx.element_bounds(window, mask_id.as_str()).unwrap(),
        mask_bounds
    );
    assert!(
        cx.element_bounds(window, format!("layer-detail-{layer}"))
            .is_ok()
    );
    let input = cx.element_bounds(window, "layer-rename").unwrap();
    assert!(input.x >= mask_bounds.x + mask_bounds.width + 5.);
    cx.simulate_keystroke(
        window,
        Keystroke::new(Key::Character("a".into()), Modifiers::CONTROL),
    )
    .unwrap();
    cx.simulate_keystrokes(window, "f o r e g r o u n d")
        .unwrap();
    cx.simulate_keystroke(window, Keystroke::new(Key::Enter, Modifiers::empty()))
        .unwrap();
    cx.read(editor, |e| {
        assert!(e.rename.is_none());
        assert_eq!(e.status, e.tool_hint());
    })
    .unwrap();
    assert_eq!(
        cx.read(editor, |e| e.session().document.layers[0].name.clone())
            .unwrap(),
        "foreground"
    );
    cx.click(
        window,
        quickgui::Menubar::new("application-menu").item_id(6),
    )
    .unwrap();
    let rename = cx
        .read(editor, |e| e.menus.command_id("Rename Layer…"))
        .unwrap();
    cx.click(window, rename).unwrap();
    cx.simulate_keystrokes(window, "d i s c a r d").unwrap();
    cx.simulate_keystroke(window, Keystroke::new(Key::Escape, Modifiers::empty()))
        .unwrap();
    cx.read(editor, |e| {
        assert!(e.rename.is_none());
        assert_eq!(e.status, e.tool_hint());
    })
    .unwrap();
    assert_eq!(
        cx.read(editor, |e| e.session().document.layers[0].name.clone())
            .unwrap(),
        "foreground"
    );
    cx.focus(window, "workspace").unwrap();
    cx.simulate_keystroke(
        window,
        quickgui::Keystroke::new(Key::Character("z".into()), Modifiers::CONTROL),
    )
    .unwrap();
    assert_eq!(
        cx.read(editor, |e| e.session().document.layers[0].name.clone())
            .unwrap(),
        "Layer 1"
    );
}

#[test]
fn crop_frame_stays_editable_until_apply_and_cancel_leaves_no_history() {
    let mut view = Editor::with_test_document();
    view.tabs = vec![Session::new(Document::new(100, 80).unwrap(), None).into()];
    view.tools.tool = Tool::Crop;
    let original = view.session().document.clone();
    let (mut cx, editor) = Application::new()
        .into_test_context(WindowOptions::new("Crop").size(1280., 900.), view)
        .unwrap();
    let window = editor.window_handle();
    let bounds = cx.element_bounds(window, "canvas").unwrap();
    let (zoom, offset) = cx
        .update(editor, |e, _| e.viewport(bounds.width, bounds.height))
        .unwrap();
    let p = |x, y| {
        quickgui::Point::new(
            bounds.x + (offset[0] + x * zoom) as f32,
            bounds.y + (offset[1] + y * zoom) as f32,
        )
    };
    cx.simulate_pointer_drag(window, "canvas", p(20., 20.), p(80., 60.))
        .unwrap();
    assert_eq!(
        cx.read(editor, |e| e.session().document.clone()).unwrap(),
        original
    );
    assert_eq!(
        cx.read(editor, |e| e
            .tools
            .pending_crop
            .as_ref()
            .unwrap()
            .frame
            .size)
            .unwrap(),
        [60., 40.]
    );
    cx.simulate_pointer_drag(window, "canvas", p(20., 20.), p(10., 10.))
        .unwrap();
    assert_eq!(
        cx.read(editor, |e| e
            .tools
            .pending_crop
            .as_ref()
            .unwrap()
            .frame
            .size)
            .unwrap(),
        [70., 50.]
    );
    cx.click(window, "crop-cancel").unwrap();
    assert!(
        cx.read(editor, |e| e.session().undo_label().is_none())
            .unwrap()
    );
    cx.simulate_pointer_drag(window, "canvas", p(20., 20.), p(80., 60.))
        .unwrap();
    cx.click(window, "crop-apply").unwrap();
    assert_eq!(
        cx.read(editor, |e| (
            e.session().document.width,
            e.session().document.height
        ))
        .unwrap(),
        (60, 40)
    );
    cx.focus(window, "workspace").unwrap();
    cx.simulate_keystroke(
        window,
        quickgui::Keystroke::new(Key::Character("z".into()), Modifiers::CONTROL),
    )
    .unwrap();
    assert_eq!(
        cx.read(editor, |e| e.session().document.clone()).unwrap(),
        original
    );
}

#[test]
fn pixel_adjustment_preview_is_replaceable_and_apply_works_with_preview_off() {
    let mut view = Editor::with_test_document();
    let mut doc = Document::new(2, 1).unwrap();
    compositor::edits::fill(&mut doc, [80, 80, 80, 128], false, false).unwrap();
    doc.selection = Some(compositor::selection::Selection::rectangle(
        2,
        1,
        [0., 0.],
        [1., 1.],
        false,
    ));
    view.tabs = vec![Session::new(doc.clone(), None).into()];
    view.open_pixel_adjustment(Kind::Exposure).unwrap();
    if let Some(Form::Edit { fields, .. }) = &mut view.modal {
        fields[0].1 = "1".into();
    }
    view.preview_adjustment().unwrap();
    let first = view.session().document.clone();
    view.preview_adjustment().unwrap();
    assert_eq!(view.session().document, first);
    let pixels = first.layers[0].raster().unwrap();
    assert!(pixels[(0, 0)][0] > 80);
    assert_eq!(pixels[(0, 0)][3], 128);
    assert_eq!(pixels[(1, 0)], image::Rgba([80, 80, 80, 128]));
    let (mut cx, editor) = Application::new()
        .into_test_context(
            WindowOptions::new("Pixel adjustment").size(1280., 850.),
            view,
        )
        .unwrap();
    let window = editor.window_handle();
    cx.click(window, "adjustment-preview").unwrap();
    assert_eq!(
        cx.read(editor, |e| e.session().document.clone()).unwrap(),
        doc
    );
    cx.click(window, "form-apply").unwrap();
    assert_eq!(
        cx.read(editor, |e| e.session().document.clone()).unwrap(),
        first
    );
    cx.focus(window, "workspace").unwrap();
    cx.simulate_keystroke(
        window,
        quickgui::Keystroke::new(Key::Character("z".into()), Modifiers::CONTROL),
    )
    .unwrap();
    assert_eq!(
        cx.read(editor, |e| e.session().document.clone()).unwrap(),
        doc
    );
}

#[test]
fn curve_graph_adds_drags_removes_and_resets_points() {
    let mut view = Editor::with_test_document();
    view.tabs = vec![Session::new(Document::new(2, 2).unwrap(), None).into()];
    view.open_adjustment(Some(Kind::Curves)).unwrap();
    let (mut cx, editor) = Application::new()
        .into_test_context(WindowOptions::new("Curves").size(1280., 1000.), view)
        .unwrap();
    let window = editor.window_handle();
    let bounds = cx.element_bounds(window, "curve-graph").unwrap();
    let position = |x: f32, y: f32| {
        quickgui::Point::new(bounds.x + x * bounds.width, bounds.y + y * bounds.height)
    };
    let from = position(0.5, 0.5);
    let to = position(0.6, 0.2);
    let base = quickgui::PointerEvent {
        size: quickgui::Size::ZERO,
        phase: quickgui::PointerPhase::Down,
        position: from,
        origin: from,
        local_position: from,
        local_origin: from,
        delta: quickgui::Vector::ZERO,
        button: quickgui::MouseButton::Left,
        modifiers: Modifiers::empty(),
    };
    cx.simulate_pointer(window, "curve-graph", base).unwrap();
    assert_eq!(
        cx.element_bounds(window, "curve-graph").unwrap(),
        bounds,
        "graph moved on pointer down"
    );
    cx.simulate_pointer(
        window,
        "curve-graph",
        quickgui::PointerEvent {
            phase: quickgui::PointerPhase::Move,
            position: to,
            local_position: to,
            delta: to - from,
            ..base
        },
    )
    .unwrap();
    assert_eq!(
        cx.element_bounds(window, "curve-graph").unwrap(),
        bounds,
        "graph moved during drag"
    );
    cx.simulate_pointer(
        window,
        "curve-graph",
        quickgui::PointerEvent {
            phase: quickgui::PointerPhase::Up,
            position: to,
            local_position: to,
            ..base
        },
    )
    .unwrap();
    let points = cx
        .read(editor, |e| {
            e.adjustment_edit.as_ref().unwrap().settings.curves.channels[0].clone()
        })
        .unwrap();
    assert_eq!(points.len(), 3);
    assert!((points[1].x - 153.).abs() < 0.01);
    assert!(
        (points[1].y - 204.).abs() < 0.01,
        "points: {points:?}; before: {bounds:?}; after: {:?}",
        cx.element_bounds(window, "curve-graph").unwrap()
    );
    cx.click(window, "curve-remove").unwrap();
    assert_eq!(
        cx.read(editor, |e| e
            .adjustment_edit
            .as_ref()
            .unwrap()
            .settings
            .curves
            .channels[0]
            .len())
            .unwrap(),
        2
    );
    cx.simulate_pointer_drag(window, "curve-graph", position(0., 1.), position(0.5, 0.8))
        .unwrap();
    assert_eq!(
        cx.read(editor, |e| e
            .adjustment_edit
            .as_ref()
            .unwrap()
            .settings
            .curves
            .channels[0][0]
            .x)
            .unwrap(),
        0.
    );
    cx.click(window, "curve-reset").unwrap();
    assert_eq!(
        cx.read(editor, |e| e
            .adjustment_edit
            .as_ref()
            .unwrap()
            .settings
            .curves
            .channels[0]
            .clone())
            .unwrap(),
        compositor::adjustment::Curves::default().channels[0]
    );
}

#[test]
fn layer_transfer_between_tabs_of_the_same_project_copies_the_requested_tab() {
    let mut view = Editor::with_test_document();
    let doc = Document::new(2, 2).unwrap();
    view.tabs = vec![
        Session::new(doc.clone(), None).into(),
        Session::new(doc, None).into(),
    ];
    view.tabs[0].session_mut().unwrap().document.layers[0].name = "First tab".into();
    view.tabs[1].session_mut().unwrap().document.layers[0].name = "Second tab".into();
    let drag = layer_drag::LayerDrag {
        session: view.tabs[1].id,
        layer: view.tabs[1].session().unwrap().document.layers[0].id,
        operation: layer_drag::Transfer::Move,
    };
    view.copy_drag_to_tab(&drag, 0).unwrap();
    assert_eq!(view.tabs[0].session().unwrap().document.layers.len(), 2);
    assert_eq!(
        view.tabs[0].session().unwrap().document.layers[1].name,
        "Second tab"
    );
    assert_eq!(view.tabs[1].session().unwrap().document.layers.len(), 1);
    view.tabs[0].session_mut().unwrap().undo();
    assert_eq!(view.tabs[0].session().unwrap().document.layers.len(), 1);
    view.copy_drag_to_tab(&drag, 1).unwrap();
    assert_eq!(view.tabs[1].session().unwrap().document.layers.len(), 1);
    let duplicate = layer_drag::LayerDrag {
        operation: layer_drag::Transfer::Copy,
        ..drag
    };
    view.copy_drag_to_tab(&duplicate, 1).unwrap();
    assert_eq!(view.tabs[1].session().unwrap().document.layers.len(), 2);
    view.tabs[1].session().unwrap().document.validate().unwrap();
}

#[test]
fn parameter_controls_mount_and_cancel_without_changing_the_document() {
    let mut view = Editor::with_test_document();
    view.session_mut().document.layers[0].content =
        compositor::document::LayerContent::Raster(Some(std::sync::Arc::new(
            image::RgbaImage::from_pixel(1, 1, image::Rgba([255; 4])),
        )));
    let (mut cx, editor) = Application::new()
        .into_test_context(
            WindowOptions::new("Compositor test").size(1280., 850.),
            view,
        )
        .unwrap();
    let window = editor.window_handle();
    let original = cx.read(editor, |e| e.session().document.clone()).unwrap();
    for (tool, id) in [(Tool::Brush, 400_u64), (Tool::Move, 402), (Tool::Move, 320)] {
        cx.update(editor, |e, cx| e.select_tool(tool, cx)).unwrap();
        if tool == Tool::Brush {
            assert!(cx.element_bounds(window, "brush-size").is_ok());
            assert!(cx.element_bounds(window, "brush-hardness").is_ok());
            assert!(cx.element_bounds(window, "brush-opacity").is_ok());
            cx.click(window, "header-foreground").unwrap();
        } else if id == 402 {
            // The source transform command starts the inline inspector.
            cx.focus(window, "workspace").unwrap();
            cx.simulate_keystroke(
                window,
                quickgui::Keystroke::new(Key::Character("t".into()), Modifiers::CONTROL),
            )
            .unwrap();
        } else {
            cx.click(window, id).unwrap();
        }
        if id == 402 {
            assert!(
                cx.read(editor, |e| e.modal.is_none() && e.transform_edit.is_some())
                    .unwrap()
            );
            cx.click(window, "transform-cancel").unwrap();
        } else {
            assert!(cx.read(editor, |e| e.modal.is_some()).unwrap());
            cx.click(window, "form-cancel").unwrap();
        }
        assert_eq!(
            cx.read(editor, |e| e.session().document.clone()).unwrap(),
            original
        );
    }
}

#[test]
fn creating_a_tab_preserves_the_previous_project_and_close_can_be_cancelled() {
    let (mut cx, editor) = Application::new()
        .into_test_context(
            WindowOptions::new("Compositor test").size(1280., 850.),
            Editor::with_test_document(),
        )
        .unwrap();
    let window = editor.window_handle();
    let original = cx.read(editor, |e| e.session().document.id).unwrap();
    cx.click(window, 100_u64).unwrap();
    assert!(
        cx.read(editor, |e| e.modal.is_none() && !e.has_document())
            .unwrap()
    );
    cx.click(window, "welcome-create").unwrap();
    assert_eq!(cx.read(editor, |e| e.tabs.len()).unwrap(), 2);
    assert_eq!(
        cx.read(editor, |e| e.tabs[0].session().unwrap().document.id)
            .unwrap(),
        original
    );
    let current = cx.read(editor, |e| e.tabs[e.current].id).unwrap();
    cx.click(window, format!("close-tab-{current}")).unwrap();
    assert!(
        cx.read(editor, |e| matches!(e.modal, Some(Form::Close)))
            .unwrap()
    );
    cx.click(window, "form-cancel").unwrap();
    assert_eq!(cx.read(editor, |e| e.tabs.len()).unwrap(), 2);
}

#[test]
fn pending_gradient_settings_replace_preview_and_commit_as_one_undo_step() {
    let mut view = Editor::with_test_document();
    view.tabs = vec![Session::new(Document::new(5, 1).unwrap(), None).into()];
    view.tools.tool = Tool::Gradient;
    let original = view.session().document.clone();
    view.begin_gradient([0.5, 0.5]).unwrap();
    view.pending_gradient.as_mut().unwrap().end = [4.5, 0.5];
    view.refresh_gradient().unwrap();
    let (mut cx, editor) = Application::new()
        .into_test_context(WindowOptions::new("Gradient test").size(1280., 850.), view)
        .unwrap();
    let window = editor.window_handle();
    cx.click(window, "gradient-reverse").unwrap();
    assert_eq!(
        cx.read(editor, |e| e.session().document.layers[0].raster().unwrap()
            [(0, 0)][3])
            .unwrap(),
        0
    );
    cx.click(window, "gradient-style").unwrap();
    cx.simulate_keystrokes(window, "home enter").unwrap();
    assert_eq!(
        cx.read(editor, |e| e.session().document.layers[0].raster().unwrap()
            [(0, 0)][3])
            .unwrap(),
        255
    );
    assert!(
        cx.read(editor, |e| e.session().undo_label().is_none())
            .unwrap()
    );
    cx.click(window, "gradient-apply").unwrap();
    assert!(cx.read(editor, |e| e.pending_gradient.is_none()).unwrap());
    cx.focus(window, "workspace").unwrap();
    cx.simulate_keystroke(
        window,
        quickgui::Keystroke::new(Key::Character("z".into()), Modifiers::CONTROL),
    )
    .unwrap();
    assert_eq!(
        cx.read(editor, |e| e.session().document.clone()).unwrap(),
        original
    );
}

#[test]
fn cancel_gradient_restores_original_and_repeated_drags_replace_preview() {
    let mut view = Editor::with_test_document();
    view.tabs = vec![Session::new(Document::new(5, 1).unwrap(), None).into()];
    view.tools.tool = Tool::Gradient;
    let original = view.session().document.clone();
    for _ in 0..2 {
        view.begin_gradient([0.5, 0.5]).unwrap();
        view.pending_gradient.as_mut().unwrap().end = [4.5, 0.5];
        view.refresh_gradient().unwrap();
        assert_eq!(
            view.session().document.layers[0].raster().unwrap()[(2, 0)][3],
            128
        );
    }
    let (mut cx, editor) = Application::new()
        .into_test_context(WindowOptions::new("Gradient test").size(1280., 850.), view)
        .unwrap();
    cx.click(editor.window_handle(), "gradient-cancel").unwrap();
    assert_eq!(
        cx.read(editor, |e| e.session().document.clone()).unwrap(),
        original
    );
    assert!(
        cx.read(editor, |e| e.session().undo_label().is_none())
            .unwrap()
    );
}

#[test]
fn adjustment_dialog_cancel_keeps_the_created_layer_and_undo_removes_it() {
    use quickgui::Keystroke;
    let mut view = Editor::with_test_document();
    let mut doc = Document::new(2, 2).unwrap();
    compositor::edits::fill(&mut doc, [100, 100, 100, 255], false, false).unwrap();
    view.tabs = vec![Session::new(doc, None).into()];
    let original = view.session().document.clone();
    view.open_adjustment(Some(Kind::Exposure)).unwrap();
    let created = view.session().document.clone();
    let (mut cx, editor) = Application::new()
        .into_test_context(
            WindowOptions::new("Adjustment test").size(1280., 850.),
            view,
        )
        .unwrap();
    let window = editor.window_handle();
    cx.focus(window, 50_000_u64).unwrap();
    cx.simulate_keystroke(
        window,
        Keystroke::new(Key::Character("a".into()), Modifiers::CONTROL),
    )
    .unwrap();
    cx.simulate_input(window, "1").unwrap();
    assert!(
        cx.read(editor, |e| compositor::render::render(
            &e.session().document,
            2,
            2
        )[(0, 0)][0]
            > 100)
            .unwrap()
    );
    cx.click(window, "form-cancel").unwrap();
    assert_eq!(
        cx.read(editor, |e| e.session().document.clone()).unwrap(),
        created
    );
    assert_eq!(
        cx.read(editor, |e| e.session().undo_label().map(str::to_owned))
            .unwrap(),
        Some("New Exposure Adjustment".into())
    );
    cx.simulate_keystrokes(window, "ctrl-z").unwrap();
    assert_eq!(
        cx.read(editor, |e| e.session().document.clone()).unwrap(),
        original
    );
}

#[test]
fn editing_existing_adjustment_preserves_other_channels_and_undo_restores_settings() {
    use compositor::{
        adjustment::{Adjustment, Channel},
        document::{Layer, LayerContent},
    };
    let mut view = Editor::with_test_document();
    view.tabs = vec![Session::new(Document::new(2, 2).unwrap(), None).into()];
    let mut settings = Adjustment::new(Kind::Levels);
    settings.levels.channel = Channel::Red;
    settings.levels.ranges[3].gamma = 1.7;
    let mut layer = Layer::blank("Custom levels", 2, 2);
    layer.content = LayerContent::Adjustment(Box::new(settings));
    layer.opacity = 0.6;
    view.session_mut().document.add(layer).unwrap();
    let original = view.session().document.clone();
    view.open_adjustment(None).unwrap();
    if let Some(Form::Edit { fields, .. }) = &mut view.modal {
        fields[0].1 = "20".into();
    }
    view.preview_adjustment().unwrap();
    view.finish_adjustment().unwrap();
    let layer = view.session().document.active_layer().unwrap();
    let LayerContent::Adjustment(settings) = &layer.content else {
        panic!("Expected an adjustment layer");
    };
    assert_eq!(settings.levels.ranges[1].black, 20.);
    assert_eq!(settings.levels.ranges[3].gamma, 1.7);
    assert_eq!(layer.opacity, 0.6);
    view.session_mut().undo();
    assert_eq!(view.session().document, original);
}

#[test]
fn folder_controls_collapse_and_expand_without_changing_project_pixels() {
    let mut view = Editor::with_test_document();
    view.tabs = vec![Session::new(Document::new(2, 2).unwrap(), None).into()];
    let (mut cx, editor) = Application::new()
        .into_test_context(WindowOptions::new("Folder test").size(1280., 850.), view)
        .unwrap();
    let window = editor.window_handle();
    cx.click(window, 511_u64).unwrap();
    let group = cx
        .read(editor, |e| e.session().document.active.unwrap())
        .unwrap();
    let original = cx.read(editor, |e| e.session().document.clone()).unwrap();
    cx.click(window, 1002_u64).unwrap();
    assert!(
        cx.read(editor, |e| e.session().collapsed.contains(&group))
            .unwrap()
    );
    cx.click(window, 1002_u64).unwrap();
    assert!(
        !cx.read(editor, |e| e.session().collapsed.contains(&group))
            .unwrap()
    );
    assert_eq!(
        cx.read(editor, |e| e.session().document.clone()).unwrap(),
        original
    );
}
