use super::*;

pub(super) struct DimensionLink {
    width: usize,
    height: usize,
    pub(super) ratio: f64,
    pub(super) locked: bool,
}

impl DimensionLink {
    pub fn for_form(action: Action, fields: &[(&str, String)]) -> Option<Self> {
        let (width, height, locked) = match action {
            Action::Transform => (2, 3, true),
            Action::ImageSize => (0, 1, true),
            Action::CanvasSize => (0, 1, false),
            _ => return None,
        };
        let w = fields.get(width)?.1.parse::<f64>().ok()?;
        let h = fields.get(height)?.1.parse::<f64>().ok()?;
        (w.is_finite() && h.is_finite() && w > 0. && h > 0.).then_some(Self {
            width,
            height,
            ratio: w / h,
            locked,
        })
    }
}

impl Editor {
    pub(super) fn update_dimension(&mut self, index: usize, value: &str) {
        if matches!(
            self.modal,
            Some(Form::Edit {
                action: Action::ImageSize,
                ..
            })
        ) {
            self.image_size_input(index, value);
            return;
        }
        if matches!(index, 4 | 5)
            && matches!(
                self.modal,
                Some(Form::Edit {
                    action: Action::CanvasSize,
                    ..
                })
            )
        {
            let result = self.change_canvas_measurement(index, value);
            if let Some(Form::Edit { error, .. }) = &mut self.modal {
                *error = result
                    .err()
                    .map_or_else(String::new, |error| error.to_string());
            }
            return;
        }
        let Some(Form::Edit { action, fields, .. }) = &mut self.modal else {
            return;
        };
        if let Some(field) = fields.get_mut(index) {
            field.1 = value.to_owned();
        }
        let Some(link) = &self.dimension_link else {
            return;
        };
        if !link.locked
            || !matches!(
                action,
                Action::Transform | Action::ImageSize | Action::CanvasSize
            )
        {
            return;
        }
        let other = if index == link.width {
            link.height
        } else if index == link.height {
            link.width
        } else {
            return;
        };
        let Ok(number) = value.parse::<f64>() else {
            return;
        };
        if !number.is_finite() {
            return;
        }
        let percent = matches!(action, Action::CanvasSize)
            && fields
                .get(4)
                .is_some_and(|f| f.1.trim().eq_ignore_ascii_case("percent"));
        let ratio = if percent { 1. } else { link.ratio };
        let linked = if index == link.width {
            number / ratio
        } else {
            number * ratio
        };
        let pixels = matches!(action, Action::ImageSize)
            || (matches!(action, Action::CanvasSize)
                && fields
                    .get(4)
                    .is_some_and(|f| f.1.trim().eq_ignore_ascii_case("px")));
        if linked.is_finite()
            && let Some(field) = fields.get_mut(other)
        {
            field.1 = if pixels { linked.round() } else { linked }.to_string();
        }
    }

    /// Like CanvasSizeDraft, unit and relative-mode changes preserve the final pixel size.
    fn change_canvas_measurement(&mut self, index: usize, value: &str) -> Result<()> {
        let doc = &self.session().document;
        let original = [f64::from(doc.width), f64::from(doc.height)];
        let resolution = doc.resolution;
        let Some(Form::Edit {
            action: Action::CanvasSize,
            fields,
            ..
        }) = &mut self.modal
        else {
            return Ok(());
        };
        let factors = |unit: &str| -> Result<[f64; 2]> {
            match unit {
                "px" => Ok([1.; 2]),
                "percent" => Ok(original.map(|v| v / 100.)),
                "inches" => Ok([resolution; 2]),
                "cm" => Ok([resolution / 2.54; 2]),
                _ => Err(compositor::invalid(
                    "Choose pixels, percent, inches, or centimeters.",
                )),
            }
        };
        let relative = |value: &str| -> Result<bool> {
            match value {
                "0" => Ok(false),
                "1" => Ok(true),
                _ => Err(compositor::invalid("Relative dimensions accepts 0 or 1.")),
            }
        };
        let previous = factors(&fields[4].1)?;
        let previous_relative = relative(&fields[5].1)?;
        let next = factors(if index == 4 { value } else { &fields[4].1 })?;
        let next_relative = relative(if index == 5 { value } else { &fields[5].1 })?;
        let mut displayed = [0.; 2];
        for axis in 0..2 {
            let amount = fields[axis].1.parse::<f64>().ok().filter(|v| v.is_finite())
                .ok_or_else(|| compositor::invalid("Finish entering both canvas dimensions before changing units or relative mode. The current settings are preserved."))?;
            let pixels = amount * previous[axis]
                + if previous_relative {
                    original[axis]
                } else {
                    0.
                };
            displayed[axis] =
                (pixels - if next_relative { original[axis] } else { 0. }) / next[axis];
            if !displayed[axis].is_finite() {
                return Err(compositor::invalid(
                    "Canvas dimensions are too large to convert. Enter smaller dimensions; the current settings are preserved.",
                ));
            }
        }
        for (axis, amount) in displayed.into_iter().enumerate() {
            fields[axis].1 = amount.to_string();
        }
        fields[index].1 = value.into();
        Ok(())
    }

    pub(super) fn dimension_controls(
        &self,
        cx: &mut ViewContext<'_, Self>,
        action: Action,
    ) -> Element {
        let Some(link) = self.dimension_link.as_ref().filter(|_| {
            matches!(
                action,
                Action::Transform | Action::ImageSize | Action::CanvasSize
            )
        }) else {
            return div();
        };
        let control = match action {
            Action::CanvasSize => Self::check_control("Lock original aspect ratio", link.locked)
                .text_size(13.)
                .line_height(16.),
            Action::ImageSize => Self::check_control("Lock aspect ratio", link.locked)
                .text_size(13.)
                .line_height(16.)
                .disabled(!self.image_sizing.resamples()),
            _ => Self::control("Constrain proportions").gap(8.).child(
                if link.locked {
                    Icon::Link
                } else {
                    Icon::Unlink
                }
                .element(14.),
            ),
        };
        control
            .self_start()
            .id("dimension-link")
            .on_click(cx.listener("dimension-link", |this, cx| {
                let original_ratio =
                    this.session().document.width as f64 / this.session().document.height as f64;
                if let Some(link) = &mut this.dimension_link {
                    if !link.locked
                        && let Some(Form::Edit { action, fields, .. }) = &this.modal
                        && !matches!(action, Action::CanvasSize)
                        && let Some(current) = DimensionLink::for_form(*action, fields)
                    {
                        link.ratio = current.ratio;
                        if matches!(action, Action::ImageSize)
                            && matches!(this.image_sizing, super::image_size::ImageSizing::Percent)
                        {
                            link.ratio *= original_ratio;
                        }
                    }
                    link.locked = !link.locked;
                    if matches!(
                        this.modal,
                        Some(Form::Edit {
                            action: Action::Transform,
                            ..
                        })
                    ) {
                        this.tools.transform_ratio = link.locked;
                    }
                }
                if matches!(
                    &this.modal,
                    Some(Form::Edit {
                        action: Action::CanvasSize,
                        ..
                    })
                ) && this.dimension_link.as_ref().is_some_and(|link| link.locked)
                    && let Some(Form::Edit { fields, .. }) = &this.modal
                {
                    let width = fields[0].1.clone();
                    this.update_dimension(0, &width);
                }
                this.changed(cx);
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply_and_undo(editor: Editor) -> [(u32, u32); 2] {
        let (mut cx, view) = quickgui::Application::new()
            .into_test_context(
                quickgui::WindowOptions::new("Dimensions").size(1280., 900.),
                editor,
            )
            .unwrap();
        cx.click(view.window_handle(), "form-apply").unwrap();
        let applied = cx
            .read(view, |e| {
                (e.session().document.width, e.session().document.height)
            })
            .unwrap();
        cx.update(view, |e, _| e.session_mut().undo()).unwrap();
        let undone = cx
            .read(view, |e| {
                (e.session().document.width, e.session().document.height)
            })
            .unwrap();
        [applied, undone]
    }

    fn values(editor: &Editor) -> Vec<String> {
        match &editor.modal {
            Some(Form::Edit { fields, .. }) => {
                fields.iter().map(|(_, value)| value.clone()).collect()
            }
            _ => panic!("Expected dimension fields"),
        }
    }

    #[test]
    fn transform_link_toggle_changes_handle_constraint_and_survives_reopening() {
        let mut editor = Editor::with_test_document();
        editor.tabs = vec![Session::new(Document::new(200, 100).unwrap(), None).into()];
        editor.open_form(Action::Transform);
        let (mut cx, view) = quickgui::Application::new()
            .into_test_context(
                quickgui::WindowOptions::new("Transform dimensions").size(1280., 900.),
                editor,
            )
            .unwrap();
        cx.click(view.window_handle(), "dimension-link").unwrap();
        cx.update(view, |e, _| {
            assert!(!e.tools.transform_ratio);
            e.update_dimension(2, "300");
            assert_eq!(values(e)[3], "100");
            e.open_form(Action::Transform);
            assert!(!e.dimension_link.as_ref().unwrap().locked);
        })
        .unwrap();
        cx.click(view.window_handle(), "dimension-link").unwrap();
        assert!(cx.read(view, |e| e.tools.transform_ratio).unwrap());
    }

    #[test]
    fn image_size_links_either_axis_and_undo_restores_dimensions() {
        let mut editor = Editor::with_test_document();
        editor.tabs = vec![Session::new(Document::new(200, 100).unwrap(), None).into()];
        editor.open_form(Action::ImageSize);
        editor.update_dimension(0, "400");
        assert_eq!(&values(&editor)[..2], &["400", "200"]);
        editor.update_dimension(1, "150");
        assert_eq!(&values(&editor)[..2], &["300", "150"]);
        assert_eq!(apply_and_undo(editor), [(300, 150), (200, 100)]);
    }

    #[test]
    fn changing_canvas_units_or_relative_mode_preserves_requested_size() {
        let mut editor = Editor::with_test_document();
        editor.tabs = vec![Session::new(Document::new(200, 100).unwrap(), None).into()];
        editor.open_form(Action::CanvasSize);
        editor.update_dimension(4, "percent");
        assert_eq!(&values(&editor)[..2], &["100", "100"]);
        editor.update_dimension(5, "1");
        assert_eq!(&values(&editor)[..2], &["0", "0"]);
        editor.update_dimension(0, "25");
        editor.update_dimension(1, "10");
        editor.update_dimension(4, "px");
        assert_eq!(&values(&editor)[..2], &["50", "10"]);
        editor.update_dimension(5, "0");
        assert_eq!(&values(&editor)[..2], &["250", "110"]);
        assert_eq!(apply_and_undo(editor), [(250, 110), (200, 100)]);
    }

    #[test]
    fn canvas_link_uses_original_ratio_for_relative_pixels_and_equal_percentages() {
        let mut editor = Editor::with_test_document();
        editor.tabs = vec![Session::new(Document::new(200, 100).unwrap(), None).into()];
        editor.open_form(Action::CanvasSize);
        editor.update_dimension(0, "400");
        assert_eq!(&values(&editor)[..2], &["400", "100"]);
        editor.dimension_link.as_mut().unwrap().locked = true;
        editor.update_dimension(5, "1");
        editor.update_dimension(0, "-20");
        assert_eq!(&values(&editor)[..2], &["-20", "-10"]);
        editor.update_dimension(4, "percent");
        editor.update_dimension(0, "25");
        assert_eq!(&values(&editor)[..2], &["25", "25"]);
        assert_eq!(apply_and_undo(editor), [(250, 125), (200, 100)]);
    }

    #[test]
    fn all_canvas_anchors_preserve_source_pixels_and_mask_placement_with_one_undo() {
        use compositor::document::{LayerContent, Mask};
        use compositor::geometry::Transform;
        for (horizontal, x) in [("left", 0.), ("center", 0.5), ("right", 1.)] {
            for (vertical, y) in [("top", 0.), ("center", 0.5), ("bottom", 1.)] {
                for size in [[103, 85], [97, 75]] {
                    let mut doc = Document::new(100, 80).unwrap();
                    let pixels = Arc::new(image::RgbaImage::from_pixel(
                        10,
                        8,
                        image::Rgba([25, 90, 170, 255]),
                    ));
                    doc.layers[0].content = LayerContent::Raster(Some(pixels.clone()));
                    doc.layers[0].transform.origin = [24., 17.];
                    doc.layers[0].mask = Some(Mask {
                        pixels: Arc::new(image::GrayImage::from_pixel(3, 3, image::Luma([192]))),
                        enabled: true,
                        linked: false,
                        placement: Some(Transform {
                            origin: [21., 15.],
                            ..Transform::new(3, 3)
                        }),
                    });
                    let original = doc.clone();
                    let mut editor = Editor::with_test_document();
                    editor.tabs = vec![Session::new(doc, None).into()];
                    editor.open_form(Action::CanvasSize);
                    for (index, value) in [
                        (0, size[0].to_string()),
                        (1, size[1].to_string()),
                        (2, horizontal.into()),
                        (3, vertical.into()),
                    ] {
                        editor.update_dimension(index, &value);
                    }
                    let (mut cx, view) = quickgui::Application::new()
                        .into_test_context(
                            quickgui::WindowOptions::new("Canvas anchors").size(1280., 900.),
                            editor,
                        )
                        .unwrap();
                    cx.click(view.window_handle(), "form-apply").unwrap();
                    cx.read(view, |e| {
                        let doc = &e.session().document;
                        assert_eq!([doc.width, doc.height], size);
                        let shift = [
                            ((size[0] as f64 - 100.) * x).floor(),
                            ((size[1] as f64 - 80.) * y).floor(),
                        ];
                        assert_eq!(
                            doc.layers[0].transform.origin,
                            [24. + shift[0], 17. + shift[1]]
                        );
                        assert_eq!(
                            doc.layers[0]
                                .mask
                                .as_ref()
                                .unwrap()
                                .placement
                                .unwrap()
                                .origin,
                            [21. + shift[0], 15. + shift[1]]
                        );
                        assert!(Arc::ptr_eq(doc.layers[0].raster().unwrap(), &pixels));
                        doc.validate().unwrap();
                    })
                    .unwrap();
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
        }
    }
}
