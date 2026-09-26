use super::*;
use quickgui::{Application, MouseButton, PointerEvent, PointerPhase, Size, Vector, WindowOptions};

fn brush_pointer(editor: &mut Editor, phase: PointerPhase) {
    let (zoom, offset) = editor.viewport(320., 200.);
    let position = quickgui::Point::new(
        (offset[0] + 8. * zoom) as f32,
        (offset[1] + 8. * zoom) as f32,
    );
    editor
        .pointer(&PointerEvent {
            tablet: None,
            phase,
            position,
            origin: position,
            local_position: position,
            local_origin: position,
            delta: Vector::ZERO,
            button: MouseButton::Left,
            modifiers: Modifiers::empty(),
            size: Size::new(320., 200.),
        })
        .unwrap();
}

#[test]
fn close_after_save_waits_for_uncommitted_brush_text_and_transform_edits() {
    for draft in ["brush", "text", "text after edit", "transform"] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Snapshot.comp");
        let mut document = Document::new(32, 24).unwrap();
        compositor::edits::fill(&mut document, [90, 120, 150, 255], false, false).unwrap();
        let original = document.clone();
        let mut editor = Editor::with_test_document();
        editor.tabs = vec![Session::new(document, None).into()];
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Save with newer draft").size(1280., 850.),
                editor,
            )
            .unwrap();
        cx.update(view, |editor, cx| {
            editor.request_close(CloseIntent::Window, cx);
            assert!(matches!(editor.modal, Some(Form::Close)));
            // Same state transition as the close dialog's Save action, with the
            // worker held until a newer user interaction has started.
            editor.modal = None;
            editor.save_to(path.clone(), cx);
            let job = editor.saves.queue.pop_front().unwrap();
            editor.saves.running = true;
            if draft == "text after edit" {
                editor.session_mut().edit("Resolution", |doc| { doc.resolution = 144.; Ok(()) }).unwrap();
            }
            let pending_revision = editor.session().revision();
            match draft {
                "brush" => {
                    editor.tools.tool = Tool::Brush;
                    editor.tools.brush.diameter = 4.;
                    editor.tools.brush_smoothing = 0.;
                    brush_pointer(editor, PointerPhase::Down);
                    assert!(editor.gesture.is_some());
                }
                "text" | "text after edit" => {
                    editor.begin_text([4., 4.], [4., 4.], true).unwrap();
                    let Some(Form::Text(text)) = &mut editor.modal else { panic!("Text draft missing"); };
                    text.style.content = "Unsaved text".into();
                }
                "transform" => {
                    editor.start_toolbar_transform().unwrap();
                    editor.set_transform_number(0, 6.).unwrap();
                }
                _ => unreachable!(),
            }
            assert_eq!(editor.session().revision(), pending_revision);
            let saved = job.run().unwrap();
            editor.saves.running = false;
            editor.finish_save(saved, cx).unwrap();
            assert!(editor.close_intent.is_some());
            assert_eq!(editor.tabs.len(), 1);
            match draft {
                "brush" => {
                    assert!(editor.gesture.is_some());
                    assert!(editor.session().has_pending_edit());
                    brush_pointer(editor, PointerPhase::Up);
                }
                "text" | "text after edit" => {
                    assert!(matches!(&editor.modal, Some(Form::Text(text)) if text.style.content == "Unsaved text"));
                    editor.apply_text().unwrap();
                }
                "transform" => {
                    assert!(editor.transform_edit.is_some());
                    editor.finish_toolbar_transform(true).unwrap();
                }
                _ => unreachable!(),
            }
            editor.changed(cx);
            assert!(editor.session().dirty());
            assert!(matches!(editor.modal, Some(Form::Close)));
            assert_eq!(project::load(&path).unwrap(), original);
        }).unwrap();
        assert!(cx.is_window_open(view.window_handle()));
        cx.click(view.window_handle(), "form-cancel").unwrap();
        assert!(cx.is_window_open(view.window_handle()));
    }
}
