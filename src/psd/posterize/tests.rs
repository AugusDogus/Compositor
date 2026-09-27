use super::*;
use crate::document::{Document, Layer, LayerContent};
use image::{Rgba, RgbaImage};
use std::sync::Arc;

#[test]
fn posterize_psd_roundtrip_retains_editable_level_and_appearance() {
    for level in [2, 4, 128, 255, 256] {
        let mut doc = Document::new(256, 1).unwrap();
        doc.layers[0].content =
            LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(256, 1, |x, _| {
                Rgba([x as u8, x as u8, x as u8, 128])
            }))));
        let mut layer = Layer::blank("Posterize", 256, 1);
        layer.opacity = 0.75;
        layer.content = LayerContent::ExtendedAdjustment(Box::new(ExtendedAdjustment::Posterize(
            Posterize::new(level).unwrap(),
        )));
        doc.add(layer).unwrap();
        assert!(
            !super::super::export_report(&doc)
                .description()
                .contains("flattened")
        );
        let bytes = super::super::encode(&doc).unwrap();
        let reopened = super::super::decode(&bytes).unwrap().document;
        assert_eq!(reopened.layers.len(), 2);
        assert_eq!(reopened.layers[1].content, doc.layers[1].content);
        let before = crate::render::render(&doc, 256, 1).unwrap();
        let after = crate::render::render(&reopened, 256, 1).unwrap();
        for (a, b) in before.as_raw().iter().zip(after.as_raw()) {
            assert!(a.abs_diff(*b) <= 1);
        }
    }
}

#[test]
fn posterize_psd_rejects_missing_fractional_and_out_of_range_levels() {
    for level in [
        None,
        Some(-1.),
        Some(257.),
        Some(1.),
        Some(127.5),
        Some(f64::NAN),
        Some(f64::INFINITY),
    ] {
        assert!(import(&ps::PosterizeAdjustment { levels: level }).is_err());
    }
    for data in [vec![], vec![0], vec![1, 1, 0, 0], vec![255, 255, 0, 0]] {
        assert!(super::super::adjustments::validate_record(b"post", &data).is_err());
    }
    for [high, low] in [[0, 2], [0, 255], [1, 0]] {
        super::super::adjustments::validate_record(b"post", &[high, low, 0, 0]).unwrap();
    }
}
