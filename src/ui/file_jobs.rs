use super::project_open::OpenedProject;
use super::*;
use compositor::{document::Layer, invalid};
use uuid::Uuid;

pub(super) enum FileJob {
    Open(Vec<PathBuf>),
    Import {
        session: Uuid,
        paths: Vec<PathBuf>,
        center: Option<compositor::geometry::Point>,
    },
    Save {
        session: Uuid,
        document: Document,
        path: PathBuf,
    },
    ExportPsd {
        document: Document,
        path: PathBuf,
    },
    ExportJpeg {
        path: PathBuf,
        bytes: Vec<u8>,
        quality: u8,
    },
    Export {
        document: Document,
        path: PathBuf,
        quality: u8,
    },
}

pub(super) enum Completed {
    Opened {
        projects: Vec<OpenedProject>,
        failures: Vec<(PathBuf, compositor::Error)>,
    },
    Imported {
        session: Uuid,
        layers: Vec<Layer>,
        center: Option<compositor::geometry::Point>,
        projects: Vec<OpenedProject>,
        psds: Vec<(String, compositor::psd::Imported)>,
        raws: Vec<PathBuf>,
    },
    Saved {
        session: Uuid,
        path: PathBuf,
    },
    Exported {
        path: PathBuf,
        jpeg_quality: Option<u8>,
    },
}

impl FileJob {
    fn operation(&self) -> alerts::Operation {
        match self {
            Self::Open(_) => alerts::Operation::Open,
            Self::Import { .. } => alerts::Operation::Import,
            Self::Save { .. } => alerts::Operation::Save,
            Self::Export { path, .. } => match path
                .extension()
                .and_then(|s| s.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref()
            {
                Some("tif" | "tiff") => alerts::Operation::ExportTiff,
                Some("webp") => alerts::Operation::ExportWebp,
                _ => alerts::Operation::ExportPng,
            },
            Self::ExportPsd { .. } => alerts::Operation::ExportPsd,
            Self::ExportJpeg { .. } => alerts::Operation::ExportJpeg,
        }
    }

    fn run(self, open: &[(Uuid, PathBuf)]) -> Result<Completed> {
        match self {
            Self::ExportPsd { document, path } => {
                let bytes = compositor::psd::encode(&document)?;
                image_io::export_encoded(&path, &bytes)?;
                Ok(Completed::Exported {
                    path,
                    jpeg_quality: None,
                })
            }
            Self::ExportJpeg {
                path,
                bytes,
                quality,
            } => {
                image_io::export_encoded(&path, &bytes)?;
                Ok(Completed::Exported {
                    path,
                    jpeg_quality: Some(quality),
                })
            }
            Self::Open(paths) => {
                let mut projects = Vec::new();
                let mut failures = Vec::new();
                for path in paths {
                    match OpenedProject::load(path.clone(), open) {
                        Ok(project) => projects.push(project),
                        Err(error) => failures.push((path, error)),
                    }
                }
                Ok(Completed::Opened { projects, failures })
            }
            Self::Import {
                session,
                paths,
                center,
            } => {
                let mut layers = Vec::new();
                let mut psds = Vec::new();
                let mut raws = Vec::new();
                let mut projects = Vec::new();
                for path in paths {
                    if path.is_dir() || open.iter().any(|(_, saved)| *saved == path) {
                        projects.push(OpenedProject::load(path, open)?);
                    } else if compositor::raw::is_raw(&path) {
                        raws.push(path);
                    } else if compositor::psd::is_psd(&path)? {
                        let name = path
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned();
                        psds.push((name, compositor::psd::load(&path)?));
                    } else {
                        layers.push(image_io::import(&path)?);
                    }
                }
                Ok(Completed::Imported {
                    session,
                    layers,
                    center,
                    projects,
                    psds,
                    raws,
                })
            }
            Self::Save {
                session,
                document,
                path,
            } => {
                project::save(&document, &path)?;
                Ok(Completed::Saved {
                    session,
                    path: path.canonicalize()?,
                })
            }
            Self::Export {
                document,
                path,
                quality,
            } => {
                image_io::export(&document, &path, quality)?;
                Ok(Completed::Exported {
                    path,
                    jpeg_quality: None,
                })
            }
        }
    }
}

impl Editor {
    pub(super) fn queue_file(&mut self, job: FileJob) {
        self.status = match &job {
            FileJob::Import { .. } => "Importing images…",
            _ => "Working…",
        }
        .into();
        self.file_job = Some(job);
        self.pending = true;
    }

    pub(super) fn open_paths(&mut self, paths: Vec<PathBuf>, import: bool) -> Result<()> {
        self.finish_pending_edits()?;
        if import {
            self.tools.pending_crop = None;
        }
        if !paths.is_empty() {
            self.queue_file(if import {
                FileJob::Import {
                    session: self.tabs[self.current].id,
                    paths,
                    center: self.tabs[self.current]
                        .session()
                        .map(|s| [s.document.width as f64 / 2., s.document.height as f64 / 2.]),
                }
            } else {
                FileJob::Open(paths)
            });
        }
        Ok(())
    }

    pub(super) fn start_file_job(&mut self, cx: &ViewContext<'_, Self>) {
        let Some(job) = self.file_job.take() else {
            return;
        };
        let operation = job.operation();
        let open: Vec<_> = self
            .tabs
            .iter()
            .filter_map(|tab| {
                tab.session()
                    .and_then(|session| session.path.clone().map(|path| (tab.id, path)))
            })
            .collect();
        let launched = cx.spawn_background(move || job.run(&open), move |this, result, cx| {
            this.pending = false;
            let result = result.map_err(|e| invalid(format!("File worker failed: {e}. Your open edits are preserved. Retry the file operation.")))
                .and_then(|r| r).and_then(|completed| this.finish_file_job(completed, cx));
            if result.is_err() { this.cancel_close(); }
            this.operation_result(operation, result, cx);
        });
        if let Err(error) = launched {
            self.pending = false;
            self.cancel_close();
            self.show_error(operation, format!(
                "Could not start the file operation: {error}. Your open edits are preserved. Retry the operation."
            ));
        }
    }

    fn finish_file_job(&mut self, completed: Completed, cx: &mut EventContext) -> Result<()> {
        if let Some(report) = completed.psd_report() {
            self.psd_conversion = Some(super::psd_conversion::Conversion::Files {
                completed: Box::new(completed),
                report,
            });
            return Ok(());
        }
        self.finish_approved_file_job(completed, cx)
    }

    pub(super) fn finish_approved_file_job(
        &mut self,
        completed: Completed,
        cx: &mut EventContext,
    ) -> Result<()> {
        match completed {
            Completed::Opened { projects, failures } => {
                let opened = projects.len();
                self.show_opened_projects(projects)?;
                self.status = match failures.first() {
                    Some((path, error)) => format!(
                        "Opened {opened} of {} files. Could not open {}: {error} Existing edits are preserved. Check the failed files and try opening them again.",
                        opened + failures.len(),
                        path.display(),
                    ),
                    None => "Opened successfully.".into(),
                };
                if !failures.is_empty() {
                    self.show_error(alerts::Operation::Open, self.status.clone());
                }
            }
            Completed::Imported {
                session,
                layers,
                center,
                projects,
                psds,
                raws,
            } => {
                for path in raws {
                    self.queue_raw(
                        path,
                        raw_develop::Target::Insert {
                            tab: session,
                            center,
                        },
                    );
                }
                self.apply_psd_imports(session, psds, center)?;
                let destination =
                    self.tabs
                        .iter_mut()
                        .find(|s| s.id == session)
                        .ok_or_else(|| {
                            invalid("The destination project closed before the import completed.")
                        })?;
                if !layers.is_empty() {
                    let size = layers[0].transform.size;
                    let import = |doc: &mut Document| {
                        let center =
                            center.unwrap_or([doc.width as f64 / 2., doc.height as f64 / 2.]);
                        let parent = doc.active_layer().and_then(|layer| {
                            if layer.is_group() {
                                Some(layer.id)
                            } else {
                                layer.parent
                            }
                        });
                        for mut layer in layers {
                            layer.transform.origin = [
                                (center[0] - layer.transform.size[0] / 2.).floor(),
                                (center[1] - layer.transform.size[1] / 2.).floor(),
                            ];
                            layer.parent = parent;
                            doc.add(layer)?;
                        }
                        Ok(())
                    };
                    if let Some(session) = destination.session_mut() {
                        session.edit_committed("Import Images", import)?;
                    } else {
                        destination.edit_or_create(
                            "Import Images",
                            || {
                                let mut doc = Document::new(size[0] as u32, size[1] as u32)?;
                                doc.layers.clear();
                                doc.active = None;
                                doc.selected.clear();
                                Ok(doc)
                            },
                            import,
                        )?;
                    }
                    if let Some(destination) = destination.session_mut()
                        && let Some(parent) = destination
                            .committed_document()
                            .active_layer()
                            .and_then(|l| l.parent)
                    {
                        destination.collapsed.remove(&parent);
                    }
                    destination.parked_tools.mask_target = false;
                    if self.tabs[self.current].id == session {
                        self.tools.mask_target = false;
                        self.refresh_adjustment_document();
                        self.refresh_filter_document();
                    }
                }
                self.show_opened_projects(projects)?;
                self.status = "Imported successfully. Ctrl+Z undoes the import.".into();
            }
            Completed::Saved { session, path } => {
                let session = self
                    .tabs
                    .iter_mut()
                    .find(|s| s.id == session)
                    .and_then(ProjectTab::session_mut)
                    .ok_or_else(|| {
                        invalid(format!(
                            "Saved {} successfully, but its tab has closed.",
                            path.display()
                        ))
                    })?;
                session.mark_saved(path);
                self.status = "Project saved.".into();
                if let Some(intent) = self.close_intent.take() {
                    self.finish_close(intent, cx);
                }
            }
            Completed::Exported { path, jpeg_quality } => {
                self.status = format!(
                    "Exported {}. Editable project save is unchanged.",
                    path.display()
                );
                if let Some(quality) = jpeg_quality
                    && let Err(error) = super::jpeg_preferences::remember(quality)
                {
                    self.status
                        .push_str(&format!(" JPEG quality could not be remembered: {error}"));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_import_creation_history_restores_welcome_and_exact_image() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.png");
        image::RgbaImage::from_pixel(7, 5, image::Rgba([180, 30, 60, 255]))
            .save(&path)
            .unwrap();
        let mut e = Editor::new(Vec::new()).unwrap();
        e.open_paths(vec![path], true).unwrap();
        let completed = e.file_job.take().unwrap().run(&[]).unwrap();
        e.pending = false;
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Import creation history").size(1500., 900.),
                e,
            )
            .unwrap();
        cx.update(view, |e, cx| {
            e.finish_file_job(completed, cx).unwrap();
            let imported = e.session().document.clone();
            assert_eq!(e.tabs[0].undo_label(), Some("Import Images"));
            e.action(Action::Undo, cx);
            assert!(!e.has_document());
            e.action(Action::Redo, cx);
            assert_eq!(e.session().document, imported);
        })
        .unwrap();
    }

    #[test]
    fn failed_open_keeps_the_workspace_and_does_not_discard_other_files_in_the_batch() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("First.comp");
        let second = directory.path().join("Second.comp");
        let missing = directory.path().join("Missing.comp");
        project::save(&Document::new(3, 4).unwrap(), &first).unwrap();
        project::save(&Document::new(5, 6).unwrap(), &second).unwrap();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Open failures").size(1280., 850.),
                Editor::new(Vec::new()).unwrap(),
            )
            .unwrap();
        let failed = FileJob::Open(vec![missing.clone()]).run(&[]).unwrap();
        cx.update(view, |e, cx| {
            e.finish_file_job(failed, cx).unwrap();
            cx.invalidate();
        })
        .unwrap();
        cx.read(view, |e| {
            assert_eq!(e.tabs.len(), 1);
            assert!(!e.has_document());
            assert!(e.status.contains("Opened 0 of 1 files"));
            assert!(e.status.contains("Missing.comp"));
            assert_eq!(e.errors.front().unwrap().operation, alerts::Operation::Open);
        })
        .unwrap();
        cx.click(view.window_handle(), "error-ok").unwrap();
        let mixed = FileJob::Open(vec![first, missing, second])
            .run(&[])
            .unwrap();
        cx.update(view, |e, cx| e.finish_file_job(mixed, cx).unwrap())
            .unwrap();
        cx.read(view, |e| {
            assert_eq!(e.tabs.len(), 2);
            assert_eq!(e.tabs[0].session().unwrap().document.width, 3);
            assert_eq!(e.tabs[1].session().unwrap().document.width, 5);
            assert!(e.status.contains("Opened 2 of 3 files"));
            assert!(e.status.contains("Missing.comp"));
            assert_eq!(e.errors.len(), 1);
            assert_eq!(e.errors.front().unwrap().operation, alerts::Operation::Open);
        })
        .unwrap();
    }

    #[test]
    fn image_import_initializes_its_empty_tab_at_native_size_even_after_switching_tabs() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Image.png");
        image::RgbaImage::from_pixel(7, 3, image::Rgba([80, 100, 120, 255]))
            .save(&path)
            .unwrap();
        let mut e = Editor::new(Vec::new()).unwrap();
        let destination = e.tabs[0].id;
        let completed = FileJob::Import {
            session: destination,
            paths: vec![path],
            center: None,
        }
        .run(&[])
        .unwrap();
        e.add_empty_tab();
        let (mut cx, view) = quickgui::Application::new()
            .into_test_context(
                quickgui::WindowOptions::new("Welcome import").size(1280., 850.),
                e,
            )
            .unwrap();
        cx.update(view, |e, cx| e.finish_file_job(completed, cx).unwrap())
            .unwrap();
        cx.read(view, |e| {
            assert!(!e.has_document());
            let doc = &e.tabs[0].session().unwrap().document;
            assert_eq!((doc.width, doc.height), (7, 3));
            assert_eq!(doc.layers.len(), 1);
            assert_eq!(doc.layers[0].transform.origin, [0., 0.]);
            doc.validate().unwrap();
        })
        .unwrap();
    }
    use quickgui::{Application, DroppedFiles, Point, WindowOptions};

    #[test]
    fn dropped_images_keep_the_destination_and_canvas_position_and_projects_open_separately() {
        let directory = tempfile::tempdir().unwrap();
        let image_path = directory.path().join("Layer.png");
        image::RgbaImage::from_pixel(7, 5, image::Rgba([180, 30, 60, 255]))
            .save(&image_path)
            .unwrap();
        let project_path = directory.path().join("Opened.comp");
        let project_doc = Document::new(8, 6).unwrap();
        project::save(&project_doc, &project_path).unwrap();
        let mut e = Editor::with_test_document();
        e.tabs = vec![
            Session::new(Document::new(100, 80).unwrap(), None).into(),
            Session::new(Document::new(20, 20).unwrap(), None).into(),
        ];
        e.session_mut().group().unwrap();
        let group = e.session().document.active.unwrap();
        e.session_mut().collapsed.insert(group);
        e.session_mut().fit = false;
        e.session_mut().zoom = 2.;
        e.session_mut().pan = [13., -7.];
        let destination = e.session().id;
        let original = e.session().document.clone();
        let other = e.tabs[1].session().unwrap().document.clone();
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Drop files").size(1280., 850.), e)
            .unwrap();
        let window = view.window_handle();
        let bounds = cx.element_bounds(window, "canvas").unwrap();
        let job = cx
            .update(view, |e, _| {
                let (zoom, offset) = e.viewport(bounds.width, bounds.height);
                let position = Point::new(
                    bounds.x + (offset[0] + 35.5 * zoom) as f32,
                    bounds.y + (offset[1] + 27.5 * zoom) as f32,
                );
                e.prepare_file_drop(
                    &DroppedFiles::new([image_path, project_path]),
                    Some(destination),
                    position,
                )
                .unwrap();
                e.file_job.take().unwrap()
            })
            .unwrap();
        let completed = job.run(&[]).unwrap();
        cx.update(view, |e, cx| {
            e.activate_tab(1);
            e.pending = false;
            e.finish_file_job(completed, cx).unwrap();
        })
        .unwrap();
        cx.read(view, |e| {
            assert_eq!(e.tabs.len(), 3);
            assert_eq!(e.tabs[1].session().unwrap().document, other);
            assert_eq!(e.session().document, project_doc);
            let target = e.tabs[0].session().unwrap();
            let added = target.document.active_layer().unwrap();
            assert_eq!(added.transform.origin, [32., 25.]);
            assert_eq!(added.parent, Some(group));
            assert!(!target.collapsed.contains(&group));
        })
        .unwrap();
        cx.update(view, |e, _| e.tabs[0].session_mut().unwrap().undo())
            .unwrap();
        assert_eq!(
            cx.read(view, |e| e.tabs[0].session().unwrap().document.clone())
                .unwrap(),
            original
        );
    }

    #[test]
    fn import_centers_images_and_new_canvas_drop_creates_image_sized_tabs() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.png");
        image::RgbaImage::from_pixel(7, 5, image::Rgba([180, 30, 60, 255]))
            .save(&path)
            .unwrap();
        let mut e = Editor::with_test_document();
        e.tabs = vec![Session::new(Document::new(100, 80).unwrap(), None).into()];
        e.open_paths(vec![path.clone()], true).unwrap();
        let job = e.file_job.take().unwrap();
        let completed = job.run(&[]).unwrap();
        e.pending = false;
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Import placement").size(1280., 850.), e)
            .unwrap();
        cx.update(view, |e, cx| e.finish_file_job(completed, cx).unwrap())
            .unwrap();
        assert_eq!(
            cx.read(view, |e| e
                .session()
                .document
                .active_layer()
                .unwrap()
                .transform
                .origin)
                .unwrap(),
            [46., 37.]
        );
        let job = cx
            .update(view, |e, _| {
                e.prepare_file_drop(&DroppedFiles::new([path]), None, Point::ZERO)
                    .unwrap();
                e.file_job.take().unwrap()
            })
            .unwrap();
        let completed = job.run(&[]).unwrap();
        cx.update(view, |e, cx| {
            e.pending = false;
            e.finish_file_job(completed, cx).unwrap();
        })
        .unwrap();
        cx.read(view, |e| {
            assert_eq!(e.tabs.len(), 2);
            assert_eq!(
                (e.session().document.width, e.session().document.height),
                (7, 5)
            );
            assert_eq!(e.session().document.layers.len(), 1);
            assert_eq!(e.session().document.layers[0].transform.origin, [0., 0.]);
        })
        .unwrap();
    }

    #[test]
    fn file_workers_round_trip_and_failed_import_does_not_change_a_session() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Roundtrip.comp");
        let document = Document::new(3, 2).unwrap();
        let session = Uuid::new_v4();
        assert!(matches!(
            FileJob::Save {
                session,
                document: document.clone(),
                path: path.clone()
            }
            .run(&[])
            .unwrap(),
            Completed::Saved { .. }
        ));
        let Completed::Opened { projects, failures } = FileJob::Open(vec![path]).run(&[]).unwrap()
        else {
            panic!("Expected loaded projects");
        };
        assert!(failures.is_empty());
        let OpenedProject::Loaded {
            document: loaded, ..
        } = &projects[0]
        else {
            panic!("Expected a newly loaded project");
        };
        assert_eq!(loaded.layers, document.layers);
        assert_eq!(loaded.id, document.id);
        assert!(
            FileJob::Import {
                session,
                paths: vec![directory.path().join("missing.png")],
                center: Some([1.5, 1.]),
            }
            .run(&[])
            .is_err()
        );
    }
}

#[cfg(test)]
#[path = "preview_file_tests.rs"]
mod preview_file_tests;
