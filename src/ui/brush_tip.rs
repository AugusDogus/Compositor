//! BrushCursorOverlay/EditorCanvas right-drag controls, independent of document history.
use super::*;
use compositor::{brush::Brush, geometry::Point};
use quickgui::{MouseButton, PointerEvent, PointerPhase};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Adjustment {
    Diameter,
    Hardness,
}

#[derive(Clone, Copy)]
pub(super) struct Drag {
    pub anchor: Point,
    brush: Brush,
    pub adjustment: Adjustment,
}

impl Editor {
    pub(super) fn brush_tip_pointer(&mut self, event: &PointerEvent) -> bool {
        if let Some(Gesture::BrushTip(mut drag)) = self.gesture {
            if event.button != MouseButton::Right {
                return true;
            }
            match event.phase {
                PointerPhase::Move => {
                    drag.adjustment = if event.modifiers.contains(Modifiers::SHIFT) {
                        Adjustment::Hardness
                    } else {
                        Adjustment::Diameter
                    };
                    let dx = f64::from(event.local_position.x) - drag.anchor[0];
                    let (zoom, _) = self.viewport(event.size.width, event.size.height);
                    match drag.adjustment {
                        Adjustment::Hardness => {
                            self.tools.brush.hardness =
                                (drag.brush.hardness + dx / 200.).clamp(0., 1.);
                            self.tools.brush.diameter = drag.brush.diameter;
                        }
                        Adjustment::Diameter => {
                            self.tools.brush.diameter = (drag.brush.diameter
                                + 2. * dx / zoom.max(0.0001))
                            .round()
                            .clamp(1., 2000.);
                            self.tools.brush.hardness = drag.brush.hardness;
                        }
                    }
                    self.gesture = Some(Gesture::BrushTip(drag));
                }
                PointerPhase::Up | PointerPhase::Cancel => {
                    self.gesture = None;
                    self.canvas_pointer =
                        Some([event.local_position.x as f64, event.local_position.y as f64]);
                }
                PointerPhase::Down => {}
            }
            return true;
        }
        if event.button != MouseButton::Right {
            return false;
        }
        if event.phase == PointerPhase::Down
            && self.tools.tool.has_brush_cursor()
            && self.has_document()
            && self.gesture.is_none()
            && !self.space_pan
            && !self.pending
            && self.modal.is_none()
        {
            self.gesture = Some(Gesture::BrushTip(Drag {
                anchor: [event.local_position.x as f64, event.local_position.y as f64],
                brush: self.tools.brush,
                adjustment: if event.modifiers.contains(Modifiers::SHIFT) {
                    Adjustment::Hardness
                } else {
                    Adjustment::Diameter
                },
            }));
        }
        // A secondary press or release must not finish an active painting stroke.
        true
    }
}
