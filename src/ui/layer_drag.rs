use super::*;
use compositor::layer_ops;
use uuid::Uuid;

#[derive(Clone)]
pub(super) struct LayerDrag {
    pub session: Uuid,
    pub layer: Uuid,
    pub operation: Transfer,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Transfer {
    Move,
    Copy,
    CopyMask,
}

#[derive(Clone, Copy)]
pub(super) enum DropPlacement {
    Above(Uuid),
    Below(Uuid),
    Into(Uuid),
    Root,
}

impl DropPlacement {
    pub(super) fn resolve(self, doc: &Document) -> Result<(Option<Uuid>, layer_ops::Position)> {
        match self {
            Self::Root => Ok((None, layer_ops::Position::Top)),
            Self::Into(id) => Ok((Some(id), layer_ops::Position::Top)),
            Self::Above(id) | Self::Below(id) => {
                let layer = doc.layer(id).ok_or_else(|| {
                    compositor::invalid("The drop target was removed. Drag onto an existing layer.")
                })?;
                Ok((
                    layer.parent,
                    match self {
                        Self::Below(_) => layer_ops::Position::Below(id),
                        _ => layer_ops::Position::Above(id),
                    },
                ))
            }
        }
    }
}

fn needs_copy_worker(source: &Document) -> bool {
    source.layers.iter().any(|layer| {
        layer.clip_source.is_some()
            && layer.raster().is_some_and(|pixels| {
                u64::from(pixels.width()) * u64::from(pixels.height()) >= 1_000_000
            })
    })
}

fn duplicate_label(source: &Document, layer: Uuid) -> Result<&'static str> {
    Ok(if layer_ops::drag_roots(source, layer)?.len() > 1 {
        "Duplicate Layers"
    } else {
        "Duplicate Layer"
    })
}

pub(super) fn empty_copy_destination(source: &Document) -> Result<Document> {
    let mut doc = Document::new(source.width, source.height)?;
    doc.resolution = source.resolution;
    doc.layers.clear();
    doc.active = None;
    doc.selected.clear();
    Ok(doc)
}

impl Editor {
    pub(super) fn copy_drag_to_new_tab(&mut self, drag: &LayerDrag) -> Result<()> {
        if drag.operation == Transfer::CopyMask {
            return Err(compositor::invalid("Drop the mask onto an existing layer."));
        }
        self.finish_pending_edits()?;
        let source = self
            .tabs
            .iter()
            .filter_map(ProjectTab::session)
            .find(|s| s.id == drag.session)
            .ok_or_else(|| {
                compositor::invalid("The source project has closed. Drag from an open project.")
            })?
            .document
            .clone();
        if source.layer(drag.layer).is_none() {
            return Err(compositor::invalid(
                "The dragged layer was removed. Drag an existing layer.",
            ));
        }
        let mut destination = ProjectTab::empty(format!("Untitled {}", self.next_tab_number));
        let center = [source.width as f64 / 2., source.height as f64 / 2.];
        let background = needs_copy_worker(&source);
        if !background {
            destination.edit_or_create(
                "Copy Layers from Project",
                || empty_copy_destination(&source),
                |doc| layer_ops::copy_to_project(&source, doc, drag.layer, center),
            )?;
        }
        self.tabs.push(destination);
        self.next_tab_number += 1;
        self.activate_tab(self.tabs.len() - 1);
        if background {
            self.queue(jobs::Job::CopyLayers {
                source: Box::new(source),
                layer: drag.layer,
                center,
                placement: None,
            });
        }
        Ok(())
    }

    pub(super) fn layer_drop_zone(
        &self,
        cx: &mut ViewContext<'_, Self>,
        target: Uuid,
        below: bool,
    ) -> Element {
        let id = format!(
            "layer-drop-{}-{target}",
            if below { "below" } else { "above" }
        );
        let placement = if below {
            DropPlacement::Below(target)
        } else {
            DropPlacement::Above(target)
        };
        div()
            .id(id.clone())
            .absolute()
            .w_full()
            .h(8.)
            .left(0.)
            .top(if below { 44. } else { 0. })
            .drag_over(|s| s.bg(Color::rgb8(130, 185, 245)))
            .on_drop(
                cx.drop_listener(id, move |this, payload: &LayerDrag, _, cx| {
                    this.drop_layer(payload, placement, cx)
                }),
            )
    }

    pub(super) fn drop_layer(
        &mut self,
        drag: &LayerDrag,
        placement: DropPlacement,
        cx: &mut EventContext,
    ) {
        if !self.can_edit_layers() {
            return;
        }
        let result = self.finish_pending_edits().and_then(|()| {
            if drag.operation == Transfer::CopyMask {
                let target = match placement {
                    DropPlacement::Above(id)
                    | DropPlacement::Below(id)
                    | DropPlacement::Into(id) => id,
                    DropPlacement::Root => {
                        return Err(compositor::invalid(
                            "Drop the mask onto an existing pixel or adjustment layer.",
                        ));
                    }
                };
                let source = self
                    .tabs
                    .iter()
                    .filter_map(ProjectTab::session)
                    .find(|s| s.id == drag.session)
                    .ok_or_else(|| {
                        compositor::invalid(
                            "The source project has closed. Drag from an open project.",
                        )
                    })?;
                if source.id == self.session().id && drag.layer == target {
                    return Ok(());
                }
                let layer = source.document.layer(drag.layer).ok_or_else(|| {
                    compositor::invalid(
                        "The mask source layer was removed. Drag from an existing mask.",
                    )
                })?;
                let mut mask = layer.mask.clone().ok_or_else(|| {
                    compositor::invalid("The source mask was removed. Drag from an existing mask.")
                })?;
                mask.placement = Some(mask.placement.unwrap_or(layer.transform));
                let label = if self
                    .session()
                    .document
                    .layer(target)
                    .is_some_and(|layer| layer.mask.is_some())
                {
                    "Replace Layer Mask"
                } else {
                    "Copy Layer Mask"
                };
                self.session_mut().edit(label, |doc| {
                    let layer = doc
                        .layers
                        .iter_mut()
                        .find(|layer| layer.id == target && !layer.is_group())
                        .ok_or_else(|| {
                            compositor::invalid("Drop the mask onto a pixel or adjustment layer.")
                        })?;
                    layer.mask = Some(mask);
                    doc.select(target, false);
                    Ok(())
                })?;
                self.tools.mask_target = true;
                return Ok(());
            }
            let source = if drag.session != self.session().id || drag.operation == Transfer::Copy {
                Some(
                    self.tabs
                        .iter()
                        .filter_map(ProjectTab::session)
                        .find(|s| s.id == drag.session)
                        .ok_or_else(|| {
                            compositor::invalid(
                                "The source project has closed. Drag from an open project.",
                            )
                        })?
                        .document
                        .clone(),
                )
            } else {
                None
            };
            let (parent, position) = placement.resolve(&self.session().document)?;
            let cross_project = drag.session != self.session().id;
            let center = [
                self.session().document.width as f64 / 2.,
                self.session().document.height as f64 / 2.,
            ];
            if cross_project && source.as_ref().is_some_and(needs_copy_worker) {
                if let Some(source) = source {
                    self.queue(jobs::Job::CopyLayers {
                        source: Box::new(source),
                        layer: drag.layer,
                        center,
                        placement: Some(placement),
                    });
                }
                return Ok(());
            }
            let label = if cross_project {
                "Copy Layers from Project"
            } else if let Some(source) = &source {
                duplicate_label(source, drag.layer)?
            } else if layer_ops::drag_roots(&self.session().document, drag.layer)?.len() > 1 {
                "Move Layers"
            } else {
                "Move Layer"
            };
            self.session_mut().edit(label, |doc| {
                let id = if let Some(source) = &source {
                    if !cross_project {
                        return layer_ops::duplicate_to(doc, drag.layer, parent, position);
                    }
                    layer_ops::copy_to_project(source, doc, drag.layer, center)?;
                    doc.active
                        .ok_or_else(|| compositor::invalid("The copied layer is missing."))?
                } else {
                    drag.layer
                };
                layer_ops::place(doc, id, parent, position)
            })?;
            if let Some(parent) = parent {
                self.session_mut().collapsed.remove(&parent);
            }
            Ok(())
        });
        self.operation_result(alerts::Operation::Paint, result, cx);
    }

    pub(super) fn copy_drag_to_tab(&mut self, drag: &LayerDrag, index: usize) -> Result<()> {
        self.copy_drag_to_tab_at(drag, index, None)
    }

    pub(super) fn drop_layer_on_canvas(
        &mut self,
        drag: &LayerDrag,
        position: quickgui::Point,
        cx: &mut EventContext,
    ) {
        if !self.can_switch_projects() {
            return;
        }
        let Some(bounds) = self.canvas_bounds.bounds() else {
            return;
        };
        let (zoom, offset) = self.viewport(bounds.width, bounds.height);
        let center = [
            (f64::from(position.x - bounds.x) - offset[0]) / zoom,
            (f64::from(position.y - bounds.y) - offset[1]) / zoom,
        ];
        let result = self.copy_drag_to_tab_at(drag, self.current, Some(center));
        self.operation_result(alerts::Operation::Paint, result, cx);
    }

    fn copy_drag_to_tab_at(
        &mut self,
        drag: &LayerDrag,
        index: usize,
        center: Option<compositor::geometry::Point>,
    ) -> Result<()> {
        if !self.can_receive_tab_layers(index) {
            return Err(compositor::invalid(
                "The destination project cannot receive layers while its crop or another edit is pending. Apply or cancel that edit, then drag again. No layers were copied.",
            ));
        }
        if drag.operation == Transfer::CopyMask {
            return Err(compositor::invalid(
                "Drop the mask onto a layer. A project tab is not a mask target.",
            ));
        }
        self.finish_pending_edits()?;
        let source = self
            .tabs
            .iter()
            .filter_map(ProjectTab::session)
            .find(|s| s.id == drag.session)
            .ok_or_else(|| {
                compositor::invalid("The source project has closed. Drag from an open project.")
            })?
            .document
            .clone();
        if source.layer(drag.layer).is_none() {
            return Err(compositor::invalid(
                "The dragged layer was removed. Drag an existing layer.",
            ));
        }
        let destination = self
            .tabs
            .get_mut(index)
            .ok_or_else(|| compositor::invalid("The destination project has closed."))?;
        let dimensions = destination
            .session()
            .map_or((source.width, source.height), |session| {
                (session.document.width, session.document.height)
            });
        let center = center.unwrap_or([dimensions.0 as f64 / 2., dimensions.1 as f64 / 2.]);
        let background = destination.id != drag.session && needs_copy_worker(&source);
        if !background && (destination.id != drag.session || drag.operation == Transfer::Copy) {
            let cross_project = destination.id != drag.session;
            destination.edit_or_create(
                if cross_project {
                    "Copy Layers from Project"
                } else {
                    duplicate_label(&source, drag.layer)?
                },
                || empty_copy_destination(&source),
                |doc| {
                    if cross_project {
                        layer_ops::copy_to_project(&source, doc, drag.layer, center)
                    } else {
                        let parent = source.layer(drag.layer).and_then(|layer| layer.parent);
                        layer_ops::duplicate_to(
                            doc,
                            drag.layer,
                            parent,
                            layer_ops::Position::Above(drag.layer),
                        )
                    }
                },
            )?;
        }
        self.activate_tab(index);
        self.tools.mask_target = false;
        if background {
            self.queue(jobs::Job::CopyLayers {
                source: Box::new(source),
                layer: drag.layer,
                center,
                placement: None,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compositor::document::{Layer, Mask};
    use quickgui::{Application, WindowOptions};

    #[test]
    fn mixed_group_and_clipping_stack_drop_preserves_hierarchy_references_and_undo() {
        use compositor::document::LayerContent;
        use std::collections::HashSet;
        for operation in [Transfer::Move, Transfer::Copy] {
            let mut doc = Document::new(4, 4).unwrap();
            let child = doc.layers[0].id;
            layer_ops::group(&mut doc).unwrap();
            let group = doc.active.unwrap();
            let mut inside_clip = Layer::blank("Inside clip", 4, 4);
            inside_clip.parent = Some(group);
            inside_clip.clip_source = Some(child);
            let inside_clip_id = inside_clip.id;
            doc.add(inside_clip).unwrap();
            let base = Layer::blank("Base", 4, 4);
            let base_id = base.id;
            doc.add(base).unwrap();
            let mut clip = Layer::blank("Clip", 4, 4);
            clip.clip_source = Some(base_id);
            let clip_id = clip.id;
            doc.add(clip).unwrap();
            let mut target = Layer::blank("Destination", 4, 4);
            target.content = LayerContent::Group;
            let target_id = target.id;
            doc.add(target).unwrap();
            // A selected descendant belongs to its selected group, not a second root.
            doc.selected = HashSet::from([group, child, base_id, clip_id]);
            doc.active = Some(child);
            doc.validate().unwrap();
            let original = doc.clone();
            let mut e = Editor::with_test_document();
            e.tabs = vec![Session::new(doc, None).into()];
            e.session_mut().collapsed.insert(target_id);
            let drag = LayerDrag {
                session: e.session().id,
                layer: child,
                operation,
            };
            let (mut cx, view) = Application::new()
                .into_test_context(WindowOptions::new("Mixed layer drop").size(1280., 900.), e)
                .unwrap();
            cx.update(view, |e, cx| {
                e.drop_layer(&drag, DropPlacement::Into(target_id), cx)
            })
            .unwrap();
            cx.read(view, |e| {
                let doc = &e.session().document;
                doc.validate().unwrap();
                assert!(!e.session().collapsed.contains(&target_id));
                let roots: Vec<_> = doc
                    .layers
                    .iter()
                    .filter(|l| l.parent == Some(target_id))
                    .collect();
                assert_eq!(
                    roots.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(),
                    ["Folder 1", "Base", "Clip"]
                );
                assert_eq!(doc.selected, roots.iter().map(|l| l.id).collect());
                assert_eq!(roots[2].clip_source, Some(roots[1].id));
                let children: Vec<_> = doc
                    .layers
                    .iter()
                    .filter(|l| l.parent == Some(roots[0].id))
                    .collect();
                assert_eq!(children.len(), 2);
                assert_eq!(children[1].clip_source, Some(children[0].id));
                if operation == Transfer::Copy {
                    assert_eq!(doc.layers.len(), original.layers.len() + 5);
                    for id in [group, child, inside_clip_id, base_id, clip_id] {
                        assert_eq!(doc.layer(id), original.layer(id));
                    }
                } else {
                    assert_eq!(doc.layers.len(), original.layers.len());
                }
            })
            .unwrap();
            cx.update(view, |e, _| e.session_mut().undo()).unwrap();
            assert_eq!(
                cx.read(view, |e| e.session().document.clone()).unwrap(),
                original
            );
            assert!(
                cx.read(view, |e| e.session().undo_label().is_none())
                    .unwrap()
            );
        }
    }

    #[test]
    fn mask_copy_replaces_target_mask_at_source_placement_and_undo_restores_it() {
        let mut e = Editor::with_test_document();
        let mut doc = Document::new(100, 100).unwrap();
        let source = doc.layers[0].id;
        let placement = compositor::geometry::Transform {
            origin: [20., 30.],
            rotation: 27.,
            ..compositor::geometry::Transform::new(20, 30)
        };
        doc.layers[0].transform = placement;
        doc.layers[0].mask = Some(Mask {
            pixels: Arc::new(image::GrayImage::from_pixel(20, 30, image::Luma([123]))),
            enabled: false,
            linked: true,
            placement: None,
        });
        let pixels = doc.layers[0].mask.as_ref().unwrap().pixels.clone();
        let mut target = Layer::blank("Target", 100, 100);
        target.mask = Some(Mask {
            pixels: Arc::new(image::GrayImage::from_pixel(100, 100, image::Luma([255]))),
            enabled: true,
            linked: false,
            placement: None,
        });
        let target_id = target.id;
        doc.add(target).unwrap();
        let original = doc.clone();
        e.tabs = vec![Session::new(doc, None).into()];
        let drag = LayerDrag {
            session: e.session().id,
            layer: source,
            operation: Transfer::CopyMask,
        };
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Mask copy").size(1280., 900.), e)
            .unwrap();
        cx.update(view, |e, cx| {
            e.drop_layer(&drag, DropPlacement::Above(target_id), cx)
        })
        .unwrap();
        cx.read(view, |e| {
            let doc = &e.session().document;
            let target = doc.layer(target_id).unwrap();
            let mask = target.mask.as_ref().unwrap();
            assert_eq!(e.session().undo_label(), Some("Replace Layer Mask"));
            assert_eq!(doc.layers.len(), 2);
            assert_eq!(doc.layers[0], original.layers[0]);
            assert_eq!(target.transform, original.layers[1].transform);
            assert_eq!(mask.placement, Some(placement));
            assert!(Arc::ptr_eq(&mask.pixels, &pixels));
            assert!(!mask.enabled);
            assert!(mask.linked);
            assert_eq!(doc.active, Some(target_id));
            assert!(e.tools.mask_target);
        })
        .unwrap();
        cx.update(view, |e, _| e.session_mut().undo()).unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            original
        );
        cx.update(view, |e, cx| {
            e.session_mut().document.layers[1].mask = None;
            let before = e.session().document.clone();
            e.drop_layer(&drag, DropPlacement::Above(target_id), cx);
            assert_eq!(e.session().undo_label(), Some("Copy Layer Mask"));
            assert_eq!(
                e.session()
                    .document
                    .layer(target_id)
                    .unwrap()
                    .mask
                    .as_ref()
                    .unwrap()
                    .placement,
                Some(placement)
            );
            e.undo_document();
            assert_eq!(e.session().document, before);
            assert_eq!(e.session().redo_label(), Some("Copy Layer Mask"));
        })
        .unwrap();
    }
}
