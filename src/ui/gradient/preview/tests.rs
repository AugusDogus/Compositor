use super::*;

fn editor() -> Editor {
    let mut editor = Editor::with_test_document();
    editor.tabs = vec![Session::new(Document::new(16, 16).unwrap(), None).into()];
    editor.tools.tool = Tool::Gradient;
    editor.tools.brush.color = [230, 40, 90, 255];
    editor.begin_gradient([0.5, 0.5]).unwrap();
    editor
        .move_gradient([15.5, 0.5], Endpoint::End, false)
        .unwrap();
    editor
}

#[test]
fn pointer_updates_coalesce_without_changing_pixels_or_starting_more_workers() {
    let mut editor = editor();
    let original = editor.session().document.clone();
    let first = editor.take_gradient_request().unwrap();
    for x in 1..200 {
        editor
            .move_gradient([f64::from(x), 15.5], Endpoint::End, false)
            .unwrap();
        assert!(editor.take_gradient_request().is_none());
    }
    assert_eq!(editor.session().document, original);
    assert!(editor.commit_gradient().is_err());
    let (id, revision) = (first.id, first.revision);
    editor
        .receive_gradient_preview(id, revision, first.render())
        .unwrap();
    assert_ne!(editor.session().document, original);
    assert!(
        editor.gradient_busy(),
        "An outdated preview must not enable Apply"
    );
    let latest = editor.take_gradient_request().unwrap();
    assert_eq!(latest.end, [199., 15.5]);
    let (id, revision) = (latest.id, latest.revision);
    let pixels = latest.render().unwrap();
    editor
        .receive_gradient_preview(id, revision, Ok(pixels.clone()))
        .unwrap();
    editor.commit_gradient().unwrap();
    assert_eq!(editor.session().document, pixels);
    assert_eq!(editor.session().undo_label(), Some("Gradient"));
    editor.session_mut().undo();
    assert_eq!(editor.session().document, original);
    editor.session_mut().redo();
    assert_eq!(editor.session().document, pixels);
}

#[test]
fn cancelled_worker_cannot_change_pixels_or_overwrite_a_new_gradient() {
    let mut editor = editor();
    let original = editor.session().document.clone();
    let cancelled = editor.take_gradient_request().unwrap();
    editor.undo_document();
    editor.begin_gradient([1., 1.]).unwrap();
    editor
        .move_gradient([8., 8.], Endpoint::End, false)
        .unwrap();
    assert!(
        editor.take_gradient_request().is_none(),
        "The old worker still owns the slot"
    );
    let (id, revision) = (cancelled.id, cancelled.revision);
    editor
        .receive_gradient_preview(id, revision, cancelled.render())
        .unwrap();
    assert_eq!(editor.session().document, original);
    assert!(editor.gradient_busy());
    editor.finish_gradient_preview_for_test().unwrap();
    editor.commit_gradient().unwrap();
    assert_ne!(editor.session().document, original);
}

#[test]
fn returning_to_zero_length_rejects_in_flight_pixels() {
    let mut editor = editor();
    let original = editor.session().document.clone();
    let request = editor.take_gradient_request().unwrap();
    editor
        .move_gradient([0.5, 0.5], Endpoint::End, false)
        .unwrap();
    let (id, revision) = (request.id, request.revision);
    editor
        .receive_gradient_preview(id, revision, request.render())
        .unwrap();
    assert_eq!(editor.session().document, original);
    editor.commit_gradient().unwrap();
    assert!(editor.session().undo_label().is_none());
}

#[test]
fn outdated_errors_do_not_discard_a_newer_valid_request() {
    let mut editor = editor();
    let request = editor.take_gradient_request().unwrap();
    editor.tools.gradient.reversed = true;
    editor.refresh_gradient().unwrap();
    editor
        .receive_gradient_preview(
            request.id,
            request.revision,
            Err(compositor::invalid("Old worker failed")),
        )
        .unwrap();
    editor.finish_gradient_preview_for_test().unwrap();
    editor.commit_gradient().unwrap();
    assert_eq!(
        editor.session().document.layers[0].raster().unwrap()[(0, 0)].0,
        [0; 4]
    );
}

#[test]
#[ignore = "release-mode screenshot-size gradient pointer profiling"]
fn screenshot_size_gradient_pointer_handlers() {
    assert!(!cfg!(debug_assertions), "Run with --release");
    for (width, height) in [(1920, 1080), (3840, 2160)] {
        let mut editor = editor();
        editor.pending_gradient = None;
        editor.tabs = vec![Session::new(Document::new(width, height).unwrap(), None).into()];
        let started = std::time::Instant::now();
        editor.begin_gradient([100., 100.]).unwrap();
        for x in 0..1000 {
            std::hint::black_box(&mut editor)
                .move_gradient([500. + f64::from(x), 800.], Endpoint::End, false)
                .unwrap();
        }
        println!(
            "{width}x{height}: 1000 gradient endpoint updates {:.3}ms",
            started.elapsed().as_secs_f64() * 1000.
        );
        assert!(editor.session().document.layers[0].raster().is_none());
        assert!(editor.gradient_busy());
    }
}
