use super::canvas_content::CanvasContent;
use super::*;
use compositor::{document::LayerContent, geometry::Point, invalid, render};
use image::RgbaImage;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PreviewKey {
    pub session: Uuid,
    pub live_edit: Option<Uuid>,
    pub revision: u64,
    pub origin: Point,
    pub size: [u32; 2],
    pub scale: f64,
    pub zoom: f64,
}

pub(super) struct CanvasPreview {
    pub key: PreviewKey,
    pub image: Image,
}

impl CanvasPreview {
    pub(super) fn element(&self, rect: quickgui::Rect, origin: Point, zoom: f64) -> Element {
        // A completed frame may cover an older, wider crop. Clip its pixels immediately
        // when the crop shrinks or closes, while retaining useful pixels during rendering.
        div()
            .absolute()
            .left(0.)
            .top(0.)
            .size(rect.width, rect.height)
            .translate(rect.x, rect.y)
            .overflow_hidden()
            .child(
                quickgui::img(&self.image)
                    .absolute()
                    .left(0.)
                    .top(0.)
                    .w((f64::from(self.key.size[0]) / self.key.scale * zoom) as f32)
                    .h((f64::from(self.key.size[1]) / self.key.scale * zoom) as f32)
                    .translate(
                        ((self.key.origin[0] - origin[0]) * zoom) as f32,
                        ((self.key.origin[1] - origin[1]) * zoom) as f32,
                    ),
            )
    }
}

#[derive(Default)]
enum Work {
    #[default]
    Idle,
    Running(PreviewKey),
    Failed(PreviewKey),
}

#[derive(Default)]
pub(super) struct CanvasRendering {
    desired: Option<PreviewKey>,
    content: Option<CanvasContent>,
    revision: u64,
    work: Work,
    cache: render::DownsampleCache,
}

fn expensive(document: &Document, key: PreviewKey) -> bool {
    let samples = u64::from(key.size[0]) * u64::from(key.size[1]);
    samples >= 500_000
        || document.layers.iter().any(|layer| {
            (samples >= 65_536 && matches!(layer.content, LayerContent::Adjustment(_)))
                || layer
                    .raster()
                    .is_some_and(|p| u64::from(p.width()) * u64::from(p.height()) >= 4_000_000)
        })
}

fn calculate(
    document: &Document,
    key: PreviewKey,
    cache: &mut render::DownsampleCache,
) -> Result<RgbaImage> {
    let [width, height] = key.size;
    render::region_accelerated(
        document,
        width,
        height,
        key.origin,
        [1. / key.scale; 2],
        cache,
    )
}

impl Editor {
    pub(super) fn live_canvas_edit(&self) -> Option<Uuid> {
        match &self.gesture {
            Some(Gesture::Paint { id, .. } | Gesture::Warp { id, .. }) => Some(*id),
            _ => None,
        }
    }

    fn canvas_content(&self) -> Document {
        let mut document = self.preview_document();
        // Selection outlines and active-layer changes do not alter the composite.
        document.selection = None;
        document.active = None;
        document.selected.clear();
        document
    }

    pub(super) fn request_canvas_preview(
        &mut self,
        cx: &ViewContext<'_, Self>,
        mut key: PreviewKey,
    ) {
        let document = self.canvas_content();
        if self
            .canvas_rendering
            .content
            .as_ref()
            .is_none_or(|previous| !previous.matches(&document))
        {
            self.canvas_rendering.content = Some(CanvasContent::new(&document));
            self.canvas_rendering.revision = self.canvas_rendering.revision.wrapping_add(1);
        }
        key.revision = self.canvas_rendering.revision;
        self.canvas_rendering.desired = Some(key);
        if self
            .preview
            .as_ref()
            .is_some_and(|p| p.key.session != key.session)
        {
            self.preview = None;
        }
        if self.preview.as_ref().is_some_and(|p| p.key == key)
            || matches!(self.canvas_rendering.work, Work::Running(_))
            || matches!(self.canvas_rendering.work, Work::Failed(failed) if failed == key)
        {
            return;
        }
        let mut cache = std::mem::take(&mut self.canvas_rendering.cache);
        if !expensive(&document, key) {
            self.canvas_rendering.work = Work::Running(key);
            let pixels = calculate(&document, key, &mut cache);
            self.canvas_rendering.cache = cache;
            self.receive_canvas_preview(key, pixels);
            return;
        }
        // At most one snapshot and render are in flight. Subsequent edits only replace
        // the desired key; completion schedules a render of the latest document.
        self.canvas_rendering.work = Work::Running(key);
        let launched = cx.spawn_background(
            move || {
                let pixels = calculate(&document, key, &mut cache);
                (pixels, cache)
            },
            move |this, result, cx| {
                let result = result.map_err(|error| invalid(format!(
                    "Canvas rendering failed: {error}. Your project is preserved. Retry the preview."
                ))).and_then(|(pixels, cache)| {
                    this.canvas_rendering.cache = cache;
                    pixels
                });
                this.receive_canvas_preview(key, result);
                cx.invalidate();
            },
        );
        if let Err(error) = launched {
            self.receive_canvas_preview(key, Err(invalid(format!("Could not start canvas rendering: {error}. Your project is preserved. Retry the preview."))));
        }
    }

    fn receive_canvas_preview(&mut self, key: PreviewKey, result: Result<RgbaImage>) {
        if !matches!(self.canvas_rendering.work, Work::Running(running) if running == key) {
            return;
        }
        self.canvas_rendering.work = Work::Idle;
        // One worker renders snapshots while the pointer keeps advancing. Present
        // completed frames from this same live stroke instead of discarding every
        // frame until release. A unique edit ID excludes cancelled/replaced strokes.
        let live = key.live_edit.is_some() && key.live_edit == self.live_canvas_edit();
        if !self.has_document()
            || self.canvas_rendering.desired.is_none_or(|desired| {
                desired.session != key.session || (!live && desired.revision != key.revision)
            })
            || self.session().id != key.session
            || (!live
                && self
                    .canvas_rendering
                    .content
                    .as_ref()
                    .is_none_or(|content| !content.matches(&self.canvas_content())))
            || (result.is_err() && self.canvas_rendering.desired != Some(key))
        {
            return;
        }
        let result = result.and_then(|pixels| {
            Image::from_rgba(key.size[0], key.size[1], pixels.into_raw()).map_err(|error| {
                invalid(format!(
                    "Canvas preview failed: {error}. Your project is preserved. Retry the preview."
                ))
            })
        });
        match result {
            Ok(image) => self.preview = Some(CanvasPreview { key, image }),
            Err(error) => {
                self.canvas_rendering.work = Work::Failed(key);
                self.status = error.to_string();
            }
        }
    }

    pub(super) fn canvas_rendering_status(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        match self.canvas_rendering.work {
            // Normal repaint work retains the previous frame without flashing
            // implementation status over the canvas. Failures remain actionable.
            Work::Idle | Work::Running(_) => div().into_element(),
            Work::Failed(_) => Self::control("Retry preview")
                .id("canvas-retry")
                .absolute()
                .translate(12., 12.)
                .on_click(cx.listener("canvas-retry", |this, cx| {
                    this.canvas_rendering.work = Work::Idle;
                    cx.invalidate();
                }))
                .into_element(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RetainedCrop {
        preview: CanvasPreview,
        bounds: [f64; 4],
    }

    impl View for RetainedCrop {
        fn render(&mut self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let [left, top, right, bottom] = self.bounds;
            div()
                .size_full()
                .bg(Color::WHITE)
                .child(self.preview.element(
                    quickgui::Rect::new(
                        (20. + left) as f32,
                        (20. + top) as f32,
                        (right - left) as f32,
                        (bottom - top) as f32,
                    ),
                    [left, top],
                    1.,
                ))
        }
    }

    #[test]
    fn retained_expanded_frames_clip_immediately_when_the_crop_closes() {
        let preview = CanvasPreview {
            key: PreviewKey {
                origin: [-10., -10.],
                size: [40, 40],
                ..key(&Editor::with_test_document())
            },
            image: Image::from_rgba(
                40,
                40,
                image::RgbaImage::from_pixel(40, 40, image::Rgba([210, 30, 20, 255])).into_raw(),
            )
            .unwrap(),
        };
        let (mut cx, view) = quickgui::Application::new()
            .into_test_context(
                quickgui::WindowOptions::new("Retained crop frame").size(60., 60.),
                RetainedCrop {
                    preview,
                    bounds: [-10., -10., 30., 30.],
                },
            )
            .unwrap();
        let window = view.window_handle();
        let expanded = cx.capture_screenshot(window).unwrap();
        let scale = expanded.width() / 60;
        for (x, y) in [(15, 25), (25, 15), (45, 25), (25, 45), (25, 25)] {
            assert_eq!(
                expanded.pixel(x * scale, y * scale),
                Some([210, 30, 20, 255])
            );
        }
        cx.update(view, |sample, cx| {
            sample.bounds = [0., 0., 20., 20.];
            cx.invalidate();
        })
        .unwrap();
        let closed = cx.capture_screenshot(window).unwrap();
        for (x, y) in [(15, 25), (25, 15), (45, 25), (25, 45)] {
            assert_eq!(closed.pixel(x * scale, y * scale), Some([255; 4]));
        }
        assert_eq!(
            closed.pixel(25 * scale, 25 * scale),
            Some([210, 30, 20, 255])
        );
    }

    fn key(editor: &Editor) -> PreviewKey {
        PreviewKey {
            session: editor.session().id,
            live_edit: editor.live_canvas_edit(),
            revision: editor.revision,
            origin: [0., 0.],
            size: [2, 2],
            scale: 1.,
            zoom: 1.,
        }
    }

    #[test]
    fn moving_brush_presents_completed_frames_without_reviving_cancelled_strokes() {
        let mut editor = Editor::with_test_document();
        editor.session_mut().begin("Paint").unwrap();
        let stroke = compositor::brush::Stroke::start(
            &mut editor.session_mut().document,
            [0.5, 0.5],
            Brush {
                diameter: 1.,
                ..Brush::default()
            },
            compositor::brush::PaintMode::Paint,
            false,
            false,
        )
        .unwrap();
        let id = Uuid::new_v4();
        editor.gesture = Some(Gesture::Paint {
            id,
            stroke: Box::new(stroke),
        });
        let first = key(&editor);
        let next = PreviewKey {
            revision: first.revision + 1,
            ..first
        };
        editor.canvas_rendering = CanvasRendering {
            desired: Some(next),
            content: Some(CanvasContent::new(&editor.canvas_content())),
            work: Work::Running(first),
            ..Default::default()
        };
        editor.receive_canvas_preview(first, Ok(RgbaImage::new(2, 2)));
        assert_eq!(
            editor.preview.as_ref().map(|p| p.key),
            Some(first),
            "A busy stroke must show completed intermediate frames instead of starving until release"
        );

        let cancelled = editor.gesture.take().unwrap();
        editor.session_mut().cancel();
        editor.canvas_rendering.work = Work::Running(next);
        editor.canvas_rendering.desired = Some(PreviewKey {
            revision: next.revision + 1,
            live_edit: None,
            ..next
        });
        editor.receive_canvas_preview(next, Ok(RgbaImage::new(2, 2)));
        assert_eq!(
            editor.preview.as_ref().unwrap().key,
            first,
            "Cancelled paint must not return"
        );
        let Gesture::Paint { stroke, .. } = cancelled else {
            unreachable!()
        };
        editor.gesture = Some(Gesture::Paint {
            id: Uuid::new_v4(),
            stroke,
        });
        editor.canvas_rendering.work = Work::Running(next);
        editor.receive_canvas_preview(next, Ok(RgbaImage::new(2, 2)));
        assert_eq!(
            editor.preview.as_ref().unwrap().key,
            first,
            "Starting a different stroke must not admit a cancelled stroke's frame"
        );
    }

    #[test]
    fn normal_repaints_are_silent_but_failed_previews_can_be_retried() {
        let mut editor = Editor::with_test_document();
        // The deterministic UI harness has no background worker pool. Keep the
        // retry synchronous so this test exercises a successful replacement frame.
        editor.session_mut().document = Document::new(2, 2).unwrap();
        editor.canvas_rendering.work = Work::Running(key(&editor));
        let (mut cx, view) = quickgui::Application::new()
            .into_test_context(
                quickgui::WindowOptions::new("Canvas feedback").size(900., 600.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        let tree = cx.accessibility_update(window).unwrap();
        assert!(
            !tree
                .nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Rendering..."))
        );
        assert!(!cx.contains_element(window, "canvas-retry").unwrap());
        cx.update(view, |e, cx| {
            e.canvas_rendering.work = Work::Failed(e.canvas_rendering.desired.unwrap());
            cx.invalidate();
        })
        .unwrap();
        assert!(cx.contains_element(window, "canvas-retry").unwrap());
        cx.click(window, "canvas-retry").unwrap();
        assert!(cx.read(view, |e| e.preview.is_some()).unwrap());
        assert!(!cx.contains_element(window, "canvas-retry").unwrap());
    }

    #[test]
    fn canvas_workers_reject_old_edits_tabs_and_duplicate_results() {
        let mut editor = Editor::with_test_document();
        let original = key(&editor);
        for desired in [
            PreviewKey {
                revision: 1,
                ..original
            },
            PreviewKey {
                session: Uuid::new_v4(),
                ..original
            },
        ] {
            editor.canvas_rendering = CanvasRendering {
                desired: Some(desired),
                content: Some(CanvasContent::new(&editor.canvas_content())),
                work: Work::Running(original),
                ..Default::default()
            };
            editor.receive_canvas_preview(original, Ok(RgbaImage::new(2, 2)));
            assert!(editor.preview.is_none());
            assert!(matches!(editor.canvas_rendering.work, Work::Idle));
        }
        editor.canvas_rendering = CanvasRendering {
            desired: Some(original),
            content: Some(CanvasContent::new(&editor.canvas_content())),
            work: Work::Running(original),
            ..Default::default()
        };
        editor.receive_canvas_preview(original, Ok(RgbaImage::new(2, 2)));
        assert_eq!(editor.preview.as_ref().unwrap().key, original);
        editor.receive_canvas_preview(original, Err(invalid("Late failure")));
        assert!(matches!(editor.canvas_rendering.work, Work::Idle));
    }

    #[test]
    fn panning_accepts_completed_unchanged_pixels_but_rejects_unrendered_edits() {
        let mut editor = Editor::with_test_document();
        editor.tabs = vec![Session::new(Document::new(100, 100).unwrap(), None).into()];
        let original = key(&editor);
        let desired = PreviewKey {
            origin: [2., 0.],
            ..original
        };
        editor.canvas_rendering = CanvasRendering {
            desired: Some(desired),
            content: Some(CanvasContent::new(&editor.canvas_content())),
            work: Work::Running(original),
            ..Default::default()
        };
        editor.session_mut().document.selection = Some(
            compositor::selection::Selection::rectangle(100, 100, [0., 0.], [100., 100.], false),
        );
        editor.revision += 1;
        editor.receive_canvas_preview(original, Ok(RgbaImage::new(2, 2)));
        assert_eq!(editor.preview.as_ref().unwrap().key, original);
        assert!(matches!(editor.canvas_rendering.work, Work::Idle));
        editor.canvas_rendering.work = Work::Running(desired);
        editor.session_mut().document.layers[0].opacity = 0.5;
        editor.receive_canvas_preview(desired, Ok(RgbaImage::new(2, 2)));
        assert_eq!(editor.preview.as_ref().unwrap().key, original);
    }

    #[test]
    fn render_identity_tracks_appearance_but_ignores_layer_names_and_selection() {
        let mut a = Document::new(4, 4).unwrap();
        compositor::edits::fill(&mut a, [80, 100, 120, 255], false, false).unwrap();
        compositor::edits::add_mask(&mut a, false).unwrap();
        let mut b = a.clone();
        b.layers[0].name = "Renamed".into();
        b.active = None;
        b.selected.clear();
        b.selection = Some(compositor::selection::Selection::rectangle(
            4,
            4,
            [0., 0.],
            [4., 4.],
            false,
        ));
        assert!(CanvasContent::new(&a).matches(&b));
        b.layers[0].mask.as_mut().unwrap().enabled = false;
        assert!(!CanvasContent::new(&a).matches(&b));
        b = a.clone();
        b.layers[0].transform.origin[0] += 1.;
        assert!(!CanvasContent::new(&a).matches(&b));
        b = a.clone();
        compositor::edits::fill(&mut b, [80, 100, 121, 255], false, false).unwrap();
        assert!(!CanvasContent::new(&a).matches(&b));
    }

    #[test]
    fn canvas_failures_keep_the_previous_frame_and_small_rendering_matches_composite() {
        let mut editor = Editor::with_test_document();
        editor.tabs = vec![Session::new(Document::new(2, 2).unwrap(), None).into()];
        compositor::edits::fill(
            &mut editor.session_mut().document,
            [80, 100, 120, 128],
            false,
            false,
        )
        .unwrap();
        let original = key(&editor);
        let pixels = calculate(
            &editor.session().document,
            original,
            &mut render::DownsampleCache::default(),
        )
        .unwrap();
        assert_eq!(pixels[(0, 0)], image::Rgba([80, 100, 120, 128]));
        editor.canvas_rendering = CanvasRendering {
            desired: Some(original),
            content: Some(CanvasContent::new(&editor.canvas_content())),
            work: Work::Running(original),
            ..Default::default()
        };
        editor.receive_canvas_preview(original, Ok(pixels));
        editor.canvas_rendering.work = Work::Running(original);
        editor.receive_canvas_preview(original, Err(invalid("Worker failed")));
        assert_eq!(editor.preview.as_ref().unwrap().key, original);
        assert!(matches!(editor.canvas_rendering.work, Work::Failed(_)));
        assert_eq!(editor.status, "Worker failed");
        assert!(!expensive(&editor.session().document, original));
        assert!(expensive(
            &editor.session().document,
            PreviewKey {
                size: [1000, 1000],
                ..original
            }
        ));
    }
}
