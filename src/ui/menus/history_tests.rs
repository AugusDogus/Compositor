use super::*;
use compositor::{document::Layer, geometry::Transform, layer_ops};
use quickgui::{Application, WindowOptions};

pub(in crate::ui) fn history(editor: &mut Editor, label: &str, before: Document) {
    let after = editor.session().document.clone();
    assert_ne!(after, before, "{label} must perform a real edit");
    assert_eq!(editor.session().undo_label(), Some(label));
    assert_eq!(
        editor.build_menu(1).items()[0].label().as_ref(),
        format!("Undo {label}")
    );
    editor.undo_document();
    assert_eq!(editor.session().document, before);
    assert_eq!(
        editor.build_menu(1).items()[1].label().as_ref(),
        format!("Redo {label}")
    );
    editor.redo_document();
    assert_eq!(editor.session().document, after);
}

#[test]
fn adding_a_blank_layer_names_the_edit_and_round_trips_the_document() {
    let e = Editor::with_test_document();
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("New layer history"), e)
        .unwrap();
    cx.update(view, |e, cx| {
        let before = e.session().document.clone();
        e.action(Action::AddLayer, cx);
        history(e, "New Blank Layer", before);
    })
    .unwrap();
}

#[test]
fn layer_commands_name_the_actual_mask_clipping_merge_and_flip_edits() {
    let mut editor = Editor::with_test_document();
    let mut doc = Document::new(8, 8).unwrap();
    compositor::edits::fill(&mut doc, [90, 120, 150, 255], false, false).unwrap();
    doc.layers[0].transform = Transform {
        origin: [1., 2.],
        ..Transform::new(4, 3)
    };
    layer_ops::duplicate_active(&mut doc).unwrap();
    editor.tabs = vec![Session::new(doc, None).into()];
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Layer history"), editor)
        .unwrap();
    for (action, label) in [
        (Action::Clip, "Create Clipping Mask"),
        (Action::Clip, "Release Clipping Mask"),
        (Action::AddMask, "Add Reveal-All Mask"),
        (Action::ToggleMask, "Disable Layer Mask"),
        (Action::ToggleMask, "Enable Layer Mask"),
        (Action::DeleteMask, "Delete Layer Mask"),
        (Action::HideMask, "Add Hide-All Mask"),
        (Action::DeleteMask, "Delete Layer Mask"),
        (Action::FlipX, "Flip Horizontal"),
        (Action::FlipY, "Flip Vertical"),
        (Action::FlipCanvasX, "Flip Canvas Horizontal"),
        (Action::FlipCanvasY, "Flip Canvas Vertical"),
        (Action::Merge, "Merge Down"),
    ] {
        cx.update(view, |e, cx| {
            let before = e.session().document.clone();
            e.action(action, cx);
            history(e, label, before);
        })
        .unwrap();
    }
    cx.update(view, |e, cx| {
        e.action(Action::Undo, cx);
        let doc = &mut e.session_mut().document;
        doc.selected = doc.layers.iter().map(|layer| layer.id).collect();
        let before = doc.clone();
        e.action(Action::Merge, cx);
        history(e, "Merge Layers", before);
        e.action(Action::Undo, cx);
        e.action(Action::Group, cx);
        let before = e.session().document.clone();
        e.action(Action::Merge, cx);
        history(e, "Merge Group", before);
    })
    .unwrap();
}

#[test]
fn adding_a_mask_from_selection_names_the_selection_edit() {
    let mut e = Editor::with_test_document();
    let mut doc = Document::new(8, 8).unwrap();
    doc.add(Layer::blank("Target", 8, 8)).unwrap();
    doc.selection = Some(compositor::selection::Selection::rectangle(
        8,
        8,
        [1., 1.],
        [4., 5.],
        false,
    ));
    let before = doc.clone();
    e.tabs = vec![Session::new(doc, None).into()];
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Selection mask history"), e)
        .unwrap();
    cx.update(view, |e, cx| {
        e.action(Action::AddMask, cx);
        history(e, "Add Mask from Selection", before);
    })
    .unwrap();
}

#[test]
fn layer_controls_name_visibility_mask_link_and_blend_edits() {
    let mut e = Editor::with_test_document();
    let mut doc = Document::new(8, 8).unwrap();
    compositor::edits::add_mask(&mut doc, false).unwrap();
    let id = doc.active.unwrap();
    e.tabs = vec![Session::new(doc, None).into()];
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Layer control history").size(1500., 900.),
            e,
        )
        .unwrap();
    let window = view.window_handle();
    for label in ["Unlink Layer Mask", "Link Layer Mask"] {
        let before = cx.read(view, |e| e.session().document.clone()).unwrap();
        cx.click(window, format!("mask-link-{id}")).unwrap();
        cx.update(view, |e, _| history(e, label, before)).unwrap();
    }
    for label in ["Disable Layer Mask", "Enable Layer Mask"] {
        let before = cx.read(view, |e| e.session().document.clone()).unwrap();
        cx.simulate_mouse_down(
            window,
            format!("layer-thumbnail-{id}-true"),
            quickgui::MouseDownEvent {
                button: quickgui::MouseButton::Left,
                position: quickgui::Point::new(1355., 277.),
                modifiers: Modifiers::SHIFT,
                click_count: 1,
                first_mouse: false,
            },
        )
        .unwrap();
        cx.update(view, |e, _| history(e, label, before)).unwrap();
    }
    for label in ["Hide Layer", "Show Layer"] {
        cx.update(view, |e, cx| {
            let before = e.session().document.clone();
            e.open_layer_context(id, quickgui::Point::new(1400., 277.), cx);
            e.invoke_row_menu(Command::Visibility, cx);
            history(e, label, before);
        })
        .unwrap();
    }
    cx.update(view, |e, _| {
        let before = e.session().document.clone();
        e.step_blend(true).unwrap();
        history(e, "Layer Blend Mode", before);
    })
    .unwrap();
}
