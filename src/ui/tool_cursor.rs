//! Cursor decisions share the same geometry and gesture state as canvas presses.
use super::cursor_art::{Badge, Glyph};
use super::*;
use compositor::{
    geometry::{Point, Transform},
    selection::SelectionMode,
    transform::{self, Handle},
};
use quickgui::CursorStyle;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Cursor {
    System(CursorStyle),
    Image(Glyph),
    Hidden,
}

impl Cursor {
    pub fn style(self) -> CursorStyle {
        match self {
            Self::System(style) => style,
            _ => CursorStyle::Arrow,
        }
    }
}

fn resize(bounds: Transform, index: usize) -> Cursor {
    let offsets = [45., 90., 135., 0., 45., 90., 135., 0.];
    let direction = ((bounds.rotation + offsets[index]) / 45.).round() as i32;
    Cursor::System(match direction.rem_euclid(4) {
        0 => CursorStyle::ResizeLeftRight,
        1 => CursorStyle::ResizeUpLeftDownRight,
        2 => CursorStyle::ResizeUpDown,
        _ => CursorStyle::ResizeUpRightDownLeft,
    })
}

fn pixel_drag(drag: &floating::PixelDrag) -> Cursor {
    match drag.kind {
        floating::DragKind::Move => Cursor::Image(Glyph::Move),
        floating::DragKind::Distort(_) => Cursor::Image(Glyph::Distort),
        floating::DragKind::Transform(Handle::Rotate) => Cursor::Image(Glyph::Rotate),
        floating::DragKind::Transform(Handle::Resize(i)) => resize(drag.original.bounds(), i),
    }
}

impl Editor {
    pub(super) fn canvas_cursor(&self, zoom: f64, offset: Point) -> Cursor {
        use Cursor::{Hidden, Image, System};
        if self.pending || !self.has_document() {
            return System(CursorStyle::Arrow);
        }
        if matches!(self.gesture, Some(Gesture::Pan)) {
            return System(CursorStyle::ClosedHand);
        }
        if self.space_pan {
            return System(CursorStyle::OpenHand);
        }
        let alt = self.keyboard_modifiers.contains(Modifiers::ALT);
        let control = self.keyboard_modifiers.contains(Modifiers::CONTROL);
        let targeting = self.adjustment_edit.as_ref().is_some_and(|edit| {
            matches!(
                edit.hue_sampling,
                hue_sampling::HueSampling::Target | hue_sampling::HueSampling::Dragging { .. }
            )
        });
        if targeting {
            return System(CursorStyle::ResizeLeftRight);
        }
        if self.picking_color() || self.adjustment_sampling() {
            return Image(Glyph::Eyedropper);
        }
        if self.modal.is_some() {
            return System(CursorStyle::Arrow);
        }
        if self.tools.tool == Tool::Eyedropper
            || matches!(self.gesture, Some(Gesture::Sample))
            || (alt
                && matches!(
                    self.tools.tool,
                    Tool::Brush | Tool::Erase | Tool::Heal | Tool::Gradient
                )
                && !matches!(
                    self.gesture,
                    Some(Gesture::Paint { .. } | Gesture::Gradient(_))
                ))
        {
            return Image(Glyph::Eyedropper);
        }
        let pointer = self.canvas_pointer.unwrap_or([0.; 2]);
        let point = [
            (pointer[0] - offset[0]) / zoom,
            (pointer[1] - offset[1]) / zoom,
        ];
        match &self.gesture {
            Some(Gesture::PixelTransform(drag)) => return pixel_drag(drag),
            Some(Gesture::HeaderTransform(drag)) => return pixel_drag(drag.cursor_drag()),
            Some(Gesture::Transform { bounds, handle, .. }) => {
                return match handle {
                    Handle::Rotate => Image(Glyph::Rotate),
                    Handle::Resize(i) => resize(*bounds, *i),
                };
            }
            Some(Gesture::Move {
                duplicate_on_drag, ..
            }) => {
                return Image(if *duplicate_on_drag {
                    Glyph::Duplicate
                } else {
                    Glyph::Move
                });
            }
            Some(Gesture::Pixels { duplicate, .. }) => {
                return Image(if *duplicate {
                    Glyph::Duplicate
                } else {
                    Glyph::MovePixels
                });
            }
            Some(Gesture::SelectionMove { .. }) => return Image(Glyph::MoveSelection),
            Some(Gesture::Crop(drag)) => {
                if let compositor::crop::Mode::Resize(i) = drag.mode {
                    return resize(drag.original, i);
                }
            }
            _ => {}
        }
        match self.tools.tool {
            Tool::Text => System(CursorStyle::IBeam),
            Tool::Hand => System(CursorStyle::OpenHand),
            Tool::Zoom => Image(if alt { Glyph::ZoomOut } else { Glyph::ZoomIn }),
            Tool::Clone if self.tools.clone_source.is_some() && !alt => Hidden,
            Tool::Rectangle
            | Tool::Ellipse
            | Tool::Lasso
            | Tool::Polygon
            | Tool::Wand
            | Tool::Object => {
                let mode = self.displayed_selection_mode();
                let over_selection = self.gesture.is_none()
                    && self.tools.polygon.is_none()
                    && self
                        .session()
                        .document
                        .selection
                        .as_ref()
                        .is_some_and(|s| s.coverage(point) > 0.5);
                if over_selection
                    && (control
                        || (mode == SelectionMode::Replace && self.tools.tool != Tool::Object))
                {
                    return Image(if control {
                        if alt {
                            Glyph::Duplicate
                        } else {
                            Glyph::MovePixels
                        }
                    } else {
                        Glyph::MoveSelection
                    });
                }
                let badge = match mode {
                    SelectionMode::Replace => Badge::New,
                    SelectionMode::Add => Badge::Add,
                    _ => Badge::Subtract,
                };
                Image(match self.tools.tool {
                    Tool::Rectangle => Glyph::Rectangle(badge),
                    Tool::Ellipse => Glyph::Ellipse(badge),
                    Tool::Lasso => Glyph::Lasso(badge),
                    Tool::Polygon => Glyph::Polygon(badge),
                    Tool::Object => Glyph::Object(badge),
                    _ => Glyph::Wand(badge),
                })
            }
            Tool::Move => {
                if let Some(placement) = self.transform_placement() {
                    let drag = floating::PixelDrag::new(
                        placement,
                        point,
                        zoom,
                        self.keyboard_modifiers,
                        self.tools.show_transform_controls
                            || self.transform_edit.is_some()
                            || self.pending_pixels.is_some(),
                    );
                    if !matches!(drag.kind, floating::DragKind::Move) {
                        return pixel_drag(&drag);
                    }
                    return Image(if alt { Glyph::Duplicate } else { Glyph::Move });
                }
                let doc = &self.session().document;
                if (self.tools.transform_auto_select || control)
                    && transform::pick(doc, point, control).is_some()
                {
                    Image(if alt { Glyph::Duplicate } else { Glyph::Move })
                } else {
                    System(CursorStyle::Arrow)
                }
            }
            Tool::Crop => {
                if let Some(crop) = &self.tools.pending_crop
                    && let Some(Handle::Resize(i)) = transform::hit_handle(
                        Transform::HANDLES.map(|u| crop.frame.geometry_point(u)),
                        None,
                        point,
                        zoom,
                    )
                {
                    return resize(crop.frame, i);
                }
                System(CursorStyle::Crosshair)
            }
            Tool::Idle => System(CursorStyle::Arrow),
            _ => System(CursorStyle::Crosshair),
        }
    }

    pub(super) fn sync_native_cursor(&mut self, cx: &mut ViewContext<'_, Self>) {
        let window = cx.window_state();
        // Project/file drags own cursor feedback even while crossing the canvas.
        let choice = if self.tab_scrolling.dragging {
            Cursor::System(CursorStyle::Arrow)
        } else if window.focused
            && self.has_document()
            && (self.canvas_pointer.is_some() || self.gesture.is_some())
            && !self.menus.is_open()
        {
            self.canvas_bounds
                .bounds()
                .map(|bounds| {
                    let (zoom, offset) = self.viewport(bounds.width, bounds.height);
                    self.canvas_cursor(zoom, offset)
                })
                .unwrap_or(Cursor::System(CursorStyle::Arrow))
        } else if window.focused && !self.menus.is_open() {
            self.layer_cursor()
        } else {
            Cursor::System(CursorStyle::Arrow)
        };
        // Draggable rows default to OpenHand in QuickGUI. The source layer list
        // explicitly uses Arrow unless a modifier selects a custom cursor.
        let layer_arrow = window.focused
            && !self.tab_scrolling.dragging
            && !self.menus.is_open()
            && self.canvas_pointer.is_none()
            && self.gesture.is_none()
            && self.layer_cursor_hovered();
        let image = if let Cursor::Image(glyph) = choice {
            match self.cursor_art.image(glyph, cx.scale_factor()) {
                Ok(image) => Some(quickgui::CursorOverride::Image(image)),
                Err(error) => {
                    self.status = format!(
                        "{error} The document is preserved; move the pointer outside the canvas to retry."
                    );
                    None
                }
            }
        } else if let Cursor::System(style) = choice {
            (style != CursorStyle::Arrow || layer_arrow)
                .then_some(quickgui::CursorOverride::System(style))
        } else {
            None
        };
        let visible = choice != Cursor::Hidden;
        if window.cursor_visible == visible
            && window.cursor_override == image.as_ref().map(quickgui::CursorOverride::id)
        {
            return;
        }
        match cx.spawn(move |async_cx| async move {
            // A closed view cannot receive an update and needs no cursor restoration.
            let _ = async_cx.update(move |this, cx| {
                let result = cx.set_cursor_override(image).and_then(|()| cx.set_cursor_visible(visible));
                if let Err(error) = result {
                    this.status = format!("Could not update the tool cursor: {error}. Move the pointer outside the canvas and try again.");
                }
            }).await;
        }) {
            Ok(task) => task.detach(),
            Err(error) => self.status = format!("Could not schedule the tool cursor update: {error}. Move the pointer outside the canvas and try again."),
        }
    }
}

#[cfg(test)]
#[path = "tool_cursor_tests.rs"]
mod tests;
