use super::file_dialogs::{SaveDialog, file_filter};
use super::*;
use quickgui::{AsyncViewContext, PathPromptOptions, PlatformError, PlatformResponse};

impl Editor {
    pub(super) fn file_action(&mut self, action: Action, cx: &mut EventContext) {
        let operation = alerts::Operation::for_action(action);
        match action {
            Action::Open | Action::OpenPsd | Action::OpenRaw | Action::Import => {
                let options = if matches!(action, Action::Open) {
                    PathPromptOptions::new()
                        .files(false)
                        .directories(true)
                        .title("Open Project")
                } else if matches!(action, Action::OpenRaw) {
                    PathPromptOptions::new()
                        .multiple(true)
                        .title("Open Nikon RAW")
                        .filters([file_filter("Nikon RAW", &["nef", "nrw"])])
                } else if matches!(action, Action::OpenPsd) {
                    PathPromptOptions::new()
                        .title("Open Photoshop Document")
                        .filters([file_filter("Photoshop document", &["psd"])])
                } else {
                    PathPromptOptions::new()
                        .multiple(true)
                        .title("Import Images")
                        .filters([file_filter(
                            "Images",
                            &[
                                "jpg", "jpeg", "png", "heic", "heif", "tif", "tiff", "webp", "psd",
                                "nef", "nrw",
                            ],
                        )])
                };
                match cx.prompt_for_paths(options) {
                    Ok(response) => self.await_response(cx, operation, response, move |this, result, cx| {
                        match result {
                            Ok(Some(paths)) => {
                                let result = this.open_paths(paths, matches!(action, Action::Import));
                                this.operation_result(operation, result, cx);
                            },
                            Ok(None) => this.status = "Open cancelled.".into(),
                            Err(error) => this.show_error(operation, format!("File dialog failed: {error}. You can also drop a file onto the canvas.")),
                        }
                    }),
                    Err(error) => self.show_error(operation, format!("Could not open a file dialog: {error}. Drop files onto the canvas instead.")),
                }
            }
            Action::ExportPsd => self.prepare_psd_export(),
            Action::ExportTiff | Action::ExportWebp => self.prompt_raster_export(action, cx),
            Action::Save if self.session().path.is_some() => {
                if let Some(path) = self.session().path.clone() {
                    self.save_to(path, cx);
                }
            }
            Action::Save | Action::SaveAs | Action::ExportPng => {
                let export = matches!(action, Action::ExportPng);
                let dialog = if export {
                    SaveDialog::Png
                } else if matches!(action, Action::SaveAs) {
                    SaveDialog::ProjectAs
                } else {
                    SaveDialog::Project
                };
                let options = dialog.options(self.session().path.as_deref());
                match cx.prompt_for_new_path(options) {
                    Ok(response) => {
                        self.await_response(cx, operation, response, move |this, result, cx| {
                            match result {
                                Ok(Some(path)) => {
                                    if export {
                                        let result = this.export_png_to(path);
                                        this.operation_result(operation, result, cx);
                                    } else {
                                        match dialog.destination(path) {
                                            Ok(path) => this.save_to(path, cx),
                                            Err(error) => {
                                                this.cancel_close();
                                                this.operation_result(operation, Err(error), cx);
                                            }
                                        }
                                    }
                                }
                                Ok(None) => {
                                    if !export {
                                        this.cancel_close();
                                    }
                                    this.status = if export {
                                        "PNG export cancelled. Your edits are still open."
                                    } else {
                                        "Save cancelled. Your edits are still open."
                                    }
                                    .into();
                                }
                                Err(error) => {
                                    this.cancel_close();
                                    let name = if export { "PNG export" } else { "Save" };
                                    this.show_error(operation, format!(
                                    "{name} dialog failed: {error}. Your edits are still open."
                                ));
                                }
                            }
                        })
                    }
                    Err(error) => {
                        self.cancel_close();
                        let name = if export { "PNG export" } else { "save" };
                        self.show_error(operation, format!(
                            "Could not open the {name} dialog: {error}. Your edits are still open."
                        ));
                    }
                }
            }
            _ => {}
        }
        cx.invalidate();
    }

    pub(super) fn prompt_jpeg_path(&mut self, bytes: Vec<u8>, quality: u8, cx: &mut EventContext) {
        let operation = alerts::Operation::ExportJpeg;
        let options = SaveDialog::Jpeg.options(self.session().path.as_deref());
        match cx.prompt_for_new_path(options) {
            Ok(response) => self.await_response(cx, operation, response, move |this, result, _| match result {
                Ok(Some(path)) => {
                    match SaveDialog::Jpeg.destination(path) {
                        Ok(path) => this.queue_file(super::file_jobs::FileJob::ExportJpeg { path, bytes, quality }),
                        Err(error) => this.show_error(operation, error.to_string()),
                    }
                }
                Ok(None) => this.status = "JPEG export cancelled. Your edits are still open.".into(),
                Err(error) => this.show_error(operation, format!("JPEG export dialog failed: {error}. Your edits are still open. Choose Export JPEG to retry.")),
            }),
            Err(error) => self.show_error(operation, format!("Could not open the JPEG export dialog: {error}. Your edits are still open. Choose Export JPEG to retry.")),
        }
    }

    fn prompt_raster_export(&mut self, action: Action, cx: &mut EventContext) {
        let (dialog, name) = match action {
            Action::ExportTiff => (SaveDialog::Tiff, "TIFF"),
            Action::ExportWebp => (SaveDialog::Webp, "WebP"),
            _ => return,
        };
        let operation = alerts::Operation::for_action(action);
        let options = dialog.options(self.session().path.as_deref());
        let document = self.session().committed_document().clone();
        match cx.prompt_for_new_path(options) {
            Ok(response) => self.await_response(cx, operation, response, move |this, result, _| match result {
                Ok(Some(path)) => match dialog.destination(path) {
                    Ok(path) => this.queue_file(super::file_jobs::FileJob::Export { document, path, quality: 100 }),
                    Err(error) => this.show_error(operation, error.to_string()),
                },
                Ok(None) => this.status = format!("{name} export cancelled. Your edits are still open."),
                Err(error) => this.show_error(operation, format!("{name} export dialog failed: {error}. Your edits are still open. Choose Export {name} to retry.")),
            }),
            Err(error) => self.show_error(operation, format!("Could not open the {name} export dialog: {error}. Your edits are still open. Choose Export {name} to retry.")),
        }
    }

    pub(super) fn prompt_psd_path(&mut self, document: Document, cx: &mut EventContext) {
        let operation = alerts::Operation::ExportPsd;
        let options = SaveDialog::Psd.options(self.session().path.as_deref());
        match cx.prompt_for_new_path(options) {
            Ok(response) => {
                self.await_response(
                    cx,
                    operation,
                    response,
                    move |this, result, _| match result {
                        Ok(Some(path)) => match SaveDialog::Psd.destination(path) {
                            Ok(path) => this.queue_file(super::file_jobs::FileJob::ExportPsd {
                                document,
                                path,
                            }),
                            Err(error) => this.show_error(operation, error.to_string()),
                        },
                        Ok(None) => {
                            this.status = "PSD export cancelled. Your edits are still open.".into()
                        }
                        Err(error) => this.show_error(
                            operation,
                            format!(
                                "PSD export dialog failed: {error}. Your edits are still open."
                            ),
                        ),
                    },
                )
            }
            Err(error) => self.show_error(
                operation,
                format!("Could not open PSD export dialog: {error}. Your edits are still open."),
            ),
        }
    }

    pub(super) fn export_png_to(&mut self, path: PathBuf) -> Result<()> {
        let path = SaveDialog::Png.destination(path)?;
        self.queue_file(super::file_jobs::FileJob::Export {
            document: self.session().committed_document().clone(),
            path,
            quality: 85,
        });
        Ok(())
    }

    fn save_to(&mut self, path: PathBuf, cx: &mut EventContext) {
        self.queue_file(super::file_jobs::FileJob::Save {
            session: self.session().id,
            document: self.session().committed_document().clone(),
            path,
        });
        self.changed(cx);
    }

    pub(super) fn await_response<T: 'static>(
        &mut self,
        cx: &mut EventContext,
        operation: alerts::Operation,
        response: PlatformResponse<T>,
        apply: impl FnOnce(&mut Self, std::result::Result<T, PlatformError>, &mut EventContext)
        + 'static,
    ) {
        self.pending = true;
        self.status = "Working…".into();
        match cx.spawn(|async_cx: AsyncViewContext<Self>| async move {
            let result = response.await;
            // A closed window makes update fail; there is then no live view to update.
            let _ = async_cx
                .update(move |this, cx| {
                    this.pending = false;
                    apply(this, result, cx);
                    this.changed(cx);
                })
                .await;
        }) {
            Ok(task) => task.detach(),
            Err(error) => {
                self.pending = false;
                self.cancel_close();
                self.show_error(
                    operation,
                    format!("Could not wait for the file dialog: {error}. Retry the operation."),
                );
            }
        }
    }
}
