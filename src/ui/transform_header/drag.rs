//! Canvas drags share the inspector transaction and retain a stable warp source.
use super::*;
use crate::ui::floating::{DragKind, PixelDrag, Placement};

#[derive(Clone)]
pub(super) enum Draft {
    Affine(Transform),
    Perspective {
        corners: [Point; 4],
        original: Box<Document>,
        bounds: Transform,
    },
}

impl Draft {
    pub(super) fn placement(&self) -> Placement {
        match self {
            Self::Affine(t) => Placement::Affine(*t),
            Self::Perspective { corners, .. } => Placement::Perspective(*corners),
        }
    }
}

pub(in crate::ui) struct HeaderDrag {
    drag: PixelDrag,
    before: Draft,
    pub(in crate::ui) guides: [Option<f64>; 2],
}

impl HeaderDrag {
    pub(in crate::ui) fn cursor_drag(&self) -> &PixelDrag {
        &self.drag
    }
}

impl Editor {
    pub(in crate::ui) fn nudge_pending_transform(&mut self, delta: Point) -> Result<()> {
        if let Some(edit) = &self.transform_edit {
            if !edit.valid {
                return Err(invalid(
                    "Enter valid transform values, or Cancel to restore the layer.",
                ));
            }
            let mut draft = edit.current.clone();
            match &mut draft {
                Draft::Affine(t) => {
                    t.origin = [t.origin[0] + delta[0], t.origin[1] + delta[1]];
                }
                Draft::Perspective { corners, .. } => {
                    *corners = corners.map(|p| [p[0] + delta[0], p[1] + delta[1]]);
                }
            }
            self.restore_header_draft(draft)
        } else if let Some(edit) = &self.pending_pixels {
            let placement = match edit.placement {
                Placement::Affine(mut t) => {
                    t.origin = [t.origin[0] + delta[0], t.origin[1] + delta[1]];
                    Placement::Affine(t)
                }
                Placement::Perspective(corners) => {
                    Placement::Perspective(corners.map(|p| [p[0] + delta[0], p[1] + delta[1]]))
                }
            };
            self.preview_pixels(placement)
        } else {
            Ok(())
        }
    }

    pub(in crate::ui) fn transform_placement(&self) -> Option<Placement> {
        self.transform_edit
            .as_ref()
            .map(|e| e.current.placement())
            .or_else(|| self.pending_pixels.as_ref().map(|e| e.placement))
            .or_else(|| {
                compositor::transform::selection_bounds(
                    &self.session().document,
                    self.tools.mask_target,
                )
                .map(Placement::Affine)
            })
    }

    pub(in crate::ui) fn header_drag(
        &mut self,
        point: Point,
        zoom: f64,
        modifiers: Modifiers,
    ) -> Result<Option<HeaderDrag>> {
        let Some(edit) = &self.transform_edit else {
            return Ok(None);
        };
        if !matches!(edit.source, TransformSource::Layers(_)) {
            return Ok(None);
        }
        if !edit.valid {
            return Err(invalid(
                "Enter valid transform values, or Cancel to restore the layer.",
            ));
        }
        let drag = PixelDrag::new(edit.current.placement(), point, zoom, modifiers, true);
        if matches!(drag.kind, DragKind::Move) && modifiers.contains(Modifiers::ALT) {
            self.finish_header_transform(true)?;
            return Ok(None);
        }
        Ok(Some(HeaderDrag {
            drag,
            before: edit.current.clone(),
            guides: [None; 2],
        }))
    }

    pub(in crate::ui) fn begin_header_distortion(
        &mut self,
        point: Point,
        zoom: f64,
        modifiers: Modifiers,
    ) -> Result<Option<HeaderDrag>> {
        self.begin_header_transform()?;
        self.header_drag(point, zoom, modifiers)
    }

    pub(in crate::ui) fn drag_header(
        &mut self,
        drag: &mut HeaderDrag,
        point: Point,
        zoom: f64,
        modifiers: Modifiers,
    ) -> Result<()> {
        let mut placement = drag
            .drag
            .placement(point, modifiers, self.tools.transform_ratio);
        if matches!(drag.drag.kind, DragKind::Move) {
            let bounds = drag.drag.original.bounds();
            let next = placement.bounds();
            let mut delta = [
                next.origin[0] - bounds.origin[0],
                next.origin[1] - bounds.origin[1],
            ];
            drag.guides = [None; 2];
            if !modifiers.contains(Modifiers::CONTROL) {
                (delta, drag.guides) = compositor::transform::snap(
                    &self.session().document,
                    bounds,
                    delta,
                    10. / zoom,
                );
            }
            if modifiers.contains(Modifiers::SHIFT) {
                if delta[0].abs() >= delta[1].abs() {
                    delta[1] = 0.;
                    drag.guides[1] = None;
                } else {
                    delta[0] = 0.;
                    drag.guides[0] = None;
                }
            }
            placement = drag.drag.original.following(Transform {
                origin: [bounds.origin[0] + delta[0], bounds.origin[1] + delta[1]],
                ..bounds
            });
        }
        match placement {
            Placement::Affine(next) => self.preview_header_transform(next)?,
            Placement::Perspective(corners) => {
                if !compositor::distort::usable_corners(corners) {
                    return Ok(());
                }
                let Some(edit) = &self.transform_edit else {
                    return Ok(());
                };
                let (original, bounds) = match &edit.current {
                    Draft::Affine(bounds) => (Box::new(self.session().document.clone()), *bounds),
                    Draft::Perspective {
                        original, bounds, ..
                    } => (original.clone(), *bounds),
                };
                self.restore_header_draft(Draft::Perspective {
                    corners,
                    original,
                    bounds,
                })?;
            }
        }
        if let Some(edit) = &mut self.transform_edit {
            edit.values = values(edit.current.placement().bounds(), edit.pixels);
        }
        Ok(())
    }

    fn restore_header_draft(&mut self, draft: Draft) -> Result<()> {
        match &draft {
            Draft::Affine(t) => self.preview_header_transform(*t)?,
            Draft::Perspective {
                corners,
                original,
                bounds,
            } => {
                let mut preview = original.as_ref().clone();
                compositor::distort::apply(
                    &mut preview,
                    *bounds,
                    *corners,
                    self.tools.mask_target,
                )?;
                preview.validate()?;
                self.session_mut().document = preview;
            }
        }
        if let Some(edit) = &mut self.transform_edit {
            edit.values = values(draft.placement().bounds(), edit.pixels);
            edit.current = draft;
            edit.valid = true;
        }
        Ok(())
    }

    pub(in crate::ui) fn cancel_header_drag(&mut self, drag: HeaderDrag) -> Result<()> {
        self.restore_header_draft(drag.before)
    }
}
