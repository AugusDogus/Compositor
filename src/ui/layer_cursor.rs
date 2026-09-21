//! Layer cursor feedback and Alt-click clipping share the rendered row geometry.
use super::*;
use compositor::{clipping, document::Layer};
use quickgui::{LayoutBoundsHandle, MouseButton, MouseDownEvent, Point};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Default)]
pub(super) struct RowBounds {
    pub row: LayoutBoundsHandle,
    pub image: LayoutBoundsHandle,
    pub mask: Option<LayoutBoundsHandle>,
}

#[derive(Default)]
pub(super) struct State {
    pub pointer: Option<Point>,
    pub viewport: LayoutBoundsHandle,
    pub rows: HashMap<Uuid, RowBounds>,
}

#[derive(Clone, Copy)]
enum Target {
    Image,
    Mask,
    Body,
}

struct Hit {
    id: Uuid,
    target: Target,
    clipping_zone: bool,
}

impl State {
    pub fn prepare<'a>(&mut self, layers: impl Iterator<Item = &'a Layer>) {
        let mut rows = std::mem::take(&mut self.rows);
        self.rows = layers
            .map(|layer| {
                let mut bounds = rows.remove(&layer.id).unwrap_or_default();
                if layer.mask.is_some() {
                    bounds.mask.get_or_insert_default();
                } else {
                    bounds.mask = None;
                }
                (layer.id, bounds)
            })
            .collect();
    }

    fn hit(&self, point: Point) -> Option<Hit> {
        if !self.viewport.bounds()?.contains(point) {
            return None;
        }
        self.rows.iter().find_map(|(id, bounds)| {
            let row = bounds.row.bounds()?;
            if !row.contains(point) {
                return None;
            }
            let target = if bounds
                .mask
                .as_ref()
                .and_then(LayoutBoundsHandle::bounds)
                .is_some_and(|r| r.contains(point))
            {
                Target::Mask
            } else if bounds.image.bounds().is_some_and(|r| r.contains(point)) {
                Target::Image
            } else {
                Target::Body
            };
            Some(Hit {
                id: *id,
                target,
                clipping_zone: point.y >= row.y + row.height * 0.75,
            })
        })
    }
}

impl Editor {
    pub(super) fn layer_cursor_hovered(&self) -> bool {
        self.has_document()
            && self.rename.is_none()
            && self.modal.is_none()
            && self.layer_list.cursors.pointer.is_some_and(|point| {
                self.layer_list
                    .cursors
                    .viewport
                    .bounds()
                    .is_some_and(|bounds| bounds.contains(point))
            })
    }

    pub(super) fn layer_cursor(&self) -> tool_cursor::Cursor {
        use cursor_art::Glyph;
        use tool_cursor::Cursor;
        let arrow = Cursor::System(quickgui::CursorStyle::Arrow);
        if !self.can_edit_layers() || self.modal.is_some() {
            return arrow;
        }
        let Some(hit) = self
            .layer_list
            .cursors
            .pointer
            .and_then(|point| self.layer_list.cursors.hit(point))
        else {
            return arrow;
        };
        let Some(layer) = self.session().document.layer(hit.id) else {
            return arrow;
        };
        let modifiers = self.keyboard_modifiers;
        if modifiers.contains(Modifiers::CONTROL) {
            return match hit.target {
                Target::Image | Target::Mask => Cursor::Image(Glyph::LoadSelection),
                Target::Body => arrow,
            };
        }
        if !modifiers.contains(Modifiers::ALT) {
            return arrow;
        }
        if matches!(hit.target, Target::Mask) {
            return Cursor::Image(Glyph::Duplicate);
        }
        if hit.clipping_zone {
            return match clipping::change(&self.session().document, hit.id) {
                Some(clipping::Change::Create(_)) => Cursor::Image(Glyph::CreateClipping),
                Some(clipping::Change::Release) => Cursor::Image(Glyph::ReleaseClipping),
                None => arrow,
            };
        }
        if layer.is_group() {
            arrow
        } else {
            Cursor::Image(Glyph::Duplicate)
        }
    }

    pub(super) fn layer_clipping_press(&mut self, event: &MouseDownEvent, cx: &mut EventContext) {
        if event.button != MouseButton::Left
            || !event.modifiers.contains(Modifiers::ALT)
            || event.modifiers.contains(Modifiers::CONTROL)
        {
            return;
        }
        let Some(hit) = self.layer_list.cursors.hit(event.position) else {
            return;
        };
        if !hit.clipping_zone || matches!(hit.target, Target::Mask) {
            return;
        }
        // Consume even an unavailable clipping-zone press, matching NSTableView.
        cx.prevent_default();
        cx.stop_propagation();
        if !self.can_edit_layers() || clipping::change(&self.session().document, hit.id).is_none() {
            return;
        }
        self.pending_layer_click = None;
        let result = self
            .finish_pending_edits()
            .and_then(|()| self.toggle_clipping(hit.id));
        self.result(result, cx);
    }
}

#[cfg(test)]
#[path = "layer_cursor_tests.rs"]
mod tests;
