use super::*;
use crate::document::{Document, Layer, LayerContent};
use image::{Rgba, RgbaImage};
use std::sync::Arc;

#[test]
fn vibrance_psd_roundtrip_retains_editable_values_and_appearance() {
    for (vibrance, saturation) in [(-100., 100.), (0., -100.), (60., 20.), (0., 0.)] {
        let mut doc = Document::new(256, 1).unwrap();
        doc.layers[0].content =
            LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(256, 1, |x, _| {
                Rgba([x as u8, 120, 80, 128])
            }))));
        let mut layer = Layer::blank("Vibrance", 256, 1);
        layer.opacity = 0.75;
        layer.content = LayerContent::ExtendedAdjustment(Box::new(ExtendedAdjustment::Vibrance(
            Vibrance::new(vibrance, saturation).unwrap(),
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
fn fractional_vibrance_exports_rendered_with_notice_instead_of_truncating() {
    let mut doc = Document::new(2, 1).unwrap();
    doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        2,
        1,
        Rgba([160, 100, 80, 255]),
    ))));
    let mut layer = Layer::blank("Vibrance", 2, 1);
    let settings = Vibrance::new(47.25, -23.75).unwrap();
    assert!(export(settings).is_none());
    layer.content =
        LayerContent::ExtendedAdjustment(Box::new(ExtendedAdjustment::Vibrance(settings)));
    doc.add(layer).unwrap();
    let report = super::super::export_report(&doc).description();
    assert!(report.contains("flattened"), "{report}");
    let bytes = super::super::encode(&doc).unwrap();
    let reopened = super::super::decode(&bytes).unwrap().document;
    assert_eq!(
        crate::render::render(&doc, 2, 1).unwrap(),
        crate::render::render(&reopened, 2, 1).unwrap()
    );
    assert!(
        reopened
            .layers
            .iter()
            .all(|layer| !matches!(layer.content, LayerContent::ExtendedAdjustment(_)))
    );
}

#[test]
fn psd_vibrance_defaults_and_ranges_are_validated_before_conversion() {
    assert_eq!(
        import(&ps::VibranceAdjustment::default()).unwrap(),
        ExtendedAdjustment::Vibrance(Vibrance::default())
    );
    for value in [f64::NAN, f64::INFINITY, -100.00000001, 100.00000001] {
        assert!(
            import(&ps::VibranceAdjustment {
                vibrance: Some(value),
                saturation: None
            })
            .is_err()
        );
        assert!(
            import(&ps::VibranceAdjustment {
                vibrance: None,
                saturation: Some(value)
            })
            .is_err()
        );
    }
}
