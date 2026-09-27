//! Projective placements accompany affine native sources and a rendered preview.
use super::*;
use crate::geometry::Transform;

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "target", deny_unknown_fields)]
pub(super) enum Saved {
    Layer { layer: Uuid, placement: Transform },
    Mask { layer: Uuid, placement: Transform },
}

pub(super) fn extract(document: &mut Document) -> Vec<Saved> {
    let mut saved = Vec::new();
    for layer in &mut document.layers {
        if layer.transform.warp.is_some() {
            saved.push(Saved::Layer {
                layer: layer.id,
                placement: layer.transform,
            });
            layer.transform.warp = None;
        }
        if let Some(placement) = layer.mask.as_mut().and_then(|mask| mask.placement.as_mut())
            && placement.warp.is_some()
        {
            saved.push(Saved::Mask {
                layer: layer.id,
                placement: *placement,
            });
            placement.warp = None;
        }
    }
    saved
}

impl Saved {
    fn parts(self) -> (Uuid, Transform, bool) {
        match self {
            Self::Layer { layer, placement } => (layer, placement, false),
            Self::Mask { layer, placement } => (layer, placement, true),
        }
    }
}

pub(super) fn validate(saved: &[Saved]) -> Result<()> {
    let mut seen = HashSet::new();
    for entry in saved {
        let (id, placement, mask) = entry.parts();
        if id.is_nil() || !seen.insert((id, mask)) || placement.warp.is_none() || !placement.valid()
        {
            return Err(invalid(
                "The perspective snapshot has an invalid placement or repeated target. The source files are unchanged.",
            ));
        }
    }
    Ok(())
}

fn restore_placement(current: &mut Transform, saved: Transform) -> Result<()> {
    let affine = Transform {
        warp: None,
        ..saved
    };
    if *current != affine {
        return Err(invalid(
            "The perspective placement does not match its saved source. Open the rendered copy or restore a complete project backup.",
        ));
    }
    *current = saved;
    Ok(())
}

pub(super) fn preflight(document: &Document, saved: &[Saved]) -> Result<()> {
    for entry in saved {
        let (id, placement, mask) = entry.parts();
        let layer = document
            .layer(id)
            .ok_or_else(|| invalid("Perspective refers to a missing source layer."))?;
        if !mask {
            if layer.is_group() || layer.is_adjustment() {
                return Err(invalid(
                    "A projective image placement requires a pixel, text, or shape source.",
                ));
            }
            restore_placement(&mut { layer.transform }, placement)?;
        }
    }
    Ok(())
}

pub(super) fn restore(document: &mut Document, saved: Vec<Saved>) -> Result<()> {
    for entry in saved {
        let (id, placement, mask) = entry.parts();
        let layer = document
            .layers
            .iter_mut()
            .find(|layer| layer.id == id)
            .ok_or_else(|| invalid("Perspective refers to a missing source layer."))?;
        let target = if mask {
            layer
                .mask
                .as_mut()
                .and_then(|mask| mask.placement.as_mut())
                .ok_or_else(|| invalid("Perspective refers to a missing mask placement."))?
        } else {
            &mut layer.transform
        };
        restore_placement(target, placement)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
