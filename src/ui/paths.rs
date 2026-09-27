//! Saved working paths and transactional anchor gestures.
use super::path_target::Target;
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
    pub target: Target,
    pub selected: Option<usize>,
    pub mode: Mode,
}
pub(super) struct State {
    pub active: Option<Active>,
    pub picker: dropdown::Dropdown<Option<Target>>,
    pub rename: Option<super::path_rename::Draft>,
    names: Vec<(Target, String)>,
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
    shape: Option<compositor::path_shape::Gesture>,
    target: Target,
    original: BezierPath,
    hit: Hit,
    start: Point,
    handles: bool,
    prior: Option<Active>,
}

impl Editor {
    pub(super) fn active_path(&self) -> Option<&SavedPath> {
        let Target::Saved(id) = self.tools.paths.active?.target else {
            return None;
        };
        self.session().document.paths.iter().find(|p| p.id == id)
    }
    pub(super) fn path_snapshot(&self) -> Result<Option<SavedPath>> {
        self.tools
            .paths
            .active
            .map(|a| a.target.snapshot(&self.session().document))
            .transpose()
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
            .map(|p| (Target::Saved(p.id), p.name.clone()))
            .chain(
                self.session()
                    .document
                    .layers
                    .iter()
                    .filter(|l| l.is_path_shape())
                    .map(|l| (Target::Shape(l.id), format!("Layer: {}", l.name))),
            )
            .collect();
        if self
            .tools
            .paths
            .active
            .is_some_and(|a| a.target.name(&self.session().document).is_none())
        {
            self.tools.paths.active = None;
        }
        if self.tools.paths.names != names {
            let items = std::iter::once(PickerItem::new("New path", None).id("new-path")).chain(
                names
                    .iter()
                    .map(|(id, name)| PickerItem::new(name.clone(), Some(*id)).id(id.key())),
            );
            self.tools.paths.picker =
                dropdown::Dropdown::new(items).expect("Validated unique path IDs");
            self.tools.paths.names = names;
        }
        let id = self
            .tools
            .paths
            .active
            .map_or_else(|| "new-path".to_owned(), |a| a.target.key());
        self.tools.paths.picker.select_id(id);
    }
    pub(super) fn finish_path_drag(&mut self, commit: bool) -> Result<()> {
        if !matches!(self.gesture, Some(Gesture::Path(_))) {
            return Ok(());
        }
        if let Some(Gesture::Path(drag)) = self.gesture.take() {
            if commit {
                if let Some(shape) = &drag.shape
                    && let Err(error) = shape.validate(&self.session().document)
                {
                    self.session_mut().cancel();
                    self.tools.paths.active = drag.prior;
                    return Err(error);
                }
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
        let active = prior.filter(|a| a.target.name(&self.session().document).is_some());
        let existing = active
            .map(|a| a.target.snapshot(&self.session().document))
            .transpose()?;
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
            let mut geometry = path.geometry.clone();
            geometry.closure = Closure::Closed;
            self.session_mut()
                .edit("Close Path", |doc| active.target.replace(doc, geometry))?;
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
        let target = active.map_or(Target::Saved(path.id), |a| a.target);
        let shape = match target {
            Target::Saved(_) => None,
            Target::Shape(id) => Some(compositor::path_shape::Gesture::begin(
                &self.session().document,
                id,
            )?),
        };
        self.session_mut().begin("Edit Path")?;
        let result = if active.is_none() {
            self.session_mut().document.paths.push(path);
            vector_path::validate(&self.session().document.paths)
        } else if let Some(shape) = &shape {
            shape.update(&mut self.session_mut().document, path.geometry)
        } else {
            target.replace(&mut self.session_mut().document, path.geometry)
        };
        if let Err(error) = result {
            self.session_mut().cancel();
            return Err(error);
        }
        let selected = match hit {
            Hit::Anchor(i) | Hit::Incoming(i) | Hit::Outgoing(i) => i,
        };
        self.tools.paths.active = Some(Active {
            target,
            selected: Some(selected),
            mode: active.map_or(Mode::Drawing, |a| a.mode),
        });
        self.gesture = Some(Gesture::Path(Box::new(Drag {
            shape,
            target,
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
        let target = drag.target;
        let shape = drag.shape.clone();
        let result = match shape {
            Some(shape) => shape.update(&mut self.session_mut().document, geometry),
            None => target.replace(&mut self.session_mut().document, geometry),
        };
        if let Err(error) = result {
            self.finish_path_drag(false)?;
            return Err(error);
        }
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
            target,
            selected: Some(index),
            ..
        }) = self.tools.paths.active
        else {
            return Ok(());
        };
        self.session_mut().edit("Delete Path Anchor", |doc| {
            let mut geometry = target.snapshot(doc)?.geometry;
            if index < geometry.anchors.len() {
                geometry.anchors.remove(index);
            }
            if geometry.anchors.len() < 3 {
                geometry.closure = Closure::Open;
            }
            if geometry.anchors.is_empty()
                && let Target::Saved(id) = target
            {
                doc.paths.retain(|p| p.id != id);
                Ok(())
            } else {
                target.replace(doc, geometry)
            }
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
