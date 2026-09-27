//! Saved working paths and transactional anchor gestures.
use super::*;
use compositor::{
    geometry::Point,
    invalid,
    vector_path::{self, Anchor, BezierPath, Closure, Hit, SavedPath},
};
use quickgui::{PickerItem, PointerEvent, PointerPhase};
use uuid::Uuid;

#[derive(Clone, Copy, Default)]
pub(super) enum Mode {
    #[default]
    Drawing,
    Editing,
}
#[derive(Clone, Copy)]
pub(super) struct Active {
    pub id: Uuid,
    pub selected: Option<usize>,
    pub mode: Mode,
}
pub(super) struct State {
    pub active: Option<Active>,
    pub picker: dropdown::Dropdown<Option<Uuid>>,
    pub rename: Option<super::path_rename::Draft>,
    names: Vec<(Uuid, String)>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            active: None,
            rename: None,
            picker: dropdown::Dropdown::new([PickerItem::new("New path", None).id("new-path")])
                .expect("Unique path option"),
            names: Vec::new(),
        }
    }
}
pub(super) struct Drag {
    id: Uuid,
    original: BezierPath,
    hit: Hit,
    start: Point,
    handles: bool,
    prior: Option<Active>,
}

impl Editor {
    pub(super) fn active_path(&self) -> Option<&SavedPath> {
        let id = self.tools.paths.active?.id;
        self.session().document.paths.iter().find(|p| p.id == id)
    }
    pub(super) fn sync_paths(&mut self) {
        self.sync_path_rename();
        if !self.has_document() {
            return;
        }
        let names: Vec<_> = self
            .session()
            .document
            .paths
            .iter()
            .map(|p| (p.id, p.name.clone()))
            .collect();
        if self.tools.paths.active.is_some() && self.active_path().is_none() {
            self.tools.paths.active = None;
        }
        if self.tools.paths.names != names {
            let items = std::iter::once(PickerItem::new("New path", None).id("new-path")).chain(
                names
                    .iter()
                    .map(|(id, name)| PickerItem::new(name.clone(), Some(*id)).id(id.to_string())),
            );
            self.tools.paths.picker =
                dropdown::Dropdown::new(items).expect("Validated unique path IDs");
            self.tools.paths.names = names;
        }
        let id = self
            .tools
            .paths
            .active
            .map_or_else(|| "new-path".to_owned(), |a| a.id.to_string());
        self.tools.paths.picker.select_id(id);
    }
    pub(super) fn finish_path_drag(&mut self, commit: bool) -> Result<()> {
        if !matches!(self.gesture, Some(Gesture::Path(_))) {
            return Ok(());
        }
        if let Some(Gesture::Path(drag)) = self.gesture.take() {
            if commit {
                self.session_mut().commit()?;
            } else {
                self.session_mut().cancel();
                self.tools.paths.active = drag.prior;
            }
        }
        Ok(())
    }
    pub(super) fn path_down(
        &mut self,
        point: Point,
        zoom: f64,
        modifiers: Modifiers,
    ) -> Result<()> {
        if !self.can_edit_layers() {
            return Ok(());
        }
        let prior = self.tools.paths.active;
        let active = prior.filter(|a| self.session().document.paths.iter().any(|p| p.id == a.id));
        let existing = active
            .and_then(|a| self.session().document.paths.iter().find(|p| p.id == a.id))
            .cloned();
        let hit = existing
            .as_ref()
            .map(|p| {
                let mut targets = p.geometry.clone();
                for (i, anchor) in targets.anchors.iter_mut().enumerate() {
                    if active.and_then(|a| a.selected) != Some(i) {
                        anchor.incoming = None;
                        anchor.outgoing = None;
                    }
                }
                targets.hit(point, 7. / zoom)
            })
            .transpose()?
            .flatten();
        if let (Some(path), Some(Hit::Anchor(0)), Some(active)) = (&existing, hit, active)
            && matches!(active.mode, Mode::Drawing)
            && path.geometry.anchors.len() >= 3
        {
            let id = path.id;
            self.session_mut().edit("Close Path", |doc| {
                path_mut(doc, id)?.geometry.closure = Closure::Closed;
                Ok(())
            })?;
            self.tools.paths.active = Some(Active {
                mode: Mode::Editing,
                selected: Some(0),
                ..active
            });
            return Ok(());
        }
        let mut path = match existing {
            Some(path) => path,
            None => SavedPath::new(
                format!("Path {}", self.session().document.paths.len() + 1),
                BezierPath::default(),
            )?,
        };
        let adding = hit.is_none() && active.is_none_or(|a| matches!(a.mode, Mode::Drawing));
        if hit.is_none() && !adding {
            return Ok(());
        }
        let hit = if adding {
            path.geometry.anchors.push(Anchor::corner(point));
            Hit::Anchor(path.geometry.anchors.len() - 1)
        } else {
            hit.ok_or_else(|| invalid("The path anchor is unavailable."))?
        };
        let original = path.geometry.clone();
        let id = path.id;
        let mut paths = self.session().document.paths.clone();
        if let Some(current) = paths.iter_mut().find(|p| p.id == id) {
            *current = path;
        } else {
            paths.push(path);
        }
        vector_path::validate(&paths)?;
        self.session_mut().begin("Edit Path")?;
        self.session_mut().document.paths = paths;
        let selected = match hit {
            Hit::Anchor(i) | Hit::Incoming(i) | Hit::Outgoing(i) => i,
        };
        self.tools.paths.active = Some(Active {
            id,
            selected: Some(selected),
            mode: active.map_or(Mode::Drawing, |a| a.mode),
        });
        self.gesture = Some(Gesture::Path(Box::new(Drag {
            id,
            original,
            hit,
            start: point,
            handles: adding || modifiers.contains(Modifiers::ALT),
            prior,
        })));
        Ok(())
    }
    pub(super) fn path_pointer(&mut self, event: &PointerEvent, point: Point) -> Result<()> {
        if event.phase == PointerPhase::Cancel {
            return self.finish_path_drag(false);
        }
        let Some(Gesture::Path(drag)) = &self.gesture else {
            return Ok(());
        };
        let mut geometry = drag.original.clone();
        let index = match drag.hit {
            Hit::Anchor(i) | Hit::Incoming(i) | Hit::Outgoing(i) => i,
        };
        let anchor = geometry
            .anchors
            .get_mut(index)
            .ok_or_else(|| invalid("The path anchor is unavailable."))?;
        let delta = [point[0] - drag.start[0], point[1] - drag.start[1]];
        let translated = |p: Point| [p[0] + delta[0], p[1] + delta[1]];
        match drag.hit {
            Hit::Anchor(_) if drag.handles => {
                if delta[0].hypot(delta[1]) > 0.001 {
                    anchor.outgoing = Some(translated(anchor.point));
                    anchor.incoming =
                        Some([anchor.point[0] - delta[0], anchor.point[1] - delta[1]]);
                }
            }
            Hit::Anchor(_) => {
                anchor.point = translated(anchor.point);
                anchor.incoming = anchor.incoming.map(translated);
                anchor.outgoing = anchor.outgoing.map(translated);
            }
            Hit::Incoming(_) | Hit::Outgoing(_) => {
                let incoming = matches!(drag.hit, Hit::Incoming(_));
                let handle = if incoming {
                    anchor.incoming
                } else {
                    anchor.outgoing
                }
                .unwrap_or(anchor.point);
                let position = translated(handle);
                if incoming {
                    anchor.incoming = Some(position);
                } else {
                    anchor.outgoing = Some(position);
                }
                if !event.modifiers.contains(Modifiers::ALT) {
                    let opposite = [
                        2. * anchor.point[0] - position[0],
                        2. * anchor.point[1] - position[1],
                    ];
                    if incoming {
                        anchor.outgoing = Some(opposite);
                    } else {
                        anchor.incoming = Some(opposite);
                    }
                }
            }
        }
        if let Err(error) = geometry.validate() {
            self.finish_path_drag(false)?;
            return Err(error);
        }
        let id = drag.id;
        path_mut(&mut self.session_mut().document, id)?.geometry = geometry;
        if event.phase == PointerPhase::Up {
            self.finish_path_drag(true)?;
        }
        Ok(())
    }
    pub(super) fn path_key(
        &mut self,
        key: &Key,
        modifiers: Modifiers,
        cx: &mut EventContext,
    ) -> bool {
        if self.tools.tool != Tool::Pen || !modifiers.is_empty() {
            return false;
        }
        if !matches!(self.gesture, Some(Gesture::Path(_))) && !self.can_edit_layers() {
            return false;
        }
        let result = match key {
            Key::Escape => self.finish_path_drag(false).map(|()| {
                if let Some(a) = &mut self.tools.paths.active {
                    a.mode = Mode::Editing;
                }
            }),
            Key::Enter => self.finish_path_drag(true).map(|()| {
                if let Some(a) = &mut self.tools.paths.active {
                    a.mode = Mode::Editing;
                }
            }),
            Key::Delete | Key::Backspace if self.gesture.is_none() => self.delete_path_anchor(),
            _ => return false,
        };
        cx.prevent_default();
        self.result(result, cx);
        true
    }
    fn delete_path_anchor(&mut self) -> Result<()> {
        let Some(Active {
            id,
            selected: Some(index),
            ..
        }) = self.tools.paths.active
        else {
            return Ok(());
        };
        self.session_mut().edit("Delete Path Anchor", |doc| {
            let path = path_mut(doc, id)?;
            if index < path.geometry.anchors.len() {
                path.geometry.anchors.remove(index);
            }
            if path.geometry.anchors.len() < 3 {
                path.geometry.closure = Closure::Open;
            }
            if path.geometry.anchors.is_empty() {
                doc.paths.retain(|p| p.id != id);
            }
            Ok(())
        })?;
        if let Some(a) = &mut self.tools.paths.active {
            a.selected = None;
        }
        Ok(())
    }
}
pub(super) fn path_mut(doc: &mut Document, id: Uuid) -> Result<&mut SavedPath> {
    doc.paths.iter_mut().find(|p| p.id == id).ok_or_else(|| {
        invalid("The saved path is no longer available. Choose a path in the Pen toolbar.")
    })
}

#[cfg(test)]
mod tests;
