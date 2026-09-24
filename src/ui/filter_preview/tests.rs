use super::*;

#[test]
fn noise_distribution_labels_are_centered_and_choices_remain_operable() {
    use quickgui::{Application, WindowOptions};

    let mut editor = Editor::with_test_document();
    compositor::edits::fill(
        &mut editor.session_mut().document,
        [80, 120, 160, 255],
        false,
        false,
    )
    .unwrap();
    let original = editor.session().document.clone();
    editor
        .open_filter(Filter::Noise {
            amount: 10.,
            gaussian: false,
            monochromatic: false,
            seed: 42,
        })
        .unwrap();
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(
            WindowOptions::new("Noise distribution").size(1280., 900.),
            editor,
        )
        .unwrap();
    let window = view.window_handle();
    let frame = cx.capture_screenshot(window).unwrap();
    let scale = frame.width() as f32 / 1280.;
    for choice in ["0", "1"] {
        let bounds = cx
            .element_bounds(window, format!("form-choice-1-{choice}"))
            .unwrap();
        let mut ink = [f32::INFINITY, f32::INFINITY, 0_f32, 0_f32];
        for y in (bounds.y * scale) as u32..((bounds.y + bounds.height) * scale) as u32 {
            for x in (bounds.x * scale) as u32..((bounds.x + bounds.width) * scale) as u32 {
                if frame.pixel(x, y).is_some_and(|pixel| pixel[0] > 200) {
                    ink[0] = ink[0].min(x as f32 / scale);
                    ink[1] = ink[1].min(y as f32 / scale);
                    ink[2] = ink[2].max((x + 1) as f32 / scale);
                    ink[3] = ink[3].max((y + 1) as f32 / scale);
                }
            }
        }
        let offset = [
            (ink[0] + ink[2]) / 2. - (bounds.x + bounds.width / 2.),
            (ink[1] + ink[3]) / 2. - (bounds.y + bounds.height / 2.),
        ];
        assert!(
            offset.into_iter().all(|value| value.abs() <= 2.),
            "Distribution label must be centered: choice={choice}, offset={offset:?}"
        );
    }
    for (label, choice) in [("Gaussian", "1"), ("Uniform", "0")] {
        cx.focus(window, format!("form-choice-1-{choice}")).unwrap();
        cx.simulate_keystrokes(window, "space").unwrap();
        cx.read(view, |editor| {
            assert!(matches!(&editor.modal, Some(Form::Edit { fields, .. })
                if fields[1].1 == choice));
            assert!(!editor.space_pan);
        })
        .unwrap();
        let tree = cx.accessibility_update(window).unwrap();
        assert!(
            tree.nodes.iter().any(|(_, node)| {
                node.label() == Some(label) && node.is_selected() == Some(true)
            })
        );
    }
    cx.click(window, "form-cancel").unwrap();
    cx.read(view, |editor| {
        assert_eq!(editor.session().document, original);
        assert!(editor.session().undo_label().is_none());
    })
    .unwrap();
}

#[test]
fn automatic_filters_reject_apply_until_a_matching_preview_succeeds() {
    for background in [false, true] {
        let mut e = Editor::with_test_document();
        e.tabs = vec![Session::new(Document::new(4, 4).unwrap(), None).into()];
        compositor::edits::fill(
            &mut e.session_mut().document,
            [10, 20, 30, 255],
            false,
            false,
        )
        .unwrap();
        let action = if background {
            e.open_background().unwrap();
            Action::RemoveBackground
        } else {
            e.open_filter(Filter::ContentFill).unwrap();
            Action::Filter(Filter::ContentFill)
        };
        let values = match &e.modal {
            Some(Form::Edit { fields, .. }) => {
                fields.iter().map(|f| f.1.clone()).collect::<Vec<_>>()
            }
            _ => panic!("missing filter form"),
        };
        let original = e.session().document.clone();
        assert!(e.apply_form(action, values.clone()).is_err());
        let edit = e.filter_edit.as_mut().unwrap();
        edit.work = Work::Running;
        let (id, revision) = (edit.id, edit.revision);
        assert!(e.apply_form(action, values.clone()).is_err());
        e.receive_filter_preview(id, revision, Err(invalid("Preview failed")));
        assert!(e.apply_form(action, values).is_err());
        assert_eq!(e.session().document, original);
        assert!(e.filter_edit.is_some());
        assert!(!e.pending);
        assert!(e.job.is_none());
        assert!(e.session().undo_label().is_none());
    }
}

#[test]
fn automatic_preview_completed_while_hidden_is_retained_and_content_fill_commits_it() {
    let mut e = Editor::with_test_document();
    e.tabs = vec![Session::new(Document::new(4, 4).unwrap(), None).into()];
    compositor::edits::fill(
        &mut e.session_mut().document,
        [10, 20, 30, 255],
        false,
        false,
    )
    .unwrap();
    let original = e.session().document.clone();
    e.open_filter(Filter::ContentFill).unwrap();
    let edit = e.filter_edit.as_mut().unwrap();
    edit.work = Work::Running;
    edit.enabled = false;
    let (id, revision) = (edit.id, edit.revision);
    let mut prepared = original.clone();
    compositor::edits::fill(&mut prepared, [50, 120, 200, 255], false, false).unwrap();
    e.receive_filter_preview(
        id,
        revision,
        Ok(PreviewOutput {
            camera_scope: None,
            document: prepared.clone(),
            subject: None,
        }),
    );
    assert_eq!(e.session().document, original);
    assert!(e.filter_edit.as_ref().unwrap().prepared.is_some());
    e.apply_form(Action::Filter(Filter::ContentFill), vec![])
        .unwrap();
    assert_eq!(e.session().document, prepared);
    assert!(e.filter_edit.is_none());
    assert!(!e.pending);
    assert!(e.job.is_none());
    assert_eq!(e.session().undo_label(), Some("Content-Aware Fill"));
    e.session_mut().undo();
    assert_eq!(e.session().document, original);
}

#[test]
fn automatic_filter_buttons_and_enter_wait_for_success_and_toggles_reuse_results() {
    use quickgui::{Application, WindowOptions};
    for background in [false, true] {
        let mut e = Editor::with_test_document();
        compositor::edits::fill(
            &mut e.session_mut().document,
            [80, 120, 160, 255],
            false,
            false,
        )
        .unwrap();
        if background {
            e.open_background().unwrap();
        } else {
            e.open_filter(Filter::ContentFill).unwrap();
        }
        e.filter_edit.as_mut().unwrap().work = Work::Running;
        let original = e.session().document.clone();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Automatic preview readiness").size(1280., 900.),
                e,
            )
            .unwrap();
        let window = view.window_handle();
        assert!(cx.click(window, "form-apply").is_err());
        cx.focus(window, "workspace").unwrap();
        cx.simulate_keystrokes(window, "enter").unwrap();
        cx.update(view, |e, _| {
            assert!(e.modal.is_some());
            let edit = e.filter_edit.as_ref().unwrap();
            let (id, revision) = (edit.id, edit.revision);
            e.receive_filter_preview(id, revision, Err(invalid("Injected failure")));
        })
        .unwrap();
        assert!(cx.click(window, "form-apply").is_err());
        cx.simulate_keystrokes(window, "enter").unwrap();
        cx.read(view, |e| {
            assert!(e.modal.is_some());
            assert!(
                matches!(&e.modal, Some(Form::Edit { error, .. }) if error == "Injected failure")
            );
            assert_eq!(e.session().document, original);
        })
        .unwrap();
        cx.click(window, "filter-preview").unwrap();
        cx.update(view, |e, _| {
            let edit = e.filter_edit.as_mut().unwrap();
            // Inject completion of the retry without invoking an external model.
            edit.work = Work::Running;
            let (id, revision) = (edit.id, edit.revision);
            let mut document = original.clone();
            if background {
                compositor::edits::add_mask(&mut document, true).unwrap();
            } else {
                compositor::edits::fill(&mut document, [100, 10, 20, 255], false, false).unwrap();
            }
            e.receive_filter_preview(
                id,
                revision,
                Ok(PreviewOutput {
                    camera_scope: None,
                    document,
                    subject: None,
                }),
            );
        })
        .unwrap();
        for _ in 0..2 {
            cx.click(window, "filter-preview").unwrap();
            cx.read(view, |e| {
                let edit = e.filter_edit.as_ref().unwrap();
                assert_eq!(edit.revision, 0);
                assert!(edit.work == Work::Idle);
                assert!(edit.prepared.is_some());
                assert!(!e.automatic_filter_unavailable(if background {
                    Action::RemoveBackground
                } else {
                    Action::Filter(Filter::ContentFill)
                }));
            })
            .unwrap();
        }
        cx.click(window, "form-cancel").unwrap();
        cx.read(view, |e| {
            assert_eq!(e.session().document, original);
            assert!(e.session().undo_label().is_none());
        })
        .unwrap();
    }
}

#[test]
fn changed_background_settings_cannot_apply_an_older_successful_preview() {
    let mut e = Editor::with_test_document();
    compositor::edits::fill(
        &mut e.session_mut().document,
        [10, 20, 30, 255],
        false,
        false,
    )
    .unwrap();
    e.open_background().unwrap();
    let id = e.filter_edit.as_ref().unwrap().id;
    let original = e.session().document.clone();
    let mut document = original.clone();
    compositor::edits::add_mask(&mut document, true).unwrap();
    e.receive_filter_preview(
        id,
        0,
        Ok(PreviewOutput {
            camera_scope: None,
            document,
            subject: None,
        }),
    );
    assert!(!e.automatic_filter_unavailable(Action::RemoveBackground));
    e.background_mode = super::super::background_controls::Mode::Advanced;
    e.refresh_filter();
    assert!(e.automatic_filter_unavailable(Action::RemoveBackground));
    assert!(
        e.apply_form(
            Action::RemoveBackground,
            vec!["12".into(), "25".into(), "0".into()]
        )
        .is_err()
    );
    e.receive_filter_preview(id, 1, Err(invalid("Refinement failed")));
    assert!(e.automatic_filter_unavailable(Action::RemoveBackground));
    assert_eq!(e.session().document, original);
    assert!(e.session().undo_label().is_none());
}

#[test]
fn applying_a_filter_keeps_the_panel_and_preview_until_success_or_failure() {
    use quickgui::{Application, WindowOptions};
    for fail in [false, true] {
        let mut e = Editor::with_test_document();
        let mut original = Document::new(8, 8).unwrap();
        compositor::edits::fill(&mut original, [100, 30, 180, 255], false, false).unwrap();
        e.tabs = vec![Session::new(original.clone(), None).into()];
        let filter = Filter::Gaussian { radius: 3. };
        e.open_filter(filter).unwrap();
        let id = e.filter_edit.as_ref().unwrap().id;
        let mut preview = original.clone();
        compositor::filters::apply(&mut preview, filter, false).unwrap();
        e.receive_filter_preview(
            id,
            0,
            Ok(PreviewOutput {
                camera_scope: None,
                document: preview.clone(),
                subject: None,
            }),
        );
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Filter commit").size(1280., 900.), e)
            .unwrap();
        let window = view.window_handle();
        let job = cx
            .update(view, |e, cx| {
                e.submit_form(cx);
                e.job.take().unwrap()
            })
            .unwrap();
        cx.read(view, |e| {
            assert!(e.pending);
            assert!(e.modal.is_some());
            assert_eq!(e.session().document, preview);
            assert_eq!(e.session().committed_document(), &original);
        })
        .unwrap();
        assert!(cx.click(window, "form-cancel").is_err());
        assert!(cx.click(window, "form-apply").is_err());
        cx.focus(window, "workspace").unwrap();
        cx.simulate_keystrokes(window, "escape").unwrap();
        cx.read(view, |e| {
            assert!(e.modal.is_some());
            assert!(e.filter_edit.is_some());
        })
        .unwrap();
        let completion = job.completion();
        let result = if fail {
            Err(invalid("Injected commit failure"))
        } else {
            job.run(original.clone())
        };
        cx.update(view, |e, _| {
            // A preview worker finishing during Apply cannot replace the displayed preview.
            e.receive_filter_preview(id, 0, Err(invalid("Late preview failure")));
            assert_eq!(e.session().document, preview);
            let result = e.complete_job(e.session().id, original.clone(), result, completion);
            assert_eq!(result.is_err(), fail);
            assert!(!e.pending);
            assert!(e.modal.is_none());
            assert!(e.filter_edit.is_none());
            if fail {
                assert_eq!(e.session().document, original);
                assert!(e.session().undo_label().is_none());
            } else {
                assert_eq!(e.session().document, preview);
                assert_eq!(e.session().undo_label(), Some("Gaussian Blur"));
                e.session_mut().undo();
                assert_eq!(e.session().document, original);
            }
        })
        .unwrap();
    }
}

#[test]
fn filter_preview_failures_stay_inline_and_preserve_the_edit() {
    let mut editor = Editor::with_test_document();
    let mut original = Document::new(4, 4).unwrap();
    compositor::edits::fill(&mut original, [50, 120, 200, 255], false, false).unwrap();
    editor.tabs = vec![Session::new(original.clone(), None).into()];
    editor.open_filter(Filter::Gaussian { radius: 1. }).unwrap();
    let edit = editor.filter_edit.as_mut().unwrap();
    edit.work = Work::Running;
    let (id, revision) = (edit.id, edit.revision);
    editor.receive_filter_preview(id, revision, Err(invalid("Filter preview failed")));
    assert!(editor.errors.is_empty());
    assert!(
        matches!(&editor.modal, Some(Form::Edit { error, .. }) if error == "Filter preview failed")
    );
    assert!(editor.filter_edit.is_some());
    assert_eq!(editor.session().document, original);
    editor.cancel_filter();
    assert_eq!(editor.session().document, original);
    assert!(editor.session().undo_label().is_none());
}
#[test]
fn mask_metadata_from_history_survives_delayed_preview_refresh_and_cancel() {
    let mut e = Editor::with_test_document();
    let mut doc = Document::new(8, 8).unwrap();
    compositor::edits::fill(&mut doc, [80, 120, 160, 255], false, false).unwrap();
    doc.layers[0].mask = Some(compositor::document::Mask {
        pixels: Arc::new(image::GrayImage::from_pixel(8, 8, image::Luma([255]))),
        enabled: true,
        linked: true,
        placement: Some(compositor::geometry::Transform::new(4, 4)),
    });
    e.tabs = vec![Session::new(doc.clone(), None).into()];
    e.session_mut()
        .edit("Mask settings", |doc| {
            let mask = doc.layers[0].mask.as_mut().unwrap();
            mask.enabled = false;
            mask.linked = false;
            mask.placement = None;
            Ok(())
        })
        .unwrap();
    let changed = e.session().document.clone();
    let filter = Filter::Gaussian { radius: 1. };
    e.open_filter(filter).unwrap();
    let id = e.filter_edit.as_ref().unwrap().id;
    let mut prepared = changed.clone();
    compositor::filters::apply(&mut prepared, filter, false).unwrap();
    e.undo_document();
    e.receive_filter_preview(
        id,
        0,
        Ok(PreviewOutput {
            camera_scope: None,
            document: prepared.clone(),
            subject: None,
        }),
    );
    assert_eq!(e.session().document.layers[0].mask, doc.layers[0].mask);
    e.redo_document();
    let mask = e.session().document.layers[0].mask.as_ref().unwrap();
    assert!(!mask.enabled);
    assert!(!mask.linked);
    assert_eq!(
        mask.placement,
        prepared.layers[0].mask.as_ref().unwrap().placement
    );
    e.undo_document();
    e.cancel_filter();
    assert_eq!(e.session().document, doc);
    assert_eq!(e.session().redo_label(), Some("Mask settings"));
}

#[test]
fn invert_keeps_filter_preview_and_disabled_preview_uses_the_new_baseline() {
    use quickgui::{Application, WindowOptions};
    for apply in [false, true] {
        let mut e = Editor::with_test_document();
        let mut doc = Document::new(4, 4).unwrap();
        compositor::edits::fill(&mut doc, [50, 120, 200, 255], false, false).unwrap();
        e.tabs = vec![Session::new(doc.clone(), None).into()];
        e.open_filter(Filter::Gaussian { radius: 1. }).unwrap();
        let id = e.filter_edit.as_ref().unwrap().id;
        let mut preview = doc.clone();
        compositor::filters::apply(&mut preview, Filter::Gaussian { radius: 1. }, false).unwrap();
        e.receive_filter_preview(
            id,
            0,
            Ok(PreviewOutput {
                camera_scope: None,
                document: preview.clone(),
                subject: None,
            }),
        );
        e.invert().unwrap();
        let inverted = e.session().committed_document().clone();
        assert_ne!(inverted, doc);
        assert_eq!(e.session().document, preview);
        assert!(!e.filter_source_is_current());
        // A completed worker can still display the old-source preview, but cannot alter history.
        e.receive_filter_preview(
            id,
            0,
            Ok(PreviewOutput {
                camera_scope: None,
                document: preview.clone(),
                subject: None,
            }),
        );
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Invert beneath filter").size(1500., 1000.),
                e,
            )
            .unwrap();
        cx.click(view.window_handle(), "filter-preview").unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            inverted
        );
        cx.update(view, |e, cx| {
            if apply {
                e.submit_form(cx);
            } else {
                e.cancel_filter();
                e.modal = None;
            }
            assert_eq!(e.session().document, inverted);
            assert!(e.filter_edit.is_none());
            assert!(!e.pending);
            assert_eq!(e.session().undo_label(), Some("Invert"));
            e.session_mut().undo();
            assert_eq!(e.session().document, doc);
        })
        .unwrap();
    }
}

#[test]
fn background_quality_controls_validate_and_cancel_without_modifying_pixels() {
    use quickgui::{Application, WindowOptions};
    let mut editor = Editor::with_test_document();
    editor.tabs = vec![Session::new(Document::new(4, 4).unwrap(), None).into()];
    compositor::edits::fill(
        &mut editor.session_mut().document,
        [100, 50, 25, 255],
        false,
        false,
    )
    .unwrap();
    let original = editor.session().document.clone();
    editor.open_background().unwrap();
    // Exercise controls without launching an external inference process in unit tests.
    editor.filter_edit.as_mut().unwrap().enabled = false;
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Background").size(1280., 900.), editor)
        .unwrap();
    let window = view.window_handle();
    assert!(cx.element_bounds(window, 50_000_u64).is_err());
    cx.click(window, "background-advanced").unwrap();
    assert!(cx.element_bounds(window, 50_000_u64).is_ok());
    assert!(
        cx.read(view, |e| matches!(
            e.filter_edit.as_ref().unwrap().desired,
            Some(Settings::Background(
                compositor::background::Quality::Advanced {
                    refine_edges: 12.,
                    contrast: 25.,
                    shift_edge: 0.
                }
            ))
        ))
        .unwrap()
    );
    cx.update(view, |e, _| {
        if let Some(Form::Edit { fields, .. }) = &mut e.modal {
            fields[0].1 = "41".into();
        }
        e.refresh_filter();
    })
    .unwrap();
    assert!(
        cx.read(view, |e| e.filter_edit.as_ref().unwrap().desired.is_none())
            .unwrap()
    );
    cx.click(window, "background-basic").unwrap();
    assert!(
        cx.read(view, |e| matches!(
            e.filter_edit.as_ref().unwrap().desired,
            Some(Settings::Background(compositor::background::Quality::Basic))
        ))
        .unwrap()
    );
    cx.click(window, "form-cancel").unwrap();
    assert_eq!(
        cx.read(view, |e| e.session().document.clone()).unwrap(),
        original
    );
    assert!(
        cx.read(view, |e| e.session().undo_label().is_none())
            .unwrap()
    );
}

#[test]
fn cancelled_background_preview_cannot_restore_a_mask() {
    let mut e = Editor::with_test_document();
    e.tabs = vec![Session::new(Document::new(3, 3).unwrap(), None).into()];
    compositor::edits::fill(
        &mut e.session_mut().document,
        [100, 50, 25, 255],
        false,
        false,
    )
    .unwrap();
    let original = e.session().document.clone();
    e.open_background().unwrap();
    let id = e.filter_edit.as_ref().unwrap().id;
    let mut preview = original.clone();
    compositor::edits::add_mask(&mut preview, true).unwrap();
    e.receive_filter_preview(
        id,
        0,
        Ok(PreviewOutput {
            camera_scope: None,
            document: preview.clone(),
            subject: None,
        }),
    );
    assert_eq!(e.session().document, preview);
    e.cancel_filter();
    e.receive_filter_preview(
        id,
        0,
        Ok(PreviewOutput {
            camera_scope: None,
            document: preview,
            subject: None,
        }),
    );
    assert_eq!(e.session().document, original);
    assert!(e.session().undo_label().is_none());
}
#[test]
fn stale_and_cancelled_filter_previews_cannot_replace_current_pixels() {
    let mut editor = Editor::with_test_document();
    editor.tabs = vec![Session::new(Document::new(3, 3).unwrap(), None).into()];
    compositor::edits::fill(
        &mut editor.session_mut().document,
        [100, 50, 25, 255],
        false,
        false,
    )
    .unwrap();
    let original = editor.session().document.clone();
    editor.open_filter(Filter::Gaussian { radius: 1. }).unwrap();
    let id = editor.filter_edit.as_ref().unwrap().id;
    let mut preview = original.clone();
    compositor::filters::apply(&mut preview, Filter::Gaussian { radius: 1. }, false).unwrap();
    editor.refresh_filter();
    editor.receive_filter_preview(
        id,
        0,
        Ok(PreviewOutput {
            camera_scope: None,
            document: preview.clone(),
            subject: None,
        }),
    );
    assert_eq!(editor.session().document, original);
    editor.receive_filter_preview(
        id,
        1,
        Ok(PreviewOutput {
            camera_scope: None,
            document: preview.clone(),
            subject: None,
        }),
    );
    assert_eq!(editor.session().document, preview);
    editor.cancel_filter();
    assert_eq!(editor.session().document, original);
    editor.receive_filter_preview(
        id,
        1,
        Ok(PreviewOutput {
            camera_scope: None,
            document: preview,
            subject: None,
        }),
    );
    assert_eq!(editor.session().document, original);
    assert!(editor.session().undo_label().is_none());
}
#[test]
fn delayed_filter_result_cannot_restore_removed_target_or_old_metadata() {
    let mut e = Editor::with_test_document();
    e.session_mut()
        .edit("New Layer", |doc| {
            compositor::layer_ops::add_blank(doc)?;
            compositor::edits::fill(doc, [180, 60, 20, 255], false, false)
        })
        .unwrap();
    e.open_filter(Filter::Gaussian { radius: 1. }).unwrap();
    let edit = e.filter_edit.as_ref().unwrap();
    let id = edit.id;
    let mut source = edit.original.clone();
    compositor::filters::apply(&mut source, Filter::Gaussian { radius: 1. }, false).unwrap();
    e.undo_document();
    let undone = e.session().document.clone();
    e.receive_filter_preview(
        id,
        0,
        Ok(PreviewOutput {
            camera_scope: None,
            document: source.clone(),
            subject: None,
        }),
    );
    assert_eq!(e.session().document, undone);
    e.redo_document();
    assert_eq!(e.session().document, source);
    let target = source.active.unwrap();
    e.session_mut()
        .edit_committed("Metadata", |doc| {
            doc.active_layer_mut().unwrap().name = "Renamed".into();
            doc.selection = Some(compositor::selection::Selection::rectangle(
                doc.width,
                doc.height,
                [0., 0.],
                [1., 1.],
                false,
            ));
            compositor::layer_ops::add_blank(doc)
        })
        .unwrap();
    let baseline = e.session().committed_document().clone();
    e.receive_filter_preview(
        id,
        0,
        Ok(PreviewOutput {
            camera_scope: None,
            document: source.clone(),
            subject: None,
        }),
    );
    assert_eq!(e.session().document.active, baseline.active);
    assert_eq!(e.session().document.selection, baseline.selection);
    assert_eq!(e.session().document.layers.len(), baseline.layers.len());
    assert_eq!(e.session().document.layer(target).unwrap().name, "Renamed");
    assert_eq!(
        e.session().document.layer(target).unwrap().content,
        source.layer(target).unwrap().content
    );
    e.filter_edit.as_mut().unwrap().enabled = false;
    e.refresh_filter_document();
    assert_eq!(e.session().document, baseline);
}
#[test]
fn filter_apply_preserves_current_pixels_when_original_target_is_a_mask() {
    let mut e = Editor::with_test_document();
    let mut doc = Document::new(8, 8).unwrap();
    compositor::edits::fill(&mut doc, [80, 120, 160, 255], false, false).unwrap();
    let target = doc.active.unwrap();
    let mut mask = image::GrayImage::from_pixel(8, 8, image::Luma([0]));
    for y in 0..8 {
        for x in 4..8 {
            mask[(x, y)] = image::Luma([255]);
        }
    }
    doc.active_layer_mut().unwrap().mask = Some(compositor::document::Mask {
        pixels: Arc::new(mask),
        enabled: true,
        linked: true,
        placement: None,
    });
    let filter = Filter::Gaussian { radius: 1. };
    let mut expected = doc.clone();
    compositor::filters::apply(&mut expected, filter, true).unwrap();
    e.tabs = vec![Session::new(doc, None).into()];
    e.tools.mask_target = true;
    e.open_filter(filter).unwrap();
    e.session_mut()
        .edit_committed("Change pixels and active layer", |doc| {
            compositor::edits::fill(doc, [200, 30, 40, 255], false, false)?;
            compositor::layer_ops::add_blank(doc)
        })
        .unwrap();
    e.refresh_filter_document();
    e.tools.mask_target = false;
    let baseline = e.session().committed_document().clone();
    e.apply_form(Action::Filter(filter), vec!["1".into()])
        .unwrap();
    let result = e
        .job
        .take()
        .unwrap()
        .run(e.session().document.clone())
        .unwrap();
    assert_eq!(result.active, baseline.active);
    assert_eq!(
        result.layer(target).unwrap().content,
        baseline.layer(target).unwrap().content
    );
    assert_eq!(
        result.layer(target).unwrap().mask,
        expected.layer(target).unwrap().mask
    );
    assert_eq!(result.active_layer(), baseline.active_layer());
}
#[test]
fn filter_apply_rejects_a_target_moved_by_history() {
    let mut e = Editor::with_test_document();
    let mut doc = Document::new(8, 8).unwrap();
    compositor::edits::fill(&mut doc, [80, 120, 160, 255], false, false).unwrap();
    e.tabs = vec![Session::new(doc, None).into()];
    e.session_mut()
        .edit("Move", |doc| {
            doc.active_layer_mut().unwrap().transform.origin[0] = 3.;
            Ok(())
        })
        .unwrap();
    let filter = Filter::Gaussian { radius: 1. };
    e.open_filter(filter).unwrap();
    e.undo_document();
    let restored = e.session().committed_document().clone();
    assert!(!e.filter_source_is_current());
    e.apply_form(Action::Filter(filter), vec!["1".into()])
        .unwrap();
    assert!(!e.pending);
    assert!(e.job.is_none());
    assert!(e.filter_edit.is_none());
    assert_eq!(e.session().document, restored);
}
