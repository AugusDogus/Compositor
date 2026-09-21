use super::selection_paths::{OutlinePaths, Viewport};
use super::*;
use compositor::{geometry::Point, selection::Selection};
use std::time::{Duration, Instant};

const FRAME_TIME: Duration = Duration::from_millis(120);

pub(super) struct SelectionOutline {
    selection: Option<Selection>,
    viewport: Viewport,
    outline: Option<OutlinePaths>,
    started: Instant,
}

fn same_selection(a: Option<&Selection>, b: Option<&Selection>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.origin == b.origin && Arc::ptr_eq(&a.pixels, &b.pixels),
        (None, None) => true,
        _ => false,
    }
}

fn animation_frame(started: Instant, now: Instant) -> (u8, Instant) {
    let elapsed = now.saturating_duration_since(started);
    let ticks = elapsed.as_millis() / FRAME_TIME.as_millis();
    (
        (ticks % 8) as u8,
        now + FRAME_TIME
            - Duration::from_nanos((elapsed.as_nanos() % FRAME_TIME.as_nanos()) as u64),
    )
}

impl Editor {
    pub(super) fn selection_outline(
        &mut self,
        cx: &mut ViewContext<'_, Self>,
        size: [f32; 2],
        zoom: f64,
        offset: Point,
    ) -> Element {
        let now = Instant::now();
        let viewport = Viewport { size, zoom, offset };
        let selection = self.session().document.selection.as_ref();
        if self.selection_outline.as_ref().is_none_or(|cache| {
            !same_selection(cache.selection.as_ref(), selection) || cache.viewport != viewport
        }) {
            let selection = selection.cloned();
            let paths = selection
                .as_ref()
                .map_or(Ok(None), |selection| OutlinePaths::new(selection, viewport));
            let paths = match paths {
                Ok(paths) => paths,
                Err(error) => {
                    self.status = error.to_string();
                    None
                }
            };
            let started = self
                .selection_outline
                .as_ref()
                .map_or(now, |cache| cache.started);
            self.selection_outline = Some(SelectionOutline {
                selection,
                viewport,
                outline: paths,
                started,
            });
        }
        let mut overlay = div().absolute().size_full().accessibility_hidden(true);
        if let Some(cache) = self.selection_outline.as_mut()
            && let Some(outline) = cache.outline.as_mut()
        {
            let (phase, next) = animation_frame(cache.started, now);
            cx.request_repaint_at(next);
            match outline.black(phase) {
                Ok(black) => {
                    let white = outline.white.clone();
                    overlay = overlay.child(
                        quickgui::canvas(move |_, painter| {
                            for path in white.iter() {
                                painter.paint_path(path, Color::WHITE);
                            }
                            for path in black.iter() {
                                painter.paint_path(path, Color::BLACK);
                            }
                        })
                        .absolute()
                        .size_full(),
                    );
                }
                Err(error) => self.status = error.to_string(),
            }
        }
        overlay
    }
}

#[cfg(test)]
#[path = "selection_outline_tests.rs"]
mod tests;
