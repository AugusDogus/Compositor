use super::*;
use compositor::{
    crop::{Drag, Mode},
    geometry::{Point, Transform},
};

pub(super) struct CropPreview {
    pub frame: Transform,
    pub guides: [Option<f64>; 2],
}

impl Editor {
    /// EditorCanvas renders the union while cropping, preserving the original canvas outside it.
    pub(super) fn canvas_render_bounds(&self) -> [f64; 4] {
        let doc = &self.session().document;
        let original = [0., 0., f64::from(doc.width), f64::from(doc.height)];
        if self.tools.tool == Tool::Crop
            && let Some(crop) = &self.tools.pending_crop
        {
            let bounds = crop.frame.bounds();
            [
                bounds[0].min(0.),
                bounds[1].min(0.),
                bounds[2].max(original[2]),
                bounds[3].max(original[3]),
            ]
        } else {
            original
        }
    }

    pub(super) fn change_crop_ratio(&mut self) {
        if let Some(ratio) = self.current_crop_ratio() {
            let doc = &self.session().document;
            let mut frame = self
                .tools
                .pending_crop
                .as_ref()
                .map_or(Transform::new(doc.width, doc.height), |c| c.frame);
            let height = frame.size[0] / ratio;
            frame.origin[1] += (frame.size[1] - height) / 2.;
            frame.size[1] = height;
            frame = compositor::crop::rounded(frame);
            if compositor::crop::valid(frame) {
                self.tools.pending_crop = Some(CropPreview {
                    frame,
                    guides: [None; 2],
                });
            }
        }
    }

    pub(super) fn begin_crop(&mut self, point: Point, zoom: f64) -> Drag {
        let doc = &self.session().document;
        let canvas = Transform::new(doc.width, doc.height);
        let frame = self.tools.pending_crop.as_ref().map_or(canvas, |c| c.frame);
        let handle = Transform::HANDLES.iter().position(|u| {
            let p = frame.point(*u);
            (p[0] - point[0]).hypot(p[1] - point[1]) * zoom <= 8.
        });
        let unit = frame.unit(point);
        let mode = if let Some(handle) = handle {
            Mode::Resize(handle)
        } else if self.tools.pending_crop.is_some()
            && frame != canvas
            && unit.iter().all(|v| (0. ..=1.).contains(v))
        {
            Mode::Move
        } else {
            Mode::Create
        };
        let drag = Drag::new(doc, point, frame, mode).with_targets(self.tools.layout.targets(
            doc,
            &Default::default(),
            false,
        ));
        if matches!(mode, Mode::Create) {
            self.tools.pending_crop = None;
        }
        drag
    }

    pub(super) fn commit_crop(&mut self) -> Result<()> {
        if let Some(edit) = &self.tools.pending_crop {
            let frame = edit.frame;
            self.session_mut().edit("Crop", |doc| {
                compositor::edits::crop(
                    doc,
                    frame.origin,
                    [
                        frame.origin[0] + frame.size[0],
                        frame.origin[1] + frame.size[1],
                    ],
                )
            })?;
            self.tools.pending_crop = None;
            self.session_mut().fit = true;
        }
        Ok(())
    }

    pub(super) fn crop_controls(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        div()
            .flex_row()
            .gap(6.)
            .flex_shrink_0()
            .child(
                Self::tool_header_control("Cancel")
                    .disabled(self.tools.pending_crop.is_none())
                    .on_click(cx.listener("crop-cancel", |this, cx| {
                        this.tools.pending_crop = None;
                        this.changed(cx);
                    })),
            )
            .child(
                Self::tool_header_control("Apply Crop")
                    .disabled(self.tools.pending_crop.is_none())
                    .on_click(cx.listener("crop-apply", |this, cx| {
                        let result = this.commit_crop();
                        this.operation_result(alerts::Operation::Crop, result, cx);
                    })),
            )
    }

    pub(super) fn crop_overlay(&self, zoom: f64, offset: Point, backing_scale: f64) -> Element {
        let grid = self.guide_grid_shader.clone();
        let doc = &self.session().document;
        let frame = self
            .tools
            .pending_crop
            .as_ref()
            .map_or(Transform::new(doc.width, doc.height), |c| c.frame);
        quickgui::canvas(move |bounds, painter| {
            let x = (offset[0] + frame.origin[0] * zoom) as f32;
            let y = (offset[1] + frame.origin[1] * zoom) as f32;
            let w = (frame.size[0] * zoom) as f32;
            let h = (frame.size[1] * zoom) as f32;
            let left = x.clamp(0., bounds.width);
            let top = y.clamp(0., bounds.height);
            let right = (x + w).clamp(0., bounds.width);
            let bottom = (y + h).clamp(0., bounds.height);
            let dim = Color::BLACK.with_alpha(0.6);
            for r in [
                quickgui::Rect::new(0., 0., bounds.width, top),
                quickgui::Rect::new(0., bottom, bounds.width, bounds.height - bottom),
                quickgui::Rect::new(0., top, left, bottom - top),
                quickgui::Rect::new(right, top, bounds.width - right, bottom - top),
            ] {
                painter.fill_rect(r, dim);
            }
            for f in [0., 1.] {
                painter.fill_rect(quickgui::Rect::new(x + w * f - 0.5, y, 1., h), Color::WHITE);
                painter.fill_rect(quickgui::Rect::new(x, y + h * f - 0.5, w, 1.), Color::WHITE);
            }
            if let Some(region) =
                quickgui::Rect::new(x - 0.5, y - 0.5, w + 1., h + 1.).intersection(bounds)
            {
                painter.paint_shader(
                    region,
                    &grid,
                    quickgui::ShaderParameters::new()
                        .vector(0, [x - region.x, y - region.y, w, h])
                        .vector(1, [backing_scale as f32, 3., 1., 0.4]),
                );
            }
            for u in Transform::HANDLES {
                for (size, color) in [(9., Color::BLACK), (7., Color::WHITE)] {
                    painter.fill_rect(
                        quickgui::Rect::new(
                            x + w * u[0] as f32 - size / 2.,
                            y + h * u[1] as f32 - size / 2.,
                            size,
                            size,
                        ),
                        color,
                    );
                }
            }
        })
        .absolute()
        .size_full()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, WindowOptions};

    #[test]
    fn expanded_crop_previews_reveal_source_pixels_without_editing_the_document() {
        use compositor::document::LayerContent;
        let mut editor = Editor::with_test_document();
        let mut doc = Document::new(64, 64).unwrap();
        doc.layers[0].transform = Transform {
            origin: [-32., 0.],
            ..Transform::new(128, 64)
        };
        doc.layers[0].content = LayerContent::Raster(Some(Arc::new(image::RgbaImage::from_fn(
            128,
            64,
            |x, _| {
                image::Rgba(if x < 32 {
                    [210, 30, 20, 255]
                } else if x < 96 {
                    [30, 210, 20, 255]
                } else {
                    [30, 20, 210, 255]
                })
            },
        ))));
        editor.tabs = vec![Session::new(doc.clone(), None).into()];
        editor.session_mut().zoom = 4.;
        editor.session_mut().fit = false;
        editor.tools.tool = Tool::Idle;
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Expanded crop preview").size(1000., 700.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        let canvas = cx.element_bounds(window, "canvas").unwrap();
        let (zoom, offset) = cx
            .update(view, |e, _| e.viewport(canvas.width, canvas.height))
            .unwrap();
        let before = cx.capture_screenshot(window).unwrap();
        let scale = f64::from(before.width()) / 1000.;
        let pixel = |shot: &quickgui::VisualSnapshot, x: f64, y: f64| {
            shot.pixel(
                ((f64::from(canvas.x) + offset[0] + x * zoom) * scale) as u32,
                ((f64::from(canvas.y) + offset[1] + y * zoom) * scale) as u32,
            )
            .unwrap()
        };
        assert_eq!(pixel(&before, -24., 11.), [27, 27, 27, 255]);
        assert_eq!(pixel(&before, 88., 11.), [27, 27, 27, 255]);
        cx.update(view, |e, cx| {
            e.tools.tool = Tool::Crop;
            e.tools.pending_crop = Some(CropPreview {
                frame: Transform {
                    origin: [-32., -16.],
                    ..Transform::new(128, 96)
                },
                guides: [None; 2],
            });
            cx.invalidate();
        })
        .unwrap();
        let expanded = cx.capture_screenshot(window).unwrap();
        for (x, color) in [
            (-24., [210, 30, 20, 255]),
            (16., [30, 210, 20, 255]),
            (88., [30, 20, 210, 255]),
        ] {
            assert_eq!(pixel(&expanded, x, 11.), color, "Crop preview at {x}");
        }
        assert!(matches!(
            pixel(&expanded, 0., -8.),
            [77, 77, 77, 255] | [89, 89, 89, 255]
        ));
        cx.update(view, |e, cx| {
            e.tools.pending_crop = Some(CropPreview {
                frame: Transform {
                    origin: [16., 16.],
                    ..Transform::new(16, 16)
                },
                guides: [None; 2],
            });
            cx.invalidate();
        })
        .unwrap();
        let smaller = cx.capture_screenshot(window).unwrap();
        assert_eq!(
            pixel(&smaller, 5., 11.),
            [12, 84, 8, 255],
            "The original canvas must remain visible beneath crop dimming"
        );
        cx.update(view, |e, cx| {
            assert_eq!(e.session().document, doc);
            assert_eq!(e.session().undo_label(), None);
            e.tools.pending_crop = None;
            e.tools.tool = Tool::Idle;
            cx.invalidate();
        })
        .unwrap();
        let cancelled = cx.capture_screenshot(window).unwrap();
        for (x, y) in [(-24., 11.), (16., 11.), (88., 11.), (0., -8.)] {
            assert_eq!(pixel(&cancelled, x, y), pixel(&before, x, y));
        }
    }

    struct CropGuide(Editor);
    impl View for CropGuide {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            div()
                .size_full()
                .relative()
                .bg(Color::rgb8(100, 100, 100))
                .child(
                    self.0
                        .crop_overlay(1., [20.5, 20.5], f64::from(cx.scale_factor())),
                )
                .child(self.0.snap_guides_overlay(1., [20.5, 20.5]))
        }
    }

    #[test]
    fn crop_guides_use_white_framing_and_the_source_dimming_opacity() {
        let mut editor = Editor::with_test_document();
        editor.tools.tool = Tool::Crop;
        editor.tabs = vec![Session::new(Document::new(100, 100).unwrap(), None).into()];
        editor.tools.pending_crop = Some(CropPreview {
            frame: Transform {
                origin: [10., 10.],
                ..Transform::new(60, 60)
            },
            guides: [None; 2],
        });
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Crop guides")
                    .size(160., 160.)
                    .minimum_size(1., 1.),
                CropGuide(editor),
            )
            .unwrap();
        let frame = cx.capture_screenshot(view.window_handle()).unwrap();
        let scale = frame.width() as f32 / 160.;
        let pixel = |x: f32, y: f32| frame.pixel((x * scale) as u32, (y * scale) as u32).unwrap();
        assert_eq!(pixel(30.5, 40.5), [255; 4], "Crop boundary must be white");
        assert_eq!(
            pixel(30.5, 30.5),
            [255; 4],
            "Handles must have white centers"
        );
        assert_eq!(
            pixel(10.5, 10.5),
            [40, 40, 40, 255],
            "Outside must dim by 60%"
        );
        assert_eq!(
            pixel(50.5, 40.5),
            [162, 162, 162, 255],
            "Thirds must use white at 40%"
        );
        assert_eq!(
            pixel(50.5, 50.5),
            [162, 162, 162, 255],
            "Crossing thirds must blend only once"
        );
        assert_eq!(pixel(40.5, 40.5), [100, 100, 100, 255]);
        cx.update(view, |view, cx| {
            view.0.tools.pending_crop.as_mut().unwrap().guides[0] = Some(80.);
            cx.invalidate();
        })
        .unwrap();
        let guided = cx.capture_screenshot(view.window_handle()).unwrap();
        assert_eq!(
            guided.pixel((100.5 * scale) as u32, (60.5 * scale) as u32),
            Some([0, 122, 255, 255])
        );
        assert_eq!(
            guided.pixel((100.5 * scale) as u32, (150.5 * scale) as u32),
            Some([40, 40, 40, 255]),
            "Snap guides must end at the document edge"
        );
    }

    #[test]
    fn entering_crop_initializes_the_canvas_and_reselecting_preserves_the_frame() {
        let editor = Editor::with_test_document();
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Crop entry").size(1500., 900.), editor)
            .unwrap();
        cx.update(view, |e, cx| {
            e.tools.crop_ratio = Some(1.);
            e.tools.crop_picker.select_id("square");
            e.select_tool(Tool::Crop, cx);
            let doc = &e.session().document;
            assert_eq!(
                e.tools.pending_crop.as_ref().unwrap().frame,
                Transform::new(doc.width, doc.height)
            );
            assert_eq!(e.current_crop_ratio(), None);
            assert!(!e.can_edit_layers());
            let frame = Transform::new(10, 10);
            e.tools.pending_crop.as_mut().unwrap().frame = frame;
            e.select_tool(Tool::Crop, cx);
            assert_eq!(e.tools.pending_crop.as_ref().unwrap().frame, frame);
            e.select_tool(Tool::Move, cx);
            assert!(e.tools.pending_crop.is_none());
            assert!(e.can_edit_layers());
            e.select_tool(Tool::Crop, cx);
        })
        .unwrap();
        cx.click(view.window_handle(), "crop-cancel").unwrap();
        cx.read(view, |e| assert!(e.tools.pending_crop.is_none()))
            .unwrap();
    }
}
