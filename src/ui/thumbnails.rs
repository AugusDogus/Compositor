use super::*;
use compositor::{document::Layer, invalid, thumbnail::Thumbnails};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Clone)]
struct Source {
    layer: Layer,
    canvas: [u32; 2],
}
impl Source {
    fn matches(&self, layer: &Layer, canvas: [u32; 2]) -> bool {
        self.canvas == canvas
            && self.layer.transform == layer.transform
            && match (self.layer.raster(), layer.raster()) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
            && match (&self.layer.mask, &layer.mask) {
                (Some(a), Some(b)) => {
                    a.placement == b.placement && Arc::ptr_eq(&a.pixels, &b.pixels)
                }
                (None, None) => true,
                _ => false,
            }
    }
}
struct Entry {
    source: Source,
    images: Result<(Image, Option<Image>)>,
}

#[derive(Default)]
pub(super) struct ThumbnailCache {
    entries: HashMap<Uuid, Entry>,
    running: bool,
}

impl Editor {
    pub(super) fn start_thumbnails(&mut self, cx: &ViewContext<'_, Self>) {
        // Refresh the sidebar after release. Resizing full-resolution snapshots
        // during a stroke competes with painting and forces extra pixel copies.
        if self.thumbnails.running || self.live_canvas_edit().is_some() {
            return;
        }
        let Some(session) = self.tabs[self.current].session() else {
            return;
        };
        let doc = &session.document;
        self.thumbnails
            .entries
            .retain(|id, _| doc.layer(*id).is_some());
        let canvas = [doc.width, doc.height];
        // Bound each job to avoid retaining every layer snapshot during rapid edits.
        let sources: Vec<_> = doc
            .layers
            .iter()
            .filter(|layer| !layer.is_group() || layer.mask.is_some())
            .filter(|layer| {
                self.thumbnails
                    .entries
                    .get(&layer.id)
                    .is_none_or(|entry| !entry.source.matches(layer, canvas))
            })
            .take(16)
            .map(|layer| Source {
                layer: layer.clone(),
                canvas,
            })
            .collect();
        if sources.is_empty() {
            return;
        }
        let work = sources.clone();
        let completed_sources = sources.clone();
        let session = self.session().id;
        self.thumbnails.running = true;
        let launched = cx.spawn_background(
            move || work.iter().map(|source| Thumbnails::render(&source.layer, source.canvas)).collect::<Vec<_>>(),
            move |this, result, cx| {
                this.thumbnails.running = false;
                if this.tabs[this.current].id != session || !this.has_document() { cx.invalidate(); return; }
                match result {
                    Ok(results) => {
                        for (source, result) in completed_sources.into_iter().zip(results) {
                            let doc = &this.session().document;
                            if doc.layer(source.layer.id).is_none_or(|layer| !source.matches(layer, [doc.width, doc.height])) { continue; }
                            let images = result.and_then(|result| {
                                let layer = image(result.layer)?;
                                let mask = result.mask.map(image).transpose()?;
                                Ok((layer, mask))
                            });
                            if let Err(error) = &images { this.status = error.to_string(); }
                            this.thumbnails.entries.insert(source.layer.id, Entry { source, images });
                        }
                    }
                    Err(error) => {
                        this.status = format!("Layer thumbnail rendering failed: {error}. The project is preserved. Reopen it to retry.");
                        for source in completed_sources {
                            this.thumbnails.entries.insert(source.layer.id, Entry { source, images: Err(invalid(this.status.clone())) });
                        }
                    }
                }
                cx.invalidate();
            },
        );
        if let Err(error) = launched {
            self.thumbnails.running = false;
            self.status = format!(
                "Could not start layer thumbnail rendering: {error}. The project is preserved."
            );
            for source in sources {
                self.thumbnails.entries.insert(
                    source.layer.id,
                    Entry {
                        source,
                        images: Err(invalid(self.status.clone())),
                    },
                );
            }
        }
    }

    fn press_thumbnail(&mut self, id: Uuid, mask: bool, modifiers: Modifiers) -> Result<()> {
        self.pending_layer_click = None;
        if modifiers.contains(Modifiers::CONTROL) {
            if !self.can_edit_layers() {
                return Ok(());
            }
            self.finish_pending_edits()?;
            let mode = super::canvas::selection_mode(
                modifiers,
                compositor::selection::SelectionMode::Replace,
            );
            let antialiased = self.tools.selection_antialiased;
            let label = if mask {
                "Load Mask Selection"
            } else {
                "Load Layer Selection"
            };
            return self.session_mut().edit(label, |doc| {
                compositor::edits::load_selection_from(doc, id, mask, mode, antialiased)
            });
        }
        if self.session().document.active != Some(id)
            || !self.session().document.selected.contains(&id)
        {
            self.finish_pending_edits()?;
        } else {
            // Swift selectLayerTarget resolves a gradient even on the same thumbnail,
            // but leaves an ordinary transform pending on the same layer.
            self.commit_gradient()?;
        }
        let shift = modifiers.contains(Modifiers::SHIFT);
        if mask && shift {
            self.session_mut().select_layer(id, false);
            self.tools.mask_target = true;
            if !self.can_edit_layers() {
                return Ok(());
            }
            let label = if self
                .session()
                .document
                .active_layer()
                .and_then(|layer| layer.mask.as_ref())
                .is_some_and(|mask| mask.enabled)
            {
                "Disable Layer Mask"
            } else {
                "Enable Layer Mask"
            };
            return self.session_mut().edit(label, |doc| {
                if let Some(mask) = doc.active_layer_mut().and_then(|layer| layer.mask.as_mut()) {
                    mask.enabled = !mask.enabled;
                }
                Ok(())
            });
        }
        self.pending_layer_click = (!shift).then_some(id);
        if shift || !self.session().document.selected.contains(&id) {
            self.session_mut().select_layer(id, shift);
        } else {
            self.session_mut().select_layer(id, true);
        }
        self.tools.mask_target = mask;
        Ok(())
    }

    pub(super) fn layer_thumbnails(
        &self,
        cx: &mut ViewContext<'_, Self>,
        layer: &Layer,
    ) -> Element {
        let images = self
            .thumbnails
            .entries
            .get(&layer.id)
            .and_then(|entry| entry.images.as_ref().ok());
        let mut row = div().flex_row().items_center().gap(0.).flex_shrink(0.);
        for mask in [false, true] {
            if mask && layer.mask.is_none() {
                continue;
            }
            if mask
                && let Some(matte) = &layer.mask
                && matches!(layer.content, compositor::document::LayerContent::Raster(_))
            {
                row = row.child(self.mask_link_slot(cx, layer, matte));
            } else if mask {
                row = row.child(div().w(5.).flex_shrink_0());
            }
            let image =
                images.and_then(|(pixels, matte)| if mask { matte.as_ref() } else { Some(pixels) });
            let icon = if mask {
                None
            } else {
                match &layer.content {
                    compositor::document::LayerContent::Group => Some(Icon::Folder),
                    compositor::document::LayerContent::Adjustment(adjustment) => {
                        Some(match adjustment.kind {
                            Kind::HueSaturation => Icon::Adjustment,
                            Kind::Levels => Icon::Settings2,
                            Kind::Curves => Icon::Curves,
                            Kind::Exposure => Icon::Exposure,
                            Kind::GradientMap => Icon::Palette,
                            Kind::Grain => Icon::Grain,
                            Kind::GaussianBlur
                            | Kind::MotionBlur
                            | Kind::Invert
                            | Kind::BlackWhite
                            | Kind::ColorBalance => Icon::Palette,
                        })
                    }
                    _ => None,
                }
            };
            let edge = if mask {
                compositor::thumbnail::MASK_BOX
            } else {
                compositor::thumbnail::LAYER_BOX
            };
            let doc = &self.session().document;
            let size = if icon.is_some() {
                [edge as f32; 2]
            } else {
                compositor::thumbnail::fitted_size([doc.width, doc.height], edge).map(|v| v as f32)
            };
            let selected = doc.selected.len() == 1
                && doc.active == Some(layer.id)
                && self.tools.mask_target == mask;
            let content = if let Some(icon) = icon {
                icon.element(if layer.is_group() { 28.8 } else { 16. })
            } else if let Some(image) = image {
                quickgui::img(image).w(size[0]).h(size[1])
            } else {
                div()
            };
            let cursor_bounds = self
                .layer_list
                .cursors
                .rows
                .get(&layer.id)
                .and_then(|bounds| {
                    if mask {
                        bounds.mask.as_ref()
                    } else {
                        Some(&bounds.image)
                    }
                });
            let mut thumbnail = div()
                .report_bounds(cursor_bounds.cloned().unwrap_or_default())
                .relative()
                .w(edge as f32)
                .h(36.)
                .items_center()
                .justify_center()
                .child(
                    div()
                        .id(format!("layer-thumbnail-frame-{}-{mask}", layer.id))
                        .relative()
                        .w(size[0])
                        .h(size[1])
                        .rounded(3.)
                        .overflow_hidden()
                        .items_center()
                        .justify_center()
                        .child(content)
                        .child(
                            div()
                                .absolute()
                                .size_full()
                                .rounded(3.)
                                .border(if selected { 2. } else { 0. }, Color::rgb8(89, 147, 211)),
                        ),
                );
            if mask && layer.mask.as_ref().is_some_and(|mask| !mask.enabled) {
                thumbnail = thumbnail.child(
                    div()
                        .absolute()
                        .size_full()
                        .flex_row()
                        .items_center()
                        .justify_center()
                        .text_color(Color::rgb8(240, 70, 70))
                        .child(text("╱").text_size(32.).font_medium()),
                );
            }
            let id = layer.id;
            let session = self.session().id;
            let drag = cx.drag_listener(
                format!("layer-thumbnail-{id}-{mask}"),
                move |this, event, _| {
                    this.pending_layer_click = None;
                    quickgui::Drag::new(super::layer_drag::LayerDrag {
                        session,
                        layer: id,
                        operation: if event.modifiers.contains(Modifiers::ALT) {
                            if mask {
                                super::layer_drag::Transfer::CopyMask
                            } else {
                                super::layer_drag::Transfer::Copy
                            }
                        } else {
                            super::layer_drag::Transfer::Move
                        },
                    })
                },
            );
            row = row.child(
                thumbnail
                    .accessibility_label(format!("Select {}: {}", if mask { "mask" } else { "image" }, layer.name))
                    .tooltip(if mask { "Select layer mask; Shift-click to enable/disable; Ctrl-click to select its black areas (Ctrl-Shift adds, Ctrl-Alt subtracts)" } else { "Select image pixels" })
                    .when(self.can_edit_layers(), |thumbnail| thumbnail.on_drag(drag))
                    .on_mouse_down(
                        quickgui::MouseButton::Left,
                        cx.mouse_down_listener(
                            format!("layer-thumbnail-{id}-{mask}"),
                            move |this, event, cx| {
                                cx.stop_propagation();
                                let result = this.press_thumbnail(id, mask, event.modifiers);
                                this.result(result, cx);
                            },
                        ),
                    ),
            );
        }
        if layer.mask.is_none() {
            row = row.child(div().w(5.));
        }
        row
    }
}

fn image(pixels: image::RgbaImage) -> Result<Image> {
    Image::from_rgba(pixels.width(), pixels.height(), pixels.into_raw()).map_err(|error| {
        invalid(format!(
            "Could not display a layer thumbnail: {error}. The project is preserved."
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use compositor::document::{LayerContent, Mask};
    use quickgui::{Application, MouseButton, MouseDownEvent, Point, WindowOptions};

    #[test]
    fn thumbnail_frames_round_to_whole_points_and_keep_a_one_point_minimum() {
        let editor = Editor::with_test_document();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Thumbnail frames").size(1500., 900.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        for (canvas, image_size, mask_size) in [
            ([1003, 317], [36., 11.], [30., 9.]),
            ([317, 1003], [11., 36.], [9., 30.]),
            ([10_000, 1], [36., 1.], [30., 1.]),
            ([1, 10_000], [1., 36.], [1., 30.]),
        ] {
            let id = cx
                .update(view, |editor, cx| {
                    let mut document = Document::new(canvas[0], canvas[1]).unwrap();
                    compositor::edits::add_mask(&mut document, false).unwrap();
                    let id = document.layers[0].id;
                    editor.tabs[0].set_document(document, None);
                    editor.changed(cx);
                    id
                })
                .unwrap();
            for (mask, expected) in [(false, image_size), (true, mask_size)] {
                let frame = cx
                    .element_bounds(window, format!("layer-thumbnail-frame-{id}-{mask}"))
                    .unwrap();
                assert_eq!([frame.width, frame.height], expected);
            }
        }
    }

    #[test]
    fn mask_link_slot_matches_the_layer_kind_and_unlinked_state() {
        let mut editor = Editor::with_test_document();
        editor.tabs = vec![Session::new(Document::new(8, 8).unwrap(), None).into()];
        compositor::edits::add_mask(&mut editor.session_mut().document, false).unwrap();
        let original = editor.session().document.clone();
        let id = original.layers[0].id;
        let link = format!("mask-link-{id}");
        let glyph = format!("mask-link-glyph-{id}");
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Mask link presentation").size(1500., 900.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        let bounds = cx.element_bounds(window, link.as_str()).unwrap();
        assert_eq!((bounds.width, bounds.height), (9., 20.));
        assert!(cx.contains_element(window, glyph.as_str()).unwrap());
        cx.click(window, link.as_str()).unwrap();
        assert_eq!(cx.element_bounds(window, link.as_str()).unwrap(), bounds);
        assert!(!cx.contains_element(window, glyph.as_str()).unwrap());
        let tree = cx.accessibility_update(window).unwrap();
        assert!(
            tree.nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Link mask: Layer 1"))
        );
        cx.update(view, |e, cx| e.action(Action::Undo, cx)).unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            original
        );
        for content in [
            LayerContent::Group,
            LayerContent::Adjustment(Box::new(compositor::adjustment::Adjustment::new(
                Kind::Levels,
            ))),
        ] {
            cx.update(view, |e, cx| {
                e.session_mut().document.layers[0].content = content;
                e.changed(cx);
            })
            .unwrap();
            assert!(!cx.contains_element(window, link.as_str()).unwrap());
            let image = cx
                .element_bounds(window, format!("layer-thumbnail-frame-{id}-false"))
                .unwrap();
            let mask = cx
                .element_bounds(window, format!("layer-thumbnail-frame-{id}-true"))
                .unwrap();
            assert_eq!(mask.x - image.x - image.width, 5.);
        }
    }

    #[test]
    fn selected_mask_border_renders_above_opaque_thumbnail_pixels() {
        let mut editor = Editor::with_test_document();
        compositor::edits::add_mask(&mut editor.session_mut().document, false).unwrap();
        editor.tools.mask_target = true;
        let layer = editor.session().document.layers[0].clone();
        let id = layer.id;
        let doc = &editor.session().document;
        let source = Source {
            layer,
            canvas: [doc.width, doc.height],
        };
        // The thumbnail must share the canvas aspect ratio so the border sample
        // has opaque pixels beneath it, rather than the image widget's letterbox.
        let pixels = image(image::RgbaImage::from_pixel(128, 80, image::Rgba([255; 4]))).unwrap();
        editor.thumbnails.entries.insert(
            id,
            Entry {
                source,
                images: Ok((pixels.clone(), Some(pixels))),
            },
        );
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Mask outline").size(1280., 900.), editor)
            .unwrap();
        let bounds = cx
            .element_bounds(
                view.window_handle(),
                format!("layer-thumbnail-frame-{id}-true"),
            )
            .unwrap();
        let frame = cx.capture_screenshot(view.window_handle()).unwrap();
        let scale = frame.width() as f32 / 1280.;
        let border = frame
            .pixel(
                ((bounds.x + 1.) * scale) as u32,
                ((bounds.y + bounds.height / 2.) * scale) as u32,
            )
            .unwrap();
        assert_eq!(border, [89, 147, 211, 255]);
        let center = frame
            .pixel(
                ((bounds.x + bounds.width / 2.) * scale) as u32,
                ((bounds.y + bounds.height / 2.) * scale) as u32,
            )
            .unwrap();
        assert_eq!(center, [255; 4]);
        cx.update(view, |e, cx| {
            e.session_mut()
                .document
                .add(Layer::blank("Second", 1280, 800))
                .unwrap();
            e.session_mut().document.select(id, true);
            e.changed(cx);
        })
        .unwrap();
        let frame = cx.capture_screenshot(view.window_handle()).unwrap();
        let bounds = cx
            .element_bounds(
                view.window_handle(),
                format!("layer-thumbnail-frame-{id}-true"),
            )
            .unwrap();
        assert_eq!(
            frame
                .pixel(
                    ((bounds.x + 1.) * scale) as u32,
                    ((bounds.y + bounds.height / 2.) * scale) as u32
                )
                .unwrap(),
            [255; 4],
            "Multiple selected layers must not show a single editing-target border"
        );
    }

    #[test]
    fn thumbnails_target_masks_and_load_coverage_without_changing_active_layers() {
        let mut e = Editor::with_test_document();
        let mut doc = Document::new(8, 8).unwrap();
        let source = doc.layers[0].id;
        doc.layers[0].content =
            LayerContent::Raster(Some(Arc::new(image::RgbaImage::from_fn(8, 8, |x, _| {
                image::Rgba([255, 0, 0, if x < 4 { 255 } else { 0 }])
            }))));
        doc.layers[0].mask = Some(Mask {
            pixels: Arc::new(image::GrayImage::from_fn(8, 8, |_, y| {
                image::Luma([if y < 4 { 0 } else { 255 }])
            })),
            enabled: true,
            linked: true,
            placement: None,
        });
        doc.add(Layer::blank("Other", 8, 8)).unwrap();
        let active = doc.active;
        let selected = doc.selected.clone();
        e.tabs = vec![Session::new(doc, None).into()];
        e.tools.selection_mode = compositor::selection::SelectionMode::Subtract;
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Thumbnails").size(1280., 900.), e)
            .unwrap();
        let window = view.window_handle();
        let press = |cx: &mut quickgui::TestAppContext, mask, modifiers| {
            cx.simulate_mouse_down(
                window,
                format!("layer-thumbnail-{source}-{mask}"),
                MouseDownEvent {
                    button: MouseButton::Left,
                    position: Point::new(1100., 220.),
                    modifiers,
                    click_count: 1,
                    first_mouse: false,
                },
            )
            .unwrap();
        };
        cx.click(window, format!("mask-link-{source}")).unwrap();
        cx.read(view, |e| {
            let doc = &e.session().document;
            assert_eq!(doc.active, active);
            assert!(!doc.layer(source).unwrap().mask.as_ref().unwrap().linked);
        })
        .unwrap();
        press(&mut cx, false, Modifiers::CONTROL);
        cx.read(view, |e| {
            let doc = &e.session().document;
            assert_eq!(doc.active, active);
            assert_eq!(doc.selected, selected);
            let selection = doc.selection.as_ref().unwrap();
            assert_eq!(selection.coverage([1.5, 6.5]), 1.);
            assert_eq!(selection.coverage([6.5, 1.5]), 0.);
        })
        .unwrap();
        press(&mut cx, true, Modifiers::CONTROL | Modifiers::SHIFT);
        cx.read(view, |e| {
            let doc = &e.session().document;
            assert_eq!(doc.active, active);
            assert!(!e.tools.mask_target);
            let selection = doc.selection.as_ref().unwrap();
            assert_eq!(selection.coverage([6.5, 1.5]), 1.);
            assert_eq!(selection.coverage([6.5, 6.5]), 0.);
        })
        .unwrap();
        press(&mut cx, true, Modifiers::empty());
        assert!(cx.read(view, |e| e.tools.mask_target).unwrap());
        press(&mut cx, true, Modifiers::SHIFT);
        assert!(
            !cx.read(view, |e| e
                .session()
                .document
                .layer(source)
                .unwrap()
                .mask
                .as_ref()
                .unwrap()
                .enabled)
                .unwrap()
        );
        cx.update(view, |e, _| e.session_mut().undo()).unwrap();
        assert!(
            cx.read(view, |e| e
                .session()
                .document
                .layer(source)
                .unwrap()
                .mask
                .as_ref()
                .unwrap()
                .enabled)
                .unwrap()
        );
        press(&mut cx, false, Modifiers::empty());
        assert!(!cx.read(view, |e| e.tools.mask_target).unwrap());
    }
}
