use super::*;
use quickgui::{Application, WindowOptions};

#[test]
fn floating_adjustment_panels_block_selection_shape_crop_and_transform_pointer_edits() {
    for panel in ["hue", "levels", "filter"] {
        for tool in [
            Tool::Move,
            Tool::Rectangle,
            Tool::Ellipse,
            Tool::Lasso,
            Tool::Polygon,
            Tool::Wand,
            Tool::Shape,
            Tool::Crop,
        ] {
            let mut editor = Editor::with_test_document();
            let mut doc = Document::new(100, 100).unwrap();
            compositor::edits::fill(&mut doc, [80, 120, 200, 255], false, false).unwrap();
            editor.tabs = vec![Session::new(doc, None).into()];
            editor.tools.tool = tool;
            match panel {
                "hue" => editor.open_pixel_adjustment(Kind::HueSaturation).unwrap(),
                "levels" => editor.open_pixel_adjustment(Kind::Levels).unwrap(),
                _ => editor
                    .open_filter(compositor::filters::Filter::Gaussian { radius: 1. })
                    .unwrap(),
            }
            let (mut cx, view) = Application::new()
                .into_test_context(
                    WindowOptions::new("Panel pointer eligibility").size(1500., 900.),
                    editor,
                )
                .unwrap();
            let window = view.window_handle();
            let original = cx
                .read(view, |e| {
                    assert!(!e.can_edit_layers());
                    e.session().document.clone()
                })
                .unwrap();
            let bounds = cx.element_bounds(window, "canvas").unwrap();
            let start = quickgui::Point::new(bounds.x + 40., bounds.y + bounds.height - 80.);
            let end = quickgui::Point::new(start.x + 40., start.y + 30.);
            cx.simulate_pointer_drag(window, "canvas", start, end)
                .unwrap();
            cx.read(view, |e| {
                assert_eq!(e.session().document, original, "{panel}, {tool:?}");
                assert!(e.gesture.is_none());
                assert!(e.tools.polygon.is_none());
                assert!(e.tools.pending_crop.is_none());
                assert!(e.transform_edit.is_none());
                assert!(e.session().undo_label().is_none());
            })
            .unwrap();
        }
    }
}

#[test]
fn delete_removes_the_targeted_mask_and_preserves_clipping_layers_with_undo() {
    let mut e = Editor::with_test_document();
    let base = e.session().document.layers[0].id;
    compositor::edits::add_mask(&mut e.session_mut().document, false).unwrap();
    let mut clipped = compositor::document::Layer::blank("Clipped", 4, 4);
    clipped.clip_source = Some(base);
    e.session_mut().document.add(clipped).unwrap();
    e.session_mut().document.select(base, false);
    e.tools.mask_target = true;
    let original = e.session().document.clone();
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Delete mask").size(1280., 850.), e)
        .unwrap();
    cx.update(view, |e, cx| e.action(Action::DeleteLayer, cx))
        .unwrap();
    cx.read(view, |e| {
        assert_eq!(e.session().document.layers.len(), 2);
        assert!(e.session().document.layers[0].mask.is_none());
        assert_eq!(e.session().document.layers[1].clip_source, Some(base));
        assert!(!e.tools.mask_target);
        assert!(e.modal.is_none());
    })
    .unwrap();
    cx.update(view, |e, cx| e.action(Action::Undo, cx)).unwrap();
    assert_eq!(
        cx.read(view, |e| e.session().document.clone()).unwrap(),
        original
    );
    cx.update(view, |e, cx| {
        e.tools.mask_target = true;
        let id = e.session().document.layers[1].id;
        e.session_mut().document.select(id, true);
        e.action(Action::DeleteLayer, cx);
    })
    .unwrap();
    assert!(
        cx.read(view, |e| e.session().document.layers.is_empty())
            .unwrap()
    );
}

#[test]
fn pixel_grid_defaults_on_at_eight_hundred_percent_and_toggling_preserves_the_document() {
    let mut e = Editor::with_test_document();
    e.session_mut().fit = false;
    e.session_mut().zoom = 7.99;
    let original = e.session().document.clone();
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Pixel grid").size(1280., 850.), e)
        .unwrap();
    let window = view.window_handle();
    assert!(!cx.contains_element(window, "pixel-grid").unwrap());
    cx.update(view, |e, cx| {
        e.session_mut().zoom = 8.;
        cx.invalidate();
    })
    .unwrap();
    assert!(cx.contains_element(window, "pixel-grid").unwrap());
    cx.update(view, |e, cx| e.action(Action::PixelGrid, cx))
        .unwrap();
    assert!(!cx.contains_element(window, "pixel-grid").unwrap());
    cx.update(view, |e, cx| e.action(Action::PixelGrid, cx))
        .unwrap();
    assert!(cx.contains_element(window, "pixel-grid").unwrap());
    cx.read(view, |e| {
        assert_eq!(e.session().document, original);
        assert!(e.session().undo_label().is_none());
    })
    .unwrap();
}

#[test]
fn selection_commands_preserve_pending_edits_and_view_commands_keep_them_editable() {
    use compositor::{geometry::Transform, selection::Selection};
    for draft in 0..4 {
        let mut e = Editor::with_test_document();
        let mut doc = Document::new(20, 20).unwrap();
        compositor::edits::fill(&mut doc, [50, 120, 200, 255], false, false).unwrap();
        doc.selection = Some(Selection::rectangle(20, 20, [2., 2.], [10., 10.], false));
        e.tabs = vec![Session::new(doc, None).into()];
        e.tools.tool = Tool::Rectangle;
        match draft {
            0 => e.start_toolbar_transform().unwrap(),
            1 => e.begin_gradient([0., 0.]).unwrap(),
            2 => {
                e.tools.pending_crop = Some(crop::CropPreview {
                    frame: Transform::new(10, 10),
                    guides: [None; 2],
                })
            }
            _ => e.begin_pixel_transform().unwrap(),
        }
        let before = e.session().document.clone();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Pending selection and view commands").size(1500., 900.),
                e,
            )
            .unwrap();
        if matches!(draft, 1 | 2) {
            for id in [
                quickgui::ElementId::from("header-deselect"),
                413_u64.into(),
                414_u64.into(),
            ] {
                assert!(
                    matches!(
                        cx.click(view.window_handle(), id),
                        Err(quickgui::TestAppError::NotClickable { .. })
                    ),
                    "{id:?} enabled during draft {draft}"
                );
            }
        }
        cx.update(view, |e, cx| {
            for action in [
                Action::SelectAll,
                Action::Deselect,
                Action::InvertSelection,
                Action::LoadAlpha,
                Action::LoadMask,
            ] {
                e.action(action, cx);
                assert_eq!(e.session().document, before);
            }
            assert!(!e.can_modify_selection());
            assert!(e.resize_selection(true, 1).is_err());
            for action in [
                Action::Fit,
                Action::Actual,
                Action::ZoomIn,
                Action::ZoomOut,
                Action::PixelGrid,
            ] {
                e.action(action, cx);
                assert_eq!(e.session().document, before);
                assert!(e.session().undo_label().is_none());
                assert!(
                    match draft {
                        0 => e.transform_edit.is_some(),
                        1 => e.pending_gradient.is_some(),
                        2 => e.tools.pending_crop.is_some(),
                        _ => e.pending_pixels.is_some(),
                    },
                    "View command discarded draft {draft}"
                );
            }
            assert!(!e.tools.pixel_grid);
            assert!(!e.session().fit);
            assert_eq!(e.session().zoom, 1.);
        })
        .unwrap();
    }
}

#[test]
fn menu_selection_loading_replaces_regardless_of_the_tool_mode() {
    use compositor::selection::{Selection, SelectionMode};
    let mut e = Editor::with_test_document();
    let mut doc = Document::new(20, 20).unwrap();
    compositor::edits::fill(&mut doc, [50, 120, 200, 255], false, false).unwrap();
    compositor::edits::add_mask(&mut doc, true).unwrap();
    doc.selection = Some(Selection::rectangle(20, 20, [2., 2.], [10., 10.], false));
    e.tabs = vec![Session::new(doc, None).into()];
    e.tools.selection_mode = SelectionMode::Subtract;
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Load selection").size(1500., 900.), e)
        .unwrap();
    cx.update(view, |e, cx| {
        for action in [Action::LoadAlpha, Action::LoadMask] {
            e.action(action, cx);
            assert_eq!(
                e.session()
                    .document
                    .selection
                    .as_ref()
                    .and_then(Selection::bounds),
                Some([0., 0., 20., 20.])
            );
            e.action(Action::Undo, cx);
        }
        assert_eq!(e.tools.selection_mode, SelectionMode::Subtract);
    })
    .unwrap();
}
