//! Artboards isolate their children before applying the owner mask and opacity.
use super::*;

pub(super) fn paint(
    doc: &Document,
    layer: &Layer,
    point: Point,
    inherited: InheritedCoverage,
    out: &mut [f64; 4],
    depth: usize,
    state: &RenderState,
) -> bool {
    let LayerContent::Artboard(board) = &layer.content else {
        return false;
    };
    let inside = layer
        .transform
        .unit(point)
        .iter()
        .all(|v| (0. ..1.).contains(v));
    let mut local = board.background.map(|v| f64::from(v) / 255.);
    // Still traverse outside the frame: an early stop before a board adjustment
    // must return transparent, rather than continuing into unrelated layers.
    let stopped = paint_children(
        doc,
        Some(layer.id),
        point,
        InheritedCoverage {
            mask: 1.,
            opacity: 1.,
        },
        &mut local,
        depth + 1,
        state,
    );
    if !inside {
        local = [0.; 4];
    }
    if stopped {
        *out = local;
        return true;
    }
    local[3] *= mask_alpha(layer, point, &state.backgrounds)
        * layer.opacity
        * inherited.mask
        * inherited.opacity;
    *out = layer.blend.composite(*out, local);
    false
}

#[cfg(test)]
mod tests;
