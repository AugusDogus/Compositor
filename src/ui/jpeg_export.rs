use super::*;
use compositor::{image_io::JpegOptions, invalid};
use uuid::Uuid;

struct Preview {
    image: Image,
    bytes: Vec<u8>,
    options: JpegOptions,
}
impl Preview {
    fn render(source: &image::RgbaImage, resolution: f64, options: JpegOptions) -> Result<Self> {
        let bytes = image_io::encode_jpeg_with_options(source, resolution, options)?;
        let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg)?
            .thumbnail(1120, 660)
            .into_rgba8();
        let image = Image::from_rgba(decoded.width(), decoded.height(), decoded.into_raw())
            .map_err(|e| invalid(format!("JPEG preview could not be displayed: {e}")))?;
        Ok(Self {
            image,
            bytes,
            options,
        })
    }
}

pub(super) struct JpegExport {
    id: Uuid,
    path: Option<PathBuf>,
    document: Document,
    source: Option<Arc<image::RgbaImage>>,
    options: JpegOptions,
    queued: Option<JpegOptions>,
    running: bool,
    preview: Option<Preview>,
}
impl JpegExport {
    fn ready(&self) -> bool {
        !self.running
            && self.queued.is_none()
            && self
                .preview
                .as_ref()
                .is_some_and(|p| p.options == self.options)
    }
    fn accept(&mut self, source: Arc<image::RgbaImage>, preview: Preview) {
        self.source = Some(source);
        if preview.options == self.options {
            self.preview = Some(preview);
            self.queued = None;
        }
    }
}

impl Editor {
    pub(super) fn open_jpeg(&mut self, path: Option<PathBuf>) {
        self.retain_tool_panel();
        let (quality, error) = match super::jpeg_preferences::load() {
            Ok(quality) => (quality, String::new()),
            Err(error) => (
                85,
                format!("Could not read the previous JPEG quality: {error}. Using 85%."),
            ),
        };
        let options = JpegOptions {
            quality,
            background: [255; 3],
        };
        self.jpeg_export = Some(JpegExport {
            id: Uuid::new_v4(),
            path,
            document: self.session().committed_document().clone(),
            source: None,
            options,
            queued: Some(options),
            running: false,
            preview: None,
        });
        self.modal = Some(Form::Edit {
            title: "Export JPEG",
            action: Action::ExportJpeg,
            fields: vec![("Quality", quality.to_string())],
            error,
        });
    }

    pub(super) fn refresh_jpeg(&mut self) {
        let Some(edit) = &mut self.jpeg_export else {
            return;
        };
        let Some(Form::Edit {
            action: Action::ExportJpeg,
            fields,
            error,
            ..
        }) = &mut self.modal
        else {
            return;
        };
        let parsed = fields
            .first()
            .and_then(|(_, v)| v.trim().parse::<u8>().ok())
            .filter(|v| *v <= 100);
        if let Some(quality) = parsed {
            edit.options.quality = quality;
            edit.queued = Some(edit.options);
            error.clear();
        } else {
            *error = "JPEG quality must be a whole number from 0 to 100.".into();
        }
    }

    pub(super) fn jpeg_background(&self) -> Option<[u8; 3]> {
        self.jpeg_export.as_ref().map(|e| e.options.background)
    }
    pub(super) fn set_jpeg_background(&mut self, background: [u8; 3]) {
        if let Some(edit) = &mut self.jpeg_export {
            edit.options.background = background;
            edit.queued = Some(edit.options);
        }
        if let Some(Form::Edit { error, .. }) = &mut self.modal {
            error.clear();
        }
    }
    pub(super) fn jpeg_ready(&self) -> bool {
        self.jpeg_export.as_ref().is_some_and(JpegExport::ready)
            && matches!(&self.modal, Some(Form::Edit { fields, error, .. }) if error.is_empty()
                && fields.first().is_some_and(|(_, v)| v.parse::<u8>().is_ok_and(|q| q <= 100)))
    }

    pub(super) fn finish_jpeg(&mut self, cx: &mut EventContext) -> Result<()> {
        if !self.jpeg_ready() {
            return Err(invalid(
                "Wait for the current JPEG preview before exporting. Your image is unchanged.",
            ));
        }
        let Some(edit) = &mut self.jpeg_export else {
            return Ok(());
        };
        let Some(preview) = edit.preview.take() else {
            return Ok(());
        };
        let path = edit.path.take();
        self.jpeg_export = None;
        self.finish_form();
        match path {
            Some(path) => self.queue_file(super::file_jobs::FileJob::ExportJpeg {
                path,
                bytes: preview.bytes,
                quality: preview.options.quality,
            }),
            None => self.prompt_jpeg_path(preview.bytes, preview.options.quality, cx),
        }
        Ok(())
    }

    pub(super) fn jpeg_controls(&self, cx: &mut ViewContext<'_, Self>, error: &str) -> Element {
        let Some(edit) = &self.jpeg_export else {
            return div();
        };
        let mut preview = div()
            .w(560.)
            .h(330.)
            .flex_shrink_0()
            .relative()
            .bg(Color::rgb8(31, 31, 31))
            .overflow_hidden();
        if let Some(rendered) = &edit.preview {
            preview = preview.child(
                quickgui::img(&rendered.image)
                    .size_full()
                    .object_fit(quickgui::ObjectFit::Contain),
            );
        }
        if !edit.ready() && error.is_empty() {
            preview = preview.child(
                div()
                    .absolute()
                    .left(261.)
                    .top(146.)
                    .p(10.)
                    .rounded(8.)
                    .bg(Color::rgba8(45, 45, 45, 217))
                    .backdrop_blur(20.)
                    .child(super::icons::progress("jpeg-preview-progress", 18.))
                    .accessibility_label("Updating JPEG preview"),
            );
        }
        let quality = div()
            .flex_row()
            .items_center()
            .gap(12.)
            .child(text("Quality").w(50.).text_size(13.).line_height(16.))
            .child(self.scalar_slider(
                cx,
                "jpeg-quality",
                "JPEG quality",
                super::scalar_controls::Scalar::JpegQuality,
                (0., 100.),
                441.,
            ))
            .child(
                text(format!("{}%", edit.options.quality))
                    .w(45.)
                    .text_size(13.)
                    .line_height(16.)
                    .text_right()
                    .font_features(
                        quickgui::FontFeatures::new()
                            .enable(quickgui::FontFeatureTag::TABULAR_NUMBERS),
                    ),
            );
        let [r, g, b] = edit.options.background;
        let background = div()
            .flex_row()
            .items_center()
            .gap(12.)
            .child(
                text("Background for transparency")
                    .text_size(13.)
                    .line_height(16.),
            )
            .child(div().flex_1())
            .child(
                Self::color_well(Color::rgb8(r, g, b), "JPEG background color").on_click(
                    cx.listener("jpeg-background", |this, cx| {
                        this.open_jpeg_background_picker();
                        this.changed(cx);
                    }),
                ),
            );
        div()
            .flex_col()
            .gap(16.)
            .child(preview)
            .child(quality)
            .child(background)
            .child(
                text(format!(
                    "{} × {} px · sRGB",
                    edit.document.width, edit.document.height
                ))
                .text_size(13.)
                .line_height(16.)
                .text_color(Color::rgb8(180, 180, 180)),
            )
    }
    pub(super) fn jpeg_status(&self, error: &str) -> Element {
        if !error.is_empty() {
            return text(error.to_owned())
                .id("jpeg-status")
                .text_size(13.)
                .line_height(16.)
                .text_color(Color::rgb8(255, 69, 58))
                .wrap();
        }
        let preview = self
            .jpeg_export
            .as_ref()
            .filter(|e| e.ready())
            .and_then(|e| e.preview.as_ref());
        let label = if let Some(preview) = preview {
            let size = super::byte_count::Style::File.format(preview.bytes.len() as u64);
            quickgui::StyledText::new(format!("{size} · encoded preview, fitted to window"))
                .with_highlights([(
                    0..size.len(),
                    quickgui::HighlightStyle::default().color(Color::rgb8(224, 224, 224)),
                )])
        } else {
            quickgui::StyledText::new("Updating preview…")
        };
        label
            .into_element()
            .id("jpeg-status")
            .text_size(13.)
            .line_height(16.)
            .text_color(Color::rgb8(180, 180, 180))
            .wrap()
    }

    pub(super) fn start_jpeg_preview(&mut self, cx: &ViewContext<'_, Self>) {
        let Some(edit) = &mut self.jpeg_export else {
            return;
        };
        if edit.running {
            return;
        }
        let Some(options) = edit.queued.take() else {
            return;
        };
        edit.running = true;
        let id = edit.id;
        let document = edit.document.clone();
        let cached = edit.source.clone();
        let launched = cx.spawn_background(
            move || -> Result<(Arc<image::RgbaImage>, Preview)> {
                compositor::document::validate_size(document.width, document.height)?;
                let source = match cached {
                    Some(source) => source,
                    None => Arc::new(compositor::render::render(
                        &document,
                        document.width,
                        document.height,
                    )?),
                };
                let preview = Preview::render(&source, document.resolution, options)?;
                Ok((source, preview))
            },
            move |this, result, cx| {
                let Some(edit) = &mut this.jpeg_export else {
                    return;
                };
                if edit.id != id {
                    return;
                }
                edit.running = false;
                let result = result
                    .map_err(|e| {
                        invalid(format!(
                            "JPEG preview worker failed: {e}. Change quality to retry."
                        ))
                    })
                    .and_then(|r| r);
                match result {
                    Ok((source, preview)) => edit.accept(source, preview),
                    Err(e) if edit.options == options => {
                        if let Some(Form::Edit { error, .. }) = &mut this.modal {
                            *error = e.to_string();
                        }
                    }
                    Err(_) => {}
                }
                this.changed(cx);
            },
        );
        if let Err(error) = launched {
            if let Some(edit) = &mut self.jpeg_export {
                edit.running = false;
            }
            if let Some(Form::Edit { error: message, .. }) = &mut self.modal {
                *message =
                    format!("Could not start the JPEG preview: {error}. Change quality to retry.");
            }
        }
    }
}

#[cfg(test)]
mod tests;
