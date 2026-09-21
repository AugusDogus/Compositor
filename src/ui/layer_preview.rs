//! Apply a tool's pixel override to current document metadata.
use compositor::document::{Document, Layer};

/// History can remove the target or change its placement, mask, and siblings while
/// a preview remains open. Only fields changed by the pixel operation override it.
pub(super) fn overlay(
    document: &mut Document,
    original: &Layer,
    prepared: &Layer,
    mask_only: bool,
) {
    let Some(current) = document
        .layers
        .iter_mut()
        .find(|layer| layer.id == original.id)
    else {
        return;
    };
    if !mask_only {
        current.content = prepared.content.clone();
        current.shape = prepared.shape;
        current.text = prepared.text.clone();
    }
    if prepared.transform != original.transform {
        current.transform = prepared.transform;
    }
    if let (Some(current), Some(prepared), Some(original)) =
        (&mut current.mask, &prepared.mask, &original.mask)
    {
        if mask_only || prepared.pixels != original.pixels {
            current.pixels = prepared.pixels.clone();
        }
        // A pixel filter may grow its source grid. Carry a still-normalized mask,
        // but do not undo a placement restored independently through history.
        if !mask_only && current.placement == original.placement {
            current.placement = prepared.placement;
        }
    }
}

/// Background removal creates or replaces coverage and explicitly enables it.
/// Existing link and placement settings belong to the current document.
pub(super) fn background_mask(document: &mut Document, prepared: &Layer) {
    let Some(current) = document
        .layers
        .iter_mut()
        .find(|layer| layer.id == prepared.id)
    else {
        return;
    };
    if let Some(prepared) = &prepared.mask {
        if let Some(current) = &mut current.mask {
            current.pixels = prepared.pixels.clone();
            current.enabled = true;
        } else {
            current.mask = Some(prepared.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compositor::{document::Mask, geometry::Transform};
    use std::sync::Arc;

    #[test]
    fn background_preview_creates_or_enables_mask_without_restoring_old_metadata() {
        let mut doc = Document::new(8, 8).unwrap();
        let mut prepared = doc.layers[0].clone();
        prepared.mask = Some(Mask {
            pixels: Arc::new(image::GrayImage::from_pixel(8, 8, image::Luma([128]))),
            enabled: true,
            linked: true,
            placement: None,
        });
        background_mask(&mut doc, &prepared);
        assert_eq!(doc.layers[0].mask, prepared.mask);
        let current = doc.layers[0].mask.as_mut().unwrap();
        current.enabled = false;
        current.linked = false;
        current.placement = Some(Transform::new(4, 4));
        let mut expected = current.clone();
        expected.enabled = true;
        background_mask(&mut doc, &prepared);
        assert_eq!(doc.layers[0].mask.as_ref(), Some(&expected));
        doc.layers.clear();
        background_mask(&mut doc, &prepared);
        assert!(doc.layers.is_empty());
    }

    #[test]
    fn mask_pixel_override_preserves_current_metadata_and_does_not_restore_removed_mask() {
        let mut doc = Document::new(8, 8).unwrap();
        let mask = Mask {
            pixels: Arc::new(image::GrayImage::from_pixel(8, 8, image::Luma([255]))),
            enabled: true,
            linked: true,
            placement: None,
        };
        doc.layers[0].mask = Some(mask);
        let original = doc.layers[0].clone();
        let mut prepared = original.clone();
        prepared.mask.as_mut().unwrap().pixels =
            Arc::new(image::GrayImage::from_pixel(8, 8, image::Luma([128])));
        let current = doc.layers[0].mask.as_mut().unwrap();
        current.enabled = false;
        current.linked = false;
        current.placement = Some(Transform::new(4, 4));
        let mut expected = current.clone();
        expected.pixels = prepared.mask.as_ref().unwrap().pixels.clone();
        overlay(&mut doc, &original, &prepared, true);
        assert_eq!(doc.layers[0].mask.as_ref(), Some(&expected));
        doc.layers[0].mask = None;
        overlay(&mut doc, &original, &prepared, true);
        assert!(doc.layers[0].mask.is_none());
    }

    #[test]
    fn grown_pixel_preview_keeps_current_mask_flags_and_independent_placement() {
        let mut doc = Document::new(8, 8).unwrap();
        doc.layers[0].mask = Some(Mask {
            pixels: Arc::new(image::GrayImage::from_pixel(8, 8, image::Luma([255]))),
            enabled: true,
            linked: true,
            placement: None,
        });
        let original = doc.layers[0].clone();
        let mut prepared = original.clone();
        prepared.transform = Transform::new(12, 12);
        prepared.mask.as_mut().unwrap().placement = Some(original.transform);
        let current = doc.layers[0].mask.as_mut().unwrap();
        current.enabled = false;
        current.linked = false;
        current.placement = Some(Transform::new(3, 3));
        let expected = current.clone();
        overlay(&mut doc, &original, &prepared, false);
        assert_eq!(doc.layers[0].mask.as_ref(), Some(&expected));
        assert_eq!(doc.layers[0].transform, prepared.transform);
    }
}
