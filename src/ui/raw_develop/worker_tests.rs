use super::*;

#[test]
fn failed_preview_cancels_queued_apply_and_export_before_controls_unlock() {
    for request in [Request::Apply, Request::Export(PathBuf::from("output.tif"))] {
        let mut editor = Editor::with_test_document();
        let original = editor.session().document.clone();
        let mut develop = Develop::new(Source::Path(PathBuf::from("camera.nef")), Target::New);
        let id = develop.id;
        develop.running = true;
        develop.committing = true;
        develop.request = Some(request);
        editor.develop = Some(develop);
        editor.receive_raw(id, 0, true, Err(invalid("Preview rendering failed")));
        let develop = editor.develop.as_ref().unwrap();
        assert!(!develop.running);
        assert!(!develop.committing);
        assert!(
            develop.request.is_none(),
            "Retry must be explicit after a failed preview"
        );
        assert_eq!(develop.error.as_deref(), Some("Preview rendering failed"));
        assert_eq!(editor.session().document, original);
    }
}

#[test]
fn superseded_preview_failure_does_not_cancel_current_commit_request() {
    let mut editor = Editor::with_test_document();
    let mut develop = Develop::new(Source::Path(PathBuf::from("camera.nef")), Target::New);
    let id = develop.id;
    develop.changed();
    develop.running = true;
    develop.committing = true;
    develop.request = Some(Request::Apply);
    editor.develop = Some(develop);
    editor.receive_raw(id, 0, true, Err(invalid("Obsolete preview failure")));
    let develop = editor.develop.as_ref().unwrap();
    assert!(develop.committing);
    assert!(matches!(develop.request, Some(Request::Apply)));
    assert!(develop.error.is_none());
}
