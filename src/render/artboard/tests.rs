use super::*;
use crate::{
    adjustment::{Adjustment, Kind},
    artboard::Artboard,
    document::Mask,
    geometry::Transform,
};
use image::{GrayImage, Luma};
use std::sync::Arc;
fn board(name: &str, x: f64, color: [u8; 4]) -> Layer {
    let mut layer = Layer::blank(name, 4, 4);
    layer.transform.origin = [x, 0.];
    layer.content = LayerContent::Artboard(Artboard { background: color });
    layer
}
fn pixels(parent: Uuid, color: [u8; 4]) -> Layer {
    let mut layer = Layer::blank("Pixels", 8, 4);
    layer.parent = Some(parent);
    layer.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(8, 4, Rgba(color)))));
    layer
}
fn scene() -> Document {
    let mut doc = Document::new(8, 4).unwrap();
    let a = board("A", 0., [0; 4]);
    let b = board("B", 4., [0, 0, 255, 255]);
    let top = pixels(a.id, [255, 0, 0, 255]);
    doc.active = Some(a.id);
    doc.selected = HashSet::from([a.id]);
    doc.layers = vec![a, top, b];
    doc.validate().unwrap();
    doc
}
#[test]
fn boards_clip_children_and_apply_opacity_once_after_local_compositing() {
    let mut doc = scene();
    doc.layers[0].opacity = 0.5;
    let child = pixels(doc.layers[0].id, [0, 255, 0, 255]);
    doc.layers.insert(2, child);
    let image = render(&doc, 8, 4).unwrap();
    assert_eq!(image[(1, 1)], Rgba([0, 255, 0, 128]));
    assert_eq!(image[(6, 1)], Rgba([0, 0, 255, 255]));
    doc.layers[0].mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(4, 4, Luma([128]))),
        enabled: true,
        linked: true,
        placement: None,
    });
    assert_eq!(render(&doc, 8, 4).unwrap()[(1, 1)], Rgba([0, 255, 0, 64]));
}
#[test]
fn board_adjustments_use_own_background_and_not_the_root_backdrop() {
    let mut doc = scene();
    doc.layers[0].content = LayerContent::Artboard(Artboard {
        background: [255, 0, 0, 255],
    });
    doc.layers.remove(1);
    let mut invert = Layer::blank("Invert", 8, 4);
    invert.parent = Some(doc.layers[0].id);
    invert.content = LayerContent::Adjustment(Box::new(Adjustment::new(Kind::Invert)));
    doc.layers.insert(1, invert);
    let mut background = pixels(doc.layers[0].id, [255, 255, 0, 255]);
    background.parent = None;
    doc.layers.insert(0, background);
    let image = render(&doc, 8, 4).unwrap();
    assert_eq!(image[(1, 1)], Rgba([0, 255, 255, 255]));
    assert_eq!(image[(6, 1)], Rgba([0, 0, 255, 255]));
}
fn blurred_scene() -> Document {
    let mut doc = scene();
    let id = doc.layers[0].id;
    for kind in [Kind::GaussianBlur, Kind::MotionBlur] {
        let mut settings = Adjustment::new(kind);
        settings.blur_radius = Some(2.);
        settings.motion_distance = Some(3.);
        settings.motion_angle = Some(0.);
        let mut layer = Layer::blank("Blur", 8, 4);
        layer.parent = Some(id);
        layer.content = LayerContent::Adjustment(Box::new(settings));
        doc.layers.insert(doc.layers.len() - 1, layer);
    }
    doc
}
#[test]
fn adjacent_boards_cannot_contaminate_spatial_backdrops() {
    let mut doc = blurred_scene();
    let expected = render(&doc, 8, 4).unwrap();
    let last = doc.layers.len() - 1;
    doc.layers[last].content = LayerContent::Artboard(Artboard {
        background: [255, 255, 0, 255],
    });
    let neighbor = doc.layers.remove(last);
    doc.layers.insert(0, neighbor);
    let changed = render(&doc, 8, 4).unwrap();
    for y in 0..4 {
        for x in 0..4 {
            assert_eq!(expected[(x, y)], changed[(x, y)]);
        }
    }
    let partial = region(&doc, 2, 2, [2., 1.], [1., 1.]).unwrap();
    for y in 0..2 {
        for x in 0..2 {
            assert_eq!(partial[(x, y)], changed[(x + 2, y + 1)]);
        }
    }
    let board = doc.layers.iter_mut().find(|l| l.name == "A").unwrap();
    board.opacity = 0.5;
    let attenuated = render(&doc, 8, 4).unwrap();
    assert_eq!(&attenuated[(2, 2)].0[..3], &changed[(2, 2)].0[..3]);
    assert_eq!(attenuated[(2, 2)][3], 128);
}
#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn artboards_gpu_matches_cpu_with_clipping_masks_and_spatial_early_stops() {
    let mut engine = gpu::Engine::new().unwrap();
    for mut doc in [scene(), blurred_scene()] {
        doc.layers[0].opacity = 0.5;
        doc.layers[0].mask = Some(Mask {
            pixels: Arc::new(GrayImage::from_pixel(4, 4, Luma([128]))),
            enabled: true,
            linked: true,
            placement: Some(Transform::new(4, 4)),
        });
        let surfaces = spatial::prepare(&doc, [8, 4], [0.; 2], [1.; 2], true).unwrap();
        let program =
            gpu::scene::Scene::compile_surfaces(&doc, [0.; 2], [1.; 2], &surfaces, None).unwrap();
        let actual = engine.render(&program, [8, 4], [0.; 2], [1.; 2]).unwrap();
        let expected = render(&doc, 8, 4).unwrap();
        for (a, b) in actual.as_raw().iter().zip(expected.as_raw()) {
            assert!(
                a.abs_diff(*b) <= 1,
                "actual={actual:?} expected={expected:?}"
            );
        }
    }
}

fn nested_clipped_scene() -> Document {
    let mut doc = Document::new(12, 10).unwrap();
    let mut board = board("Foreground board", 2., [20, 40, 80, 180]);
    board.transform.origin[1] = 1.;
    board.transform.size = [7., 7.];
    board.opacity = 0.7;
    board.mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_fn(7, 7, |x, _| Luma([100 + x as u8 * 20]))),
        enabled: true,
        linked: true,
        placement: None,
    });
    let mut outer = Layer::blank("Outer folder", 12, 10);
    outer.content = LayerContent::Group;
    outer.parent = Some(board.id);
    outer.opacity = 0.8;
    let mut inner = Layer::blank("Inner folder", 12, 10);
    inner.content = LayerContent::Group;
    inner.parent = Some(outer.id);
    inner.opacity = 0.9;
    let mut base = Layer::blank("Clipping base", 12, 10);
    base.parent = Some(inner.id);
    base.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(12, 10, |x, y| {
        Rgba([
            x as u8 * 20,
            y as u8 * 22,
            110,
            if (x + y) % 3 == 0 { 100 } else { 220 },
        ])
    }))));
    let mut clipped = Layer::blank("Clipped pixels", 12, 10);
    clipped.parent = Some(inner.id);
    clipped.clip_source = Some(base.id);
    clipped.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(12, 10, |x, y| {
        Rgba([200, 100, 40, if x > y { 210 } else { 0 }])
    }))));
    let mut blur = Layer::blank("Clipped Gaussian Blur", 12, 10);
    blur.parent = Some(inner.id);
    blur.clip_source = Some(base.id);
    let mut gaussian = Adjustment::new(Kind::GaussianBlur);
    gaussian.blur_radius = Some(1.5);
    blur.content = LayerContent::Adjustment(Box::new(gaussian));
    let mut motion = Layer::blank("Outer Motion Blur", 12, 10);
    motion.parent = Some(outer.id);
    let mut settings = Adjustment::new(Kind::MotionBlur);
    settings.motion_distance = Some(3.);
    settings.motion_angle = Some(30.);
    motion.content = LayerContent::Adjustment(Box::new(settings));
    let mut behind = Layer::blank("Root pixels", 12, 10);
    behind.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        12,
        10,
        Rgba([210, 30, 70, 255]),
    ))));
    doc.active = Some(board.id);
    doc.selected = HashSet::from([board.id]);
    doc.layers = vec![behind, board, outer, inner, base, clipped, blur, motion];
    // Keep input sampling fixed so region checks isolate spatial backdrops,
    // rather than comparing preview-specific high-quality downsampling.
    for layer in &mut doc.layers {
        layer.transform.sampling = Sampling::Smooth;
    }
    doc.validate().unwrap();
    doc
}

#[test]
fn nested_clipped_blurs_sample_nonuniform_regions_without_root_backdrop_leaks() {
    let mut doc = nested_clipped_scene();
    let full = render(&doc, 12, 10).unwrap();
    let partial = region(&doc, 5, 2, [1., 1.], [1., 3.]).unwrap();
    for y in 0..2 {
        for x in 0..5 {
            assert_eq!(partial[(x, y)], full[(x + 1, y * 3 + 2)]);
        }
    }
    // Remove the unrelated root backing. A board's locally rendered RGB/alpha
    // must then composite over that backing exactly once.
    doc.layers.remove(0);
    let isolated = render(&doc, 12, 10).unwrap();
    assert_eq!(isolated[(1, 4)][3], 0);
    assert!(isolated[(4, 4)][3] > 0 && isolated[(4, 4)][3] < 255);
    for (local, actual) in isolated.pixels().zip(full.pixels()) {
        let alpha = f64::from(local[3]) / 255.;
        for channel in 0..3 {
            let expected = (f64::from(local[channel]) * alpha
                + f64::from([210, 30, 70][channel]) * (1. - alpha))
                .round() as u8;
            assert!(actual[channel].abs_diff(expected) <= 1);
        }
        assert_eq!(actual[3], 255);
    }
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn nested_clipped_artboard_blurs_match_gpu_at_nonuniform_and_fractional_sampling() {
    let doc = nested_clipped_scene();
    let mut engine = gpu::Engine::new().unwrap();
    for (size, origin, step) in [
        ([12, 10], [0., 0.], [1., 1.]),
        ([5, 2], [1., 1.], [1., 3.]),
        ([6, 7], [1.25, 0.5], [1.25, 0.75]),
        ([5, 3], [0.25, 0.5], [2., 3.]),
    ] {
        let expected = region(&doc, size[0], size[1], origin, step).unwrap();
        let surfaces = spatial::prepare(&doc, size, origin, step, true).unwrap();
        let program =
            gpu::scene::Scene::compile_surfaces(&doc, origin, step, &surfaces, None).unwrap();
        let actual = engine.render(&program, size, origin, step).unwrap();
        for (index, (a, b)) in actual.as_raw().iter().zip(expected.as_raw()).enumerate() {
            assert!(
                a.abs_diff(*b) <= 2,
                "size={size:?}, origin={origin:?}, step={step:?}, byte={index}, GPU={a}, CPU={b}"
            );
        }
    }
}
