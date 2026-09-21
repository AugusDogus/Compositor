use super::*;
use compositor::{document::validate_size, invalid};
use image::{ImageDecoder, metadata::Orientation};

pub(super) fn image_dimensions(bytes: &[u8]) -> Result<[u32; 2]> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(30_000);
    limits.max_image_height = Some(30_000);
    limits.max_alloc = Some(400_000_000);
    reader.limits(limits);
    let mut decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    validate_size(width, height)?;
    Ok(match decoder.orientation()? {
        Orientation::Rotate90
        | Orientation::Rotate270
        | Orientation::Rotate90FlipH
        | Orientation::Rotate270FlipH => [height, width],
        _ => [width, height],
    })
}

pub(super) struct CanvasDraft {
    pub dimensions: [String; 2],
    pub edited: bool,
}

impl Default for CanvasDraft {
    fn default() -> Self {
        Self {
            dimensions: ["1920".into(), "1080".into()],
            edited: false,
        }
    }
}

impl CanvasDraft {
    pub fn valid(&self) -> bool {
        self.dimensions.iter().all(|value| {
            value
                .trim()
                .parse::<u32>()
                .is_ok_and(|value| (1..=30_000).contains(&value))
        })
    }
}

impl Editor {
    pub(super) fn suggest_new_canvas(&mut self, cx: &mut EventContext) -> Result<()> {
        self.queue_clipboard(
            clipboard_jobs::Request::CanvasSize(self.tabs[self.current].id),
            cx,
        )
    }

    pub(super) fn create_welcome_canvas(&mut self, cx: &mut EventContext) {
        let Some(draft) = self.tabs[self.current].canvas_draft() else {
            return;
        };
        if !draft.valid() {
            return;
        }
        let result = self.apply_form(Action::New, draft.dimensions.to_vec());
        if result.is_ok() {
            self.status = "Canvas created. Import an image or choose a painting tool.".into();
        }
        self.operation_result(alerts::Operation::CreateCanvas, result, cx);
    }

    pub(super) fn suggest_canvas_size(
        &mut self,
        target: uuid::Uuid,
        suggestion: Result<Option<[u32; 2]>>,
    ) -> Result<()> {
        let Some(draft) = self
            .tabs
            .iter_mut()
            .find(|tab| tab.id == target)
            .and_then(ProjectTab::canvas_draft_mut)
            .filter(|draft| !draft.edited)
        else {
            return Ok(());
        };
        match suggestion {
            Ok(Some([width, height])) => {
                draft.dimensions = [width.to_string(), height.to_string()];
                if self.tabs[self.current].id == target {
                    self.status = "Canvas dimensions match the clipboard image.".into();
                }
                Ok(())
            }
            Ok(None) => {
                if self.tabs[self.current].id == target {
                    self.status = self.tool_hint().into();
                }
                Ok(())
            }
            Err(error) => Err(invalid(format!(
                "Could not suggest a canvas size from the clipboard: {error} Enter dimensions manually; the default is 1920 × 1080."
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{
        Application, ClipboardImage, ClipboardImageFormat, ClipboardItem, WindowOptions,
    };

    fn oriented_tiff(orientation: u16) -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        {
            let mut encoder = tiff::encoder::TiffEncoder::new(&mut bytes).unwrap();
            let mut image = encoder
                .new_image::<tiff::encoder::colortype::RGB8>(7, 3)
                .unwrap();
            image
                .encoder()
                .write_tag(tiff::tags::Tag::Orientation, orientation)
                .unwrap();
            image.write_data(&[128; 63]).unwrap();
        }
        bytes.into_inner()
    }

    #[test]
    fn clipboard_dimensions_respect_all_exif_orientations() {
        for orientation in 1..=8 {
            assert_eq!(
                image_dimensions(&oriented_tiff(orientation)).unwrap(),
                if orientation >= 5 { [3, 7] } else { [7, 3] }
            );
        }
        assert!(image_dimensions(b"invalid image").is_err());
    }

    #[test]
    fn new_canvas_suggests_clipboard_dimensions_and_keeps_defaults_on_failure() {
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("New canvas"),
                Editor::with_test_document(),
            )
            .unwrap();
        let dimensions = |editor: &Editor| {
            editor.tabs[editor.current]
                .canvas_draft()
                .unwrap()
                .dimensions
                .clone()
        };
        cx.update(view, |e, cx| e.action(Action::New, cx)).unwrap();
        assert_eq!(cx.read(view, dimensions).unwrap(), ["1920", "1080"]);
        let image = ClipboardImage::new(ClipboardImageFormat::Tiff, oriented_tiff(6)).unwrap();
        cx.write_to_clipboard(ClipboardItem::new_image(image).unwrap())
            .unwrap();
        cx.update(view, |e, cx| e.action(Action::New, cx)).unwrap();
        assert_eq!(cx.read(view, dimensions).unwrap(), ["3", "7"]);
        cx.click(view.window_handle(), "welcome-create").unwrap();
        assert_eq!(
            cx.read(view, |e| [
                e.session().document.width,
                e.session().document.height
            ])
            .unwrap(),
            [3, 7]
        );
        cx.update(view, |e, cx| e.action(Action::Paste, cx))
            .unwrap();
        cx.read(view, |e| {
            let layer = e.session().document.active_layer().unwrap();
            assert_eq!(layer.raster().unwrap().dimensions(), (3, 7));
            assert_eq!(layer.transform.origin, [0., 0.]);
        })
        .unwrap();
        let image = ClipboardImage::new(ClipboardImageFormat::Png, b"broken".to_vec()).unwrap();
        cx.write_to_clipboard(ClipboardItem::new_image(image).unwrap())
            .unwrap();
        let status = cx
            .update(view, |e, cx| {
                e.action(Action::New, cx);
                e.status.clone()
            })
            .unwrap();
        assert_eq!(cx.read(view, dimensions).unwrap(), ["1920", "1080"]);
        assert!(status.contains("Enter dimensions manually"), "{status}");
    }
}
