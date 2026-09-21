use super::history_tests::history;
use super::*;
use crate::ui::layer_drag::{DropPlacement, LayerDrag, Transfer};
use compositor::{document::Layer, layer_ops};
use quickgui::{Application, WindowOptions};

fn document(count: usize) -> Document {
    let mut doc = Document::new(8, 8).unwrap();
    compositor::edits::fill(&mut doc, [100, 140, 180, 128], false, false).unwrap();
    for _ in 1..count {
        layer_ops::add_blank(&mut doc).unwrap();
    }
    doc
}

#[test]
fn deletion_and_reordering_commands_name_the_selected_scope() {
    for (count, folder) in [(1, false), (2, false), (1, true)] {
        let mut doc = document(3);
        doc.selected = doc.layers.iter().take(count).map(|l| l.id).collect();
        doc.active = Some(doc.layers[0].id);
        if folder {
            doc.selected = doc.layers.iter().take(2).map(|l| l.id).collect();
            layer_ops::group(&mut doc).unwrap();
        }
        let mut e = Editor::with_test_document();
        e.tabs = vec![Session::new(doc, None).into()];
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Layer operation history"), e)
            .unwrap();
        cx.update(view, |e, cx| {
            let before = e.session().document.clone();
            e.action(Action::DeleteLayer, cx);
            history(
                e,
                if count == 1 {
                    "Delete Layer"
                } else {
                    "Delete Layers"
                },
                before,
            );
            e.undo_document();
            let before = e.session().document.clone();
            e.action(Action::Raise, cx);
            history(e, "Reorder Layers", before);
        })
        .unwrap();
    }
}

#[test]
fn moving_a_selected_folder_and_its_child_names_one_root() {
    let mut doc = document(3);
    let child = doc.layers[0].id;
    let destination = doc.layers[2].id;
    doc.select(child, false);
    layer_ops::group(&mut doc).unwrap();
    doc.select(child, true);
    let mut e = Editor::with_test_document();
    e.tabs = vec![Session::new(doc, None).into()];
    let drag = LayerDrag {
        session: e.session().id,
        layer: child,
        operation: Transfer::Move,
    };
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Folder move history"), e)
        .unwrap();
    cx.update(view, |e, cx| {
        let before = e.session().document.clone();
        e.drop_layer(&drag, DropPlacement::Above(destination), cx);
        history(e, "Move Layer", before);
    })
    .unwrap();
}

#[test]
fn baked_deletion_names_the_original_selection_before_the_worker_removes_it() {
    for count in [1, 2] {
        let mut doc = document(count);
        let selected = doc.layers.iter().map(|l| l.id).collect();
        let base = doc.layers[0].id;
        let mut clipped = Layer::blank("Clipped", 8, 8);
        clipped.content = doc.layers[0].content.clone();
        clipped.clip_source = Some(base);
        doc.add(clipped).unwrap();
        let clipped = doc.layers.pop().unwrap();
        doc.layers.insert(1, clipped);
        doc.selected = selected;
        doc.active = Some(base);
        doc.validate().unwrap();
        let mut e = Editor::with_test_document();
        e.tabs = vec![Session::new(doc.clone(), None).into()];
        let job = jobs::Job::DeleteLayersBaked;
        let completion = job.completion();
        let result = job.run(doc.clone());
        e.complete_job(e.session().id, doc.clone(), result, completion)
            .unwrap();
        history(
            &mut e,
            if count == 1 {
                "Delete Layer"
            } else {
                "Delete Layers"
            },
            doc,
        );
    }
}

#[test]
fn layer_drag_history_distinguishes_moves_duplicates_and_cross_project_copies() {
    for cross_project in [false, true] {
        for operation in [Transfer::Move, Transfer::Copy] {
            for count in [1, 2] {
                let mut source = document(3);
                source.selected = source.layers.iter().take(count).map(|l| l.id).collect();
                source.active = Some(source.layers[0].id);
                let layer = source.layers[0].id;
                let mut e = Editor::with_test_document();
                e.tabs = vec![Session::new(source, None).into()];
                let drag = LayerDrag {
                    session: e.session().id,
                    layer,
                    operation,
                };
                if cross_project {
                    e.tabs.push(Session::new(document(1), None).into());
                    e.current = 1;
                }
                let target = e.session().document.layers.last().unwrap().id;
                let label = if cross_project {
                    "Copy Layers from Project"
                } else if operation == Transfer::Copy {
                    if count == 1 {
                        "Duplicate Layer"
                    } else {
                        "Duplicate Layers"
                    }
                } else if count == 1 {
                    "Move Layer"
                } else {
                    "Move Layers"
                };
                let (mut cx, view) = Application::new()
                    .into_test_context(WindowOptions::new("Layer transfer history"), e)
                    .unwrap();
                cx.update(view, |e, cx| {
                    let before = e.session().document.clone();
                    e.drop_layer(&drag, DropPlacement::Above(target), cx);
                    history(e, label, before);
                    e.undo_document();
                    if cross_project || operation == Transfer::Copy {
                        let before = e.session().document.clone();
                        e.copy_drag_to_tab(&drag, e.current).unwrap();
                        history(e, label, before);
                    }
                })
                .unwrap();
            }
        }
    }
}
