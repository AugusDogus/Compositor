use super::*;
use crate::{
    document::Mask,
    path_shape::{self, Stroke, Style},
    vector_path::{Anchor, BezierPath, Closure},
};
use image::{GrayImage, Luma};
use std::sync::Arc;

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn editable_path_shapes_match_gpu_with_rotation_flips_masks_and_clipping() {
    let mut doc = Document::new(40, 30).unwrap();
    let geometry = BezierPath {
        anchors: vec![
            Anchor {
                outgoing: Some([0., 26.]),
                ..Anchor::corner([4., 4.])
            },
            Anchor {
                incoming: Some([35., -5.]),
                ..Anchor::corner([34., 24.])
            },
        ],
        closure: Closure::Closed,
    };
    let id = path_shape::create(
        &mut doc,
        "Curve",
        geometry,
        Style {
            fill: Some([220, 50, 120, 190]),
            stroke: Some(Stroke {
                width: 3.5,
                color: [10, 180, 50, 210],
            }),
        },
    )
    .unwrap();
    let layer = doc.active_layer_mut().unwrap();
    layer.transform.rotation = 21.;
    layer.transform.flip_x = true;
    layer.transform.sampling = Sampling::Smooth;
    layer.opacity = 0.6;
    layer.mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_fn(8, 8, |x, y| {
            Luma([((x + y) * 15) as u8])
        })),
        enabled: true,
        linked: true,
        placement: None,
    });
    let mut above = Layer::blank("Clipped color", 40, 30);
    above.clip_source = Some(id);
    above.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        40,
        30,
        Rgba([30, 40, 240, 128]),
    ))));
    doc.add(above).unwrap();
    let mut engine = gpu::Engine::new().unwrap();
    for (size, origin, step) in [
        ([40, 30], [0., 0.], [1., 1.]),
        ([34, 27], [-2.5, 0.3], [1.3, 0.85]),
    ] {
        let expected = region(&doc, size[0], size[1], origin, step).unwrap();
        let program = gpu::scene::Scene::compile(&doc, origin, step).unwrap();
        let actual = engine.render(&program, size, origin, step).unwrap();
        for (index, (a, b)) in actual.as_raw().iter().zip(expected.as_raw()).enumerate() {
            assert!(a.abs_diff(*b) <= 2, "byte {index}: GPU={a}, CPU={b}");
        }
    }
}

fn thin_shape() -> (Document, uuid::Uuid) {
    let mut doc = Document::new(100, 100).unwrap();
    let id = path_shape::create(
        &mut doc,
        "Thin stroke",
        BezierPath {
            anchors: vec![
                Anchor::corner([10., 21.]),
                Anchor::corner([90., 21.]),
                Anchor::corner([90., 90.]),
            ],
            closure: Closure::Open,
        },
        Style {
            fill: None,
            stroke: Some(Stroke {
                width: 1.,
                color: [255, 0, 0, 255],
            }),
        },
    )
    .unwrap();
    (doc, id)
}
#[test]
fn minified_path_strokes_keep_coverage_and_editable_source() {
    let (doc, id) = thin_shape();
    let actual = region(&doc, 10, 10, [0., 0.], [10., 10.]).unwrap();
    let mut raster = doc.clone();
    path_shape::rasterize(&mut raster, id).unwrap();
    let expected = region(&raster, 10, 10, [0., 0.], [10., 10.]).unwrap();
    assert_eq!(actual, expected);
    assert!(actual.pixels().any(|p| p[3] > 0));
    assert!(doc.layer(id).unwrap().is_path_shape());
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn accelerated_minified_path_strokes_keep_coverage() {
    let (doc, id) = thin_shape();
    let mut raster = doc.clone();
    path_shape::rasterize(&mut raster, id).unwrap();
    let mut cache = DownsampleCache::default();
    let actual = region_accelerated(&doc, 10, 10, [0., 0.], [10., 10.], &mut cache).unwrap();
    let expected = region_accelerated(&raster, 10, 10, [0., 0.], [10., 10.], &mut cache).unwrap();
    assert_eq!(actual, expected);
    assert!(actual.pixels().any(|p| p[3] > 0));
    assert!(doc.layer(id).unwrap().is_path_shape());
}
