use super::*;
use compositor::document::{Layer, LayerContent};
use image::{Rgba, RgbaImage};
use layer_drag::{LayerDrag, Transfer};
use quickgui::{Application, WindowOptions};

fn clipped_document(size: u32) -> Document {
    let mut doc = Document::new(size, size).unwrap();
    doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        size,
        size,
        Rgba([255, 255, 255, 128]),
    ))));
    let mut layer = Layer::blank("Clipped", size, size);
    layer.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        size,
        size,
        Rgba([80, 100, 120, 255]),
    ))));
    layer.clip_source = Some(doc.layers[0].id);
    doc.add(layer).unwrap();
    doc
}

#[test]
fn deleting_clipping_sources_can_cancel_unlink_or_fail_to_start_without_losing_pixels() {
    for choice in ["cancel", "unlink", "bake", "enter", "escape"] {
        let mut original = clipped_document(2);
        original.select(original.layers[0].id, false);
        let mut e = Editor::with_test_document();
        e.tabs = vec![Session::new(original.clone(), None).into()];
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Delete clipping source").size(1280., 850.),
                e,
            )
            .unwrap();
        cx.update(view, |e, cx| e.action(Action::DeleteLayer, cx))
            .unwrap();
        assert!(
            cx.read(view, |e| matches!(e.modal, Some(Form::DeleteLayers)))
                .unwrap()
        );
        let window = view.window_handle();
        assert_eq!(cx.focused(window).unwrap(), Some(70_001_u64.into()));
        match choice {
            "cancel" => cx.click(window, "form-cancel").unwrap(),
            "unlink" => cx.click(window, 70_002_u64).unwrap(),
            "enter" | "escape" => cx.simulate_keystrokes(window, choice).unwrap(),
            _ => cx.click(window, 70_001_u64).unwrap(),
        }
        cx.read(view, |e| {
            assert!(e.modal.is_none());
            assert!(!e.pending);
            if choice == "unlink" {
                assert_eq!(e.session().document.layers.len(), 1);
                assert!(e.session().document.layers[0].clip_source.is_none());
                assert_eq!(
                    e.session().document.layers[0].raster(),
                    original.layers[1].raster()
                );
            } else {
                // QuickGUI's headless context has no background worker pool.
                if matches!(choice, "bake" | "enter") {
                    assert!(
                        e.status.starts_with("Could not start image processing:"),
                        "{}",
                        e.status
                    );
                }
                assert_eq!(e.session().document, original);
                assert!(e.session().undo_label().is_none());
            }
        })
        .unwrap();
        if choice == "unlink" {
            cx.update(view, |e, _| e.session_mut().undo()).unwrap();
            assert_eq!(
                cx.read(view, |e| e.session().document.clone()).unwrap(),
                original
            );
        }
    }
}

#[test]
fn copying_to_a_new_tab_preserves_size_and_resolution_and_bakes_external_coverage() {
    let mut source = clipped_document(8);
    source.resolution = 144.;
    let mut e = Editor::with_test_document();
    e.tabs = vec![Session::new(source.clone(), None).into()];
    let drag = LayerDrag {
        session: e.session().id,
        layer: source.layers[1].id,
        operation: Transfer::Move,
    };
    e.copy_drag_to_new_tab(&drag).unwrap();
    assert_eq!(e.current, 1);
    assert_eq!(e.tabs[0].session().unwrap().document, source);
    let copied = &e.session().document;
    assert_eq!(
        (copied.width, copied.height, copied.resolution),
        (8, 8, 144.)
    );
    assert_eq!(copied.layers.len(), 1);
    assert!(copied.layers[0].clip_source.is_none());
    assert_eq!(
        copied.layers[0].raster().unwrap()[(0, 0)],
        Rgba([80, 100, 120, 128])
    );
    copied.validate().unwrap();
    let copied = copied.clone();
    e.undo_document();
    assert!(!e.has_document());
    assert_eq!(e.tabs[0].session().unwrap().document, source);
    e.redo_document();
    assert_eq!(e.session().document, copied);
}

#[test]
fn canvas_layer_transfer_uses_the_drop_point_at_the_current_zoom_and_pan() {
    let source = clipped_document(8);
    let mut e = Editor::with_test_document();
    e.tabs = vec![
        Session::new(source.clone(), None).into(),
        Session::new(Document::new(100, 80).unwrap(), None).into(),
    ];
    let drag = LayerDrag {
        session: e.tabs[0].id,
        layer: source.layers[1].id,
        operation: Transfer::Move,
    };
    e.activate_tab(1);
    e.session_mut().fit = false;
    e.session_mut().zoom = 2.;
    e.session_mut().pan = [13., -7.];
    let destination = e.session().document.clone();
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Canvas layer drop").size(1280., 850.), e)
        .unwrap();
    let bounds = cx.element_bounds(view.window_handle(), "canvas").unwrap();
    cx.update(view, |e, cx| {
        let (zoom, offset) = e.viewport(bounds.width, bounds.height);
        let point = quickgui::Point::new(
            bounds.x + (offset[0] + 34. * zoom) as f32,
            bounds.y + (offset[1] + 27. * zoom) as f32,
        );
        e.drop_layer_on_canvas(&drag, point, cx);
    })
    .unwrap();
    cx.read(view, |e| {
        assert_eq!(e.tabs[0].session().unwrap().document, source);
        let copied = e.session().document.active_layer().unwrap();
        assert_eq!(copied.transform.geometry_point([0.5, 0.5]), [34., 27.]);
        assert_eq!(copied.raster().unwrap()[(0, 0)][3], 128);
    })
    .unwrap();
    cx.update(view, |e, _| e.session_mut().undo()).unwrap();
    assert_eq!(
        cx.read(view, |e| e.session().document.clone()).unwrap(),
        destination
    );
}

#[test]
fn large_clipped_transfers_queue_immutable_source_pixels_for_the_destination() {
    for new_tab in [false, true] {
        let source = clipped_document(1000);
        let mut e = Editor::with_test_document();
        e.tabs = vec![
            Session::new(source.clone(), None).into(),
            Session::new(Document::new(20, 30).unwrap(), None).into(),
        ];
        let drag = LayerDrag {
            session: e.tabs[0].id,
            layer: source.layers[1].id,
            operation: Transfer::Move,
        };
        if new_tab {
            e.copy_drag_to_new_tab(&drag).unwrap();
        } else {
            e.copy_drag_to_tab(&drag, 1).unwrap();
        }
        assert!(e.pending);
        assert_eq!(e.current, if new_tab { 2 } else { 1 });
        assert_eq!(e.tabs[0].session().unwrap().document, source);
        assert!(
            e.tabs[e.current]
                .session()
                .is_none_or(|s| s.undo_label().is_none())
        );
        let Some(jobs::Job::CopyLayers {
            source: captured,
            layer,
            center,
            ..
        }) = e.job.take()
        else {
            panic!("Expected a layer-copy worker");
        };
        assert_eq!(*captured, source);
        assert_eq!(layer, drag.layer);
        assert_eq!(center, if new_tab { [500., 500.] } else { [10., 15.] });
    }
}

#[test]
fn failed_transfers_leave_welcome_tabs_empty() {
    for size in [8, 1000] {
        let source = clipped_document(size);
        let mut e = Editor::with_test_document();
        e.tabs = vec![Session::new(source.clone(), None).into()];
        let mut drag = LayerDrag {
            session: e.tabs[0].id,
            layer: uuid::Uuid::new_v4(),
            operation: Transfer::Move,
        };
        e.add_empty_tab();
        assert!(e.copy_drag_to_tab(&drag, 1).is_err());
        assert!(!e.has_document());
        assert!(!e.pending);
        assert!(e.copy_drag_to_new_tab(&drag).is_err());
        assert_eq!(e.tabs.len(), 2);
        if size == 1000 {
            drag.layer = source.layers[1].id;
            e.copy_drag_to_tab(&drag, 1).unwrap();
            // Headless QuickGUI cannot start workers. The first render reports
            // the failure and must keep the welcome tab intact.
            let (cx, view) = Application::new()
                .into_test_context(WindowOptions::new("Failed transfer").size(1280., 850.), e)
                .unwrap();
            cx.read(view, |e| {
                assert!(!e.pending);
                assert!(!e.has_document());
                assert!(e.status.starts_with("Could not start image processing:"));
                assert_eq!(e.tabs[0].session().unwrap().document, source);
            })
            .unwrap();
        }
    }
}

#[test]
fn row_copy_preserves_external_clipping_within_a_project_and_bakes_between_projects() {
    for cross_project in [false, true] {
        let source = clipped_document(8);
        let mut e = Editor::with_test_document();
        e.tabs = vec![Session::new(source.clone(), None).into()];
        let drag = LayerDrag {
            session: e.tabs[0].id,
            layer: source.layers[1].id,
            operation: Transfer::Copy,
        };
        if cross_project {
            e.tabs
                .push(Session::new(Document::new(20, 30).unwrap(), None).into());
            e.activate_tab(1);
        }
        let original = e.session().document.clone();
        let target = original.layers.last().unwrap().id;
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Clipped row copy").size(1280., 850.), e)
            .unwrap();
        cx.update(view, |e, cx| {
            e.drop_layer(&drag, layer_drag::DropPlacement::Above(target), cx)
        })
        .unwrap();
        cx.read(view, |e| {
            let copied = e.session().document.active_layer().unwrap();
            assert_ne!(copied.id, drag.layer);
            assert_eq!(
                copied.clip_source,
                if cross_project {
                    None
                } else {
                    Some(source.layers[0].id)
                }
            );
            assert_eq!(
                copied.raster().unwrap()[(0, 0)][3],
                if cross_project { 128 } else { 255 }
            );
            if cross_project {
                assert_eq!(copied.transform.geometry_point([0.5, 0.5]), [10., 15.]);
                assert_eq!(e.tabs[0].session().unwrap().document, source);
            }
        })
        .unwrap();
        cx.update(view, |e, _| e.session_mut().undo()).unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            original
        );
    }
}

#[test]
fn large_row_transfer_worker_bakes_coverage_and_keeps_the_requested_folder() {
    let source = clipped_document(1000);
    let mut destination = Session::new(Document::new(1200, 1400).unwrap(), None);
    destination.group().unwrap();
    let folder = destination.document.active.unwrap();
    let before = destination.document.clone();
    let mut e = Editor::with_test_document();
    e.tabs = vec![
        Session::new(source.clone(), None).into(),
        destination.into(),
    ];
    let drag = LayerDrag {
        session: e.tabs[0].id,
        layer: source.layers[1].id,
        operation: Transfer::Move,
    };
    e.activate_tab(1);
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Worker row drop").size(1280., 850.), e)
        .unwrap();
    // Capture the queued work before the headless runtime tries to spawn it.
    let job = cx
        .update(view, |e, cx| {
            e.drop_layer(&drag, layer_drag::DropPlacement::Into(folder), cx);
            e.job.take().unwrap()
        })
        .unwrap();
    let result = job.run(before.clone()).unwrap();
    let copied = result.active_layer().unwrap();
    assert_eq!(copied.parent, Some(folder));
    assert_eq!(copied.transform.geometry_point([0.5, 0.5]), [600., 700.]);
    assert_eq!(copied.raster().unwrap()[(0, 0)], Rgba([80, 100, 120, 128]));
    assert!(copied.clip_source.is_none());
    assert_eq!(
        cx.read(view, |e| e.session().document.clone()).unwrap(),
        before
    );
    assert_eq!(
        cx.read(view, |e| e.tabs[0].session().unwrap().document.clone())
            .unwrap(),
        source
    );
}
