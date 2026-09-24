use super::*;
use compositor::{
    brush::Stroke,
    geometry::{Point, Transform},
    selection::{Selection, SelectionMode},
    transform::Handle,
};

use super::canvas_preview::PreviewKey;

pub(super) enum Gesture {
    BrushTip(super::brush_tip::Drag),
    PixelTransform(super::floating::PixelDrag),
    HeaderTransform(Box<super::transform_header::HeaderDrag>),
    Crop(compositor::crop::Drag),
    Shape(super::shape_draft::ShapeDraft),
    Object {
        start: Point,
        end: Point,
        sample_all: bool,
        antialiased: bool,
        edge_offset: i8,
        mode: SelectionMode,
    },
    Text {
        start: Point,
        end: Point,
        force_new: bool,
    },
    Paint {
        id: uuid::Uuid,
        stroke: Box<Stroke>,
        smoothing: Option<super::brush_smoothing::Rope>,
    },
    Warp {
        id: uuid::Uuid,
        stroke: Box<compositor::warp::WarpStroke>,
    },
    Pixels {
        source: Box<compositor::floating::FloatingPixels>,
        start: Point,
        duplicate: bool,
    },
    SelectionMove {
        selection: Selection,
        start: Point,
    },
    Move {
        start: Point,
        duplicate_on_drag: bool,
        applied: Point,
        bounds: Transform,
        guides: [Option<f64>; 2],
    },
    Transform {
        original: Box<Document>,
        bounds: Transform,
        start: Point,
        handle: Handle,
    },
    Pan,
    Sample,
    Zoom {
        start: Point,
        zoom: f64,
        pan: Point,
        fit: bool,
        moved: bool,
    },
    Gradient(super::gradient::Endpoint),
    Region {
        anchor: Point,
        start: Point,
        end: Point,
        tool: Tool,
        base: Option<Selection>,
        mode: SelectionMode,
        constrain_armed: bool,
    },
    Lasso {
        points: Vec<Point>,
        base: Option<Selection>,
        mode: SelectionMode,
    },
}

pub(super) fn selection_mode(modifiers: Modifiers, choice: SelectionMode) -> SelectionMode {
    if modifiers.contains(Modifiers::ALT) {
        SelectionMode::Subtract
    } else if modifiers.contains(Modifiers::SHIFT) {
        SelectionMode::Add
    } else {
        choice
    }
}
pub(super) fn combine(
    base: &Option<Selection>,
    next: Selection,
    mode: SelectionMode,
) -> Result<Option<Selection>> {
    match (base, mode) {
        (None, SelectionMode::Subtract | SelectionMode::Intersect) => Ok(None),
        (None, _) => Ok(Some(next)),
        (Some(base), _) => base.combine(&next, mode).map(Some),
    }
}

impl Editor {
    fn zoom_canvas(&mut self, factor: f64, position: quickgui::Point) {
        if let Some(bounds) = self.canvas_bounds.bounds() {
            let anchor = [
                (position.x - bounds.x - bounds.width / 2.) as f64,
                (position.y - bounds.y - bounds.height / 2.) as f64,
            ];
            let s = self.session_mut();
            s.zoom_at(s.zoom * factor, anchor);
        }
    }

    pub(super) fn viewport(&mut self, width: f32, height: f32) -> (f64, Point) {
        let s = self.session_mut();
        if s.fit {
            s.zoom = (((width as f64 - 96.).max(1.) / s.document.width as f64)
                .min((height as f64 - 96.).max(1.) / s.document.height as f64)
                * s.backing_scale)
                .clamp(0.001, 32.);
        }
        let zoom = s.zoom / s.backing_scale;
        (
            zoom,
            [
                (width as f64 - s.document.width as f64 * zoom) / 2. + s.pan[0],
                (height as f64 - s.document.height as f64 * zoom) / 2. + s.pan[1],
            ],
        )
    }

    pub(super) fn canvas(
        &mut self,
        cx: &mut ViewContext<'_, Self>,
        width: f32,
        height: f32,
    ) -> Element {
        let backing_scale = cx.scale_factor() as f64;
        self.session_mut().set_backing_scale(backing_scale);
        if let Some(next) = self.advance_selection_scroll(std::time::Instant::now()) {
            cx.request_repaint_at(next);
        }
        let (zoom, offset) = self.viewport(width, height);
        let selection_outline = self.selection_outline(cx, [width, height], zoom, offset);
        let [min_x, min_y, max_x, max_y] = self.canvas_render_bounds();
        let render_scale = (zoom * backing_scale).min(1.);
        let origin = [
            ((-offset[0] / zoom).max(min_x) * render_scale).floor() / render_scale,
            ((-offset[1] / zoom).max(min_y) * render_scale).floor() / render_scale,
        ];
        let right = ((width as f64 - offset[0]) / zoom).min(max_x);
        let bottom = ((height as f64 - offset[1]) / zoom).min(max_y);
        let rw = ((right - origin[0]).max(0.) * render_scale).ceil().max(1.) as u32;
        let rh = ((bottom - origin[1]).max(0.) * render_scale).ceil().max(1.) as u32;
        let key = PreviewKey {
            session: self.session().id,
            live_edit: self.live_canvas_edit(),
            revision: self.revision,
            origin,
            size: [rw, rh],
            scale: render_scale,
            zoom,
        };
        self.request_canvas_preview(cx, key);
        self.request_clone_preview(cx, zoom, offset);
        let rendering_status = self.canvas_rendering_status(cx);
        let motion = cx.mouse_move_listener("canvas", |this, event, cx| {
            if let Some(bounds) = this.canvas_bounds.bounds() {
                let (zoom, offset) = this.viewport(bounds.width, bounds.height);
                let previous = this.canvas_cursor(zoom, offset);
                let entered = this.canvas_pointer.is_none();
                this.keyboard_modifiers = event.modifiers;
                // Captured motion is delivered outside the canvas too. Keep hover
                // separate from the active gesture so release restores the UI cursor.
                this.canvas_pointer = bounds.contains(event.position).then_some([
                    (event.position.x - bounds.x) as f64,
                    (event.position.y - bounds.y) as f64,
                ]);
                if entered
                    || this.tools.tool.has_brush_cursor()
                    || this.tools.polygon.is_some()
                    || previous != this.canvas_cursor(zoom, offset)
                {
                    cx.invalidate();
                }
            }
        });
        let hover = cx.hover_listener("canvas", |this, inside, cx| {
            this.canvas_pointer = if *inside {
                cx.pointer_position()
                    .zip(this.canvas_bounds.bounds())
                    .map(|(point, bounds)| {
                        [(point.x - bounds.x) as f64, (point.y - bounds.y) as f64]
                    })
            } else {
                None
            };
            cx.invalidate();
        });
        let focus = cx.focus_handle("workspace");
        let mouse_down = cx.mouse_down_listener("canvas", |this, event, cx| {
            if event.button == quickgui::MouseButton::Left
                && event.click_count >= 2
                && this.tools.tool == Tool::Move
                && !this.pending
                && this.modal.is_none()
                && let Some(bounds) = this.canvas_bounds.bounds()
            {
                let (zoom, offset) = this.viewport(bounds.width, bounds.height);
                let point = [
                    (event.position.x as f64 - bounds.x as f64 - offset[0]) / zoom,
                    (event.position.y as f64 - bounds.y as f64 - offset[1]) / zoom,
                ];
                match this.edit_text_at(point) {
                    Ok(true) => {
                        this.changed(cx);
                        cx.prevent_default();
                        return;
                    }
                    Err(error) => {
                        this.result(Err(error), cx);
                        cx.prevent_default();
                        return;
                    }
                    Ok(false) => {}
                }
                if let Some(id) = compositor::transform::pick(&this.session().document, point, true)
                    && this
                        .session()
                        .document
                        .layer(id)
                        .is_some_and(|l| l.raw.is_some())
                {
                    let result = this.start_develop_layer(id);
                    this.result(result, cx);
                    cx.prevent_default();
                    return;
                }
            }
            if event.button == quickgui::MouseButton::Left
                && event.click_count >= 2
                && this.tools.tool == Tool::Polygon
                && this.tools.polygon.is_some()
                && !this.pending
                && this.modal.is_none()
                && !this.adjustment_sampling()
            {
                this.finish_polygon(cx);
                cx.prevent_default();
            }
        });
        let pointer = cx.pointer_listener("canvas", move |this, event, cx| {
            if this.develop.is_some()
                || this.pending
                || this.psd_conversion.is_some()
                || this.layout_drag.is_some()
            {
                return;
            }
            if this.space_pan || matches!(this.gesture, Some(Gesture::Pan)) {
                cx.focus(focus);
                let result = this.pointer(event);
                this.result(result, cx);
                return;
            }
            if this.picking_color() {
                cx.focus(focus);
                this.sample_picker(event);
                let result = this.preview_picker();
                this.result(result, cx);
                return;
            }
            if this.adjustment_sampling() {
                cx.focus(focus);
                let result = this.adjustment_sample_pointer(event);
                this.result(result, cx);
                return;
            }
            if this.modal.is_some() {
                if this.floating_panel_kind().is_some() {
                    cx.focus(focus);
                    cx.invalidate();
                    if matches!(this.tools.tool, Tool::Hand | Tool::Zoom) {
                        let result = this.pointer(event);
                        this.result(result, cx);
                    }
                }
                return;
            }
            cx.focus(focus);
            let result = this.pointer(event);
            if result.is_err() {
                this.gesture = None;
                this.transform_edit = None;
                this.pending_gradient = None;
                this.pending_pixels = None;
                this.tools.pending_crop = None;
                this.session_mut().cancel();
            }
            this.operation_result(alerts::Operation::Paint, result, cx);
        });
        let scroll = cx.scroll_wheel_listener("canvas", |this, event, cx| {
            if this.gesture.is_some() || this.pending {
                return;
            }
            let delta = event.delta.pixel_delta(24.);
            if event.modifiers.contains(Modifiers::CONTROL) {
                this.zoom_canvas((delta.y as f64 * 0.01).exp(), event.position);
            } else {
                let s = this.session_mut();
                s.fit = false;
                s.pan[0] += delta.x as f64;
                s.pan[1] += delta.y as f64;
            }
            cx.invalidate();
        });
        let pinch = cx.pinch_listener("canvas", |this, event, cx| {
            if this.gesture.is_none()
                && !this.pending
                && (this.modal.is_none() || this.floating_panel_kind().is_some())
            {
                this.zoom_canvas((1. + event.delta as f64).max(0.1), event.position);
                cx.invalidate();
            }
        });
        let doc = &self.session().document;
        let canvas_offset = [offset[0] + min_x * zoom, offset[1] + min_y * zoom];
        let canvas_size = [max_x - min_x, max_y - min_y];
        let canvas_rect = quickgui::Rect::new(
            canvas_offset[0] as f32,
            canvas_offset[1] as f32,
            (canvas_size[0] * zoom) as f32,
            (canvas_size[1] * zoom) as f32,
        );
        let mut surface = div()
            .id("canvas")
            .report_bounds(self.canvas_bounds.clone())
            .on_mouse_move(motion)
            .on_hover(hover)
            .on_pinch(pinch)
            .cursor(self.canvas_cursor(zoom, offset).style())
            .flex_1()
            .min_w(0.)
            .h_full()
            .relative()
            .overflow_hidden()
            .bg(Color::rgb8(27, 27, 27))
            .on_pointer(pointer)
            .on_drop(cx.drop_listener(
                "canvas",
                |this, drag: &super::layer_drag::LayerDrag, event, cx| {
                    this.drop_layer_on_canvas(drag, event.position, cx);
                },
            ))
            .on_drop(cx.drop_listener(
                "canvas",
                |this, files: &quickgui::DroppedFiles, event, cx| {
                    this.drop_files(files, Some(this.session().id), event, cx);
                },
            ))
            .on_mouse_down(quickgui::MouseButton::Left, mouse_down)
            .on_scroll_wheel(scroll)
            .child(super::canvas_background::shadow(canvas_rect))
            .child(super::canvas_background::checkerboard(
                [width, height],
                canvas_size,
                zoom,
                canvas_offset,
            ));
        if let Some(preview) = &self.preview {
            surface = surface.child(preview.element(canvas_rect, [min_x, min_y], zoom));
        }
        if self.tools.pixel_grid && zoom * backing_scale >= 8. {
            surface = surface.child(super::pixel_grid::overlay(
                &self.pixel_grid_shader,
                [width, height],
                [doc.width, doc.height],
                zoom,
                offset,
                backing_scale,
            ));
        }
        surface = surface.child(super::canvas_background::outline(
            canvas_rect,
            backing_scale,
        ));
        // Match TransformOverlay.swift: tool guides, selection, drafts, then snap guides.
        if self.tools.tool == Tool::Crop {
            surface = surface.child(self.crop_overlay(zoom, offset, backing_scale));
        } else if self.pending_gradient.is_some() {
            surface = surface.child(self.gradient_overlay(zoom, offset, backing_scale));
        } else if self.tools.tool == Tool::Move
            && (self.tools.show_transform_controls
                || self.transform_edit.is_some()
                || self.pending_pixels.is_some())
            && let Some(placement) = self.transform_placement()
        {
            surface = surface.child(super::transform_overlay::overlay(placement, zoom, offset));
        }
        surface = surface.child(selection_outline);
        match self.selection_draft_overlay(zoom, offset) {
            Ok(draft) => surface = surface.child(draft),
            Err(error) => self.status = error.to_string(),
        }
        match self.shape_draft_overlay(zoom, offset) {
            Ok(draft) => surface = surface.child(draft),
            Err(error) => self.status = error.to_string(),
        }
        let brush_cursor = match self.brush_cursor(zoom, offset) {
            Ok(cursor) => cursor,
            Err(error) => {
                self.status = error.to_string();
                div()
            }
        };
        surface
            .child(self.snap_guides_overlay(zoom, offset))
            .child(self.layout_overlay(cx, [width, height], zoom, offset, backing_scale))
            .child(rendering_status)
            .child(brush_cursor)
            .child(self.sample_ring_overlay())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, ScrollDelta, ScrollWheelEvent, Vector, WindowOptions};

    #[test]
    fn control_scroll_up_zooms_in_and_plain_scroll_preserves_platform_direction() {
        let mut editor = Editor::with_test_document();
        editor.session_mut().document = Document::new(1000, 1000).unwrap();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Scroll direction").size(1280., 850.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        let bounds = cx.element_bounds(window, "canvas").unwrap();
        let position =
            quickgui::Point::new(bounds.x + bounds.width / 2., bounds.y + bounds.height / 2.);
        for precise in [false, true] {
            let before = cx.read(view, |editor| editor.session().zoom).unwrap();
            for direction in [1., -1.] {
                cx.simulate_scroll_wheel(
                    window,
                    "canvas",
                    ScrollWheelEvent {
                        position,
                        delta: if precise {
                            ScrollDelta::Pixels(Vector::new(0., direction * 24.))
                        } else {
                            ScrollDelta::Lines(Vector::new(0., direction))
                        },
                        modifiers: Modifiers::CONTROL,
                        ..Default::default()
                    },
                )
                .unwrap();
                let zoom = cx.read(view, |editor| editor.session().zoom).unwrap();
                if direction > 0. {
                    assert!(zoom > before, "Scroll up must zoom in");
                } else {
                    assert!(
                        (zoom - before).abs() < 1e-8,
                        "Opposite scroll must restore zoom"
                    );
                }
            }
        }
        let (zoom, pan) = cx
            .read(view, |editor| (editor.session().zoom, editor.session().pan))
            .unwrap();
        cx.simulate_scroll_wheel(
            window,
            "canvas",
            ScrollWheelEvent {
                position,
                delta: ScrollDelta::Pixels(Vector::new(12., 24.)),
                ..Default::default()
            },
        )
        .unwrap();
        cx.read(view, |editor| {
            assert_eq!(editor.session().zoom, zoom);
            assert_eq!(editor.session().pan, [pan[0] + 12., pan[1] + 24.]);
        })
        .unwrap();
    }
}
