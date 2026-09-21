use super::*;
use compositor::{document::Layer, invalid};
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq)]
enum Work {
    Idle,
    Queued,
    Running,
    Applying,
}

pub(super) struct AdjustmentPreview {
    id: Uuid,
    session: Uuid,
    revision: u64,
    work: Work,
    desired: Option<compositor::adjustment::Adjustment>,
}

impl AdjustmentPreview {
    pub fn busy(&self) -> bool {
        self.desired.is_some() && matches!(self.work, Work::Queued | Work::Running)
    }

    pub(super) fn begin_commit(&mut self) {
        self.work = Work::Applying;
    }

    pub(super) fn applying(&self) -> bool {
        self.work == Work::Applying
    }

    pub fn new(session: Uuid) -> Self {
        Self {
            id: Uuid::new_v4(),
            session,
            revision: 0,
            work: Work::Idle,
            desired: None,
        }
    }

    pub fn changed(&mut self, desired: Option<compositor::adjustment::Adjustment>) {
        self.desired = desired;
        self.revision = self.revision.wrapping_add(1);
        if self.work != Work::Running {
            self.work = Work::Queued;
        }
    }
}

impl Editor {
    pub(super) fn adjustment_applying(&self) -> bool {
        self.adjustment_edit
            .as_ref()
            .and_then(|edit| edit.preview_job.as_ref())
            .is_some_and(AdjustmentPreview::applying)
    }

    pub(super) fn finish_adjustment_commit(&mut self, session: Uuid) {
        if self
            .adjustment_edit
            .as_ref()
            .and_then(|edit| edit.preview_job.as_ref())
            .is_some_and(|job| job.session == session && job.applying())
        {
            self.cancel_adjustment();
            self.finish_form();
        }
    }

    pub(super) fn start_adjustment_preview(&mut self, cx: &ViewContext<'_, Self>) {
        let Some(edit) = &mut self.adjustment_edit else {
            return;
        };
        let selection = edit.pixel_selection().cloned();
        let Some(job) = &mut edit.preview_job else {
            return;
        };
        if !edit.preview || job.work != Work::Queued {
            return;
        }
        let (id, revision) = (job.id, job.revision);
        if self.tabs[self.current].id != job.session {
            return;
        }
        let Some(settings) = job.desired.clone() else {
            return;
        };
        let mut layer = edit.original.clone();
        job.work = Work::Running;
        let launched = cx.spawn_background(
            move || -> Result<Layer> {
                compositor::pixel_adjustment::apply(&mut layer, &settings, selection.as_ref())?;
                Ok(layer)
            },
            move |this, result, cx| {
                this.receive_adjustment_preview(id, revision, result.map_err(|e| invalid(format!(
                    "Color preview worker failed: {e}. The original pixels are preserved."
                ))).and_then(|r| r));
                this.changed(cx);
            },
        );
        if let Err(e) = launched {
            self.receive_adjustment_preview(
                id,
                revision,
                Err(invalid(format!(
                    "Could not start the color preview: {e}. Change a setting to retry."
                ))),
            );
        }
    }

    fn receive_adjustment_preview(&mut self, id: Uuid, revision: u64, result: Result<Layer>) {
        let Some(edit) = &mut self.adjustment_edit else {
            return;
        };
        let Some(job) = &mut edit.preview_job else {
            return;
        };
        if job.id != id || job.applying() {
            return;
        }
        if job.revision != revision {
            job.work = Work::Queued;
            return;
        }
        job.work = Work::Idle;
        if !edit.preview || self.tabs[self.current].id != job.session {
            return;
        }
        match result {
            Ok(layer) => {
                edit.cache_pixel_preview(layer);
                self.refresh_adjustment_document();
            }
            Err(e) => {
                if edit.settings.kind == Kind::HueSaturation {
                    self.show_error(alerts::Operation::Paint, e.to_string());
                    return;
                }
                self.status = e.to_string();
                if let Some(Form::Edit { error, .. }) = self.tool_form_mut() {
                    *error = e.to_string();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_hue_and_levels_close_without_queuing_a_commit() {
        use quickgui::{Application, WindowOptions};
        for kind in [Kind::HueSaturation, Kind::Levels] {
            let mut e = Editor::with_test_document();
            let mut original = Document::new(8, 8).unwrap();
            compositor::edits::fill(&mut original, [100, 30, 180, 128], false, false).unwrap();
            e.tabs = vec![Session::new(original.clone(), None).into()];
            e.open_pixel_adjustment(kind).unwrap();
            let session = e.session().id;
            let edit = e.adjustment_edit.as_mut().unwrap();
            edit.preview_job = Some(AdjustmentPreview::new(session));
            if kind == Kind::Levels {
                edit.settings.levels.channel = compositor::adjustment::Channel::Red;
            } else {
                edit.settings.hsv_settings = Some(compositor::adjustment::HueSaturation {
                    range: compositor::adjustment::ColorRange::Reds,
                    invert_range: true,
                    ..Default::default()
                });
            }
            e.show_adjustment_fields();
            let (mut cx, view) = Application::new()
                .into_test_context(WindowOptions::new("Identity commit").size(1500., 1000.), e)
                .unwrap();
            cx.update(view, |e, cx| {
                e.submit_form(cx);
                assert!(e.job.is_none());
                assert!(!e.pending);
                assert!(e.modal.is_none());
                assert!(e.adjustment_edit.is_none());
                assert_eq!(e.session().document, original);
                assert!(Arc::ptr_eq(
                    e.session().document.layers[0].raster().unwrap(),
                    original.layers[0].raster().unwrap(),
                ));
                assert!(e.session().undo_label().is_none());
            })
            .unwrap();
        }
    }

    #[test]
    fn hue_preview_failures_alert_without_discarding_the_edit_or_reporting_stale_jobs() {
        let mut editor = Editor::with_test_document();
        let mut doc = Document::new(2, 2).unwrap();
        compositor::edits::fill(&mut doc, [80, 120, 160, 255], false, false).unwrap();
        editor.tabs = vec![Session::new(doc.clone(), None).into()];
        editor.open_pixel_adjustment(Kind::HueSaturation).unwrap();
        let mut job = AdjustmentPreview::new(editor.session().id);
        job.changed(Some(compositor::adjustment::Adjustment::new(
            Kind::HueSaturation,
        )));
        job.work = Work::Running;
        let id = job.id;
        editor.adjustment_edit.as_mut().unwrap().preview_job = Some(job);
        editor.receive_adjustment_preview(id, 0, Err(invalid("Outdated request")));
        assert!(editor.errors.is_empty());
        editor.receive_adjustment_preview(id, 1, Err(invalid("Preview worker unavailable")));
        assert_eq!(editor.errors.len(), 1);
        assert_eq!(
            editor.errors.front().unwrap().operation,
            alerts::Operation::Paint
        );
        assert_eq!(
            editor.errors.front().unwrap().message,
            "Preview worker unavailable"
        );
        assert!(editor.adjustment_edit.is_some());
        assert!(matches!(&editor.modal, Some(Form::Edit { error, .. }) if error.is_empty()));
        assert_eq!(editor.session().document, doc);
        assert_eq!(editor.session().committed_document(), &doc);
        assert!(editor.session().undo_label().is_none());
        editor.cancel_adjustment();
        editor.receive_adjustment_preview(id, 1, Err(invalid("Cancelled request")));
        assert_eq!(editor.errors.len(), 1);
        assert_eq!(editor.session().document, doc);
    }

    #[test]
    fn applying_color_adjustments_retains_preview_and_panel_until_completion() {
        use quickgui::{Application, WindowOptions};
        for (kind, index, value) in [
            (Kind::HueSaturation, 0, "90"),
            (Kind::Levels, 1, "2"),
            (Kind::Exposure, 0, "1"),
        ] {
            for fail in [false, true] {
                let mut e = Editor::with_test_document();
                let mut original = Document::new(8, 8).unwrap();
                compositor::edits::fill(&mut original, [100, 30, 180, 255], false, false).unwrap();
                e.tabs = vec![Session::new(original.clone(), None).into()];
                e.open_pixel_adjustment(kind).unwrap();
                e.update_form_field(index, value);
                let preview = e.session().document.clone();
                assert_ne!(preview, original);
                let worker = AdjustmentPreview::new(e.session().id);
                let id = worker.id;
                e.adjustment_edit.as_mut().unwrap().preview_job = Some(worker);
                let (mut cx, view) = Application::new()
                    .font(crate::UI_FONT)
                    .into_test_context(
                        WindowOptions::new("Adjustment commit").size(1500., 1000.),
                        e,
                    )
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
                assert_eq!(
                    cx.element_bounds(window, "panel-progress").is_ok(),
                    kind != Kind::HueSaturation
                );
                cx.update(view, |e, cx| {
                    e.form_key(&Key::Character("p".into()), Modifiers::ALT, cx);
                    assert!(e.adjustment_applying());
                    assert!(e.adjustment_edit.as_ref().unwrap().preview);
                    assert_eq!(e.session().document, preview);
                })
                .unwrap();
                cx.focus(window, "workspace").unwrap();
                cx.simulate_keystrokes(window, "escape").unwrap();
                let completion = job.completion();
                let result = if fail {
                    Err(invalid("Injected commit failure"))
                } else {
                    job.run(original.clone())
                };
                cx.update(view, |e, _| {
                    assert!(e.modal.is_some());
                    let revision = e
                        .adjustment_edit
                        .as_ref()
                        .unwrap()
                        .preview_job
                        .as_ref()
                        .unwrap()
                        .revision;
                    e.receive_adjustment_preview(
                        id,
                        revision,
                        Err(invalid("Late preview failure")),
                    );
                    assert!(e.errors.is_empty());
                    assert_eq!(e.session().document, preview);
                    assert_eq!(
                        e.complete_job(e.session().id, original.clone(), result, completion)
                            .is_err(),
                        fail
                    );
                    assert!(!e.pending);
                    assert!(e.modal.is_none());
                    assert!(e.adjustment_edit.is_none());
                    if fail {
                        assert_eq!(e.session().document, original);
                        assert!(e.session().undo_label().is_none());
                    } else {
                        assert_eq!(e.session().document, preview);
                        e.session_mut().undo();
                        assert_eq!(e.session().document, original);
                    }
                })
                .unwrap();
            }
        }
    }

    #[test]
    fn old_color_previews_cannot_replace_new_settings_or_cancelled_edits() {
        let mut editor = Editor::with_test_document();
        let mut doc = Document::new(2, 2).unwrap();
        compositor::edits::fill(&mut doc, [80, 80, 80, 255], false, false).unwrap();
        editor.tabs = vec![Session::new(doc.clone(), None).into()];
        editor.open_pixel_adjustment(Kind::Exposure).unwrap();
        let mut job = AdjustmentPreview::new(editor.session().id);
        let id = job.id;
        job.changed(Some(compositor::adjustment::Adjustment::new(
            Kind::Exposure,
        )));
        editor.adjustment_edit.as_mut().unwrap().preview_job = Some(job);
        let mut layer = doc.layers[0].clone();
        compositor::pixel_adjustment::apply(
            &mut layer,
            &compositor::adjustment::Adjustment {
                exposure_settings: Some(compositor::adjustment::Exposure {
                    exposure: 1.,
                    ..Default::default()
                }),
                ..compositor::adjustment::Adjustment::new(Kind::Exposure)
            },
            None,
        )
        .unwrap();
        editor.receive_adjustment_preview(id, 0, Ok(layer.clone()));
        assert_eq!(editor.session().document, doc);
        editor.receive_adjustment_preview(id, 1, Ok(layer.clone()));
        assert_eq!(editor.session().document.layers[0], layer);
        editor.cancel_adjustment();
        editor.receive_adjustment_preview(id, 1, Ok(layer));
        assert_eq!(editor.session().document, doc);
    }

    #[test]
    fn large_pixel_adjustments_queue_from_original_and_reject_invalid_fields() {
        let mut editor = Editor::with_test_document();
        let mut doc = Document::new(1000, 1000).unwrap();
        compositor::edits::fill(&mut doc, [80, 80, 80, 255], false, false).unwrap();
        editor.tabs = vec![Session::new(doc.clone(), None).into()];
        editor.open_pixel_adjustment(Kind::Exposure).unwrap();
        assert!(
            editor
                .adjustment_edit
                .as_ref()
                .unwrap()
                .preview_job
                .is_some()
        );
        if let Some(Form::Edit { fields, .. }) = &mut editor.modal {
            fields[0].1 = "1".into();
        }
        editor.preview_adjustment().unwrap();
        assert_eq!(editor.session().document, doc);
        if let Some(Form::Edit { fields, .. }) = &mut editor.modal {
            fields[0].1 = "invalid".into();
        }
        assert!(editor.preview_adjustment().is_err());
        assert!(
            !editor
                .adjustment_edit
                .as_ref()
                .unwrap()
                .preview_job
                .as_ref()
                .unwrap()
                .busy()
        );
        if let Some(Form::Edit { fields, .. }) = &mut editor.modal {
            fields[0].1 = "2".into();
        }
        editor.adjustment_edit.as_mut().unwrap().preview = false;
        editor.finish_adjustment().unwrap();
        assert!(editor.pending);
        assert!(editor.adjustment_applying());
        assert_eq!(editor.session().document, doc);
        assert!(matches!(editor.job, Some(jobs::Job::AdjustColors { .. })));
        assert!(editor.session().undo_label().is_none());
    }
    #[test]
    fn apply_after_history_uses_original_layer_selection_and_mapping() {
        let mut e = Editor::with_test_document();
        let mut doc = Document::new(8, 8).unwrap();
        compositor::edits::fill(&mut doc, [80, 80, 80, 255], false, false).unwrap();
        let first = doc.active.unwrap();
        compositor::layer_ops::add_blank(&mut doc).unwrap();
        compositor::edits::fill(&mut doc, [100, 100, 100, 255], false, false).unwrap();
        let target = doc.active.unwrap();
        doc.select(first, false);
        e.tabs = vec![Session::new(doc, None).into()];
        e.session_mut()
            .edit("Select target", |doc| {
                doc.select(target, false);
                doc.selection = Some(compositor::selection::Selection::rectangle(
                    8,
                    8,
                    [0., 0.],
                    [4., 8.],
                    false,
                ));
                Ok(())
            })
            .unwrap();
        let source = e.session().document.clone();
        e.open_pixel_adjustment(Kind::HueSaturation).unwrap();
        e.adjustment_edit.as_mut().unwrap().preview_job =
            Some(AdjustmentPreview::new(e.session().id));
        if let Some(Form::Edit { fields, .. }) = &mut e.modal {
            fields[2].1 = "50".into();
        }
        e.preview_adjustment().unwrap();
        let settings = e.adjustment_edit.as_ref().unwrap().settings.clone();
        let mut expected = source.layer(target).unwrap().clone();
        compositor::pixel_adjustment::apply(&mut expected, &settings, source.selection.as_ref())
            .unwrap();
        e.undo_document();
        let baseline = e.session().committed_document().clone();
        assert_eq!(baseline.active, Some(first));
        assert!(baseline.selection.is_none());
        e.finish_adjustment().unwrap();
        let result = e
            .job
            .take()
            .unwrap()
            .run(e.session().document.clone())
            .unwrap();
        assert_eq!(result.active, baseline.active);
        assert_eq!(result.selection, baseline.selection);
        assert_eq!(result.layer(first), baseline.layer(first));
        assert_eq!(result.layer(target), Some(&expected));
        let pixels = result.layer(target).unwrap().raster().unwrap();
        assert_ne!(pixels[(0, 0)], image::Rgba([100, 100, 100, 255]));
        assert_eq!(pixels[(7, 0)], image::Rgba([100, 100, 100, 255]));
    }

    #[test]
    fn delayed_adjustment_cannot_resurrect_a_target_removed_by_undo() {
        let mut e = Editor::with_test_document();
        e.session_mut()
            .edit("New Layer", |doc| {
                compositor::layer_ops::add_blank(doc)?;
                compositor::edits::fill(doc, [100, 100, 100, 255], false, false)
            })
            .unwrap();
        e.open_pixel_adjustment(Kind::HueSaturation).unwrap();
        let job = AdjustmentPreview::new(e.session().id);
        let id = job.id;
        let prepared = e.adjustment_edit.as_ref().unwrap().original.clone();
        e.adjustment_edit.as_mut().unwrap().preview_job = Some(job);
        e.undo_document();
        let undone = e.session().document.clone();
        e.receive_adjustment_preview(id, 0, Ok(prepared.clone()));
        assert_eq!(e.session().document, undone);
        e.redo_document();
        assert_eq!(e.session().document.layer(prepared.id), Some(&prepared));
    }
}
