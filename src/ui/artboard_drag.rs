//! Frame gestures use a stable screen-space origin while the document can grow.
use super::*;
use compositor::{
    artboard,
    geometry::{Point, Transform},
    transform::{self, Handle},
};
use quickgui::{PointerEvent, PointerPhase};
use uuid::Uuid;
enum Mode {
    Move,
    Resize(Handle),
}
pub(super) struct Drag {
    id: Uuid,
    original: Document,
    frame: Transform,
    mode: Mode,
    start: Point,
    zoom: f64,
    pan: Point,
    fit: bool,
}
impl Editor {
    pub(super) fn artboard_down(
        &mut self,
        event: &PointerEvent,
        zoom: f64,
        offset: Point,
    ) -> Result<bool> {
        if !self.can_edit_layers() {
            return Ok(false);
        }
        let doc = &self.session().document;
        let label = doc
            .layers
            .iter()
            .rev()
            .filter(|l| l.visible && l.is_artboard())
            .find(|l| {
                super::artboard_overlay::label(l.transform, zoom, offset)
                    .contains(event.local_position)
            });
        let selected = self
            .active_artboard()
            .and_then(|id| doc.layer(id))
            .filter(|layer| doc.layer_is_visible(layer.id));
        let Some(layer) = label.or(selected) else {
            return Ok(false);
        };
        let point = [
            (f64::from(event.local_position.x) - offset[0]) / zoom,
            (f64::from(event.local_position.y) - offset[1]) / zoom,
        ];
        let handle = self
            .tools
            .show_transform_controls
            .then(|| {
                transform::hit_handle(
                    Transform::HANDLES.map(|u| layer.transform.geometry_point(u)),
                    None,
                    point,
                    zoom,
                )
            })
            .flatten();
        let mode = if label.is_some() {
            Mode::Move
        } else if let Some(handle) = handle {
            Mode::Resize(handle)
        } else if !(self.tools.transform_auto_select
            || event.modifiers.contains(Modifiers::CONTROL))
            || transform::pick(doc, point, event.modifiers.contains(Modifiers::CONTROL)).is_none()
        {
            Mode::Move
        } else {
            return Ok(false);
        };
        let id = layer.id;
        let frame = layer.transform;
        if label.is_some() {
            self.session_mut().select_layer(id, false);
        }
        self.tools.mask_target = false;
        let session = self.session();
        let drag = Drag {
            id,
            frame,
            mode,
            original: session.document.clone(),
            start: [
                f64::from(event.local_position.x),
                f64::from(event.local_position.y),
            ],
            zoom,
            pan: session.pan,
            fit: session.fit,
        };
        self.session_mut()
            .begin(if matches!(drag.mode, Mode::Move) {
                "Move Artboard"
            } else {
                "Resize Artboard"
            })?;
        self.gesture = Some(Gesture::Artboard(Box::new(drag)));
        Ok(true)
    }
    pub(super) fn finish_artboard_drag(&mut self, commit: bool) -> Result<()> {
        if !matches!(self.gesture, Some(Gesture::Artboard(_))) {
            return Ok(());
        }
        if let Some(Gesture::Artboard(drag)) = self.gesture.take() {
            if commit {
                if let Err(error) = self.session_mut().commit() {
                    self.session_mut().pan = drag.pan;
                    self.session_mut().fit = drag.fit;
                    return Err(error);
                }
            } else {
                self.session_mut().cancel();
                self.session_mut().pan = drag.pan;
                self.session_mut().fit = drag.fit;
            }
        }
        Ok(())
    }
    pub(super) fn artboard_pointer(&mut self, event: &PointerEvent) -> Result<()> {
        if event.phase == PointerPhase::Cancel {
            return self.finish_artboard_drag(false);
        }
        let Some(Gesture::Artboard(drag)) = &self.gesture else {
            return Ok(());
        };
        let at = [
            f64::from(event.local_position.x),
            f64::from(event.local_position.y),
        ];
        let delta = [
            (at[0] - drag.start[0]) / drag.zoom,
            (at[1] - drag.start[1]) / drag.zoom,
        ];
        let mut doc = drag.original.clone();
        let result = match drag.mode {
            Mode::Move => artboard::translate(
                &mut doc,
                drag.id,
                [
                    delta[0].max(-drag.frame.origin[0]),
                    delta[1].max(-drag.frame.origin[1]),
                ],
            ),
            Mode::Resize(handle) => {
                let mut frame = transform::drag(
                    drag.frame,
                    [0.; 2],
                    delta,
                    handle,
                    false,
                    event.modifiers.contains(Modifiers::SHIFT),
                    event.modifiers.contains(Modifiers::ALT),
                );
                // Pointer resizing stays in the canvas coordinate quadrant.
                // Negative coordinates remain available in explicit settings.
                for axis in 0..2 {
                    if frame.origin[axis] < 0. {
                        frame.size[axis] = (frame.size[axis] + frame.origin[axis]).max(1.);
                        frame.origin[axis] = 0.;
                    }
                }
                artboard::resize_frame(&mut doc, drag.id, frame)
            }
        };
        if let Err(error) = result {
            self.finish_artboard_drag(false)?;
            return Err(error);
        }
        let pan = [
            drag.pan[0] + (f64::from(doc.width) - f64::from(drag.original.width)) * drag.zoom / 2.,
            drag.pan[1]
                + (f64::from(doc.height) - f64::from(drag.original.height)) * drag.zoom / 2.,
        ];
        let session = self.session_mut();
        session.document = doc;
        session.pan = pan;
        if delta != [0.; 2] {
            session.fit = false;
        }
        if event.phase == PointerPhase::Up {
            self.finish_artboard_drag(true)?;
        }
        Ok(())
    }
}
