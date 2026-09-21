//! Invert edits committed pixels while retaining any open tool preview.
use super::*;
use compositor::{document::LayerContent, invalid};

impl Editor {
    pub(super) fn can_invert(&self) -> bool {
        if !self.has_document()
            || self.pending
            || self.gesture.is_some()
            || self.pending_pixels.is_some()
            || self.rename.is_some()
            || (self.modal.is_some()
                && self.floating_panel_kind().is_none()
                && !matches!(
                    self.modal,
                    Some(Form::Blend | Form::Color(_) | Form::MaskColor(_))
                ))
        {
            return false;
        }
        let doc = &self.session().document;
        doc.selected.len() == 1
            && doc
                .selection
                .as_ref()
                .is_none_or(|selection| selection.bounds().is_some())
            && doc.active_layer().is_some_and(|layer| {
                doc.layer_is_visible(layer.id)
                    && if self.tools.mask_target {
                        layer.mask.as_ref().is_some_and(|mask| mask.enabled)
                    } else {
                        layer.raster().is_some() && layer.raw.is_none()
                    }
            })
    }

    pub(super) fn invert(&mut self) -> Result<()> {
        if !self.can_invert() {
            return Ok(());
        }
        self.finish_pending_edits()?;
        let mask = self.tools.mask_target;
        let label = if mask { "Invert Mask" } else { "Invert" };
        if self.floating_panel_kind().is_some() {
            self.session_mut()
                .edit_committed(label, |doc| invert_pixels(doc, mask))?;
            if self.adjustment_edit.is_some() {
                self.preview_adjustment()?;
            } else if !self.filter_preview_enabled() {
                self.session_mut().document = self.session().committed_document().clone();
            }
        } else {
            self.session_mut()
                .edit(label, |doc| invert_pixels(doc, mask))?;
        }
        Ok(())
    }
}

fn invert_pixels(doc: &mut Document, mask_target: bool) -> Result<()> {
    let selection = doc.selection.clone();
    let layer = doc
        .active_layer_mut()
        .ok_or_else(|| invalid("Select a pixel layer."))?;
    if !mask_target {
        layer.require_rasterized()?;
    }
    let t = layer.transform;
    if mask_target {
        let pixel_size = layer
            .raster()
            .map(|pixels| pixels.dimensions())
            .unwrap_or_else(|| {
                (
                    layer.transform.size[0].round() as u32,
                    layer.transform.size[1].round() as u32,
                )
            });
        let mask = layer
            .mask
            .as_mut()
            .ok_or_else(|| invalid("The selected layer has no mask."))?;
        let t = mask.placement.unwrap_or(t);
        if selection.is_some() && mask.pixels.dimensions() == (1, 1) {
            let (w, h) = pixel_size;
            compositor::document::validate_size(w, h)?;
            mask.pixels = Arc::new(image::GrayImage::from_pixel(w, h, mask.pixels[(0, 0)]));
        }
        let (w, h) = mask.pixels.dimensions();
        for (x, y, p) in Arc::make_mut(&mut mask.pixels).enumerate_pixels_mut() {
            let amount = selection.as_ref().map_or(1., |s| {
                s.coverage(t.point([(x as f64 + 0.5) / w as f64, (y as f64 + 0.5) / h as f64]))
            });
            p[0] = (p[0] as f64 + (255. - 2. * p[0] as f64) * amount).round() as u8;
        }
        return Ok(());
    }
    if let LayerContent::Raster(Some(pixels)) = &mut layer.content {
        let (w, h) = pixels.dimensions();
        for (x, y, p) in Arc::make_mut(pixels).enumerate_pixels_mut() {
            let amount = selection.as_ref().map_or(1., |s| {
                s.coverage(t.point([(x as f64 + 0.5) / w as f64, (y as f64 + 0.5) / h as f64]))
            });
            for i in 0..3 {
                p[i] = (p[i] as f64 + (255. - 2. * p[i] as f64) * amount).round() as u8;
            }
        }
        layer.shape = None;
        layer.text = None;
        Ok(())
    } else {
        Err(invalid("Select a pixel layer to invert."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, WindowOptions};

    fn editor() -> Editor {
        let mut e = Editor::with_test_document();
        let mut doc = Document::new(4, 4).unwrap();
        compositor::edits::fill(&mut doc, [50, 120, 200, 128], false, false).unwrap();
        e.tabs = vec![Session::new(doc, None).into()];
        e
    }

    #[test]
    fn invert_beneath_adjustments_keeps_previews_and_rejects_stale_apply() {
        for kind in [Kind::Levels, Kind::HueSaturation, Kind::Exposure] {
            for apply in [false, true] {
                let e = editor();
                let original = e.session().document.clone();
                let mut inverted = original.clone();
                invert_pixels(&mut inverted, false).unwrap();
                let (mut cx, view) = Application::new()
                    .into_test_context(
                        WindowOptions::new("Invert beneath adjustment").size(1500., 1000.),
                        e,
                    )
                    .unwrap();
                cx.update(view, |e, cx| {
                    e.action(Action::AdjustPixels(kind), cx);
                    if let Some(Form::Edit { fields, .. }) = &mut e.modal {
                        let (index, value) = match kind {
                            Kind::Levels => (1, "2"),
                            Kind::HueSaturation => (0, "90"),
                            _ => (0, "1"),
                        };
                        fields[index].1 = value.into();
                    }
                    e.preview_adjustment().unwrap();
                })
                .unwrap();
                let preview = cx.read(view, |e| e.session().document.clone()).unwrap();
                assert_ne!(preview, original);
                cx.focus(view.window_handle(), 50_000_u64).unwrap();
                cx.simulate_keystrokes(view.window_handle(), "ctrl-i")
                    .unwrap();
                cx.update(view, |e, _| {
                    assert!(e.adjustment_edit.is_some());
                    assert!(e.modal.is_some());
                    assert_eq!(e.session().document, preview);
                    assert_eq!(e.session().committed_document(), &inverted);
                    assert_eq!(e.session().undo_label(), Some("Invert"));
                    e.adjustment_edit.as_mut().unwrap().preview = false;
                    e.preview_adjustment().unwrap();
                    assert_eq!(e.session().document, inverted);
                    e.adjustment_edit.as_mut().unwrap().preview = true;
                    e.preview_adjustment().unwrap();
                    assert_eq!(e.session().document, preview);
                    if apply {
                        e.finish_adjustment().unwrap();
                    } else {
                        e.cancel_adjustment();
                    }
                    assert_eq!(e.session().document, inverted);
                    assert!(!e.session().has_pending_edit());
                    e.session_mut().undo();
                    assert_eq!(e.session().document, original);
                    assert!(e.session().undo_label().is_none());
                })
                .unwrap();
            }
        }
    }

    #[test]
    fn identity_preview_shows_inversion_and_source_identity_survives_double_invert() {
        for kind in [Kind::Levels, Kind::HueSaturation] {
            let mut e = editor();
            let original = e.session().document.clone();
            e.open_pixel_adjustment(kind).unwrap();
            e.invert().unwrap();
            assert_eq!(e.session().document, *e.session().committed_document());
            assert_ne!(e.session().document, original);
            e.invert().unwrap();
            assert_eq!(e.session().document, original);
            assert!(!e.adjustment_source_is_current());
            if let Some(Form::Edit { fields, .. }) = &mut e.modal {
                fields[0].1 = "30".into();
            }
            e.finish_adjustment().unwrap();
            assert_eq!(e.session().document, original);
            assert_eq!(e.session().undo_label(), Some("Invert"));
            e.session_mut().undo();
            assert_ne!(e.session().document, original);
            e.session_mut().undo();
            assert_eq!(e.session().document, original);
        }
    }

    #[test]
    fn invert_preserves_crop_and_resolves_gradient_before_recording_history() {
        let mut e = editor();
        let frame = compositor::geometry::Transform {
            origin: [1., 1.],
            ..compositor::geometry::Transform::new(2, 2)
        };
        e.tools.pending_crop = Some(super::super::crop::CropPreview {
            frame,
            guides: [None; 2],
        });
        e.invert().unwrap();
        assert_eq!(e.tools.pending_crop.as_ref().unwrap().frame, frame);
        e.tools.pending_crop = None;
        e.begin_gradient([0., 0.]).unwrap();
        e.move_gradient([3., 3.], gradient::Endpoint::End, false)
            .unwrap();
        let gradient = e.session().document.clone();
        e.invert().unwrap();
        assert!(e.pending_gradient.is_none());
        assert_eq!(e.session().undo_label(), Some("Invert"));
        e.session_mut().undo();
        assert_eq!(e.session().document, gradient);
        assert_eq!(e.session().undo_label(), Some("Gradient"));
    }

    #[test]
    fn unavailable_invert_preserves_document_and_enabled_group_masks_can_invert() {
        for state in 0..7 {
            let mut e = editor();
            match state {
                0 => e.session_mut().document.layers[0].visible = false,
                1 => {
                    let active = e.session().document.active.unwrap();
                    let mut parent = compositor::document::Layer::blank("Hidden group", 4, 4);
                    parent.content = LayerContent::Group;
                    parent.visible = false;
                    let id = parent.id;
                    e.session_mut().document.add(parent).unwrap();
                    e.session_mut().document.layers[0].parent = Some(id);
                    e.session_mut().document.select(active, false);
                }
                2 => {
                    let active = e.session().document.active.unwrap();
                    e.session_mut()
                        .document
                        .add(compositor::document::Layer::blank("Second", 4, 4))
                        .unwrap();
                    e.session_mut().document.select(active, true);
                }
                3 => e.session_mut().document.layers[0].content = LayerContent::Raster(None),
                4 => {
                    e.session_mut().document.selection =
                        Some(compositor::selection::Selection::rectangle(
                            4,
                            4,
                            [1., 1.],
                            [1., 1.],
                            false,
                        ))
                }
                5 => {
                    compositor::edits::add_mask(&mut e.session_mut().document, false).unwrap();
                    e.session_mut().document.layers[0]
                        .mask
                        .as_mut()
                        .unwrap()
                        .enabled = false;
                    e.tools.mask_target = true;
                }
                _ => {
                    e.session_mut().document.selection =
                        Some(compositor::selection::Selection::rectangle(
                            4,
                            4,
                            [0., 0.],
                            [2., 2.],
                            false,
                        ));
                    e.begin_pixel_transform().unwrap();
                }
            }
            let before = e.session().document.clone();
            assert!(!e.can_invert(), "State {state}");
            e.invert().unwrap();
            assert_eq!(e.session().document, before);
            assert!(e.session().undo_label().is_none());
        }
        let mut e = editor();
        e.session_mut().document.layers[0].content = LayerContent::Group;
        assert!(!e.can_invert());
        compositor::edits::add_mask(&mut e.session_mut().document, false).unwrap();
        e.tools.mask_target = true;
        assert!(e.can_invert());
        e.invert().unwrap();
        assert_eq!(
            e.session().document.layers[0].mask.as_ref().unwrap().pixels[(0, 0)][0],
            0
        );
        assert_eq!(e.session().undo_label(), Some("Invert Mask"));
    }

    #[test]
    fn uniform_mask_inversion_uses_source_pixels_after_scaling() {
        let mut e = editor();
        compositor::edits::add_mask(&mut e.session_mut().document, false).unwrap();
        let doc = &mut e.session_mut().document;
        doc.width = 40;
        doc.height = 40;
        doc.layers[0].transform.size = [40., 40.];
        doc.selection = Some(compositor::selection::Selection::rectangle(
            40,
            40,
            [0., 0.],
            [20., 40.],
            false,
        ));
        e.tools.mask_target = true;
        e.invert().unwrap();
        assert_eq!(
            e.session().document.layers[0]
                .mask
                .as_ref()
                .unwrap()
                .pixels
                .dimensions(),
            (4, 4)
        );
        let mask = &e.session().document.layers[0].mask.as_ref().unwrap().pixels;
        assert_eq!(mask[(0, 0)][0], 0);
        assert_eq!(mask[(3, 0)][0], 255);
        assert_eq!(e.session().undo_label(), Some("Invert Mask"));
    }
}
