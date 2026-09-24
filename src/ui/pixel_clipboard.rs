use super::*;
use compositor::{
    clipboard::{self, PixelClipboard},
    edits, invalid,
};
use quickgui::{ClipboardImage, ClipboardImageFormat, ClipboardItem};

impl Editor {
    pub(super) fn can_copy_pixels(&self) -> bool {
        if !self.can_edit_layers() {
            return false;
        }
        let doc = &self.session().document;
        doc.selection.as_ref().is_none_or(|s| s.bounds().is_some())
            && doc.active_layer().is_some_and(|layer| {
                if self.tools.mask_target {
                    layer.mask.is_some()
                } else {
                    layer.raster().is_some()
                }
            })
    }

    pub(super) fn can_copy_merged(&self) -> bool {
        if !self.can_edit_layers() {
            return false;
        }
        let doc = &self.session().document;
        doc.selection.as_ref().is_none_or(|s| s.bounds().is_some())
            && doc
                .layers
                .iter()
                .any(|layer| layer.raster().is_some() && doc.layer_is_visible(layer.id))
    }

    pub(super) fn clipboard_available(&self, action: Action) -> bool {
        match action {
            Action::Copy => self.can_copy_pixels() || self.can_copy_layers(),
            Action::CopyMerged => self.can_copy_merged(),
            Action::Cut => self.can_copy_pixels() && self.session().document.selection.is_some(),
            Action::Paste => {
                if self.has_document() {
                    self.can_edit_layers()
                } else {
                    self.can_switch_projects()
                }
            }
            _ => false,
        }
    }

    pub(super) fn clipboard_action(&mut self, action: Action, cx: &mut EventContext) -> Result<()> {
        if !self.clipboard_available(action) {
            return Ok(());
        }
        if matches!(action, Action::Paste) {
            return self.queue_clipboard(
                clipboard_jobs::Request::Paste(self.tabs[self.current].id),
                cx,
            );
        } else {
            let layers = if matches!(action, Action::Copy) && self.can_copy_layers() {
                Some(compositor::layer_clipboard::Layers::capture(
                    &self.session().document,
                )?)
            } else {
                None
            };
            let copied = if let Some(layers) = &layers {
                PixelClipboard {
                    pixels: Arc::new(layers.pixels()?),
                    origin: [0., 0.],
                }
            } else {
                clipboard::copy(
                    &self.session().document,
                    matches!(action, Action::CopyMerged),
                    self.tools.mask_target,
                )?
            };
            let mut bytes = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(copied.pixels.as_ref().clone())
                .write_to(&mut bytes, image::ImageFormat::Png)?;
            let image = ClipboardImage::new(ClipboardImageFormat::Png, bytes.into_inner())
                .map_err(|e| invalid(e.to_string()))?;
            let item = ClipboardItem::new_image(image).map_err(|e| invalid(e.to_string()))?;
            cx.write_to_clipboard(item).map_err(|e| {
                invalid(format!(
                    "Could not write the clipboard: {e}. Pixels have not been cut."
                ))
            })?;
            self.layer_clipboard = None;
            if let Some(layers) = layers {
                self.layer_clipboard = Some(super::layer_clipboard::Copy {
                    layers,
                    pixels: copied.pixels.clone(),
                    owner: self.clipboard_owner()?,
                });
            }
            self.pixel_clipboard = Some(copied);
            if matches!(action, Action::Cut) && self.can_edit_pixels() {
                let mask = self.tools.mask_target;
                let background = self.palette_colors(mask)[1];
                self.session_mut()
                    .edit(if mask { "Fill Mask" } else { "Clear" }, |doc| {
                        edits::fill(doc, background, !mask, mask)
                    })?;
            }
            self.status = "Copied image pixels to the clipboard.".into();
        }
        Ok(())
    }

    pub(super) fn paste_pixels(
        &mut self,
        session: uuid::Uuid,
        pixels: image::RgbaImage,
    ) -> Result<()> {
        let layers = self
            .layer_clipboard
            .as_ref()
            .filter(|clip| clip.pixels.as_ref() == &pixels)
            .filter(|clip| {
                self.clipboard_owner()
                    .is_ok_and(|owner| owner == clip.owner)
            })
            .map(|clip| clip.layers.clone());
        let clip_origin = self
            .pixel_clipboard
            .as_ref()
            .filter(|clip| clip.pixels.as_ref() == &pixels)
            .map(|clip| clip.origin);
        let target = self
            .tabs
            .iter_mut()
            .find(|tab| tab.id == session)
            .ok_or_else(|| {
                invalid("The destination project closed before the clipboard image arrived.")
            })?;
        let (width, height) = pixels.dimensions();
        let clip_origin = clip_origin.filter(|_| target.session().is_some());
        target.edit_or_create(
            "Paste",
            || {
                let mut doc = Document::new(width, height)?;
                doc.layers.clear();
                doc.active = None;
                doc.selected.clear();
                Ok(doc)
            },
            |doc| {
                if let Some(layers) = &layers {
                    return layers.paste(doc);
                }
                let origin = clip_origin.unwrap_or([
                    ((doc.width as f64 - pixels.width() as f64) / 2.).floor(),
                    ((doc.height as f64 - pixels.height() as f64) / 2.).floor(),
                ]);
                clipboard::paste(
                    doc,
                    PixelClipboard {
                        pixels: Arc::new(pixels),
                        origin,
                    },
                )
            },
        )?;
        target.parked_tools.mask_target = false;
        if self.tabs[self.current].id == session {
            self.tools.mask_target = false;
        }
        self.status = "Pasted image. Ctrl+Z undoes the paste.".into();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, WindowOptions};
    use std::borrow::Cow;

    #[test]
    fn cutting_a_mask_fills_the_selection_with_its_background_color_and_undo_restores_it() {
        for white_foreground in [false, true] {
            let mut e = Editor::with_test_document();
            e.tabs = vec![Session::new(Document::new(4, 2).unwrap(), None).into()];
            edits::add_mask(&mut e.session_mut().document, false).unwrap();
            e.session_mut().document.layers[0]
                .mask
                .as_mut()
                .unwrap()
                .pixels = Arc::new(image::GrayImage::from_pixel(4, 2, image::Luma([128])));
            e.session_mut().document.selection = Some(compositor::selection::Selection::rectangle(
                4,
                2,
                [0., 0.],
                [2., 2.],
                false,
            ));
            e.tools.mask_target = true;
            e.tools.mask_paint_white = white_foreground;
            let original = e.session().document.clone();
            let (mut cx, view) = Application::new()
                .into_test_context(WindowOptions::new("Cut mask").size(1280., 850.), e)
                .unwrap();
            cx.update(view, |e, cx| e.clipboard_action(Action::Cut, cx).unwrap())
                .unwrap();
            cx.read(view, |e| {
                let mask = &e.session().document.layers[0].mask.as_ref().unwrap().pixels;
                assert_eq!(mask[(0, 0)][0], if white_foreground { 0 } else { 255 });
                assert_eq!(mask[(3, 0)][0], 128);
                assert_eq!(
                    e.pixel_clipboard.as_ref().unwrap().pixels[(0, 0)],
                    image::Rgba([128, 128, 128, 255])
                );
            })
            .unwrap();
            cx.update(view, |e, _| e.session_mut().undo()).unwrap();
            assert_eq!(
                cx.read(view, |e| e.session().document.clone()).unwrap(),
                original
            );
            cx.update(view, |e, cx| e.action(Action::Duplicate, cx))
                .unwrap();
            cx.read(view, |e| {
                assert!(!e.tools.mask_target);
                assert_eq!(e.session().document.layers.len(), 2);
                assert_eq!(
                    e.session()
                        .document
                        .active_layer()
                        .unwrap()
                        .raster()
                        .unwrap()[(0, 0)],
                    image::Rgba([128, 128, 128, 255])
                );
            })
            .unwrap();
            cx.update(view, |e, _| e.session_mut().undo()).unwrap();
            assert_eq!(
                cx.read(view, |e| e.session().document.clone()).unwrap(),
                original
            );
        }
    }

    #[test]
    fn profiled_clipboard_paste_targets_new_pixels_and_failed_paste_preserves_document() {
        let mut info = png::Info::with_size(1, 1);
        info.color_type = png::ColorType::GrayscaleAlpha;
        info.bit_depth = png::BitDepth::Eight;
        info.icc_profile = Some(Cow::Owned(
            lcms2::Profile::new_gray(lcms2::CIExyY::d50(), &lcms2::ToneCurve::new(1.))
                .unwrap()
                .icc()
                .unwrap(),
        ));
        let mut bytes = Vec::new();
        let mut writer = png::Encoder::with_info(&mut bytes, info)
            .unwrap()
            .write_header()
            .unwrap();
        writer.write_image_data(&[128, 123]).unwrap();
        writer.finish().unwrap();
        let mut e = Editor::with_test_document();
        e.tabs = vec![Session::new(Document::new(2, 2).unwrap(), None).into()];
        edits::add_mask(&mut e.session_mut().document, false).unwrap();
        e.tools.mask_target = true;
        let original = e.session().document.clone();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Profiled clipboard").size(1280., 850.),
                e,
            )
            .unwrap();
        let image = ClipboardImage::new(ClipboardImageFormat::Png, bytes).unwrap();
        cx.write_to_clipboard(ClipboardItem::new_image(image).unwrap())
            .unwrap();
        cx.update(view, |e, cx| e.action(Action::Paste, cx))
            .unwrap();
        let pasted = cx
            .read(view, |e| {
                let doc = &e.session().document;
                assert_eq!(doc.layers.len(), 2);
                let pixels = doc.active_layer().unwrap().raster().unwrap();
                assert!((187..=189).contains(&pixels[(0, 0)][0]));
                assert_eq!(pixels[(0, 0)][3], 123);
                assert!(!e.tools.mask_target);
                doc.clone()
            })
            .unwrap();
        let image = ClipboardImage::new(ClipboardImageFormat::Png, b"broken".to_vec()).unwrap();
        cx.write_to_clipboard(ClipboardItem::new_image(image).unwrap())
            .unwrap();
        cx.update(view, |e, cx| e.action(Action::Paste, cx))
            .unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            pasted
        );
        cx.update(view, |e, _| e.session_mut().undo()).unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            original
        );
        assert!(
            cx.read(view, |e| e.session().undo_label().is_none())
                .unwrap()
        );
    }
}
