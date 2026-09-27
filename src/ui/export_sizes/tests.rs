use super::*;
use quickgui::{Application, WindowOptions};

#[test]
fn export_sizes_presets_format_and_invalid_submit_preserve_document() {
    let mut editor = Editor::with_test_document();
    let original = editor.session().document.clone();
    editor.open_export_sizes();
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(WindowOptions::new("Export Sizes").size(1180., 780.), editor)
        .unwrap();
    let window = view.window_handle();
    cx.click(window, "export-sizes-Social").unwrap();
    cx.click(window, "form-choice-1-fill").unwrap();
    cx.click(window, "form-choice-2-jpeg").unwrap();
    cx.read(view, |editor| {
        let Some(Form::Edit { fields, .. }) = &editor.modal else {
            panic!("Missing export sheet");
        };
        assert_eq!(fields[0].1, "1080x1080, 1080x1350, 1080x1920");
        assert_eq!(fields[1].1, "fill");
        assert_eq!(fields[2].1, "jpeg");
        assert!(batch(fields).is_ok());
    })
    .unwrap();
    cx.update(view, |editor, _| editor.update_form_field(0, "0x2"))
        .unwrap();
    cx.click(window, "form-apply").unwrap();
    cx.read(view, |editor| {
        assert!(matches!(&editor.modal, Some(Form::Edit { error, .. }) if !error.is_empty()));
        assert!(!editor.pending);
        assert_eq!(editor.session().document, original);
    })
    .unwrap();
    cx.click(window, "form-cancel").unwrap();
    cx.read(view, |editor| {
        assert!(editor.modal.is_none());
        assert_eq!(editor.session().document, original);
        assert!(editor.session().undo_label().is_none());
    })
    .unwrap();
}

#[test]
fn export_sizes_controls_remain_visible_above_retained_adjustments() {
    for kind in [Kind::Curves, Kind::GradientMap] {
        let mut editor = Editor::with_test_document();
        let mut document = Document::new(16, 12).unwrap();
        compositor::edits::fill(&mut document, [80, 120, 160, 255], false, false).unwrap();
        editor.tabs = vec![Session::new(document, None).into()];
        editor.open_pixel_adjustment(kind).unwrap();
        let (mut cx, view) = Application::new()
            .font(crate::UI_FONT)
            .into_test_context(
                WindowOptions::new("Export with adjustment").size(1180., 780.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        cx.update(view, |editor, cx| editor.action(Action::ExportSizes, cx))
            .unwrap();
        cx.click(window, "export-sizes-Social").unwrap();
        cx.read(view, |editor| {
            assert!(matches!(&editor.modal, Some(Form::Edit { action: Action::ExportSizes, fields, .. }) if fields[0].1.starts_with("1080x1080")));
            assert!(editor.retained_panel.is_some());
            assert_eq!(editor.adjustment_edit.as_ref().unwrap().settings.kind, kind);
        }).unwrap();
        cx.click(window, "form-cancel").unwrap();
        cx.read(view, |editor| {
            assert!(editor.retained_panel.is_none());
            assert!(matches!(
                editor.modal,
                Some(Form::Edit {
                    action: Action::EditAdjustment,
                    ..
                })
            ));
        })
        .unwrap();
    }
}

#[test]
fn export_sizes_job_uses_its_snapshot_and_keeps_save_state() {
    let directory = tempfile::tempdir().unwrap();
    let mut document = Document::new(2, 2).unwrap();
    compositor::edits::fill(&mut document, [40, 80, 120, 255], false, false).unwrap();
    let mut editor = Editor::with_test_document();
    editor.tabs = vec![Session::new(document.clone(), None).into()];
    let job = super::super::file_jobs::FileJob::ExportSizes {
        document,
        batch: Batch::new(vec![[2, 2]], Fit::Contain, Format::Png).unwrap(),
        parent: directory.path().into(),
        title: "Snapshot".into(),
    };
    editor
        .session_mut()
        .edit("Fill", |document| {
            compositor::edits::fill(document, [200, 100, 0, 255], false, false)
        })
        .unwrap();
    let dirty = editor.session().dirty();
    let revision = editor.session().revision();
    let super::super::file_jobs::Completed::Exported { path, .. } = job.run(&[]).unwrap() else {
        panic!("Missing export result");
    };
    let pixels = compositor::image_io::read_image(&path.join("Snapshot-2x2.png")).unwrap();
    assert!(pixels.pixels().all(|pixel| pixel.0 == [40, 80, 120, 255]));
    assert_eq!(editor.session().dirty(), dirty);
    assert_eq!(editor.session().revision(), revision);
}

#[test]
fn export_sizes_inputs_do_not_share_ids_with_retained_filter_fields() {
    for action in [
        Action::Filter(compositor::filters::Filter::Gaussian { radius: 2. }),
        Action::AdjustPixels(Kind::HueSaturation),
    ] {
        let mut editor = Editor::with_test_document();
        let mut document = Document::new(16, 12).unwrap();
        compositor::edits::fill(&mut document, [80, 120, 160, 255], false, false).unwrap();
        editor.tabs = vec![Session::new(document, None).into()];
        let (mut cx, view) = Application::new()
            .font(crate::UI_FONT)
            .into_test_context(
                WindowOptions::new("Export above numeric fields").size(1180., 780.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        cx.update(view, |editor, cx| {
            editor.action(action, cx);
            editor.action(Action::ExportSizes, cx);
        })
        .unwrap();
        cx.focus(window, 60_000_u64).unwrap();
        cx.simulate_keystrokes(window, "ctrl-a").unwrap();
        cx.simulate_input(window, "24x24, 48x48").unwrap();
        cx.click(window, "form-choice-2-jpeg").unwrap();
        cx.focus(window, 60_003_u64).unwrap();
        cx.simulate_keystrokes(window, "ctrl-a").unwrap();
        cx.simulate_input(window, "92").unwrap();
        cx.read(view, |editor| {
            assert!(matches!(&editor.modal, Some(Form::Edit { fields, .. }) if fields[0].1 == "24x24, 48x48" && fields[3].1 == "92"));
            assert!(editor.retained_panel.is_some());
        }).unwrap();
        cx.click(window, "form-cancel").unwrap();
        cx.read(view, |editor| {
            assert!(editor.floating_panel_kind().is_some())
        })
        .unwrap();
    }
}
