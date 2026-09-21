//! Match DocumentHistory: count distinct historical layer assets, excluding the live canvas.
use super::*;
use std::sync::Arc;

const ENTRY_LIMIT: usize = 100;
const BYTE_LIMIT: usize = 256 * 1024 * 1024;

#[derive(Clone, Copy)]
enum Canvas {
    Visible,
    Welcome,
}

impl Session {
    pub(super) fn trim_history(&mut self) {
        self.trim_history_to(Canvas::Visible, ENTRY_LIMIT, BYTE_LIMIT);
    }

    /// Apply the history budget after undoing creation. If its Redo is evicted,
    /// the welcome tab must release this session while preserving its saved path.
    pub fn retain_creation_redo(&mut self) -> bool {
        debug_assert!(self.creation.is_some() && self.past.is_empty());
        self.trim_history_to(Canvas::Welcome, ENTRY_LIMIT, BYTE_LIMIT);
        self.creation.is_some()
    }

    fn trim_history_to(&mut self, canvas: Canvas, entry_limit: usize, byte_limit: usize) {
        while self.past.len() + self.future.len() + usize::from(self.creation.is_some())
            > entry_limit
            || self.retained_history_bytes(canvas) > byte_limit
        {
            if matches!(canvas, Canvas::Visible) && self.creation.take().is_some() {
                continue;
            }
            if !self.past.is_empty() {
                self.past.remove(0);
            } else if !self.future.is_empty() {
                self.future.remove(0);
            } else if matches!(canvas, Canvas::Welcome) && self.creation.take().is_some() {
                // Creation is the earliest Undo entry and the nearest Redo entry.
                continue;
            } else {
                break;
            }
        }
    }

    fn retained_history_bytes(&self, canvas: Canvas) -> usize {
        let mut seen = HashSet::new();
        if matches!(canvas, Canvas::Visible) {
            visit_assets(self.committed_document(), &mut seen);
        }
        let mut bytes = 0usize;
        for entry in self.past.iter().chain(self.future.iter()) {
            bytes = bytes.saturating_add(visit_assets(&entry.document, &mut seen));
        }
        if matches!(canvas, Canvas::Welcome) && self.creation.is_some() {
            bytes = bytes.saturating_add(visit_assets(self.committed_document(), &mut seen));
        }
        bytes
    }
}

fn visit_assets(document: &Document, seen: &mut HashSet<usize>) -> usize {
    let mut bytes = 0usize;
    for layer in &document.layers {
        if let Some(pixels) = layer.raster()
            && seen.insert(Arc::as_ptr(pixels) as usize)
        {
            bytes = bytes.saturating_add(pixels.as_raw().len());
        }
        if let Some(mask) = &layer.mask
            && seen.insert(Arc::as_ptr(&mask.pixels) as usize)
        {
            bytes = bytes.saturating_add(mask.pixels.as_raw().len());
        }
    }
    bytes
}

#[cfg(test)]
mod tests;
