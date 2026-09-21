use super::*;
use compositor::{document::LayerContent, geometry::Transform, selection::Selection};
use quickgui::{Application, ClipboardEntry, ClipboardItem, WindowOptions};

fn editor() -> Editor {
    let mut e = Editor::with_test_document();
    let mut doc = Document::new(20, 20).unwrap();
    compositor::edits::fill(&mut doc, [50, 120, 200, 255], false, false).unwrap();
    doc.selection = Some(Selection::rectangle(20, 20, [2., 2.], [10., 10.], false));
    e.tabs = vec![Session::new(doc, None).into()];
    e
}

#[test]
fn fill_and_clipboard_commands_preserve_pending_edits_and_clipboard_contents() {
    for draft in 0..4 {
        let mut e = editor();
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
                WindowOptions::new("Pixel commands with drafts").size(1500., 900.),
                e,
            )
            .unwrap();
        cx.write_to_clipboard(ClipboardItem::new_string("Keep this clipboard").unwrap())
            .unwrap();
        cx.update(view, |e, cx| {
            for action in [
                Action::Fill,
                Action::FillBackground,
                Action::Clear,
                Action::Copy,
                Action::CopyMerged,
                Action::Cut,
                Action::Paste,
            ] {
                e.action(action, cx);
                assert_eq!(e.session().document, before, "Draft {draft}");
                assert!(e.session().undo_label().is_none(), "Draft {draft}");
                assert!(match draft {
                    0 => e.transform_edit.is_some(),
                    1 => e.pending_gradient.is_some(),
                    2 => e.tools.pending_crop.is_some(),
                    _ => e.pending_pixels.is_some(),
                });
            }
        })
        .unwrap();
        let clipboard = cx.read_from_clipboard().unwrap().unwrap();
        assert!(
            matches!(clipboard.entries(), [ClipboardEntry::String(text)] if text.text() == "Keep this clipboard")
        );
    }
}

#[test]
fn hidden_pixels_disabled_masks_and_multiple_layers_can_be_copied_but_not_cleared() {
    for target in 0..3 {
        let mut e = editor();
        match target {
            0 => e.session_mut().document.layers[0].visible = false,
            1 => {
                compositor::edits::add_mask(&mut e.session_mut().document, false).unwrap();
                e.session_mut().document.layers[0]
                    .mask
                    .as_mut()
                    .unwrap()
                    .enabled = false;
                e.tools.mask_target = true;
                e.session_mut().document.selection =
                    Some(Selection::rectangle(20, 20, [2., 2.], [10., 10.], false));
            }
            _ => {
                let doc = &mut e.session_mut().document;
                let active = doc.active.unwrap();
                doc.add(compositor::document::Layer::blank("Second", 20, 20))
                    .unwrap();
                doc.select(active, true);
            }
        }
        let before = e.session().document.clone();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Copy without clearing").size(1500., 900.),
                e,
            )
            .unwrap();
        cx.update(view, |e, cx| {
            for action in [Action::Fill, Action::FillBackground, Action::Clear] {
                e.action(action, cx);
                assert_eq!(e.session().document, before);
            }
            e.action(Action::Cut, cx);
            assert!(e.pixel_clipboard.is_some());
            assert_eq!(e.session().document, before);
            assert!(e.session().undo_label().is_none());
        })
        .unwrap();
        assert!(
            cx.read_from_clipboard()
                .unwrap()
                .unwrap()
                .entries()
                .iter()
                .any(|entry| matches!(entry, ClipboardEntry::Image(_)))
        );
    }
}

#[test]
fn clear_requires_a_selection_and_fill_can_initialize_blank_pixels_or_a_group_mask() {
    let mut e = editor();
    e.session_mut().document.selection = None;
    let before = e.session().document.clone();
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Fill and Clear targets").size(1500., 900.),
            e,
        )
        .unwrap();
    cx.update(view, |e, cx| {
        e.action(Action::Clear, cx);
        assert_eq!(e.session().document, before);
        e.session_mut().document.layers[0].content = LayerContent::Raster(None);
        e.action(Action::Fill, cx);
        assert!(e.session().document.layers[0].raster().is_some());
        e.session_mut().document.layers[0].content = LayerContent::Group;
        compositor::edits::add_mask(&mut e.session_mut().document, false).unwrap();
        e.tools.mask_target = true;
        e.action(Action::Fill, cx);
        assert_eq!(
            e.session().document.layers[0].mask.as_ref().unwrap().pixels[(0, 0)][0],
            0
        );
        assert_eq!(e.session().undo_label(), Some("Fill Mask"));
    })
    .unwrap();
}
