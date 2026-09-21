use super::*;
use compositor::geometry::{Point, Transform};

impl Editor {
    pub(super) fn transform_pixel_size(&self, fallback: Point) -> Point {
        if let Some(edit) = &self.pending_pixels {
            return edit.pixel_size();
        }
        let Some(doc) = self.current_document() else {
            return fallback;
        };
        if doc.selected.len() != 1 {
            return fallback;
        }
        let Some(layer) = doc.active_layer() else {
            return fallback;
        };
        if self.tools.mask_target
            && let Some(mask) = &layer.mask
            && !mask.linked
        {
            // Swift uses the layer's size as 100% for an independent mask.
            // A linked mask transforms with the layer and uses its raster below.
            return layer.transform.size;
        }
        layer
            .raster()
            .map_or(fallback, |p| [p.width() as f64, p.height() as f64])
    }

    pub(super) fn update_transform_scale(&mut self, index: usize) {
        if !matches!(index, 2 | 3 | 5) {
            return;
        }
        let fallback = compositor::transform::selection_bounds(
            &self.session().document,
            self.tools.mask_target,
        )
        .unwrap_or(Transform::new(1, 1))
        .size;
        let pixels = self.transform_pixel_size(fallback);
        let Some(Form::Edit {
            action: Action::Transform,
            fields,
            ..
        }) = &mut self.modal
        else {
            return;
        };
        let number = |i: usize| {
            fields
                .get(i)
                .and_then(|f| f.1.parse::<f64>().ok())
                .filter(|n| n.is_finite())
        };
        if index == 5 {
            let (Some(scale), Some(x), Some(y), Some(w), Some(h)) =
                (number(5), number(0), number(1), number(2), number(3))
            else {
                return;
            };
            if scale <= 0. {
                return;
            }
            let size = pixels.map(|v| v * scale / 100.);
            let origin = [x + (w - size[0]) / 2., y + (h - size[1]) / 2.];
            if !size.iter().chain(origin.iter()).all(|v| v.is_finite()) {
                return;
            }
            for (i, value) in origin.into_iter().chain(size).enumerate() {
                if let Some(field) = fields.get_mut(i) {
                    field.1 = value.to_string();
                }
            }
            if let Some(link) = &mut self.dimension_link {
                link.ratio = size[0] / size[1];
            }
        } else if let Some(width) = number(2)
            && let Some(field) = fields.get_mut(5)
        {
            field.1 = (width / pixels[0] * 100.).to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_scale_keeps_center_uses_original_pixels_and_preserves_assets() {
        let mut editor = Editor::with_test_document();
        let mut doc = Document::new(100, 50).unwrap();
        compositor::edits::fill(&mut doc, [100, 50, 25, 255], false, false).unwrap();
        doc.layers[0].transform.size = [200., 100.];
        doc.layers[0].transform.origin = [10., 20.];
        doc.layers[0].transform.rotation = 30.;
        editor.tabs = vec![Session::new(doc.clone(), None).into()];
        editor.open_form(Action::Transform);
        editor.update_dimension(5, "50");
        editor.update_transform_scale(5);
        editor.update_dimension(5, "150");
        editor.update_transform_scale(5);
        editor.update_dimension(6, "nearest");
        let (mut cx, view) = quickgui::Application::new()
            .into_test_context(
                quickgui::WindowOptions::new("Scale").size(1280., 1000.),
                editor,
            )
            .unwrap();
        cx.click(view.window_handle(), "form-apply").unwrap();
        let changed = cx.read(view, |e| e.session().document.clone()).unwrap();
        let t = changed.layers[0].transform;
        assert_eq!(t.size, [150., 75.]);
        assert_eq!(t.origin, [35., 32.5]);
        assert_eq!(t.rotation, 30.);
        assert_eq!(t.sampling, compositor::geometry::Sampling::Nearest);
        assert!(Arc::ptr_eq(
            changed.layers[0].raster().unwrap(),
            doc.layers[0].raster().unwrap()
        ));
        cx.update(view, |e, _| e.session_mut().undo()).unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            doc
        );
    }

    #[test]
    fn mask_scale_uses_layer_pixels_when_linked_and_layer_size_when_independent() {
        let mut editor = Editor::with_test_document();
        let mut doc = Document::new(100, 80).unwrap();
        doc.layers[0].content = compositor::document::LayerContent::Raster(Some(Arc::new(
            image::RgbaImage::new(50, 40),
        )));
        compositor::edits::add_mask(&mut doc, false).unwrap();
        editor.tabs = vec![Session::new(doc, None).into()];
        editor.tools.mask_target = true;
        assert_eq!(editor.transform_pixel_size([100., 80.]), [50., 40.]);
        let mask = editor.session_mut().document.layers[0]
            .mask
            .as_mut()
            .unwrap();
        mask.linked = false;
        mask.placement = Some(Transform::new(20, 10));
        assert_eq!(editor.transform_pixel_size([20., 10.]), [100., 80.]);
    }
}
