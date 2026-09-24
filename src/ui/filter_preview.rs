use super::*;
use compositor::{filters::Filter, invalid};
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq)]
enum Work {
    Idle,
    Queued,
    Running,
    Applying,
}

#[derive(Clone, PartialEq)]
enum Settings {
    Pixels(Filter),
    CameraRaw(Arc<compositor::camera_raw::Settings>),
    Background(compositor::background::Quality),
}

impl Settings {
    fn automatic(&self) -> bool {
        matches!(
            self,
            Self::Pixels(Filter::ContentFill) | Self::Background(_)
        )
    }
}

struct Prepared {
    layer: compositor::document::Layer,
    settings: Settings,
}

struct PreviewOutput {
    document: Document,
    subject: Option<compositor::background::SubjectMask>,
}

pub(super) struct FilterEdit {
    id: Uuid,
    session: Uuid,
    original: Document,
    prepared: Option<Prepared>,
    desired: Option<Settings>,
    subject: Option<compositor::background::SubjectMask>,
    revision: u64,
    work: Work,
    enabled: bool,
    mask: bool,
}

impl Editor {
    pub(super) fn open_camera_raw(&mut self) -> Result<()> {
        self.camera_raw = Default::default();
        self.begin_filter(Settings::CameraRaw(Arc::new(
            self.camera_raw.settings.clone(),
        )))
    }

    pub(super) fn open_filter(&mut self, filter: Filter) -> Result<()> {
        self.begin_filter(Settings::Pixels(filter))
    }

    pub(super) fn open_background(&mut self) -> Result<()> {
        self.background_mode = super::background_controls::Mode::Basic;
        self.begin_filter(Settings::Background(compositor::background::Quality::Basic))
    }

    pub(super) fn background_subject(
        &self,
        quality: compositor::background::Quality,
    ) -> Result<Option<compositor::background::SubjectMask>> {
        let edit = self.filter_edit.as_ref().filter(|edit| {
            edit.work == Work::Idle && edit.prepared.as_ref().is_some_and(|prepared| {
                prepared.settings == Settings::Background(quality)
            })
        }).ok_or_else(|| invalid("Wait for a successful preview of the current background settings before applying."))?;
        Ok(edit.subject.clone())
    }

    pub(super) fn filter_applying(&self) -> bool {
        self.filter_edit
            .as_ref()
            .is_some_and(|edit| edit.work == Work::Applying)
    }

    pub(super) fn begin_filter_commit(&mut self) {
        if let Some(edit) = &mut self.filter_edit {
            edit.work = Work::Applying;
        }
    }

    pub(super) fn finish_filter_commit(&mut self, session: Uuid) {
        if self
            .filter_edit
            .as_ref()
            .is_some_and(|edit| edit.session == session && edit.work == Work::Applying)
        {
            self.cancel_filter();
            self.finish_form();
        }
    }

    pub(super) fn filter_busy(&self) -> bool {
        self.filter_edit.as_ref().is_some_and(|edit| {
            edit.work == Work::Running
                || (edit.enabled && edit.work == Work::Queued && edit.desired.is_some())
        })
    }

    pub(super) fn automatic_filter_unavailable(&self, action: Action) -> bool {
        matches!(
            action,
            Action::Filter(Filter::ContentFill) | Action::RemoveBackground
        ) && self.filter_source_is_current()
            && !self.filter_edit.as_ref().is_some_and(|edit| {
                edit.work == Work::Idle
                    && edit
                        .prepared
                        .as_ref()
                        .is_some_and(|prepared| Some(&prepared.settings) == edit.desired.as_ref())
            })
    }

    pub(super) fn commit_content_fill(&mut self) -> Result<()> {
        if self.automatic_filter_unavailable(Action::Filter(Filter::ContentFill)) {
            return Err(invalid(
                "Wait for a successful preview before applying. If the preview failed, turn Preview off and on to retry. Your original pixels are preserved.",
            ));
        }
        let edit = self
            .filter_edit
            .as_ref()
            .ok_or_else(|| invalid("Reopen Content-Aware Fill before applying."))?;
        let original = edit
            .original
            .active_layer()
            .cloned()
            .ok_or_else(|| invalid("The layer being filled is missing."))?;
        let prepared = edit
            .prepared
            .as_ref()
            .ok_or_else(|| invalid("Wait for the fill preview to finish."))?
            .layer
            .clone();
        self.cancel_filter();
        self.session_mut().edit("Content-Aware Fill", |document| {
            super::layer_preview::overlay(document, &original, &prepared, false);
            Ok(())
        })
    }

    fn toggle_filter_preview(&mut self) {
        let Some(edit) = &mut self.filter_edit else {
            return;
        };
        edit.enabled = !edit.enabled;
        // A toggle changes visibility, not the worker's settings or revision.
        // Retain completed automatic results, including a worker finishing while hidden.
        if edit.enabled && edit.work == Work::Idle && edit.prepared.is_none() {
            edit.work = Work::Queued;
            if let Some(Form::Edit { error, .. }) = &mut self.modal {
                error.clear();
            }
        }
        self.refresh_filter_document();
    }

    fn begin_filter(&mut self, settings: Settings) -> Result<()> {
        let layer = self
            .session()
            .document
            .active_layer()
            .ok_or_else(|| invalid("Select a layer to filter."))?;
        if self.tools.mask_target {
            if layer.mask.is_none()
                || !matches!(settings, Settings::Pixels(Filter::Gaussian { .. }))
            {
                return Err(invalid(
                    "Select an existing mask for Gaussian Blur, or switch to layer pixels for other filters.",
                ));
            }
        } else if layer.raster().is_none()
            && !(matches!(settings, Settings::Pixels(Filter::Vignette(_)))
                && matches!(
                    layer.content,
                    compositor::document::LayerContent::Raster(None)
                ))
        {
            return Err(invalid("Select a layer containing pixels to filter."));
        }
        self.session_mut().begin("Filter")?;
        self.filter_edit = Some(FilterEdit {
            id: Uuid::new_v4(),
            session: self.session().id,
            original: self.session().document.clone(),
            prepared: None,
            desired: Some(settings.clone()),
            subject: None,
            revision: 0,
            work: Work::Queued,
            enabled: true,
            mask: self.tools.mask_target,
        });
        self.open_form(match settings {
            Settings::Pixels(filter) => Action::Filter(filter),
            Settings::CameraRaw(_) => Action::CameraRaw,
            Settings::Background(_) => Action::RemoveBackground,
        });
        Ok(())
    }

    pub(super) fn filter_source_is_current(&self) -> bool {
        if !self.has_document() {
            return false;
        }
        self.filter_edit.as_ref().is_none_or(|edit| {
            let Some(original) = edit.original.active_layer() else {
                return false;
            };
            let Some(current) = self
                .session()
                .committed_document()
                .layers
                .iter()
                .find(|layer| layer.id == original.id)
            else {
                return false;
            };
            if current.transform != original.transform {
                return false;
            }
            if edit.mask {
                current
                    .mask
                    .as_ref()
                    .zip(original.mask.as_ref())
                    .is_some_and(|(current, original)| {
                        Arc::ptr_eq(&current.pixels, &original.pixels)
                    })
            } else {
                match (current.raster(), original.raster()) {
                    (Some(current), Some(original)) => Arc::ptr_eq(current, original),
                    (None, None) => true,
                    _ => false,
                }
            }
        })
    }

    pub(super) fn filter_preview_enabled(&self) -> bool {
        self.filter_edit.as_ref().is_some_and(|edit| edit.enabled)
    }

    pub(super) fn filter_source(&self) -> Result<(Box<Document>, bool)> {
        self.filter_edit
            .as_ref()
            .map(|edit| (Box::new(edit.original.clone()), edit.mask))
            .ok_or_else(|| invalid("The filter was closed. Reopen it to apply the settings."))
    }

    pub(super) fn refresh_filter_document(&mut self) {
        let Some(edit) = &self.filter_edit else {
            return;
        };
        let Some(session) = self
            .tabs
            .iter_mut()
            .find(|tab| tab.id == edit.session)
            .and_then(ProjectTab::session_mut)
        else {
            return;
        };
        let mut document = session.committed_document().clone();
        if edit.enabled
            && let Some(original) = edit.original.active_layer()
            && let Some(prepared) = &edit.prepared
        {
            if matches!(edit.desired, Some(Settings::Background(_))) {
                super::layer_preview::background_mask(&mut document, &prepared.layer);
            } else {
                super::layer_preview::overlay(&mut document, original, &prepared.layer, edit.mask);
            }
        }
        session.document = document;
    }

    pub(super) fn cancel_filter(&mut self) {
        if let Some(edit) = self.filter_edit.take()
            && let Some(session) = self
                .tabs
                .iter_mut()
                .find(|s| s.id == edit.session)
                .and_then(ProjectTab::history_session_mut)
        {
            session.cancel();
        }
    }

    pub(super) fn refresh_filter(&mut self) {
        let Some(Form::Edit { action, fields, .. }) = &self.modal else {
            return;
        };
        let values = fields.iter().map(|(_, v)| v.clone()).collect::<Vec<_>>();
        let result = match action {
            Action::Filter(filter) => Self::filter_values(*filter, &values).map(Settings::Pixels),
            Action::RemoveBackground => self.background_values(&values).map(Settings::Background),
            Action::CameraRaw => self
                .camera_raw
                .parse_fields(&values)
                .map(|settings| Settings::CameraRaw(Arc::new(settings))),
            _ => return,
        };
        let Some(edit) = &mut self.filter_edit else {
            return;
        };
        let Some(Form::Edit { error, .. }) = &mut self.modal else {
            return;
        };
        edit.revision = edit.revision.wrapping_add(1);
        match result {
            Ok(settings) => {
                edit.desired = Some(settings);
                error.clear();
            }
            Err(e) => {
                edit.desired = None;
                *error = e.to_string();
            }
        }
        if edit.work != Work::Running {
            edit.work = Work::Queued;
        }
    }

    pub(super) fn filter_preview_controls(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let Some(edit) = &self.filter_edit else {
            return div();
        };
        div().flex_row().items_center().gap(8.).child(
            Self::check_control("Preview", edit.enabled)
                .text_size(13.)
                .line_height(16.)
                .on_click(cx.listener("filter-preview", |this, cx| {
                    this.toggle_filter_preview();
                    this.changed(cx);
                })),
        )
    }

    pub(super) fn start_filter_preview(&mut self, cx: &ViewContext<'_, Self>) {
        let Some(edit) = &mut self.filter_edit else {
            return;
        };
        if !edit.enabled || edit.work != Work::Queued {
            return;
        }
        let Some(settings) = edit.desired.clone() else {
            return;
        };
        let (id, revision, mask) = (edit.id, edit.revision, edit.mask);
        let mut document = edit.original.clone();
        let mut subject = edit.subject.clone();
        edit.work = Work::Running;
        let launched = cx.spawn_background(
            move || -> Result<PreviewOutput> {
                match settings {
                    Settings::CameraRaw(settings) => {
                        compositor::camera_raw::preview(&mut document, &settings)?
                    }
                    Settings::Pixels(filter) => {
                        compositor::filters::apply(&mut document, filter, mask)?
                    }
                    Settings::Background(quality) => {
                        let detected = match subject.take() {
                            Some(cached) => cached,
                            None => compositor::background::SubjectMask::detect(&document)?,
                        };
                        detected.apply(&mut document, quality)?;
                        subject = Some(detected);
                    }
                }
                document.validate()?;
                Ok(PreviewOutput { document, subject })
            },
            move |this, result, cx| {
                let result = result
                    .map_err(|e| {
                        invalid(format!(
                            "Filter preview worker failed: {e}. The original pixels are preserved."
                        ))
                    })
                    .and_then(|r| r);
                this.receive_filter_preview(id, revision, result);
                this.changed(cx);
            },
        );
        if let Err(error) = launched {
            self.receive_filter_preview(
                id,
                revision,
                Err(invalid(format!(
                    "Could not start the filter preview: {error}. Change a setting to retry."
                ))),
            );
        }
    }

    fn receive_filter_preview(&mut self, id: Uuid, revision: u64, result: Result<PreviewOutput>) {
        let Some(edit) = &mut self.filter_edit else {
            return;
        };
        if edit.id != id || edit.work == Work::Applying {
            return;
        }
        // Segmentation depends only on original pixels, so even an outdated refinement
        // can supply the reusable model result for the latest settings.
        if let Ok(output) = &result {
            edit.subject = output.subject.clone();
        }
        if edit.revision != revision {
            edit.work = Work::Queued;
            return;
        }
        edit.work = Work::Idle;
        if !edit.enabled && !edit.desired.as_ref().is_some_and(Settings::automatic) {
            edit.prepared = None;
            return;
        }
        match result {
            Ok(output) => {
                edit.prepared = output
                    .document
                    .active_layer()
                    .cloned()
                    .zip(edit.desired.clone())
                    .map(|(layer, settings)| Prepared { layer, settings });
                if let Some(Form::Edit { error, .. }) = &mut self.modal {
                    error.clear();
                }
                self.refresh_filter_document();
            }
            Err(e) => {
                edit.prepared = None;
                self.refresh_filter_document();
                if let Some(Form::Edit { error, .. }) = self.tool_form_mut() {
                    *error = e.to_string();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
