//! Fade the last raster edit without retaining an extra document-sized snapshot.
use super::*;
use crate::document::{Layer, LayerContent};
use image::RgbaImage;
use rayon::prelude::*;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct Fade {
    before: Layer,
    after: Layer,
    label: String,
}

fn same_arc<T>(a: Option<&Arc<T>>, b: Option<&Arc<T>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        _ => false,
    }
}

fn same_layer_metadata(before: &Layer, after: &Layer, active: bool) -> bool {
    if !same_arc(before.raw.as_ref(), after.raw.as_ref())
        || !same_arc(
            before.mask.as_ref().map(|m| &m.pixels),
            after.mask.as_ref().map(|m| &m.pixels),
        )
        || (!active && !same_arc(before.raster(), after.raster()))
    {
        return false;
    }
    let mut comparable = after.clone();
    // Pointer checks above establish asset identity, so avoid PartialEq on RAW
    // byte buffers and other large shared assets during menu availability.
    comparable.raw = None;
    let mut original = before.clone();
    original.raw = None;
    if active {
        comparable.content = original.content.clone();
        comparable.text = original.text.clone();
        comparable.shape = original.shape;
    }
    // ImageBuffer is Eq, so Arc's equality short-circuits identical pointers.
    comparable == original
}

impl Session {
    fn fade_layers(&self) -> Option<(&Layer, &Layer, &str)> {
        if self.has_pending_edit() || !self.future.is_empty() {
            return None;
        }
        let entry = self.past.last()?;
        let before = entry.document.active_layer()?;
        let after = self.document.active_layer()?;
        let (old, new) = (before.raster()?, after.raster()?);
        if before.id != after.id
            || old.dimensions() != new.dimensions()
            || Arc::ptr_eq(old, new)
            || before.raw.is_some()
            || after.raw.is_some()
            || (after.text.is_some() && after.text != before.text)
            || (after.shape.is_some() && after.shape != before.shape)
        {
            return None;
        }
        if self.document.layers.len() != entry.document.layers.len()
            || !self
                .document
                .layers
                .iter()
                .zip(&entry.document.layers)
                .all(|(new, old)| same_layer_metadata(old, new, new.id == after.id))
        {
            return None;
        }
        // Reject geometry, stack, selection and multi-layer edits. Only the
        // active layer's pixels (and rasterized source metadata) may differ.
        if !match (&entry.document.selection, &self.document.selection) {
            (None, None) => true,
            (Some(old), Some(new)) => old.same_snapshot(new),
            _ => false,
        } {
            return None;
        }
        let mut comparable = self.document.clone();
        let mut original = entry.document.clone();
        comparable.layers.clear();
        original.layers.clear();
        comparable.selection = None;
        original.selection = None;
        if comparable != original {
            return None;
        }
        Some((before, after, &entry.label))
    }

    pub fn can_fade(&self) -> bool {
        self.fade_layers().is_some()
    }

    pub fn fade(&self) -> Result<Fade> {
        let (before, after, label) = self.fade_layers().ok_or_else(|| invalid(
            "There is no raster edit to fade. Apply a filter or paint on an existing layer first. Fade requires unchanged layer dimensions and placement.",
        ))?;
        Ok(Fade {
            before: before.clone(),
            after: after.clone(),
            label: label.into(),
        })
    }
}

impl Fade {
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Blend the two committed rasters in premultiplied alpha. The source must
    /// still be the completed edit, never a previously faded preview.
    pub fn apply(&self, document: &mut Document, amount: f64) -> Result<()> {
        if !(0. ..=1.).contains(&amount) {
            return Err(invalid(
                "Fade opacity must be between 0 and 100%. The edit is unchanged.",
            ));
        }
        let layer = document.active_layer_mut().filter(|layer| **layer == self.after)
            .ok_or_else(|| invalid("The layer changed after Fade opened. Reopen Fade for the latest edit. Current pixels are preserved."))?;
        if amount == 1. {
            return Ok(());
        }
        if amount == 0. {
            *layer = self.before.clone();
            return Ok(());
        }
        let (Some(before), Some(after)) = (self.before.raster(), self.after.raster()) else {
            return Err(invalid(
                "Fade's source pixels are missing. The edit is unchanged.",
            ));
        };
        let mut pixels = RgbaImage::new(after.width(), after.height());
        let output: &mut [u8] = pixels.as_mut();
        output
            .par_chunks_exact_mut(4)
            .zip(before.as_raw().par_chunks_exact(4))
            .zip(after.as_raw().par_chunks_exact(4))
            .for_each(|((out, old), new)| {
                if old == new {
                    out.copy_from_slice(new);
                    return;
                }
                let old_alpha = f64::from(old[3]) * (1. - amount);
                let new_alpha = f64::from(new[3]) * amount;
                let alpha = old_alpha + new_alpha;
                if alpha == 0. {
                    out.copy_from_slice(new);
                    return;
                }
                for channel in 0..3 {
                    out[channel] = ((f64::from(old[channel]) * old_alpha
                        + f64::from(new[channel]) * new_alpha)
                        / alpha)
                        .round() as u8;
                }
                out[3] = alpha.round() as u8;
            });
        layer.content = LayerContent::Raster(Some(Arc::new(pixels)));
        layer.text = None;
        layer.shape = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
