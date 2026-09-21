//! Layer-list scrolling and the source app's single-transaction visibility swipe.
use super::*;
use quickgui::{MouseButton, PointerEvent, PointerPhase, ScrollAreaOrientation, ScrollAreaState};
use uuid::Uuid;

const ROW_STEP: f32 = 54.; // NativeLayerList: 52-pixel row plus 2-pixel spacing.

#[cfg(test)]
mod tests;

struct VisibilitySwipe {
    visible: bool,
    previous: usize,
}

#[derive(Default)]
pub(super) struct LayerList {
    pub cursors: super::layer_cursor::State,
    session: Option<Uuid>,
    order: Vec<Uuid>,
    bounds: quickgui::LayoutBoundsHandle,
    scroll: ScrollAreaState,
    swipe: Option<VisibilitySwipe>,
}

impl LayerList {
    pub(super) fn content_height(&self) -> f32 {
        self.scroll.content().height
    }

    fn row_at(&self, y: f32) -> Option<usize> {
        let bounds = self.bounds.bounds()?;
        if self.order.is_empty() {
            return None;
        }
        let y = (y - bounds.y).clamp(0., (bounds.height - 1.).max(0.));
        let row = ((y + self.scroll.offset().y) / ROW_STEP).floor() as usize;
        (row < self.order.len()).then_some(row)
    }
}

fn visit(
    doc: &Document,
    parent: Option<Uuid>,
    depth: usize,
    out: &mut Vec<(usize, usize)>,
    collapsed: &std::collections::HashSet<Uuid>,
) {
    if depth > 64 {
        return;
    }
    for (index, layer) in doc
        .layers
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, l)| l.parent == parent)
    {
        out.push((index, depth));
        if layer.is_group() && !collapsed.contains(&layer.id) {
            visit(doc, Some(layer.id), depth + 1, out, collapsed);
        }
    }
}

impl Editor {
    pub(super) fn prepare_layer_list(&mut self) -> Vec<(usize, usize)> {
        let session = self.session();
        let mut ordered = Vec::new();
        visit(&session.document, None, 0, &mut ordered, &session.collapsed);
        let ids: Vec<Uuid> = ordered
            .iter()
            .map(|(index, _)| session.document.layers[*index].id)
            .collect();
        let id = session.id;
        if self.layer_list.session != Some(id) {
            self.layer_list.scroll = ScrollAreaState::new();
            self.layer_list.session = Some(id);
        }
        self.layer_list
            .cursors
            .prepare(self.tabs[self.current].session().into_iter().flat_map(|s| {
                ordered
                    .iter()
                    .map(move |(index, _)| &s.document.layers[*index])
            }));
        self.layer_list.cursors.viewport = self.layer_list.bounds.clone();
        self.layer_list.order = ids;
        let viewport = self
            .layer_list
            .bounds
            .bounds()
            .map_or(quickgui::Size::ZERO, |bounds| {
                quickgui::Size::new(bounds.width, bounds.height)
            });
        let height = if ordered.is_empty() {
            viewport.height
        } else {
            ordered.len() as f32 * ROW_STEP + 32.
        };
        self.layer_list
            .scroll
            .set_geometry(viewport, quickgui::Size::new(viewport.width, height));
        ordered
    }

    pub(super) fn layer_list_view(&self, cx: &mut ViewContext<'_, Self>, rows: Element) -> Element {
        let mut viewport = div()
            .id("layer-list")
            .flex_1()
            .min_h(0.)
            .w_full()
            .relative()
            .overflow_hidden()
            .report_bounds(self.layer_list.bounds.clone())
            .on_mouse_move(cx.mouse_move_listener("layer-list", |this, event, cx| {
                let previous = this.layer_cursor();
                let entered = this.layer_list.cursors.pointer.is_none();
                this.keyboard_modifiers = event.modifiers;
                this.layer_list.cursors.pointer = Some(event.position);
                if entered || previous != this.layer_cursor() {
                    cx.invalidate();
                }
            }))
            .on_hover(cx.hover_listener("layer-list", |this, inside, cx| {
                // Overlays can uncover a stationary pointer without a motion event.
                this.layer_list.cursors.pointer =
                    if *inside { cx.pointer_position() } else { None };
                cx.invalidate();
            }))
            .capture_any_mouse_down(cx.mouse_down_listener("layer-list", |this, event, cx| {
                this.layer_clipping_press(event, cx);
            }))
            .on_scroll_wheel(cx.scroll_wheel_listener("layer-list", |this, event, cx| {
                if this.layer_list.scroll.apply_scroll_wheel(event) {
                    cx.invalidate();
                }
                cx.prevent_default();
            }))
            .child(
                rows.absolute()
                    .left(0.)
                    .top(-self.layer_list.scroll.offset().y),
            );
        let orientation = ScrollAreaOrientation::Vertical;
        let height = self.layer_list.scroll.viewport().height;
        if self.layer_list.scroll.max_offset().y > 0. {
            let thumb = self.layer_list.scroll.thumb_length(orientation, height);
            let offset = self.layer_list.scroll.thumb_offset(orientation, height);
            viewport = viewport.child(
                div()
                    .id("layer-list-scrollbar")
                    .absolute()
                    .right(1.)
                    .top(offset)
                    .w(6.)
                    .h(thumb)
                    .rounded(3.)
                    .bg(Color::rgba8(170, 170, 170, 120))
                    .on_pointer(
                        cx.pointer_listener("layer-list-scrollbar", |this, event, cx| {
                            let height = this.layer_list.scroll.viewport().height;
                            if this.layer_list.scroll.apply_thumb_pointer(
                                event,
                                ScrollAreaOrientation::Vertical,
                                height,
                            ) {
                                cx.invalidate();
                            }
                        }),
                    ),
            );
        }
        viewport
    }

    pub(super) fn finish_visibility_swipe(&mut self) -> Result<()> {
        if self.layer_list.swipe.take().is_some()
            && let Some(session) = self
                .tabs
                .iter_mut()
                .find(|tab| Some(tab.id) == self.layer_list.session)
                .and_then(ProjectTab::session_mut)
        {
            session.commit()?;
        }
        Ok(())
    }

    pub(super) fn visibility_pointer(&mut self, id: Uuid, event: &PointerEvent) -> Result<()> {
        if event.button != MouseButton::Left {
            return Ok(());
        }
        match event.phase {
            PointerPhase::Down => self.begin_visibility_swipe(id)?,
            PointerPhase::Move if self.layer_list.swipe.is_some() => {
                if let Some(bounds) = self.layer_list.bounds.bounds() {
                    // Like NSTableView.autoscroll, continue through rows revealed at the list edge.
                    let local = event.position.y - bounds.y;
                    let edge = 22_f32.min(bounds.height / 2.);
                    let delta = if local < edge {
                        (local - edge).max(-ROW_STEP)
                    } else if local > bounds.height - edge {
                        (local - bounds.height + edge).min(ROW_STEP)
                    } else {
                        0.
                    };
                    self.layer_list
                        .scroll
                        .scroll_by(quickgui::Vector::new(0., delta));
                    if let Some(row) = self.layer_list.row_at(event.position.y) {
                        self.set_swipe_rows(row);
                    }
                }
            }
            PointerPhase::Up | PointerPhase::Cancel => self.finish_visibility_swipe()?,
            _ => {}
        }
        Ok(())
    }

    pub(super) fn begin_visibility_swipe(&mut self, id: Uuid) -> Result<()> {
        if !self.can_edit_layers() || self.modal.is_some() {
            return Ok(());
        }
        self.finish_pending_edits()?;
        let Some(layer) = self.session().document.layer(id) else {
            return Ok(());
        };
        let visible = !layer.visible;
        let Some(previous) = self.layer_list.order.iter().position(|layer| *layer == id) else {
            return Ok(());
        };
        self.session_mut()
            .begin(if visible { "Show Layer" } else { "Hide Layer" })?;
        self.layer_list.swipe = Some(VisibilitySwipe { visible, previous });
        self.set_swipe_rows(previous);
        Ok(())
    }

    fn set_swipe_rows(&mut self, row: usize) {
        let Some(swipe) = &mut self.layer_list.swipe else {
            return;
        };
        let visible = swipe.visible;
        let range = row.min(swipe.previous)..=row.max(swipe.previous);
        swipe.previous = row;
        let ids = self.layer_list.order[range].to_vec();
        for layer in &mut self.session_mut().document.layers {
            if ids.contains(&layer.id) {
                layer.visible = visible;
            }
        }
    }
}
