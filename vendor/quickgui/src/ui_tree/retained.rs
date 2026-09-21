use super::*;

/// Geometry has a separate lifetime from drawing commands. Color changes and ordinary hover
/// reuse both the natural boxes and hit stack; transforms, scrolling, and layout invalidate them.
#[derive(Default)]
pub(super) struct GeometryCache {
    natural_valid: bool,
    paint_valid: bool,
    scroll_offsets: HashMap<ElementId, Vector>,
    virtual_offsets: HashMap<ElementId, (f32, f32)>,
    pub(super) state_transforms: bool,
    hovered: HashSet<ElementId>,
    pressed: Option<ElementId>,
    dragging: Option<ElementId>,
    drag_over: Option<ElementId>,
    focused: Option<ElementId>,
    styled_focus: Option<ElementId>,
}

impl GeometryCache {
    pub(super) fn invalidate_paint(&mut self) {
        self.paint_valid = false;
    }

    pub(super) fn invalidate(&mut self) {
        self.natural_valid = false;
        self.paint_valid = false;
    }
}

pub(super) fn has_own_state_transform(element: &Element) -> bool {
    let styles = [
        &element.hover,
        &element.active,
        &element.focus,
        &element.focus_within,
        &element.dragging,
        &element.drag_over,
        &element.selected_style,
        &element.disabled_style,
        &element.invalid_style,
    ];
    styles
        .into_iter()
        .any(|style| style.transform.is_some() || style.transform_origin.is_some())
        || element
            .group_styles
            .iter()
            .any(|entry| entry.style.transform.is_some() || entry.style.transform_origin.is_some())
        || element.transition.as_ref().is_some_and(|transition| {
            transition
                .properties
                .contains(TransitionProperties::TRANSFORM)
        })
}

pub(super) fn has_state_transform(element: &Element) -> bool {
    has_own_state_transform(element) || element.children.iter().any(has_state_transform)
}

impl UiTree {
    pub(super) fn ensure_natural_geometry(&mut self) -> Result<bool, UiError> {
        let valid = self.geometry_cache.natural_valid
            && self.geometry_cache.scroll_offsets == self.scroll_offsets
            && self.virtual_scroll_handles.iter().all(|(id, binding)| {
                let requested = binding.handle.offset();
                self.geometry_cache.virtual_offsets.get(id)
                    == Some(&(requested, binding.handle.presented_offset(requested)))
            });
        if valid {
            return Ok(false);
        }
        let started = Instant::now();
        self.geometry_cache.invalidate();
        self.natural_bounds.clear();
        self.scroll_snap_geometry.clear();
        if let Some(root) = &self.root {
            let viewport = Rect::from_size(self.viewport);
            collect_layout_bounds(
                root,
                &self.taffy,
                &mut self.scroll_offsets,
                &mut self.scroll_end_states,
                &mut self.natural_bounds,
                &mut self.scroll_snap_geometry,
                None,
                LayoutFrame::root(Point::ZERO, viewport),
            )?;
        }
        self.work.geometry_nodes += self.natural_bounds.len();
        self.work.geometry_time += started.elapsed();
        self.geometry_cache
            .scroll_offsets
            .clone_from(&self.scroll_offsets);
        self.geometry_cache.virtual_offsets.clear();
        self.geometry_cache
            .virtual_offsets
            .extend(self.virtual_scroll_handles.iter().map(|(id, binding)| {
                let requested = binding.handle.offset();
                (*id, (requested, binding.handle.presented_offset(requested)))
            }));
        self.geometry_cache.natural_valid = true;
        Ok(true)
    }

    pub(super) fn needs_paint_geometry(&self) -> bool {
        !self.geometry_cache.paint_valid
            // Editable text can move its caret viewport independently of Taffy layout.
            || !self.text_inputs.is_empty()
            || (self.geometry_cache.state_transforms && (
                self.geometry_cache.hovered != self.hovered
                || self.geometry_cache.pressed != self.pressed
                || self.geometry_cache.dragging != self.dragging
                || self.geometry_cache.drag_over != self.drag_over
                || self.geometry_cache.focused != self.focused
                || self.geometry_cache.styled_focus != self.styled_focus()
                || self.style_transition_frame_requested
            ))
    }

    pub(super) fn finish_paint_geometry(&mut self) {
        self.geometry_cache.paint_valid = true;
        if self.geometry_cache.state_transforms {
            self.geometry_cache.hovered.clone_from(&self.hovered);
            self.geometry_cache.pressed = self.pressed;
            self.geometry_cache.dragging = self.dragging;
            self.geometry_cache.drag_over = self.drag_over;
            self.geometry_cache.focused = self.focused;
            self.geometry_cache.styled_focus = self.styled_focus();
        }
    }
}
