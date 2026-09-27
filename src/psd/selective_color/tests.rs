use super::*;
use crate::{
    document::{Document, Layer, LayerContent},
    render,
};
use image::{Rgba, RgbaImage};
use std::sync::Arc;

fn document(settings: SelectiveColor) -> Document {
    let mut doc = Document::new(3, 2).unwrap();
    doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        3,
        2,
        Rgba([53, 119, 187, 200]),
    ))));
    let mut layer = Layer::blank("Selective Color", 3, 2);
    layer.opacity = 0.75;
    layer.content =
        LayerContent::ExtendedAdjustment(Box::new(ExtendedAdjustment::SelectiveColor(settings)));
    doc.add(layer).unwrap();
    doc
}

#[test]
fn selective_color_psd_roundtrip_retains_all_ranges_modes_and_appearance() {
    for mode in [Mode::Relative, Mode::Absolute] {
        let settings = SelectiveColor {
            mode,
            adjustments: std::array::from_fn(|i| [i as f32 * 10., -100., 100., -15.]),
        };
        let doc = document(settings);
        assert!(
            !super::super::export_report(&doc)
                .description()
                .contains("flattened")
        );
        let bytes = super::super::encode(&doc).unwrap();
        let reopened = super::super::decode(&bytes).unwrap().document;
        assert_eq!(reopened.layers.len(), 2);
        assert_eq!(reopened.layers[1].content, doc.layers[1].content);
        let before = render::render(&doc, 3, 2).unwrap();
        let after = render::render(&reopened, 3, 2).unwrap();
        for (a, b) in before.as_raw().iter().zip(after.as_raw()) {
            assert!(a.abs_diff(*b) <= 1);
        }
    }
}

#[test]
fn fractional_selective_color_exports_explicit_rendered_copy_without_truncation() {
    let mut settings = SelectiveColor::default();
    settings.adjustments[7] = [40.5, -27.25, 14.75, 3.5];
    let doc = document(settings);
    assert!(export(&settings).is_none());
    let report = super::super::export_report(&doc).description();
    assert!(report.contains("Selective Color") && report.contains("All layers are flattened"));
    let bytes = super::super::encode(&doc).unwrap();
    let reopened = super::super::decode(&bytes).unwrap().document;
    assert_eq!(reopened.layers.len(), 1);
    assert_eq!(
        render::render(&reopened, 3, 2).unwrap(),
        render::render(&doc, 3, 2).unwrap()
    );
    assert_eq!(
        doc.layers[1].content,
        LayerContent::ExtendedAdjustment(Box::new(ExtendedAdjustment::SelectiveColor(settings)))
    );
}

#[test]
fn malformed_selective_color_psd_rows_are_rejected() {
    let ps::AdjustmentLayer::SelectiveColor(mut source) =
        export(&SelectiveColor::default()).unwrap()
    else {
        panic!("wrong PSD adjustment");
    };
    for invalid in [100.00000001, -100.1, f64::INFINITY, f64::NAN] {
        source.reds = Some(ps::Cmyk {
            c: invalid,
            ..Default::default()
        });
        assert!(import(&source).is_err());
    }
    source.reds = None;
    assert!(import(&source).is_err());
}
