//! View-only rulers/grid and transactional document guides.
use super::*;
use compositor::{
    guides::{Axis, Guide},
    invalid,
};
use quickgui::{CursorStyle, MouseButton, PointerEvent, PointerPhase, Rect, ShaderParameters};
use uuid::Uuid;
const RULER: f32 = 22.;

#[derive(Clone, Copy)]
pub(super) enum Origin {
    Ruler(Axis),
    Existing(Guide),
}
pub(super) struct Drag {
    guide: Guide,
    origin: Origin,
    destination: Option<f64>,
}

impl Editor {
    pub(super) fn layout_action(&mut self, action: Action) -> Result<()> {
        let layout = &mut self.tools.layout;
        match action {
            Action::Rulers => layout.rulers = !layout.rulers,
            Action::Grid => layout.grid = !layout.grid,
            Action::Guides => layout.guides = !layout.guides,
            Action::LockGuides => layout.locked = !layout.locked,
            Action::Snap => layout.snap = !layout.snap,
            Action::SnapGrid => layout.to_grid = !layout.to_grid,
            Action::SnapGuides => layout.to_guides = !layout.to_guides,
            Action::SnapLayers => layout.to_layers = !layout.to_layers,
            Action::SnapBounds => layout.to_bounds = !layout.to_bounds,
            Action::ClearGuides => self.session_mut().edit_committed("Clear Guides", |doc| {
                doc.guides.clear();
                Ok(())
            })?,
            _ => {}
        }
        Ok(())
    }
    pub(super) fn layout_checked(&self, action: Action) -> Option<bool> {
        let layout = self.tools.layout;
        Some(match action {
            Action::Rulers => layout.rulers,
            Action::Grid => layout.grid,
            Action::Guides => layout.guides,
            Action::LockGuides => layout.locked,
            Action::Snap => layout.snap,
            Action::SnapGrid => layout.to_grid,
            Action::SnapGuides => layout.to_guides,
            Action::SnapLayers => layout.to_layers,
            Action::SnapBounds => layout.to_bounds,
            _ => return None,
        })
    }
    fn guide_pointer(&mut self, origin: Origin, event: &PointerEvent) -> Result<()> {
        if event.button != MouseButton::Left {
            return Ok(());
        }
        if event.phase == PointerPhase::Cancel {
            self.layout_drag = None;
            return Ok(());
        }
        if event.phase == PointerPhase::Down {
            if !self.can_edit_layers() || self.tools.layout.locked || self.modal.is_some() {
                return Ok(());
            }
            self.finish_pending_edits()?;
            let guide = match origin {
                Origin::Ruler(axis) => Guide {
                    id: Uuid::new_v4(),
                    axis,
                    position: 0.,
                },
                Origin::Existing(guide) => guide,
            };
            self.layout_drag = Some(Drag {
                guide,
                origin,
                destination: None,
            });
            self.tools.layout.guides = true;
        }
        let Some(bounds) = self.canvas_bounds.bounds() else {
            self.layout_drag = None;
            return Ok(());
        };
        let (zoom, offset) = self.viewport(bounds.width, bounds.height);
        let local = [
            f64::from(event.position.x - bounds.x),
            f64::from(event.position.y - bounds.y),
        ];
        let ruler = if self.tools.layout.rulers {
            f64::from(RULER)
        } else {
            0.
        };
        let inside = local[0] >= ruler
            && local[1] >= ruler
            && local[0] < f64::from(bounds.width)
            && local[1] < f64::from(bounds.height);
        let Some(drag) = self.layout_drag.as_mut() else {
            return Ok(());
        };
        let axis = drag.guide.axis.index();
        let mut position = ((local[axis] - offset[axis]) / zoom).round();
        if self.tools.layout.snap
            && self.tools.layout.to_grid
            && self.tools.layout.grid
            && !event.modifiers.contains(Modifiers::ALT)
        {
            position = (position / 8.).round() * 8.;
        }
        drag.destination = inside.then_some(position);
        if event.phase == PointerPhase::Up {
            let Some(drag) = self.layout_drag.take() else {
                return Ok(());
            };
            self.commit_guide_drag(drag)?;
        }
        Ok(())
    }
    fn commit_guide_drag(&mut self, drag: Drag) -> Result<()> {
        let label = match (drag.origin, drag.destination) {
            (Origin::Ruler(_), None) => return Ok(()),
            (Origin::Ruler(_), Some(_)) => "Add Guide",
            (Origin::Existing(_), Some(_)) => "Move Guide",
            (Origin::Existing(_), None) => "Delete Guide",
        };
        self.session_mut().edit_committed(label, move |doc| {
            if let Some(position) = drag.destination {
                if !position.is_finite() || position.abs() > 1_000_000. {
                    return Err(invalid("Guide position exceeds one million pixels. Move the guide closer to the canvas."));
                }
                if let Some(guide) = doc.guides.iter_mut().find(|guide| guide.id == drag.guide.id) {
                    guide.position = position;
                } else {
                    doc.guides.push(Guide { position, ..drag.guide });
                }
            } else {
                doc.guides.retain(|guide| guide.id != drag.guide.id);
            }
            compositor::guides::validate(&doc.guides)
        })
    }

    pub(super) fn layout_overlay(
        &mut self,
        cx: &mut ViewContext<'_, Self>,
        size: [f32; 2],
        zoom: f64,
        offset: [f64; 2],
        scale: f64,
    ) -> Element {
        let mut overlay = div().absolute().size_full();
        let doc = &self.session().document;
        let dimensions = [doc.width, doc.height];
        if self.tools.layout.grid && zoom * 64. * scale >= 4. {
            overlay = overlay.child(grid_overlay(
                &self.pixel_grid_shader,
                size,
                dimensions,
                zoom,
                offset,
                scale,
            ));
        }
        if self.tools.layout.guides {
            let mut guides = doc.guides.clone();
            if let Some(drag) = &self.layout_drag {
                guides.retain(|g| g.id != drag.guide.id);
                if let Some(position) = drag.destination {
                    guides.push(Guide {
                        position,
                        ..drag.guide
                    });
                } else if matches!(drag.origin, Origin::Existing(_)) {
                    // Keep the capture owner mounted while hovering a ruler to delete.
                    guides.push(drag.guide);
                }
            }
            let can_drag = self.tools.tool == Tool::Move
                && !self.tools.layout.locked
                && (self.can_edit_layers() || self.layout_drag.is_some());
            let inset = if self.tools.layout.rulers { RULER } else { 0. };
            for guide in guides {
                let axis = guide.axis.index();
                let at = (offset[axis] + guide.position * zoom) as f32;
                if at < inset || at > size[axis] {
                    continue;
                }
                let mut hit = div().absolute();
                let color = Color::rgba8(69, 207, 235, 220);
                hit = if axis == 0 {
                    hit.left(at - 3.)
                        .top(inset)
                        .w(6.)
                        .h((size[1] - inset).max(0.))
                        .child(
                            div()
                                .absolute()
                                .left(3.)
                                .top(0.)
                                .w((1. / scale) as f32)
                                .h_full()
                                .bg(color),
                        )
                } else {
                    hit.top(at - 3.)
                        .left(inset)
                        .h(6.)
                        .w((size[0] - inset).max(0.))
                        .child(
                            div()
                                .absolute()
                                .top(3.)
                                .left(0.)
                                .h((1. / scale) as f32)
                                .w_full()
                                .bg(color),
                        )
                };
                if can_drag {
                    hit = hit
                        .cursor(if axis == 0 {
                            CursorStyle::ResizeLeftRight
                        } else {
                            CursorStyle::ResizeUpDown
                        })
                        .on_pointer(cx.pointer_listener(
                            format!("layout-guide-{}", guide.id),
                            move |this, event, cx| {
                                let result = this.guide_pointer(Origin::Existing(guide), event);
                                this.operation_result(alerts::Operation::Paint, result, cx);
                            },
                        ));
                }
                overlay = overlay.child(hit);
            }
        }
        if self.tools.layout.rulers {
            overlay = overlay.child(self.ruler(cx, Axis::Horizontal, size[0], zoom, offset[0]));
            overlay = overlay.child(self.ruler(cx, Axis::Vertical, size[1], zoom, offset[1]));
            overlay = overlay.child(
                div()
                    .absolute()
                    .left(0.)
                    .top(0.)
                    .w(RULER)
                    .h(RULER)
                    .bg(Color::rgb8(48, 48, 48))
                    .child(
                        text("px")
                            .text_size(9.)
                            .text_color(Color::rgb8(170, 170, 170)),
                    ),
            );
        }
        overlay
    }
    fn ruler(
        &mut self,
        cx: &mut ViewContext<'_, Self>,
        axis: Axis,
        length: f32,
        zoom: f64,
        offset: f64,
    ) -> Element {
        let horizontal = axis == Axis::Horizontal;
        let ticks = ruler_ticks(length, zoom, offset);
        let mut ruler = div()
            .absolute()
            .left(0.)
            .top(0.)
            .bg(Color::rgb8(45, 45, 45))
            .overflow_hidden();
        ruler = if horizontal {
            ruler.w(length).h(RULER)
        } else {
            ruler.w(RULER).h(length)
        };
        for (at, value, major) in ticks {
            let mut tick = div().absolute().bg(Color::rgb8(123, 123, 123));
            tick = if horizontal {
                tick.left(at)
                    .top(if major { RULER - 8. } else { RULER - 4. })
                    .w(1.)
                    .h(if major { 8. } else { 4. })
            } else {
                tick.top(at)
                    .left(if major { RULER - 8. } else { RULER - 4. })
                    .h(1.)
                    .w(if major { 8. } else { 4. })
            };
            ruler = ruler.child(tick);
            if major {
                let value_label = if !horizontal && value.abs() >= 1_000_000. {
                    format!("{:.0}M", value / 1_000_000.)
                } else if !horizontal && value.abs() >= 1_000. {
                    if value.abs() < 10_000. && value % 1000. != 0. {
                        format!("{:.1}k", value / 1000.)
                    } else {
                        format!("{:.0}k", value / 1000.)
                    }
                } else {
                    format!("{value:.0}")
                };
                let label = text(value_label)
                    .absolute()
                    .text_size(9.)
                    .text_color(Color::rgb8(185, 185, 185));
                ruler = ruler.child(if horizontal {
                    label.left(at + 3.).top(1.)
                } else {
                    label.left(1.).top(at + 3.).w(RULER - 2.).text_size(8.)
                });
            }
        }
        let id = if horizontal {
            "ruler-horizontal"
        } else {
            "ruler-vertical"
        };
        let cursor = if self.tools.layout.locked {
            CursorStyle::Arrow
        } else if horizontal {
            CursorStyle::ResizeUpDown
        } else {
            CursorStyle::ResizeLeftRight
        };
        ruler
            .cursor(cursor)
            .on_pointer(cx.pointer_listener(id, move |this, event, cx| {
                let result = this.guide_pointer(Origin::Ruler(axis), event);
                this.operation_result(alerts::Operation::Paint, result, cx);
            }))
    }
}
fn ruler_ticks(length: f32, zoom: f64, offset: f64) -> Vec<(f32, f64, bool)> {
    if !zoom.is_finite() || zoom <= 0. {
        return Vec::new();
    }
    let target = 70. / zoom;
    let exponent = 10f64.powf(target.log10().floor());
    let major = [1., 2., 5., 10.]
        .into_iter()
        .map(|factor| factor * exponent)
        .find(|step| *step >= target)
        .unwrap_or(exponent * 10.);
    let minor = (major / 5.).max(1.);
    let start = ((f64::from(RULER) - offset) / zoom / minor).ceil() as i64;
    let end = ((f64::from(length) - offset) / zoom / minor).floor() as i64;
    (start..=end)
        .take(2000)
        .map(|i| {
            let value = i as f64 * minor;
            (
                (offset + value * zoom) as f32,
                value,
                (value / major - (value / major).round()).abs() < 0.0001,
            )
        })
        .collect()
}
fn grid_overlay(
    shader: &quickgui::CustomShader,
    size: [f32; 2],
    document: [u32; 2],
    zoom: f64,
    offset: [f64; 2],
    scale: f64,
) -> Element {
    let shader = shader.clone();
    quickgui::canvas(move |_, painter| {
        let left = offset[0].max(0.) as f32;
        let top = offset[1].max(0.) as f32;
        let right = (offset[0] + f64::from(document[0]) * zoom).min(f64::from(size[0])) as f32;
        let bottom = (offset[1] + f64::from(document[1]) * zoom).min(f64::from(size[1])) as f32;
        if right <= left || bottom <= top {
            return;
        }
        for (spacing, opacity) in [(8., 60), (64., 120)] {
            let period = zoom * spacing;
            if period * scale < 4. {
                continue;
            }
            let color = Color::rgba8(110, 135, 150, opacity);
            painter.paint_shader(
                Rect::new(left, top, right - left, bottom - top),
                &shader,
                ShaderParameters::new()
                    .vector(
                        0,
                        [
                            (offset[0] - f64::from(left)).rem_euclid(period) as f32,
                            (offset[1] - f64::from(top)).rem_euclid(period) as f32,
                            period as f32,
                            scale as f32,
                        ],
                    )
                    .vector(1, [color.r, color.g, color.b, color.a]),
            );
        }
    })
    .id("layout-grid")
    .absolute()
    .size_full()
    .into_element()
}

#[cfg(test)]
#[path = "layout_guides_tests.rs"]
mod tests;
