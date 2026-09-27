use compositor::{
    document::{Document, LayerContent},
    filters::{
        self, Filter,
        radial::{Mode, Radial},
    },
    geometry::Transform,
    selection::Selection,
};
use image::{Rgba, RgbaImage};
use std::sync::Arc;

#[test]
fn radial_blur_respects_document_selection_and_keeps_layer_and_mask_placement() {
    for mode in [Mode::Spin, Mode::Zoom] {
        let mut doc = Document::new(160, 120).unwrap();
        let image = RgbaImage::from_fn(33, 29, |x, y| {
            Rgba([
                if x < 16 { 240 } else { 30 },
                (y * 8) as u8,
                80,
                if y < 4 { 0 } else { 180 },
            ])
        });
        doc.layers[0].content = LayerContent::Raster(Some(Arc::new(image.clone())));
        doc.layers[0].transform = Transform {
            origin: [15., 20.],
            size: [99., 87.],
            rotation: 20.,
            ..Transform::new(33, 29)
        };
        compositor::edits::add_mask(&mut doc, false).unwrap();
        let filter = Filter::Radial(Radial {
            mode,
            amount: 80.,
            center: [0.2, 0.7],
        });
        let mut local = Document::new(33, 29).unwrap();
        local.layers[0].content = LayerContent::Raster(Some(Arc::new(image.clone())));
        filters::apply(&mut local, filter, false).unwrap();
        let expected = local.layers[0].raster().unwrap();
        let placement = doc.layers[0].transform;
        let mask = doc.layers[0].mask.clone();
        let selection = Selection::rectangle(160, 120, [0., 0.], [60., 120.], false);
        doc.selection = Some(selection.clone());
        filters::apply(&mut doc, filter, false).unwrap();
        assert_eq!(doc.layers[0].transform, placement);
        assert_eq!(doc.layers[0].mask, mask);
        assert_eq!(doc.selection, Some(selection.clone()));
        let result = doc.layers[0].raster().unwrap();
        assert_eq!(result.dimensions(), image.dimensions());
        let mut changed = 0;
        for (x, y, pixel) in result.enumerate_pixels() {
            let point = placement.point([(f64::from(x) + 0.5) / 33., (f64::from(y) + 0.5) / 29.]);
            if selection.coverage(point) == 0. {
                assert_eq!(*pixel, image[(x, y)]);
            } else {
                if selection.coverage(point) == 1. {
                    assert_eq!(
                        *pixel,
                        expected[(x, y)],
                        "Layer-local center changed after transform"
                    );
                }
                if *pixel != image[(x, y)] {
                    changed += 1;
                }
            }
        }
        assert!(changed > 0);
        let before = doc.clone();
        assert!(filters::apply(&mut doc, Filter::Radial(Radial::default()), true).is_err());
        assert_eq!(doc, before);
    }
}
