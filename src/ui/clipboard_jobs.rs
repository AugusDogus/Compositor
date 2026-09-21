use super::*;
use compositor::invalid;

pub(super) enum Request {
    Open,
    Paste(uuid::Uuid),
    CanvasSize(uuid::Uuid),
}

impl Request {
    fn operation(&self) -> alerts::Operation {
        match self {
            Self::Open => alerts::Operation::Clipboard,
            Self::Paste(_) => alerts::Operation::Paint,
            Self::CanvasSize(_) => alerts::Operation::Clipboard,
        }
    }
}

pub(super) struct Job {
    request: Request,
    source: Source,
}

// Headless QuickGUI contexts have an in-memory clipboard. Native windows read
// encoded data directly because QuickGUI's image adapter strips ICC and EXIF.
enum Source {
    #[cfg(not(test))]
    Native(Option<compositor::native_clipboard::wayland::WaylandClipboard>),
    #[cfg(test)]
    Encoded(Vec<Vec<u8>>),
}

enum Completed {
    Open(image::RgbaImage),
    Paste(uuid::Uuid, image::RgbaImage),
    CanvasSize(uuid::Uuid, Result<Option<[u32; 2]>>),
}

impl Job {
    fn run(self) -> Result<Completed> {
        let bytes = match self.source {
            #[cfg(not(test))]
            Source::Native(clipboard) => match clipboard {
                Some(clipboard) => clipboard.read_image(),
                None => compositor::native_clipboard::read_image(),
            }
            .map(|image| image.into_iter().collect()),
            #[cfg(test)]
            Source::Encoded(bytes) => Ok(bytes),
        };
        match self.request {
            Request::Open => {
                let pixels = decode_first(bytes?, image_io::read_encoded)?.ok_or_else(|| invalid("The clipboard does not contain a supported image. Copy an image and try Open from Clipboard again."))?;
                Ok(Completed::Open(pixels))
            }
            Request::Paste(session) => {
                let pixels = decode_first(bytes?, image_io::read_encoded)?.ok_or_else(|| invalid("The clipboard does not contain a supported image. Copy a PNG, TIFF, JPEG, or WebP image first."))?;
                Ok(Completed::Paste(session, pixels))
            }
            Request::CanvasSize(target) => Ok(Completed::CanvasSize(
                target,
                bytes.and_then(|bytes| decode_first(bytes, new_canvas::image_dimensions)),
            )),
        }
    }
}

impl Editor {
    pub fn set_wayland_clipboard(
        &mut self,
        clipboard: compositor::native_clipboard::wayland::WaylandClipboard,
    ) {
        self.wayland_clipboard = Some(clipboard);
    }
    pub(super) fn queue_clipboard(
        &mut self,
        request: Request,
        _cx: &mut EventContext,
    ) -> Result<()> {
        #[cfg(test)]
        let source = {
            use quickgui::ClipboardEntry;
            let item = _cx
                .read_from_clipboard()
                .map_err(|e| invalid(e.to_string()))?;
            Source::Encoded(item.map_or_else(Vec::new, |item| {
                item.entries()
                    .iter()
                    .filter_map(|entry| match entry {
                        ClipboardEntry::Image(image) => Some(image.bytes().to_vec()),
                        _ => None,
                    })
                    .collect()
            }))
        };
        #[cfg(not(test))]
        let source = Source::Native(self.wayland_clipboard.clone());
        let job = Job { request, source };
        #[cfg(test)]
        {
            // QuickGUI's headless runtime has no background task pool.
            self.finish_clipboard_job(job.run()?)
        }
        #[cfg(not(test))]
        {
            self.clipboard_job = Some(job);
            self.pending = true;
            self.status = "Working…".into();
            Ok(())
        }
    }

    fn finish_clipboard_job(&mut self, completed: Completed) -> Result<()> {
        match completed {
            Completed::Open(pixels) => {
                let mut document = Document::new(pixels.width(), pixels.height())?;
                document.layers[0].name = "Clipboard".into();
                document.layers[0].content =
                    compositor::document::LayerContent::Raster(Some(Arc::new(pixels)));
                let session = Session::created(document, "Open from Clipboard")?;
                self.tabs.push(session.into());
                self.activate_tab(self.tabs.len() - 1);
                self.preview = None;
                self.status = "Opened clipboard image as a new project.".into();
                Ok(())
            }
            Completed::Paste(session, pixels) => self.paste_pixels(session, pixels),
            Completed::CanvasSize(target, size) => self.suggest_canvas_size(target, size),
        }
    }

    pub(super) fn start_clipboard_job(&mut self, cx: &ViewContext<'_, Self>) {
        let Some(job) = self.clipboard_job.take() else {
            return;
        };
        let operation = job.request.operation();
        let launched = cx.spawn_background(move || job.run(), move |this, result, cx| {
            this.pending = false;
            let result = result
                .map_err(|error| invalid(format!("Clipboard worker failed: {error}. The document is unchanged. Copy the image again and retry.")))
                .and_then(|result| result)
                .and_then(|completed| this.finish_clipboard_job(completed));
            this.operation_result(operation, result, cx);
        });
        if let Err(error) = launched {
            self.pending = false;
            self.show_error(operation, format!(
                "Could not start the clipboard reader: {error}. The document is unchanged. Retry the operation."
            ));
        }
    }
}

fn decode_first<T>(images: Vec<Vec<u8>>, decode: impl Fn(&[u8]) -> Result<T>) -> Result<Option<T>> {
    let mut failure = None;
    for image in images {
        match decode(&image) {
            Ok(value) => return Ok(Some(value)),
            Err(error) => failure = Some(error),
        }
    }
    failure.map_or(Ok(None), Err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_clipboard_creates_an_image_sized_project_only_after_success() {
        let mut editor = Editor::with_test_document();
        let before = editor.session().document.clone();
        let count = editor.tabs.len();
        let empty = Job {
            request: Request::Open,
            source: Source::Encoded(vec![]),
        };
        assert!(empty.run().is_err());
        assert_eq!(editor.tabs.len(), count);
        let pixels = image::RgbaImage::from_pixel(13, 7, image::Rgba([120, 40, 90, 128]));
        editor
            .finish_clipboard_job(Completed::Open(pixels.clone()))
            .unwrap();
        assert_eq!(editor.tabs.len(), count + 1);
        assert_eq!(editor.tabs[0].session().unwrap().document, before);
        let doc = &editor.session().document;
        assert_eq!((doc.width, doc.height), (13, 7));
        assert_eq!(doc.layers[0].raster().unwrap().as_ref(), &pixels);
        assert!(editor.session().dirty());
    }

    #[test]
    fn delayed_paste_stays_with_its_destination_tab_and_preserves_other_tabs() {
        let mut editor = Editor::with_test_document();
        compositor::edits::add_mask(&mut editor.session_mut().document, false).unwrap();
        editor.tools.mask_target = true;
        let destination = editor.session().id;
        let before = editor.session().document.clone();
        editor
            .tabs
            .push(Session::new(Document::new(7, 3).unwrap(), None).into());
        editor.activate_tab(1);
        editor.tools.mask_target = true;
        let other = editor.session().document.clone();
        editor
            .finish_clipboard_job(Completed::Paste(
                destination,
                image::RgbaImage::from_pixel(2, 2, image::Rgba([23, 45, 67, 89])),
            ))
            .unwrap();
        assert_eq!(editor.session().document, other);
        assert!(editor.tools.mask_target);
        assert!(!editor.tabs[0].parked_tools.mask_target);
        assert_eq!(
            editor.tabs[0].session().unwrap().document.layers.len(),
            before.layers.len() + 1
        );
        editor.tabs[0].session_mut().unwrap().undo();
        assert_eq!(editor.tabs[0].session().unwrap().document, before);
        editor.tabs.remove(0);
        editor.current = 0;
        assert!(
            editor
                .finish_clipboard_job(Completed::Paste(destination, image::RgbaImage::new(1, 1)))
                .is_err()
        );
        assert_eq!(editor.session().document, other);
    }

    #[test]
    fn delayed_canvas_suggestions_preserve_typed_values_and_closed_tabs() {
        let mut editor = Editor::new(Vec::new()).unwrap();
        let target = editor.tabs[0].id;
        let draft = editor.tabs[0].canvas_draft_mut().unwrap();
        draft.dimensions[0] = "640".into();
        draft.edited = true;
        editor
            .suggest_canvas_size(target, Ok(Some([12, 34])))
            .unwrap();
        assert_eq!(editor.tabs[0].canvas_draft().unwrap().dimensions[0], "640");
        editor.add_empty_tab();
        let other = editor.tabs[1].id;
        editor
            .suggest_canvas_size(other, Ok(Some([12, 34])))
            .unwrap();
        assert_eq!(
            editor.tabs[1].canvas_draft().unwrap().dimensions,
            ["12", "34"]
        );
        editor.tabs.remove(0);
        editor.current = 0;
        editor
            .suggest_canvas_size(target, Err(invalid("stale error")))
            .unwrap();
        assert!(!editor.status.contains("stale error"));
    }

    #[test]
    fn image_entries_try_next_format_and_report_failure_without_empty_success() {
        assert_eq!(
            decode_first(vec![vec![], vec![7]], |bytes| {
                bytes.first().copied().ok_or_else(|| invalid("empty image"))
            })
            .unwrap(),
            Some(7)
        );
        assert!(decode_first(vec![vec![]], image_io::read_encoded).is_err());
    }
}
