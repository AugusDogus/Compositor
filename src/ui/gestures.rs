use super::canvas::{Gesture, combine, selection_mode};
use super::*;
use compositor::{
    brush::{PaintMode, Stroke},
    geometry::Transform,
    invalid,
    transform::{self, Handle},
};
use quickgui::{MouseButton, PointerEvent, PointerPhase};

impl Editor {
    pub(super) fn pointer(&mut self, event: &PointerEvent) -> Result<()> {
        if !self.errors.is_empty() {
            return Ok(());
        }
        if self.brush_tip_pointer(event) {
            return Ok(());
        }
        let result = self.edit_pointer(event);
        if result.is_ok() {
            self.track_selection_scroll(event);
        } else {
            self.selection_scroll = None;
        }
        result
    }

    fn edit_pointer(&mut self, event: &PointerEvent) -> Result<()> {
        let (zoom, offset) = self.viewport(event.size.width, event.size.height);
        let point = [
            (event.local_position.x as f64 - offset[0]) / zoom,
            (event.local_position.y as f64 - offset[1]) / zoom,
        ];
        let point = if matches!(
            self.tools.tool,
            Tool::Shape | Tool::Rectangle | Tool::Ellipse
        ) {
            point.map(f64::round)
        } else {
            point
        };
        if event.phase == PointerPhase::Cancel {
            self.sample_ring = None;
            if matches!(self.gesture, Some(Gesture::Pan)) {
                self.gesture = None;
                return Ok(());
            }
            if let Some(Gesture::Zoom { zoom, pan, fit, .. }) = self.gesture {
                let session = self.session_mut();
                session.zoom = zoom;
                session.pan = pan;
                session.fit = fit;
                self.gesture = None;
                return Ok(());
            }
            if matches!(self.gesture, Some(Gesture::HeaderTransform(_))) {
                if let Some(Gesture::HeaderTransform(drag)) = self.gesture.take() {
                    self.cancel_header_drag(*drag)?;
                }
                return Ok(());
            }
            if matches!(self.gesture, Some(Gesture::PixelTransform(_))) {
                if let Some(Gesture::PixelTransform(drag)) = self.gesture.take() {
                    self.preview_pixels(drag.original)?;
                }
                return Ok(());
            }
            self.transform_edit = None;
            self.gesture = None;
            self.pending_gradient = None;
            self.pending_pixels = None;
            self.tools.pending_crop = None;
            self.session_mut().cancel();
            return Ok(());
        }
        if event.phase == PointerPhase::Down {
            self.finish_rename()?;
            if event.button != MouseButton::Left && event.button != MouseButton::Middle {
                return Ok(());
            }
            if event.button == MouseButton::Middle
                || self.tools.tool == Tool::Hand
                || self.space_pan
            {
                self.gesture = Some(Gesture::Pan);
                return Ok(());
            }
            if self.tools.tool == Tool::Move
                && let Some(drag) = self.header_drag(point, zoom, event.modifiers)?
            {
                self.gesture = Some(Gesture::HeaderTransform(Box::new(drag)));
                return Ok(());
            }
            self.finish_header_transform(true)?;
            if self.tools.tool == Tool::Idle {
                return Ok(());
            }
            if self.tools.tool == Tool::Eyedropper
                || (event.modifiers.contains(Modifiers::ALT)
                    && matches!(
                        self.tools.tool,
                        Tool::Brush | Tool::Erase | Tool::Heal | Tool::Gradient
                    ))
            {
                let original = self.tools.brush.color;
                self.sample_palette(point)?;
                self.update_sample_ring(event, original, self.tools.brush.color);
                self.gesture = Some(Gesture::Sample);
                return Ok(());
            }
            if let Some(drag) = self.pixel_drag(point, zoom, event.modifiers) {
                self.gesture = Some(Gesture::PixelTransform(drag));
                return Ok(());
            }
            let mode = selection_mode(event.modifiers, self.tools.selection_mode);
            if self.tools.tool == Tool::Polygon && self.tools.polygon.is_some() {
                return self.polygon_click(point, zoom, mode);
            }
            let selection_tool = matches!(
                self.tools.tool,
                Tool::Rectangle | Tool::Ellipse | Tool::Lasso | Tool::Polygon | Tool::Wand
            );
            if selection_tool
                && event.modifiers.contains(Modifiers::CONTROL)
                && self.can_edit_layers()
                && self
                    .session()
                    .document
                    .selection
                    .as_ref()
                    .is_some_and(|selection| selection.coverage(point) > 0.5)
            {
                if !self.can_float_selection() {
                    return Ok(());
                }
                let source = compositor::floating::FloatingPixels::lift(&self.session().document)?;
                let duplicate = event.modifiers.contains(Modifiers::ALT);
                self.session_mut().begin(if duplicate {
                    "Duplicate Pixels"
                } else {
                    "Move Pixels"
                })?;
                self.gesture = Some(Gesture::Pixels {
                    source: Box::new(source),
                    start: point,
                    duplicate,
                });
                return Ok(());
            }
            if selection_tool
                && mode == compositor::selection::SelectionMode::Replace
                && let Some(selection) = self.session().document.selection.clone()
                && selection.coverage(point) > 0.5
            {
                self.session_mut().begin("Move Selection")?;
                self.gesture = Some(Gesture::SelectionMove {
                    selection,
                    start: point,
                });
                return Ok(());
            }
            match self.tools.tool {
                Tool::Brush | Tool::Erase | Tool::Clone | Tool::Blur | Tool::Heal => {
                    if self.tools.tool == Tool::Clone && event.modifiers.contains(Modifiers::ALT) {
                        self.tools.clone_source = Some(point);
                        self.tools.clone_offset = None;
                        self.status = "Clone source set.".into();
                        return Ok(());
                    }
                    if !self.can_edit_pixels()
                        || (self.tools.mask_target
                            && matches!(self.tools.tool, Tool::Clone | Tool::Heal))
                    {
                        return Ok(());
                    }
                    let from = if event.modifiers.contains(Modifiers::SHIFT) {
                        self.tools
                            .last_brush
                            .filter(|(id, mask, _)| {
                                Some(*id) == self.session().document.active
                                    && *mask == self.tools.mask_target
                            })
                            .map_or(point, |(_, _, p)| p)
                    } else {
                        point
                    };
                    let mode = match self.tools.tool {
                        Tool::Erase if !self.tools.mask_target => PaintMode::Erase,
                        Tool::Blur => PaintMode::Blur,
                        Tool::Heal => PaintMode::Heal(self.tools.healing),
                        Tool::Clone => {
                            let offset = self.clone_stroke_offset(from).ok_or_else(|| {
                                invalid("Alt-click a clone source before painting.")
                            })?;
                            self.tools.clone_offset = Some(offset);
                            PaintMode::Clone { offset }
                        }
                        _ => PaintMode::Paint,
                    };
                    let mask = self.tools.mask_target;
                    let label = if mask {
                        "Paint Mask"
                    } else {
                        match mode {
                            PaintMode::Paint => "Brush Stroke",
                            PaintMode::Erase => "Erase",
                            PaintMode::Blur => "Blur",
                            PaintMode::Clone { .. } => "Clone Stamp",
                            PaintMode::Heal(_) => "Spot Healing",
                        }
                    };
                    self.session_mut().begin(label)?;
                    let mut brush = self.tools.brush;
                    brush.color = self.palette_colors(mask)[0];
                    let sample_all = self.tools.clone_sample_all;
                    match Stroke::start(
                        &mut self.session_mut().document,
                        from,
                        brush,
                        mode,
                        mask,
                        sample_all,
                    ) {
                        Ok(mut stroke) => {
                            stroke.to(&mut self.session_mut().document, point)?;
                            self.gesture = Some(Gesture::Paint {
                                id: uuid::Uuid::new_v4(),
                                stroke: Box::new(stroke),
                            });
                            self.tools.last_brush =
                                self.session().document.active.map(|id| (id, mask, point));
                        }
                        Err(error) => {
                            self.session_mut().cancel();
                            return Err(error);
                        }
                    }
                }
                Tool::Smudge | Tool::Liquify => {
                    if !self.can_edit_pixels() {
                        return Ok(());
                    }
                    let mode = if self.tools.tool == Tool::Smudge {
                        compositor::warp::Mode::Smudge
                    } else {
                        compositor::warp::Mode::Liquify
                    };
                    let stroke = compositor::warp::WarpStroke::start(
                        &self.session().document,
                        point,
                        mode,
                        self.tools.brush,
                        self.tools.mask_target,
                    )?;
                    self.session_mut()
                        .begin(if mode == compositor::warp::Mode::Smudge {
                            "Smudge"
                        } else {
                            "Liquify"
                        })?;
                    self.gesture = Some(Gesture::Warp {
                        id: uuid::Uuid::new_v4(),
                        stroke: Box::new(stroke),
                    });
                }
                Tool::Move => {
                    if self.tools.show_transform_controls
                        && let Some(bounds) = transform::selection_bounds(
                            &self.session().document,
                            self.tools.mask_target,
                        )
                    {
                        let handle = transform::hit_handle(
                            Transform::HANDLES.map(|u| bounds.geometry_point(u)),
                            Some(bounds.geometry_point([0.5, -28. / zoom / bounds.size[1]])),
                            point,
                            zoom,
                        );
                        if let Some(handle) = handle {
                            if event.modifiers.contains(Modifiers::CONTROL)
                                && let Handle::Resize(_) = handle
                            {
                                if let Some(drag) =
                                    self.begin_header_distortion(point, zoom, event.modifiers)?
                                {
                                    self.gesture = Some(Gesture::HeaderTransform(Box::new(drag)));
                                }
                                return Ok(());
                            }
                            let label = self.transform_history_label(false);
                            self.session_mut().begin(label)?;
                            self.gesture = Some(Gesture::Transform {
                                original: Box::new(self.session().document.clone()),
                                bounds,
                                start: point,
                                handle,
                            });
                            return Ok(());
                        }
                    }
                    let force = event.modifiers.contains(Modifiers::CONTROL);
                    if (self.tools.transform_auto_select || force)
                        && let Some(id) = transform::pick(&self.session().document, point, force)
                    {
                        self.session_mut()
                            .document
                            .select(id, event.modifiers.contains(Modifiers::SHIFT));
                        self.tools.mask_target = false;
                    }
                    let Some(bounds) = transform::selection_bounds(
                        &self.session().document,
                        self.tools.mask_target,
                    ) else {
                        return Ok(());
                    };
                    // Swift duplicates only a single pixel layer. Groups and multiple selections move.
                    let duplicate_on_drag = event.modifiers.contains(Modifiers::ALT)
                        && self.session().document.selected.len() == 1
                        && self
                            .session()
                            .document
                            .active_layer()
                            .is_some_and(|l| l.raster().is_some());
                    let label = if duplicate_on_drag {
                        "Duplicate Layer"
                    } else {
                        self.transform_history_label(false)
                    };
                    self.session_mut().begin(label)?;
                    self.gesture = Some(Gesture::Move {
                        start: point,
                        duplicate_on_drag,
                        applied: [0., 0.],
                        guides: [None; 2],
                        bounds,
                    });
                }
                Tool::Gradient => {
                    if self.pending_gradient.is_none() && !self.can_edit_pixels() {
                        return Ok(());
                    }
                    use super::gradient::Endpoint;
                    let endpoint = self.pending_gradient.as_ref().and_then(|edit| {
                        [(edit.start, Endpoint::Start), (edit.end, Endpoint::End)]
                            .into_iter()
                            .find(|(p, _)| (p[0] - point[0]).hypot(p[1] - point[1]) * zoom <= 8.)
                            .map(|(_, endpoint)| endpoint)
                    });
                    if let Some(endpoint) = endpoint {
                        self.gesture = Some(Gesture::Gradient(endpoint));
                    } else {
                        self.begin_gradient(point)?;
                        self.gesture = Some(Gesture::Gradient(Endpoint::End));
                    }
                }
                Tool::Crop => {
                    self.gesture = Some(Gesture::Crop(self.begin_crop(point, zoom)));
                }
                Tool::Shape => self.begin_shape(point)?,
                Tool::Rectangle | Tool::Ellipse => {
                    let label = match self.tools.tool {
                        Tool::Rectangle => "Rectangular Marquee",
                        Tool::Ellipse => "Elliptical Marquee",
                        _ => "Rectangular Marquee",
                    };
                    self.session_mut().begin(label)?;
                    self.gesture = Some(Gesture::Region {
                        anchor: point,
                        start: point,
                        end: point,
                        tool: self.tools.tool,
                        base: self.session().document.selection.clone(),
                        mode,
                        constrain_armed: !event.modifiers.contains(Modifiers::SHIFT),
                    });
                }
                Tool::Lasso => {
                    self.session_mut().begin("Lasso")?;
                    self.gesture = Some(Gesture::Lasso {
                        points: vec![point],
                        base: self.session().document.selection.clone(),
                        mode,
                    });
                }
                Tool::Polygon => self.polygon_click(point, zoom, mode)?,
                Tool::Wand => {
                    let (tolerance, radius, contiguous, sample_all) = (
                        self.tools.wand_tolerance,
                        self.tools.wand_radius,
                        self.tools.wand_contiguous,
                        self.tools.wand_sample_all,
                    );
                    if point[0] < 0. || point[1] < 0. {
                        return Ok(());
                    }
                    self.session_mut().edit("Magic Wand", |doc| {
                        let next = compositor::wand::select(
                            doc,
                            point,
                            compositor::wand::Settings {
                                tolerance,
                                contiguous,
                                radius,
                                sample_all,
                            },
                        )?;
                        doc.selection = combine(&doc.selection, next, mode)?;
                        Ok(())
                    })?;
                }
                Tool::Eyedropper => {}
                Tool::Zoom => {
                    let session = self.session();
                    self.gesture = Some(Gesture::Zoom {
                        start: [event.local_position.x as f64, event.local_position.y as f64],
                        zoom: session.zoom,
                        pan: session.pan,
                        fit: session.fit,
                        moved: false,
                    });
                }
                Tool::Idle | Tool::Hand => {}
            }
        } else if let Some(mut gesture) = self.gesture.take() {
            match &mut gesture {
                Gesture::BrushTip(_) => return Ok(()),
                Gesture::Sample => {
                    let original = self.tools.brush.color;
                    self.sample_palette(point)?;
                    self.update_sample_ring(event, original, self.tools.brush.color);
                }
                Gesture::Zoom {
                    start, zoom, moved, ..
                } => {
                    let dx = event.local_position.x as f64 - start[0];
                    *moved |= dx.abs() >= 3.;
                    let target = if *moved {
                        Some(*zoom * 2_f64.powf(dx / 100.))
                    } else if event.phase == PointerPhase::Up {
                        Some(
                            *zoom
                                * if event.modifiers.contains(Modifiers::ALT) {
                                    0.5
                                } else {
                                    2.
                                },
                        )
                    } else {
                        None
                    };
                    if let Some(target) = target {
                        self.session_mut().zoom_at(
                            target,
                            [
                                start[0] - event.size.width as f64 / 2.,
                                start[1] - event.size.height as f64 / 2.,
                            ],
                        );
                    }
                }
                Gesture::HeaderTransform(drag) => {
                    self.drag_header(drag, point, zoom, event.modifiers)?
                }
                Gesture::PixelTransform(drag) => self.drag_pixels(drag, point, event.modifiers)?,
                Gesture::Crop(drag) => {
                    if !matches!(drag.mode, compositor::crop::Mode::Create)
                        || (point[0] - drag.start[0]).hypot(point[1] - drag.start[1]) * zoom >= 3.
                    {
                        let ratio = if event.modifiers.contains(Modifiers::SHIFT) {
                            Some(1.)
                        } else {
                            self.current_crop_ratio()
                        };
                        let tolerance = if event.modifiers.contains(Modifiers::CONTROL) {
                            0.
                        } else {
                            6. / zoom
                        };
                        let (frame, guides) = drag.updated(
                            point,
                            ratio,
                            event.modifiers.contains(Modifiers::ALT),
                            tolerance,
                        );
                        if compositor::crop::valid(frame) {
                            self.tools.pending_crop =
                                Some(super::crop::CropPreview { frame, guides });
                        }
                    }
                }
                Gesture::Pixels {
                    source,
                    start,
                    duplicate,
                } => {
                    let mut transform = source.placement;
                    let delta = [(point[0] - start[0]).round(), (point[1] - start[1]).round()];
                    transform.origin[0] += delta[0];
                    transform.origin[1] += delta[1];
                    self.session_mut().document = source.preview(transform, *duplicate)?;
                }
                Gesture::SelectionMove { selection, start } => {
                    let mut delta = [point[0] - start[0], point[1] - start[1]];
                    if event.modifiers.contains(Modifiers::SHIFT) {
                        if delta[0].abs() > delta[1].abs() {
                            delta[1] = 0.;
                        } else {
                            delta[0] = 0.;
                        }
                    }
                    self.session_mut().document.selection = Some(selection.translated(delta)?);
                }
                Gesture::Warp { stroke, .. } => {
                    stroke.to(&mut self.session_mut().document, point)?
                }
                Gesture::Paint { stroke, .. } => {
                    self.tools.last_brush = self
                        .session()
                        .document
                        .active
                        .map(|id| (id, self.tools.mask_target, point));
                    if let Err(error) = stroke.to(&mut self.session_mut().document, point) {
                        self.session_mut().cancel();
                        return Err(error);
                    }
                }
                Gesture::Transform {
                    original,
                    bounds,
                    start,
                    handle,
                } => {
                    let new = transform::drag(
                        *bounds,
                        *start,
                        point,
                        *handle,
                        self.tools.transform_ratio,
                        event.modifiers.contains(Modifiers::SHIFT),
                        event.modifiers.contains(Modifiers::ALT),
                    );
                    let mut doc = original.as_ref().clone();
                    transform::apply(&mut doc, *bounds, new, self.tools.mask_target)?;
                    self.session_mut().document = doc;
                }
                Gesture::Move {
                    start,
                    duplicate_on_drag,
                    applied,
                    bounds,
                    guides,
                } => {
                    let mut target = [(point[0] - start[0]).round(), (point[1] - start[1]).round()];
                    if *duplicate_on_drag && target != [0., 0.] {
                        if let Err(error) = compositor::layer_ops::duplicate_active(
                            &mut self.session_mut().document,
                        ) {
                            self.session_mut().cancel();
                            return Err(error);
                        }
                        self.tools.mask_target = false;
                        *duplicate_on_drag = false;
                    }
                    *guides = [None; 2];
                    if !event.modifiers.contains(Modifiers::CONTROL) {
                        (target, *guides) =
                            transform::snap(&self.session().document, *bounds, target, 10. / zoom);
                    }
                    if event.modifiers.contains(Modifiers::SHIFT) {
                        if target[0].abs() > target[1].abs() {
                            target[1] = 0.;
                        } else {
                            target[0] = 0.;
                        }
                    }
                    let delta = [target[0] - applied[0], target[1] - applied[1]];
                    if let Some(old) = transform::selection_bounds(
                        &self.session().document,
                        self.tools.mask_target,
                    ) {
                        let mut new = old;
                        new.origin[0] += delta[0];
                        new.origin[1] += delta[1];
                        let mask = self.tools.mask_target;
                        transform::apply(&mut self.session_mut().document, old, new, mask)?;
                    }
                    *applied = target;
                }
                Gesture::Gradient(endpoint) => {
                    self.move_gradient(
                        point,
                        *endpoint,
                        event.modifiers.contains(Modifiers::SHIFT),
                    )?;
                }
                Gesture::Pan => {
                    let s = self.session_mut();
                    s.fit = false;
                    s.pan[0] += event.delta.x as f64;
                    s.pan[1] += event.delta.y as f64;
                }
                Gesture::Shape(draft) => draft.drag(point, event.modifiers),
                Gesture::Region {
                    anchor,
                    start,
                    end,
                    constrain_armed,
                    ..
                } => {
                    *start = *anchor;
                    *end = point;
                    if !event.modifiers.contains(Modifiers::SHIFT) {
                        *constrain_armed = true;
                    }
                    if event.modifiers.contains(Modifiers::SHIFT) && *constrain_armed {
                        let dx = point[0] - anchor[0];
                        let dy = point[1] - anchor[1];
                        let side = dx.abs().max(dy.abs());
                        *end = [
                            anchor[0] + if dx < 0. { -side } else { side },
                            anchor[1] + if dy < 0. { -side } else { side },
                        ];
                    }
                }
                Gesture::Lasso { points, .. } => {
                    if points
                        .last()
                        .is_none_or(|last| (last[0] - point[0]).hypot(last[1] - point[1]) >= 0.25)
                    {
                        points.push(point);
                    }
                }
            }
            if event.phase == PointerPhase::Up {
                if matches!(
                    gesture,
                    Gesture::PixelTransform(_)
                        | Gesture::HeaderTransform(_)
                        | Gesture::Sample
                        | Gesture::Zoom { .. }
                        | Gesture::Pan
                ) {
                    return Ok(());
                }
                if matches!(gesture, Gesture::Crop(_)) {
                    if let Some(edit) = &mut self.tools.pending_crop {
                        edit.guides = [None; 2];
                    }
                    self.status = "Crop preview. Drag handles to resize or drag inside to move. Enter applies, Escape cancels.".into();
                    return Ok(());
                }
                if matches!(gesture, Gesture::Gradient(_)) {
                    self.status = "Gradient preview. Enter applies, Escape cancels. Drag again to replace the line.".into();
                    return Ok(());
                }
                if let Gesture::Paint { stroke, .. } = &mut gesture
                    && let Err(error) = stroke.finish(&mut self.session_mut().document)
                {
                    self.session_mut().cancel();
                    return Err(error);
                }
                if super::selection_tools::is_deselect_gesture(&gesture) {
                    self.session_mut().cancel();
                    return self.session_mut().edit("Deselect", |doc| {
                        doc.selection = None;
                        Ok(())
                    });
                }
                if let Err(error) = self.finish_selection_draft(&gesture) {
                    self.session_mut().cancel();
                    return Err(error);
                }
                if let Gesture::Shape(draft) = gesture
                    && let Err(error) = self.finish_shape(draft)
                {
                    self.session_mut().cancel();
                    return Err(error);
                }
                self.session_mut().commit()?;
            } else {
                self.gesture = Some(gesture);
            }
        }
        Ok(())
    }
}
