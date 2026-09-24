use super::*;
use compositor::{background::Quality, filters::Filter, invalid};

pub(super) enum Job {
    Trim(compositor::trim::Options),
    SelectForeground(compositor::object_selection::Settings),
    DeleteLayersBaked,
    CopyLayers {
        source: Box<Document>,
        layer: uuid::Uuid,
        center: compositor::geometry::Point,
        placement: Option<layer_drag::DropPlacement>,
    },
    AdjustColors {
        settings: Box<compositor::adjustment::Adjustment>,
        original: Box<compositor::document::Layer>,
        selection: Option<compositor::selection::Selection>,
    },
    CameraRaw { settings: Box<compositor::camera_raw::Settings>, source: Box<Document> },
    Filter {
        filter: Filter,
        source: Box<Document>,
        mask: bool,
    },
    RemoveBackground {
        quality: Quality,
        subject: Option<compositor::background::SubjectMask>,
        source: Box<Document>,
    },
}

#[derive(Clone, Copy)]
pub(super) enum Completion {
    DeleteLayers,
    CopyLayers,
    Pixels(&'static str),
    BackgroundMask,
}

impl Completion {
    fn label(self, original: &Document) -> &'static str {
        match self {
            Self::DeleteLayers => compositor::clipping::deletion_label(original),
            Self::CopyLayers => "Copy Layers from Project",
            Self::Pixels(label) => label,
            Self::BackgroundMask => "Remove Background",
        }
    }
}

impl Job {
    pub(super) fn completion(&self) -> Completion {
        match self {
            Self::Trim(_) => Completion::Pixels("Trim"),
            Self::SelectForeground(settings) => Completion::Pixels(
                if matches!(
                    settings.target,
                    compositor::object_selection::Target::Subject
                ) {
                    "Select Subject"
                } else {
                    "Object Selection"
                },
            ),
            Self::DeleteLayersBaked => Completion::DeleteLayers,
            Self::CopyLayers { .. } => Completion::CopyLayers,
            Self::AdjustColors { settings, .. } => {
                Completion::Pixels(super::adjustment_layers::title(settings.kind))
            }
            Self::CameraRaw { .. } => Completion::Pixels("Camera Raw Filter"),
            Self::Filter { filter, .. } => Completion::Pixels(Editor::filter_fields(*filter).0),
            Self::RemoveBackground { .. } => Completion::BackgroundMask,
        }
    }

    pub(super) fn run(self, mut document: Document) -> Result<Document> {
        match self {
            Job::Trim(options) => compositor::trim::apply(&mut document, options)?,
            Job::SelectForeground(settings) => {
                compositor::object_selection::select(&mut document, settings)?
            }
            Job::CopyLayers {
                source,
                layer,
                center,
                placement,
            } => {
                compositor::layer_ops::copy_to_project(&source, &mut document, layer, center)?;
                if let Some(placement) = placement {
                    let (parent, position) = placement.resolve(&document)?;
                    let id = document
                        .active
                        .ok_or_else(|| invalid("The copied layer is missing."))?;
                    compositor::layer_ops::place(&mut document, id, parent, position)?;
                }
            }
            Job::DeleteLayersBaked => compositor::clipping::delete_selected(
                &mut document,
                compositor::clipping::DeleteMode::Bake,
            )?,
            Job::AdjustColors {
                settings,
                original,
                selection,
            } => {
                let mut prepared = original.as_ref().clone();
                compositor::pixel_adjustment::apply(&mut prepared, &settings, selection.as_ref())?;
                super::layer_preview::overlay(&mut document, &original, &prepared, false);
            }
            Job::CameraRaw { settings, mut source } => {
                refresh_source_mask(&mut source, &document)?;
                let original = source.active_layer().cloned().ok_or_else(|| invalid("The Camera Raw layer is missing."))?;
                compositor::camera_raw::apply(&mut source, &settings)?;
                if let Some(prepared) = source.layer(original.id) {
                    super::layer_preview::overlay(&mut document, &original, prepared, false);
                }
            }
            Job::Filter {
                filter,
                mut source,
                mask,
            } => {
                if !mask {
                    refresh_source_mask(&mut source, &document)?;
                }
                let original = source
                    .active_layer()
                    .cloned()
                    .ok_or_else(|| invalid("The layer being filtered is missing."))?;
                compositor::filters::apply(&mut source, filter, mask)?;
                if let Some(prepared) = source.layer(original.id) {
                    super::layer_preview::overlay(&mut document, &original, prepared, mask);
                }
            }
            Job::RemoveBackground {
                quality,
                subject,
                mut source,
            } => {
                refresh_source_mask(&mut source, &document)?;
                let original = source
                    .active_layer()
                    .cloned()
                    .ok_or_else(|| invalid("The background removal layer is missing."))?;
                match subject {
                    Some(subject) => subject.apply(&mut source, quality)?,
                    None => compositor::background::remove(&mut source, quality)?,
                }
                if let Some(prepared) = source.layer(original.id) {
                    super::layer_preview::background_mask(&mut document, prepared);
                }
            }
        }
        document.validate()?;
        Ok(document)
    }
}

// Image pixels and selection remain captured by the tool. Masks can change
// independently through history and must be read again when applying it.
fn refresh_source_mask(source: &mut Document, document: &Document) -> Result<()> {
    let source = source
        .active_layer_mut()
        .ok_or_else(|| invalid("The layer being processed is missing."))?;
    let current = document.layer(source.id).ok_or_else(|| {
        invalid("The layer being processed was removed. The document is unchanged.")
    })?;
    source.mask = current.mask.clone();
    Ok(())
}

impl Editor {
    pub(super) fn queue(&mut self, job: Job) {
        self.job = Some(job);
        self.pending = true;
        self.status = "Working…".into();
    }

    pub(super) fn start_job(&mut self, cx: &ViewContext<'_, Self>) {
        let Some(job) = self.job.take() else {
            return;
        };
        let tab = &self.tabs[self.current];
        let id = tab.id;
        let initial = match tab.session() {
            Some(session) => Ok(if self.panel_applying() {
                session.committed_document().clone()
            } else {
                session.document.clone()
            }),
            None => match &job {
                Job::CopyLayers { source, .. } => layer_drag::empty_copy_destination(source),
                _ => Err(invalid("Create a canvas before processing an image.")),
            },
        };
        let initial = match initial {
            Ok(document) => document,
            Err(error) => {
                self.pending = false;
                self.finish_panel_commit(id);
                self.show_error(alerts::Operation::Paint, error.to_string());
                return;
            }
        };
        let document = initial.clone();
        let completion = job.completion();
        let result = cx.spawn_background(
            move || job.run(document),
            move |this, result, cx| {
                let result = result
                    .map_err(|e| {
                        invalid(format!(
                            "Image processing worker failed: {e}. The document is unchanged."
                        ))
                    })
                    .and_then(|r| r);
                let result = this.complete_job(id, initial, result, completion);
                if result.is_ok() {
                    this.status = "Image processing complete. Ctrl+Z undoes the change.".into();
                }
                this.operation_result(alerts::Operation::Paint, result, cx);
            },
        );
        if let Err(error) = result {
            self.pending = false;
            self.finish_panel_commit(id);
            self.show_error(
                alerts::Operation::Paint,
                format!("Could not start image processing: {error}. The document is unchanged."),
            );
        }
    }

    pub(super) fn complete_job(
        &mut self,
        id: uuid::Uuid,
        initial: Document,
        result: Result<Document>,
        completion: Completion,
    ) -> Result<()> {
        self.pending = false;
        // Keep the preview visible until processing ends. Release its transaction
        // before recording the result, so Undo restores committed original pixels.
        self.finish_panel_commit(id);
        result.and_then(|document| self.finish_job(id, initial, document, completion))
    }

    fn finish_job(
        &mut self,
        id: uuid::Uuid,
        initial: Document,
        document: Document,
        completion: Completion,
    ) -> Result<()> {
        let current = self.tabs[self.current].id == id;
        let tab = self
            .tabs
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| invalid("The destination project was closed."))?;
        let same_mask = tab
            .session()
            .is_some_and(|session| session.document.active == document.active)
            && document
                .active_layer()
                .is_some_and(|layer| layer.mask.is_some());
        tab.edit_or_create(
            completion.label(&initial),
            || Ok(initial),
            |doc| {
                *doc = document;
                Ok(())
            },
        )?;
        if matches!(completion, Completion::CopyLayers)
            && let Some(session) = tab.session_mut()
            && let Some(parent) = session
                .document
                .active_layer()
                .and_then(|layer| layer.parent)
        {
            session.collapsed.remove(&parent);
        }
        let mask_target = if current {
            &mut self.tools.mask_target
        } else {
            &mut tab.parked_tools.mask_target
        };
        // Background removal selects its new mask after committing. Other jobs
        // preserve mask focus only when the same layer remains active.
        *mask_target =
            matches!(completion, Completion::BackgroundMask) || (*mask_target && same_mask);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structural_job_completion_updates_only_its_projects_editing_target_after_commit() {
        for parked in [false, true] {
            let mut e = Editor::with_test_document();
            let mut original = Document::new(8, 8).unwrap();
            compositor::edits::add_mask(&mut original, false).unwrap();
            original
                .add(compositor::document::Layer::blank("Second", 8, 8))
                .unwrap();
            compositor::edits::add_mask(&mut original, false).unwrap();
            e.tabs = vec![Session::new(original.clone(), None).into()];
            e.tools.mask_target = true;
            let id = e.tabs[0].id;
            if parked {
                e.tabs.push(Session::new(original.clone(), None).into());
                e.activate_tab(1);
                e.tools.mask_target = true;
            }
            let mut output = original.clone();
            compositor::clipping::delete_selected(
                &mut output,
                compositor::clipping::DeleteMode::Unlink,
            )
            .unwrap();
            assert_ne!(output.active, original.active);
            assert!(output.active_layer().unwrap().mask.is_some());
            let mut invalid = output.clone();
            invalid.width = 0;
            assert!(
                e.finish_job(id, original.clone(), invalid, Completion::DeleteLayers)
                    .is_err()
            );
            assert!(if parked {
                e.tabs[0].parked_tools.mask_target
            } else {
                e.tools.mask_target
            });
            e.finish_job(
                id,
                original.clone(),
                output.clone(),
                Completion::DeleteLayers,
            )
            .unwrap();
            assert!(!if parked {
                e.tabs[0].parked_tools.mask_target
            } else {
                e.tools.mask_target
            });
            assert_eq!(e.tabs[0].session().unwrap().document, output);
            if parked {
                assert!(
                    e.tools.mask_target,
                    "A background completion must not change the visible project's target"
                );
                assert_eq!(e.session().document, original);
                e.activate_tab(0);
            }
            e.undo_document();
            assert_eq!(e.session().document, original);
        }
    }

    #[test]
    fn background_completion_selects_mask_only_after_a_successful_commit() {
        let mut e = Editor::with_test_document();
        let original = Document::new(8, 8).unwrap();
        e.tabs = vec![Session::new(original.clone(), None).into()];
        let mut output = original.clone();
        compositor::edits::add_mask(&mut output, false).unwrap();
        let mut invalid = output.clone();
        invalid.width = 0;
        let id = e.session().id;
        assert!(
            e.finish_job(id, original.clone(), invalid, Completion::BackgroundMask)
                .is_err()
        );
        assert!(!e.tools.mask_target);
        assert_eq!(e.session().document, original);
        e.finish_job(
            id,
            original.clone(),
            output.clone(),
            Completion::BackgroundMask,
        )
        .unwrap();
        assert!(e.tools.mask_target);
        assert_eq!(e.session().document, output);
        assert_eq!(e.session().undo_label(), Some("Remove Background"));
        e.undo_document();
        assert_eq!(e.session().document, original);
        assert!(!e.tools.mask_target);
    }

    #[test]
    fn growing_filter_carries_the_current_mask_instead_of_its_opening_snapshot() {
        let mut source = Document::new(8, 8).unwrap();
        compositor::edits::fill(&mut source, [80, 120, 160, 255], false, false).unwrap();
        source.layers[0].mask = Some(compositor::document::Mask {
            pixels: Arc::new(image::GrayImage::from_pixel(8, 8, image::Luma([0]))),
            enabled: true,
            linked: true,
            placement: None,
        });
        let mut current = source.clone();
        let mask = current.layers[0].mask.as_mut().unwrap();
        mask.pixels = Arc::new(image::GrayImage::from_fn(8, 8, |x, _| {
            image::Luma([if x < 4 { 128 } else { 255 }])
        }));
        mask.enabled = false;
        mask.linked = false;
        let filter = Filter::Gaussian { radius: 1. };
        let mut expected = current.clone();
        compositor::filters::apply(&mut expected, filter, false).unwrap();
        let actual = Job::Filter {
            filter,
            source: Box::new(source),
            mask: false,
        }
        .run(current)
        .unwrap();
        assert_eq!(actual, expected);
    }
}
