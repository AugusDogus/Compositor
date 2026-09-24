use compositor::document::{
    Document, Layer, LayerContent, MAX_SURFACE_PIXELS, Mask, document_pixel_budget,
    validate_pixel_budget, validate_size,
};
use image::{GrayImage, RgbaImage};
use std::sync::Arc;

#[test]
fn surface_and_document_budgets_have_distinct_boundaries() {
    assert_eq!(MAX_SURFACE_PIXELS, 200_000_000);
    assert!(validate_size(20_000, 10_000).is_ok());
    assert!(validate_size(20_001, 10_000).is_err());
    assert!(validate_size(30_001, 1).is_err());
    assert!(validate_pixel_budget(document_pixel_budget()).is_ok());
    assert!(validate_pixel_budget(document_pixel_budget() + 1).is_err());
    assert!(Document::new(30_000, 30_000).unwrap().validate().is_ok());
}

#[test]
fn layer_and_mask_storage_share_one_budget() {
    // Sharing fixture storage exercises cumulative accounting without allocating gigabytes.
    let pixels = Arc::new(RgbaImage::new(1000, 1000));
    let mask = Arc::new(GrayImage::new(1000, 1000));
    let mut doc = Document::new(1000, 1000).unwrap();
    doc.layers.clear();
    doc.active = None;
    doc.selected.clear();
    let count = document_pixel_budget() / 2_000_000;
    for _ in 0..count {
        let mut layer = Layer::blank("Budget fixture", 1000, 1000);
        layer.content = LayerContent::Raster(Some(pixels.clone()));
        layer.mask = Some(Mask {
            pixels: mask.clone(),
            enabled: true,
            linked: true,
            placement: None,
        });
        doc.layers.push(layer);
    }
    assert!(doc.validate().is_ok());
    let mut overflow = doc.layers[0].clone();
    overflow.id = uuid::Uuid::new_v4();
    doc.layers.push(overflow);
    assert!(
        doc.validate()
            .unwrap_err()
            .to_string()
            .contains("across layers and masks")
    );
}
