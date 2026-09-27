use super::*;
use crate::{
    adjustment::{Adjustment, Kind},
    document::Mask,
    geometry::Transform,
};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use std::sync::Arc;

#[test]
fn artboard_psd_reports_flattening_and_preserves_clips_masks_and_root_adjustments() {
    let mut doc = Document::new(8, 6).unwrap();
    doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        8,
        6,
        Rgba([11, 22, 33, 255]),
    ))));
    let board = crate::artboard::create(
        &mut doc,
        "Masked board",
        Transform {
            origin: [1., 1.],
            ..Transform::new(4, 4)
        },
        [40, 100, 180, 200],
    )
    .unwrap();
    doc.active_layer_mut().unwrap().opacity = 0.5;
    doc.active_layer_mut().unwrap().mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_fn(4, 4, |x, _| Luma([64 + x as u8 * 50]))),
        enabled: true,
        linked: true,
        placement: None,
    });
    let mut child = Layer::blank("Oversized child", 8, 6);
    child.parent = Some(board);
    child.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        8,
        6,
        Rgba([220, 70, 30, 180]),
    ))));
    doc.add(child).unwrap();
    crate::artboard::create(
        &mut doc,
        "Overlapping board",
        Transform {
            origin: [4., 0.],
            ..Transform::new(3, 3)
        },
        [10, 60, 240, 190],
    )
    .unwrap();
    let mut invert = Layer::blank("Root invert", 8, 6);
    invert.content = LayerContent::Adjustment(Box::new(Adjustment::new(Kind::Invert)));
    doc.add(invert).unwrap();
    doc.resolution = 300.;
    let original = doc.clone();
    let notice = export_report(&doc).description();
    assert!(notice.contains("Artboards"));
    assert!(notice.contains("All layers are flattened"));
    assert!(notice.contains(".comp"));
    let expected = render::render(&doc, 8, 6).unwrap();
    assert_ne!(expected[(2, 2)], expected[(0, 0)]);
    assert_ne!(expected[(4, 2)], expected[(2, 2)]);
    let bytes = encode(&doc).unwrap();
    let psd = ag_psd::read_psd(
        &bytes,
        &photoshop::ReadOptions {
            use_image_data: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    let layers = psd.children.unwrap();
    assert_eq!(layers.len(), 1);
    assert!(layers[0].children.as_ref().is_none_or(Vec::is_empty));
    assert_eq!(
        layers[0].image_data.as_ref().unwrap().data,
        *expected.as_raw()
    );
    assert_eq!(psd.image_data.unwrap().data, *expected.as_raw());
    let reopened = super::super::decode(&bytes).unwrap().document;
    assert_eq!(reopened.layers.len(), 1);
    assert!(!reopened.layers.iter().any(Layer::is_artboard));
    assert_eq!(reopened.resolution, 300.);
    assert_eq!(render::render(&reopened, 8, 6).unwrap(), expected);
    assert_eq!(doc, original);
}
