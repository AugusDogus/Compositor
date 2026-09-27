use super::*;

fn pack() -> Pack {
    Pack::from_bytes(include_bytes!("../../../../tests/fixtures/abr/sampled-v2.abr").to_vec())
        .unwrap()
}
#[test]
fn preview_decodes_one_tip_without_installing_it_and_ignores_a_replaced_pack() {
    let mut editor = Editor::with_test_document();
    let document = editor.session().document.clone();
    editor.open_brush_pack(pack());
    let first = editor.brush_import_mut().unwrap();
    let source = first.pack.clone();
    first.preview.request(1);
    assert_eq!(first.preview.take(), Some(1));
    let image = render(&source, 1).unwrap();
    assert!(image.width() <= 120 && image.height() <= 120);
    editor.finish_brush_preview(&source, 1, Ok(image));
    assert!(matches!(
        editor.brush_import_mut().unwrap().preview,
        Preview::Ready { index: 1, .. }
    ));
    assert!(editor.brush_presets.tips.is_empty());
    assert_eq!(editor.session().document, document);
    assert!(editor.session().undo_label().is_none());

    editor.open_brush_pack(pack());
    editor.finish_brush_preview(&source, 1, Err(invalid("stale result")));
    assert!(matches!(
        editor.brush_import_mut().unwrap().preview,
        Preview::Empty
    ));
}
#[test]
fn rapid_requests_coalesce_and_preview_failures_remain_visible() {
    let mut preview = Preview::default();
    preview.request(0);
    assert_eq!(preview.take(), Some(0));
    preview.request(1);
    preview.request(2);
    assert_eq!(preview.take(), None, "only one decode runs at a time");
    preview.finish(0, Err(invalid("obsolete error")));
    assert_eq!(preview.take(), Some(2));
    preview.finish(0, Err(invalid("wrong completion")));
    assert!(matches!(preview, Preview::Running { index: 2, .. }));
    preview.finish(2, Err(invalid("tip is damaged")));
    assert!(matches!(&preview, Preview::Failed { index: 2, error } if error.contains("damaged")));
    preview.request(2);
    assert_eq!(preview.take(), Some(2), "failed preview can be retried");
    assert!(render(&pack(), 20).is_err());
}

#[test]
fn preview_stays_inside_the_picker_beside_its_tip_list() {
    use quickgui::{Application, WindowOptions};
    let mut editor = Editor::with_test_document();
    editor.open_brush_pack(pack());
    let import = editor.brush_import_mut().unwrap();
    import.preview.request(1);
    import.preview.take();
    import.preview.finish(1, render(&import.pack, 1));
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(WindowOptions::new("ABR preview").size(1500., 900.), editor)
        .unwrap();
    let preview = cx
        .element_bounds(view.window_handle(), "abr-tip-preview")
        .unwrap();
    let row = cx
        .element_bounds(view.window_handle(), "abr-tip-1")
        .unwrap();
    assert!(preview.x >= row.x + row.width);
    assert!(preview.width <= 120. && preview.height <= 120.);
    assert!(
        cx.element_bounds(view.window_handle(), "abr-import")
            .is_ok()
    );
}
