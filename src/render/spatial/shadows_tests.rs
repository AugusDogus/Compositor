use super::*;
use crate::{adjustment::Adjustment, document::Mask, shadows_highlights::Settings};
use image::{GrayImage, Luma};

fn document(settings: Settings) -> Document {
    let mut doc = Document::new(32, 24).unwrap();
    doc.layers[0].content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(32, 24, |x, _| {
            let value = if (12..20).contains(&x) { 35 } else { 205 };
            Rgba([value, value / 2, 80, if x < 28 { 173 } else { 0 }])
        }))));
    let mut layer = Layer::blank("Shadows/Highlights", 32, 24);
    layer.content =
        LayerContent::ExtendedAdjustment(Box::new(ExtendedAdjustment::ShadowsHighlights(settings)));
    doc.add(layer).unwrap();
    doc
}

fn rendered(doc: &Document) -> RgbaImage {
    region(doc, doc.width, doc.height, [0., 0.], [1., 1.]).unwrap()
}

#[test]
fn shadows_adjustment_preserves_source_alpha_and_respects_mask_and_opacity() {
    let mut doc = document(Settings::new(90., 15., 8.).unwrap());
    let original = doc.layers[0].raster().unwrap().clone();
    let mut baseline = doc.clone();
    baseline.layers.pop();
    let baseline = rendered(&baseline);
    let full = rendered(&doc);
    assert!(full[(16, 5)][0] > baseline[(16, 5)][0]);
    doc.layers[1].opacity = 0.5;
    doc.layers[1].mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_fn(32, 24, |_, y| {
            Luma([if y < 12 { 255 } else { 0 }])
        })),
        enabled: true,
        linked: true,
        placement: None,
    });
    let half = rendered(&doc);
    for y in 0..24 {
        for x in 0..32 {
            assert_eq!(half[(x, y)][3], baseline[(x, y)][3]);
            if y >= 12 {
                assert_eq!(half[(x, y)], baseline[(x, y)]);
            } else if half[(x, y)][3] > 0 {
                for channel in 0..3 {
                    let average = (u16::from(full[(x, y)][channel])
                        + u16::from(baseline[(x, y)][channel]))
                        / 2;
                    assert!(i32::from(half[(x, y)][channel]).abs_diff(i32::from(average)) <= 1);
                }
            }
        }
    }
    doc.layers[1].opacity = 0.;
    assert_eq!(rendered(&doc), baseline);
    assert!(Arc::ptr_eq(doc.layers[0].raster().unwrap(), &original));
}

#[test]
fn shadows_adjustment_radius_changes_neighbor_response() {
    let narrow = document(Settings::new(100., 0., 1.).unwrap());
    let mut broad = narrow.clone();
    broad.layers[1].content = LayerContent::ExtendedAdjustment(Box::new(
        ExtendedAdjustment::ShadowsHighlights(Settings::new(100., 0., 20.).unwrap()),
    ));
    assert!(rendered(&narrow)[(16, 12)][0] > rendered(&broad)[(16, 12)][0] + 10);
}

fn chained(clipped: bool) -> Document {
    let mut doc = document(Settings::new(80., 25., 9.).unwrap());
    let mut blur = Layer::blank("Gaussian", 32, 24);
    let mut adjustment = Adjustment::new(Kind::GaussianBlur);
    adjustment.blur_radius = Some(2.);
    blur.content = LayerContent::Adjustment(Box::new(adjustment));
    if clipped {
        blur.clip_source = Some(doc.layers[0].id);
        doc.layers[1].clip_source = Some(doc.layers[0].id);
    }
    doc.layers.insert(1, blur);
    doc.validate().unwrap();
    doc
}

#[test]
fn shadows_after_blur_matches_partial_regions_and_clipped_stacks() {
    for clipped in [false, true] {
        let doc = chained(clipped);
        let full = rendered(&doc);
        let part = region(&doc, 9, 7, [11., 8.], [1., 1.]).unwrap();
        for y in 0..7 {
            for x in 0..9 {
                assert_eq!(part[(x, y)], full[(x + 11, y + 8)], "clip={clipped}");
            }
        }
        let mut without_shadows = doc.clone();
        without_shadows.layers.pop();
        assert_ne!(full[(16, 12)], rendered(&without_shadows)[(16, 12)]);
    }
}

#[test]
fn grouped_shadows_adjustment_keeps_group_mask_coverage() {
    let mut doc = document(Settings::default());
    let mut group = Layer::blank("Folder", 32, 24);
    group.content = LayerContent::Group;
    group.mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_fn(32, 24, |x, _| {
            Luma([if x < 20 { 255 } else { 0 }])
        })),
        enabled: true,
        linked: true,
        placement: None,
    });
    for layer in &mut doc.layers {
        layer.parent = Some(group.id);
    }
    doc.layers.insert(0, group);
    doc.validate().unwrap();
    let full = rendered(&doc);
    assert!(full[(16, 12)][0] > 35);
    assert_eq!(full[(25, 12)][3], 0);
}

#[test]
fn shadows_adjustment_on_artboard_does_not_change_neighbor_board() {
    let mut doc = document(Settings::default());
    doc.width = 64;
    let mut board = Layer::blank("Edited board", 32, 24);
    board.content = LayerContent::Artboard(crate::artboard::Artboard { background: [0; 4] });
    for layer in &mut doc.layers {
        layer.parent = Some(board.id);
    }
    doc.layers.insert(0, board);
    let mut neighbor = Layer::blank("Neighbor", 32, 24);
    neighbor.content = LayerContent::Artboard(crate::artboard::Artboard { background: [0; 4] });
    neighbor.transform.origin[0] = 32.;
    let mut image = Layer::blank("Neighbor pixels", 32, 24);
    image.transform.origin[0] = 32.;
    image.parent = Some(neighbor.id);
    image.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        32,
        24,
        Rgba([100, 80, 60, 255]),
    ))));
    doc.layers.extend([neighbor, image]);
    doc.validate().unwrap();
    let full = rendered(&doc);
    assert!(full[(16, 12)][0] > 35);
    assert_eq!(full[(40, 12)], Rgba([100, 80, 60, 255]));
    // Changing a neighboring board must not change the edited board's local brightness.
    doc.layers.last_mut().unwrap().content = LayerContent::Raster(Some(Arc::new(
        RgbaImage::from_pixel(32, 24, Rgba([255; 4])),
    )));
    assert_eq!(rendered(&doc)[(16, 12)], full[(16, 12)]);
}

#[test]
fn identity_shadows_has_no_spatial_surface_or_reduced_zoom_quantization() {
    let doc = document(Settings::new(0., 0., 500.).unwrap());
    assert!(
        prepare(&doc, [8, 6], [0., 0.], [4., 4.], false)
            .unwrap()
            .is_empty()
    );
    let mut baseline = doc.clone();
    baseline.layers.pop();
    assert_eq!(
        region(&doc, 8, 6, [0., 0.], [4., 4.]).unwrap(),
        region(&baseline, 8, 6, [0., 0.], [4., 4.]).unwrap(),
    );
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn shadows_gpu_surfaces_match_reference_for_blur_chains_clips_and_reduced_zoom() {
    let mut engine = super::super::gpu::Engine::new().unwrap();
    for clipped in [false, true] {
        for spacing in [1., 4.] {
            let doc = chained(clipped);
            let size = [(32. / spacing) as u32, (24. / spacing) as u32];
            let cpu = prepare(&doc, size, [0., 0.], [spacing; 2], false).unwrap();
            let gpu = prepare(&doc, size, [0., 0.], [spacing; 2], true).unwrap();
            let scene = super::super::gpu::scene::Scene::compile_surfaces(
                &doc,
                [0., 0.],
                [spacing; 2],
                &gpu,
                None,
            )
            .unwrap();
            let actual = engine.render(&scene, size, [0., 0.], [spacing; 2]).unwrap();
            let mut sampler = Sampler::from_document(Cow::Borrowed(&doc));
            sampler.state.surfaces = cpu;
            for y in 0..size[1] {
                for x in 0..size[0] {
                    let expected = sampler
                        .sample([
                            (f64::from(x) + 0.5) * spacing,
                            (f64::from(y) + 0.5) * spacing,
                        ])
                        .map(|v| (v * 255.).round() as u8);
                    assert!(actual[(x, y)][3].abs_diff(expected[3]) <= 1);
                    if expected[3] == 0 && actual[(x, y)][3] == 0 {
                        continue;
                    }
                    for (a, b) in actual[(x, y)].0.into_iter().zip(expected) {
                        assert!(
                            a.abs_diff(b) <= 4,
                            "clip={clipped}, spacing={spacing}, ({x},{y}): {a} != {b}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn identity_shadows_gpu_matches_unchanged_stack_at_reduced_zoom() {
    super::super::gpu::initialize().unwrap();
    let doc = document(Settings::new(0., 0., 500.).unwrap());
    let mut baseline = doc.clone();
    baseline.layers.pop();
    let render = |doc: &Document| {
        region_accelerated(
            doc,
            8,
            6,
            [0., 0.],
            [4., 4.],
            &mut DownsampleCache::default(),
        )
        .unwrap()
    };
    assert_eq!(render(&doc), render(&baseline));
}
