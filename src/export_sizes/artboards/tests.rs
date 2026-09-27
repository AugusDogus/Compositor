use super::*;
use crate::{adjustment::Adjustment, export_sizes::Format, geometry::Sampling};

fn striped() -> Document {
    let mut document = Document::new(6, 2).unwrap();
    document.layers[0].name = "Stripes".into();
    document.layers[0].transform.sampling = Sampling::Nearest;
    document.layers[0].content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(6, 2, |x, _| {
            Rgba(if x < 2 {
                [255, 0, 0, 255]
            } else if x < 4 {
                [0, 255, 0, 255]
            } else {
                [0, 0, 255, 255]
            })
        }))));
    document
}
fn board_pixels(document: &Document, board: Uuid) -> RgbaImage {
    let frame = document.layer(board).unwrap().transform;
    let mut isolated = document.clone();
    let ids = document.descendants(board);
    isolated.layers.retain(|layer| ids.contains(&layer.id));
    isolated.active = Some(board);
    isolated.selected = HashSet::from([board]);
    crate::render::region(
        &isolated,
        frame.size[0] as u32,
        frame.size[1] as u32,
        frame.origin,
        [1.; 2],
    )
    .unwrap()
}
fn batch(sizes: Vec<[u32; 2]>, fit: Fit) -> Batch {
    Batch::new(sizes, fit, Format::Png).unwrap()
}

#[test]
fn fit_and_fill_keep_source_layers_and_frame_padding_without_revealing_off_canvas_pixels() {
    for (size, fit) in [([6, 6], Fit::Contain), ([2, 2], Fit::Cover)] {
        let mut doc = striped();
        let original = doc.clone();
        let boards = batch(vec![size], fit)
            .add_artboards(&mut doc, Source::Canvas)
            .unwrap();
        assert_eq!(
            doc.layer(original.layers[0].id).unwrap(),
            &original.layers[0]
        );
        let pixels = board_pixels(&doc, boards[0]);
        if fit == Fit::Contain {
            assert_eq!(pixels[(0, 0)], Rgba([0; 4]));
            assert_eq!(pixels[(0, 2)], Rgba([255, 0, 0, 255]));
            assert_eq!(pixels[(5, 3)], Rgba([0, 0, 255, 255]));
        } else {
            assert!(
                pixels
                    .pixels()
                    .all(|pixel| *pixel == Rgba([0, 255, 0, 255]))
            );
        }
    }
    let mut doc = striped();
    doc.width = 2;
    let board = batch(vec![[6, 2]], Fit::Contain)
        .add_artboards(&mut doc, Source::Canvas)
        .unwrap()[0];
    let pixels = board_pixels(&doc, board);
    assert_eq!(pixels[(0, 0)], Rgba([0; 4]));
    assert_eq!(pixels[(2, 0)], Rgba([255, 0, 0, 255]));
    assert_eq!(pixels[(4, 0)], Rgba([0; 4]));
}

#[test]
fn selected_board_background_mask_and_opacity_apply_once_and_ignore_other_roots() {
    let mut doc = Document::new(12, 10).unwrap();
    crate::edits::fill(&mut doc, [10, 180, 90, 255], false, false).unwrap();
    let source = crate::artboard::create(
        &mut doc,
        "Source",
        Transform {
            origin: [2., 3.],
            ..Transform::new(4, 2)
        },
        [40, 80, 120, 128],
    )
    .unwrap();
    let owner = doc.active_layer_mut().unwrap();
    owner.opacity = 0.5;
    owner.mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(1, 1, Luma([128]))),
        enabled: true,
        linked: false,
        placement: None,
    });
    let mut invert = Layer::blank("Root invert", 12, 10);
    invert.content = LayerContent::Adjustment(Box::new(Adjustment::new(Kind::Invert)));
    doc.add(invert).unwrap();
    let old_layers = doc.layers.clone();
    let original_pixels = board_pixels(&doc, source);
    let output = batch(vec![[4, 4]], Fit::Contain)
        .add_artboards(&mut doc, Source::Artboard(source))
        .unwrap()[0];
    let pixels = board_pixels(&doc, output);
    assert_eq!(pixels[(0, 0)], Rgba([0; 4]));
    assert_eq!(pixels[(1, 1)], original_pixels[(1, 0)]);
    assert_eq!(pixels[(1, 1)], Rgba([40, 80, 120, 32]));
    for original in old_layers {
        assert_eq!(doc.layer(original.id).unwrap(), &original);
    }
}

#[test]
fn copies_retain_editable_metadata_assets_hierarchy_and_remapped_clipping() {
    use crate::{
        document::{Shape, ShapeGeometry},
        effects::{LayerEffects, StrokeEffect},
        raw::{DevelopSettings, RawAsset, RawMetadata},
    };
    let mut doc = Document::new(40, 30).unwrap();
    let mut group = Layer::blank("Folder", 40, 30);
    group.content = LayerContent::Group;
    let parent = group.id;
    doc.add(group).unwrap();
    let mut text = Layer::blank("Text", 8, 8);
    text.parent = Some(parent);
    text.text = Some(crate::text::Text::default());
    text.transform.origin = [5., 7.];
    text.transform.rotation = 30.;
    text.content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(8, 8, Rgba([255; 4])))));
    text.mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(2, 2, Luma([128]))),
        enabled: true,
        linked: false,
        placement: Some(Transform {
            origin: [6., 8.],
            ..Transform::new(8, 8)
        }),
    });
    text.effects = Some(LayerEffects {
        stroke: Some(StrokeEffect::default()),
        ..Default::default()
    });
    let text_id = text.id;
    doc.add(text).unwrap();
    let mut shape = Layer::blank("Shape", 8, 8);
    shape.parent = Some(parent);
    shape.clip_source = Some(text_id);
    shape.visible = false;
    shape.shape = Some(Shape {
        geometry: ShapeGeometry::Rectangle,
        red: 1.,
        green: 0.,
        blue: 0.,
        corner_radius: 3.,
    });
    shape.content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(8, 8, Rgba([255; 4])))));
    doc.add(shape).unwrap();
    let mut raw = Layer::blank("RAW", 8, 8);
    raw.content = LayerContent::Raster(Some(Arc::new(RgbaImage::new(8, 8))));
    raw.raw = Some(Arc::new(RawAsset {
        filename: "camera.dng".into(),
        metadata: RawMetadata {
            width: 8,
            height: 8,
            ..Default::default()
        },
        settings: DevelopSettings::default(),
        bytes: Arc::new(vec![1, 2, 3]),
    }));
    doc.add(raw).unwrap();
    let original = doc.clone();
    let boards = batch(vec![[80, 60], [20, 15]], Fit::Contain)
        .add_artboards(&mut doc, Source::Canvas)
        .unwrap();
    for (&board, scale) in boards.iter().zip([2., 0.5]) {
        let ids = doc.descendants(board);
        let copied = |name: &str| {
            doc.layers
                .iter()
                .find(|layer| ids.contains(&layer.id) && layer.name == name)
                .unwrap()
        };
        let text = copied("Text");
        let shape = copied("Shape");
        let raw = copied("RAW");
        assert_eq!(text.parent, Some(copied("Folder").id));
        assert_eq!(shape.clip_source, Some(text.id));
        assert_eq!(
            shape.shape,
            original
                .layers
                .iter()
                .find(|l| l.name == "Shape")
                .unwrap()
                .shape
        );
        assert!(!shape.visible);
        let source = original.layer(text_id).unwrap();
        assert_eq!(text.text, source.text);
        assert_eq!(text.effects, source.effects);
        assert!(Arc::ptr_eq(
            text.raster().unwrap(),
            source.raster().unwrap()
        ));
        assert_eq!(text.transform.size, [8. * scale; 2]);
        assert_eq!(text.transform.rotation, 30.);
        assert_eq!(
            text.mask.as_ref().unwrap().placement.unwrap().size,
            [8. * scale; 2]
        );
        assert!(Arc::ptr_eq(
            raw.raw.as_ref().unwrap(),
            original
                .layers
                .iter()
                .find(|l| l.name == "RAW")
                .unwrap()
                .raw
                .as_ref()
                .unwrap()
        ));
    }
    for layer in &original.layers {
        assert_eq!(doc.layer(layer.id).unwrap(), layer);
    }
    doc.validate().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Variants.comp");
    crate::project::save_recovery(&doc, &path).unwrap();
    let reopened = crate::project::load(&path).unwrap();
    assert_eq!(reopened.layers, doc.layers);
}

#[test]
fn root_and_spatial_adjustments_copy_into_each_board_without_double_application() {
    let mut doc = striped();
    let mut invert = Layer::blank("Invert", 6, 2);
    invert.content = LayerContent::Adjustment(Box::new(Adjustment::new(Kind::Invert)));
    doc.add(invert).unwrap();
    let expected = crate::render::render(&doc, 6, 2).unwrap();
    let output = batch(vec![[6, 2]], Fit::Contain)
        .add_artboards(&mut doc, Source::Canvas)
        .unwrap()[0];
    assert_eq!(board_pixels(&doc, output), expected);
    let frame = doc.layer(output).unwrap().transform;
    assert_eq!(
        crate::render::region(&doc, 6, 2, frame.origin, [1.; 2]).unwrap(),
        expected
    );
    let mut doc = Document::new(20, 20).unwrap();
    for (kind, name) in [
        (Kind::GaussianBlur, "Gaussian"),
        (Kind::MotionBlur, "Motion"),
    ] {
        let mut settings = Adjustment::new(kind);
        settings.blur_radius = Some(4.);
        settings.motion_distance = Some(6.);
        let mut layer = Layer::blank(name, 20, 20);
        layer.content = LayerContent::Adjustment(Box::new(settings));
        doc.add(layer).unwrap();
    }
    let board = batch(vec![[40, 40]], Fit::Cover)
        .add_artboards(&mut doc, Source::Canvas)
        .unwrap()[0];
    let ids = doc.descendants(board);
    for layer in doc.layers.iter().filter(|layer| ids.contains(&layer.id)) {
        if let LayerContent::Adjustment(settings) = &layer.content {
            if settings.kind == Kind::GaussianBlur {
                assert_eq!(settings.blur_radius, Some(8.));
            }
            if settings.kind == Kind::MotionBlur {
                assert_eq!(settings.motion_distance, Some(12.));
            }
        }
    }
}

#[test]
fn invalid_source_noise_transforms_and_late_adjustment_limits_roll_back_the_whole_batch() {
    let mut with_board = striped();
    crate::artboard::create(&mut with_board, "Existing", Transform::new(4, 2), [0; 4]).unwrap();
    let before = with_board.clone();
    let error = batch(vec![[6, 2]], Fit::Contain)
        .add_artboards(&mut with_board, Source::Canvas)
        .unwrap_err();
    assert!(error.to_string().contains("Select one artboard"));
    assert_eq!(with_board, before);
    assert!(
        batch(vec![[6, 2]], Fit::Contain)
            .add_artboards(&mut with_board, Source::Artboard(Uuid::new_v4()))
            .is_err()
    );
    assert_eq!(with_board, before);
    for kind in [Kind::Grain, Kind::AddNoise, Kind::GaussianBlur] {
        let mut doc = Document::new(20, 20).unwrap();
        let mut settings = Adjustment::new(kind);
        settings.blur_radius = Some(200.);
        let mut layer = Layer::blank("Adjustment", 20, 20);
        layer.content = LayerContent::Adjustment(Box::new(settings));
        doc.add(layer).unwrap();
        let original = doc.clone();
        assert!(
            batch(vec![[20, 20], [40, 40]], Fit::Contain)
                .add_artboards(&mut doc, Source::Canvas)
                .is_err()
        );
        assert_eq!(doc, original);
    }
    let mut tiny = Document::new(20, 20).unwrap();
    let mut dot = Layer::blank("Tiny", 1, 1);
    dot.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(1, 1, Rgba([255; 4])))));
    tiny.add(dot).unwrap();
    let original = tiny.clone();
    assert!(
        batch(vec![[20, 20], [10, 10]], Fit::Contain)
            .add_artboards(&mut tiny, Source::Canvas)
            .is_err()
    );
    assert_eq!(tiny, original);
}

#[test]
fn layout_wraps_without_moving_sources_and_canvas_overflow_is_atomic() {
    let mut doc = Document::new(29980, 10).unwrap();
    let old = doc.layers.clone();
    let output = batch(vec![[100, 10], [200, 10]], Fit::Cover)
        .add_artboards(&mut doc, Source::Canvas)
        .unwrap();
    assert_eq!(doc.layer(output[0]).unwrap().transform.origin, [0., 42.]);
    assert_eq!(doc.layer(output[1]).unwrap().transform.origin, [132., 42.]);
    assert_eq!(&doc.layers[..old.len()], old.as_slice());
    let mut full = Document::new(30_000, 30_000).unwrap();
    let original = full.clone();
    assert!(
        batch(vec![[10, 10]], Fit::Contain)
            .add_artboards(&mut full, Source::Canvas)
            .is_err()
    );
    assert_eq!(full, original);
}

#[test]
fn layer_pixel_and_native_metadata_budgets_fail_without_partial_artboards() {
    let mut crowded = Document::new(2, 2).unwrap();
    for _ in 0..4999 {
        crowded.add(Layer::blank("Layer", 2, 2)).unwrap();
    }
    let original = crowded.clone();
    assert!(
        batch(vec![[2, 2], [4, 4]], Fit::Contain)
            .add_artboards(&mut crowded, Source::Canvas)
            .unwrap_err()
            .to_string()
            .contains("10,000")
    );
    assert_eq!(crowded, original);
    let pixels = Arc::new(RgbaImage::new(1000, 1000));
    let mut heavy = Document::new(1000, 1000).unwrap();
    heavy.layers[0].content = LayerContent::Raster(Some(pixels.clone()));
    let count = crate::document::document_pixel_budget() / 1_000_000;
    for _ in 1..count {
        let mut layer = Layer::blank("Pixels", 1000, 1000);
        layer.content = LayerContent::Raster(Some(pixels.clone()));
        heavy.add(layer).unwrap();
    }
    heavy.validate().unwrap();
    let original = heavy.clone();
    assert!(
        batch(vec![[1000, 1000]], Fit::Contain)
            .add_artboards(&mut heavy, Source::Canvas)
            .unwrap_err()
            .to_string()
            .contains("pixel budget")
    );
    assert_eq!(heavy, original);
    let mut metadata = Document::new(2, 2).unwrap();
    for _ in 0..140 {
        metadata
            .add(Layer::blank("x".repeat(16_000), 2, 2))
            .unwrap();
    }
    crate::project::validate_storage_metadata(&metadata).unwrap();
    let original = metadata.clone();
    assert!(
        batch(vec![[2, 2]], Fit::Contain)
            .add_artboards(&mut metadata, Source::Canvas)
            .unwrap_err()
            .to_string()
            .contains("4 MiB")
    );
    assert_eq!(metadata, original);
}

#[test]
fn variant_batch_is_one_undo_step() {
    let mut session = crate::session::Session::new(striped(), None);
    let original = session.document.clone();
    session
        .edit("Add size variants", |document| {
            batch(vec![[6, 6], [12, 4]], Fit::Contain)
                .add_artboards(document, Source::Canvas)
                .map(|_| ())
        })
        .unwrap();
    assert_eq!(
        session
            .document
            .layers
            .iter()
            .filter(|layer| layer.is_artboard())
            .count(),
        2
    );
    session.undo();
    assert_eq!(session.document, original);
}

#[test]
fn downscaling_folders_and_adjustments_ignores_unused_placeholder_bounds() {
    let mut doc = Document::new(20, 20).unwrap();
    crate::edits::fill(&mut doc, [40, 90, 160, 255], false, false).unwrap();
    let mut folder = Layer::blank("Folder", 1, 1);
    folder.content = LayerContent::Group;
    doc.layers[0].parent = Some(folder.id);
    let parent = folder.id;
    doc.add(folder).unwrap();
    let mut invert = Layer::blank("Invert", 1, 1);
    invert.parent = Some(parent);
    invert.content = LayerContent::Adjustment(Box::new(Adjustment::new(Kind::Invert)));
    doc.add(invert).unwrap();
    let board = batch(vec![[10, 10]], Fit::Contain)
        .add_artboards(&mut doc, Source::Canvas)
        .unwrap()[0];
    assert!(
        board_pixels(&doc, board)
            .pixels()
            .all(|pixel| *pixel == Rgba([215, 165, 95, 255]))
    );
    doc.validate().unwrap();
}

#[test]
fn duplicated_raw_settings_are_bounded_before_publishing_variants() {
    use crate::raw::{DevelopSettings, RawAsset, RawMetadata};
    let mut doc = Document::new(2, 2).unwrap();
    let raw = Arc::new(RawAsset {
        filename: "x".repeat(16_000),
        metadata: RawMetadata {
            width: 2,
            height: 2,
            camera: "c".repeat(4_000),
            lens: "l".repeat(4_000),
            ..Default::default()
        },
        settings: DevelopSettings::default(),
        bytes: Arc::new(vec![1, 2, 3]),
    });
    let image = Arc::new(RgbaImage::new(2, 2));
    for _ in 0..100 {
        let mut layer = Layer::blank("RAW", 2, 2);
        layer.raw = Some(raw.clone());
        layer.content = LayerContent::Raster(Some(image.clone()));
        doc.add(layer).unwrap();
    }
    crate::project::validate_storage_metadata(&doc).unwrap();
    let original = doc.clone();
    let error = batch(vec![[2, 2]], Fit::Contain)
        .add_artboards(&mut doc, Source::Canvas)
        .unwrap_err();
    assert!(error.to_string().contains("RAW settings"));
    assert_eq!(doc, original);
}
