use super::*;
use compositor::{
    adjustment::Adjustment,
    document::{Layer, LayerContent},
    edits, layer_ops,
    selection::Selection,
};
use quickgui::{Application, Menubar, WindowOptions};

#[test]
fn duplicate_menu_and_shortcut_reject_folders_empty_selections_and_missing_pixels() {
    for case in [
        "folder",
        "empty selection",
        "blank selection",
        "adjustment selection",
        "no active layer",
    ] {
        let mut editor = Editor::with_test_document();
        let mut doc = Document::new(16, 16).unwrap();
        edits::fill(&mut doc, [80, 120, 200, 255], false, false).unwrap();
        match case {
            "folder" => layer_ops::group(&mut doc).unwrap(),
            "empty selection" => {
                doc.selection = Some(Selection::rectangle(16, 16, [20., 20.], [30., 30.], false))
            }
            "no active layer" => {
                doc.active = None;
                doc.selected.clear();
            }
            _ => {
                doc.layers[0].content = if case == "blank selection" {
                    LayerContent::Raster(None)
                } else {
                    LayerContent::Adjustment(Box::new(Adjustment::new(Kind::Exposure)))
                };
                doc.selection = Some(Selection::rectangle(16, 16, [2., 2.], [8., 8.], false));
            }
        }
        let label = if doc.selection.is_some() {
            "Layer via Copy"
        } else {
            "Duplicate Layer"
        };
        let original = doc.clone();
        editor.tabs = vec![Session::new(doc, None).into()];
        let (mut cx, view) = Application::new()
            .bind_keys(quickgui::popover_menu_key_bindings())
            .into_test_context(
                WindowOptions::new("Duplicate availability").size(1280., 900.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        cx.click(window, Menubar::new("application-menu").item_id(6))
            .unwrap();
        let command = cx.read(view, |e| e.menus.command_id(label)).unwrap();
        let status = cx.read(view, |e| e.status.clone()).unwrap();
        assert!(
            matches!(
                cx.click(window, command),
                Err(quickgui::TestAppError::NotClickable { .. })
            ),
            "{case}"
        );
        cx.simulate_keystrokes(window, "escape").unwrap();
        cx.focus(window, "workspace").unwrap();
        cx.simulate_keystrokes(window, "ctrl-j").unwrap();
        cx.read(view, |e| {
            assert_eq!(e.session().document, original, "{case}");
            assert!(e.session().undo_label().is_none(), "{case}");
            assert_eq!(e.status, status, "{case}");
        })
        .unwrap();
    }
}

#[test]
fn duplicate_copies_only_the_active_layer_with_its_properties_and_one_undo_step() {
    for content in ["pixels", "blank", "adjustment"] {
        let mut editor = Editor::with_test_document();
        let mut doc = Document::new(16, 16).unwrap();
        let base = doc.active.unwrap();
        let mut source = Layer::blank("Source", 16, 16);
        source.clip_source = Some(base);
        let source_id = source.id;
        doc.add(source).unwrap();
        edits::fill(&mut doc, [80, 120, 200, 255], false, false).unwrap();
        edits::add_mask(&mut doc, false).unwrap();
        doc.select(base, true);
        layer_ops::group(&mut doc).unwrap();
        doc.add(Layer::blank("Other selected layer", 16, 16))
            .unwrap();
        doc.select(source_id, true);
        let source = doc.active_layer_mut().unwrap();
        source.opacity = 0.35;
        source.visible = false;
        source.transform.rotation = 17.;
        match content {
            "blank" => source.content = LayerContent::Raster(None),
            "adjustment" => {
                source.content = LayerContent::Adjustment(Box::new(Adjustment::new(Kind::Exposure)))
            }
            _ => {}
        }
        let mut expected = source.clone();
        let original = doc.clone();
        editor.tabs = vec![Session::new(doc, None).into()];
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Duplicate active layer").size(1280., 900.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        cx.focus(window, "workspace").unwrap();
        cx.simulate_keystrokes(window, "ctrl-j").unwrap();
        let changed = cx
            .read(view, |e| {
                let doc = &e.session().document;
                assert_eq!(doc.layers.len(), original.layers.len() + 1, "{content}");
                let source_index = doc
                    .layers
                    .iter()
                    .position(|layer| layer.id == source_id)
                    .unwrap();
                let copy = &doc.layers[source_index + 1];
                assert_ne!(copy.id, source_id);
                expected.id = copy.id;
                expected.name.push_str(" copy");
                assert_eq!(*copy, expected);
                assert_eq!(doc.active, Some(copy.id));
                assert_eq!(doc.selected, [copy.id].into());
                assert_eq!(e.session().undo_label(), Some("Duplicate Layer"));
                doc.clone()
            })
            .unwrap();
        cx.simulate_keystrokes(window, "ctrl-z").unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            original
        );
        cx.simulate_keystrokes(window, "ctrl-shift-z").unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            changed
        );
    }
}

#[test]
fn layer_via_copy_keeps_source_pixels_and_copies_the_selected_pixel_or_mask_region() {
    for mask in [false, true] {
        let mut editor = Editor::with_test_document();
        let mut doc = Document::new(16, 16).unwrap();
        edits::fill(&mut doc, [80, 120, 200, 255], false, false).unwrap();
        edits::add_mask(&mut doc, false).unwrap();
        doc.selection = Some(Selection::rectangle(16, 16, [2., 3.], [8., 9.], false));
        let original = doc.clone();
        editor.tabs = vec![Session::new(doc, None).into()];
        editor.tools.mask_target = mask;
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Layer via Copy").size(1280., 900.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        cx.click(window, Menubar::new("application-menu").item_id(6))
            .unwrap();
        let command = cx
            .read(view, |e| e.menus.command_id("Layer via Copy"))
            .unwrap();
        cx.click(window, command).unwrap();
        cx.read(view, |e| {
            let doc = &e.session().document;
            assert_eq!(doc.layers.len(), 2);
            assert_eq!(doc.layers[0], original.layers[0]);
            let copy = doc.active_layer().unwrap();
            assert_eq!(copy.transform.origin, [2., 3.]);
            assert_eq!(copy.raster().unwrap().dimensions(), (6, 6));
            assert!(
                copy.raster()
                    .unwrap()
                    .pixels()
                    .all(|pixel| pixel.0 == if mask { [255; 4] } else { [80, 120, 200, 255] })
            );
            assert!(doc.selection.is_none());
            assert!(!e.tools.mask_target);
            assert_eq!(e.session().undo_label(), Some("Layer via Copy"));
        })
        .unwrap();
        cx.update(view, |e, cx| e.action(Action::Undo, cx)).unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            original
        );
    }
}
