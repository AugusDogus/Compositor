use compositor::{
    brush::{Brush, PaintMode, Stroke},
    canvas_size, clipboard,
    document::Document,
    edits, image_io, image_resize, project, render,
    selection::{Selection, SelectionMode},
    session::Session,
};

#[test]
fn sparse_pixel_and_mask_blur_strokes_match_small_canvas_results() {
    for mask_target in [false, true] {
        let mut snapshots = Vec::new();
        for (size, center) in [(80, 40.), (30_000, 15_000.)] {
            let mut doc = Document::new(size, size).unwrap();
            let layer = &mut doc.layers[0];
            layer.transform = compositor::geometry::Transform::new(8, 8);
            layer.transform.origin = [center - 4., center - 4.];
            layer.content = compositor::document::LayerContent::Raster(Some(std::sync::Arc::new(
                image::RgbaImage::from_fn(8, 8, |x, _| image::Rgba([200, (x * 30) as u8, 80, 255])),
            )));
            if mask_target {
                layer.mask = Some(compositor::document::Mask {
                    pixels: std::sync::Arc::new(image::GrayImage::from_fn(8, 8, |x, _| {
                        image::Luma([if x < 4 { 0 } else { 255 }])
                    })),
                    enabled: true,
                    linked: true,
                    placement: None,
                });
            }
            let mut stroke = Stroke::start(
                &mut doc,
                [center, center],
                Brush {
                    diameter: 20.,
                    ..Brush::default()
                },
                PaintMode::Blur,
                mask_target,
                false,
            )
            .unwrap();
            stroke.to(&mut doc, [center + 6., center]).unwrap();
            doc.validate().unwrap();
            snapshots.push(doc.layers.remove(0));
        }
        if mask_target {
            assert_eq!(
                snapshots[0].mask.as_ref().unwrap().pixels,
                snapshots[1].mask.as_ref().unwrap().pixels
            );
        } else {
            assert_eq!(snapshots[0].raster(), snapshots[1].raster());
        }
    }
}

#[test]
fn sparse_warp_strokes_match_small_canvas_results_and_keep_source_assets_bounded() {
    for mode in [
        compositor::warp::Mode::Smudge,
        compositor::warp::Mode::Liquify,
    ] {
        let mut images = Vec::new();
        for (size, center) in [(80, 40.), (30_000, 15_000.)] {
            let mut doc = Document::new(size, size).unwrap();
            let layer = &mut doc.layers[0];
            layer.transform = compositor::geometry::Transform::new(8, 8);
            layer.transform.origin = [center - 4., center - 4.];
            layer.content = compositor::document::LayerContent::Raster(Some(std::sync::Arc::new(
                image::RgbaImage::from_fn(8, 8, |x, _| image::Rgba([200, (x * 30) as u8, 80, 255])),
            )));
            let original = doc.clone();
            let mut session = Session::new(doc, None);
            session
                .edit("Warp", |doc| {
                    let mut stroke = compositor::warp::WarpStroke::start(
                        doc,
                        [center, center],
                        mode,
                        Brush {
                            diameter: 12.,
                            opacity: 0.7,
                            ..Brush::default()
                        },
                        false,
                    )?;
                    stroke.to(doc, [center + 6., center])?;
                    stroke.to(doc, [center + 12., center + 2.])
                })
                .unwrap();
            let after = session.document.clone();
            assert!(after.layers[0].raster().unwrap().width() <= 30);
            images.push(
                render::region(&after, 40, 30, [center - 10., center - 10.], [1., 1.]).unwrap(),
            );
            session.undo();
            assert_eq!(session.document, original);
            session.redo();
            assert_eq!(session.document, after);
        }
        for (small, sparse) in images[0].pixels().zip(images[1].pixels()) {
            assert_eq!(small[3], sparse[3]);
            if small[3] > 0 {
                assert_eq!(small, sparse);
            }
        }
    }
}

#[test]
fn canvas_wide_selections_stay_sparse_through_boolean_edits_flips_and_transforms() {
    let all =
        Selection::marquee(30_000, 30_000, [0., 0.], [30_000., 30_000.], false, true).unwrap();
    assert!(all.pixels.dense().is_none());
    assert_eq!(all.bounds(), Some([0., 0., 30_000., 30_000.]));
    assert_eq!(all.coverage([29_999.5, 29_999.5]), 1.);
    assert_eq!(all.coverage([30_000.5, 29_999.5]), 0.);
    let empty = all.invert(30_000, 30_000).unwrap();
    assert!(empty.bounds().is_none());
    let inner = all.resized(-5, 30_000, 30_000).unwrap();
    assert_eq!(inner.bounds(), Some([5., 5., 29_995., 29_995.]));
    let hole = Selection::marquee(30_000, 30_000, [100., 200.], [130., 240.], false, true).unwrap();
    let mut cut = all.combine(&hole, SelectionMode::Subtract).unwrap();
    assert!(cut.pixels.dense().is_none());
    assert_eq!(cut.coverage([110.5, 220.5]), 0.);
    assert_eq!(cut.coverage([500.5, 500.5]), 1.);
    let original = cut.clone();
    cut.mirror(true, 15_000.);
    assert_eq!(cut.coverage([30_000. - 110.5, 220.5]), 0.);
    let inverse = cut.invert(30_000, 30_000).unwrap();
    assert_eq!(inverse.bounds(), Some([29_870., 200., 29_900., 240.]));
    cut.mirror(true, 15_000.);
    assert_eq!(cut, original);
    let shifted = cut.translated([-20., 30.]).unwrap();
    assert_eq!(shifted.coverage([90.5, 250.5]), 0.);
    assert_eq!(shifted.coverage([-10.5, 31.5]), 1.);
    let mapped = all
        .mapped(
            [0., 0., 15_000., 30_000.],
            |p| [p[0] * 0.5, p[1]],
            |p| [p[0] * 2., p[1]],
        )
        .unwrap();
    assert!(mapped.pixels.dense().is_none());
    assert_eq!(mapped.bounds(), Some([0., 0., 15_000., 30_000.]));
}

#[test]
fn sparse_canvas_paint_selection_history_and_project_round_trip_keep_small_assets() {
    let mut session = Session::new(Document::new(30_000, 30_000).unwrap(), None);
    assert!(session.document.layers[0].raster().is_none());
    let original = session.document.clone();
    session
        .edit("Brush", |doc| {
            let mut stroke = Stroke::start(
                doc,
                [15_000.5, 15_000.5],
                Brush {
                    diameter: 20.,
                    color: [200, 40, 80, 255],
                    ..Brush::default()
                },
                PaintMode::Paint,
                false,
                false,
            )?;
            stroke.to(doc, [15_050.5, 15_000.5])
        })
        .unwrap();
    let painted = session.document.clone();
    let pixels = painted.layers[0].raster().unwrap();
    assert!(
        pixels.width() <= 80 && pixels.height() <= 30,
        "{:?}",
        pixels.dimensions()
    );
    assert_eq!(
        render::sample(&painted, [15_025.5, 15_000.5]).unwrap(),
        [200. / 255., 40. / 255., 80. / 255., 1.]
    );
    assert_eq!(render::sample(&painted, [500., 500.]).unwrap(), [0.; 4]);
    session.undo();
    assert_eq!(session.document, original);
    session.redo();
    assert_eq!(session.document, painted);

    session.document.selection = Some(
        Selection::marquee(
            30_000,
            30_000,
            [15_010., 14_990.],
            [15_030., 15_010.],
            false,
            true,
        )
        .unwrap(),
    );
    let selection = session.document.selection.as_ref().unwrap();
    assert_eq!(selection.pixels.dimensions(), (20, 20));
    let clip = clipboard::copy(&session.document, false, false).unwrap();
    assert_eq!(clip.origin, [15_010., 14_990.]);
    assert_eq!(clip.pixels.dimensions(), (20, 20));
    let expanded = selection.resized(3, 30_000, 30_000).unwrap();
    assert!(expanded.pixels.width() <= 26 && expanded.pixels.height() <= 26);

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Sparse.comp");
    project::save(&session.document, &path).unwrap();
    let reopened = project::load(&path).unwrap();
    assert_eq!((reopened.width, reopened.height), (30_000, 30_000));
    assert_eq!(reopened.layers, painted.layers);
    assert_eq!(
        render::render(&reopened, 150, 150).unwrap().dimensions(),
        (150, 150)
    );
    for sample_all in [false, true] {
        let mut cloned = painted.clone();
        let stroke = Stroke::start(
            &mut cloned,
            [15_100.5, 15_000.5],
            Brush {
                diameter: 10.,
                ..Brush::default()
            },
            PaintMode::Clone { offset: [-75., 0.] },
            false,
            sample_all,
        )
        .unwrap();
        drop(stroke);
        assert_eq!(
            render::sample(&cloned, [15_100.5, 15_000.5]).unwrap(),
            render::sample(&painted, [15_025.5, 15_000.5]).unwrap()
        );
        assert!(cloned.layers[0].raster().unwrap().width() < 130);
    }
    let mut merged = painted.clone();
    let mut copy = merged.layers[0].clone();
    copy.id = uuid::Uuid::new_v4();
    copy.transform.origin[0] += 10.;
    copy.opacity = 0.5;
    merged.add(copy).unwrap();
    let before = render::region(&merged, 100, 40, [14_990., 14_980.], [1., 1.]).unwrap();
    compositor::layer_ops::merge(&mut merged, true).unwrap();
    assert_eq!(merged.layers.len(), 1);
    assert!(merged.layers[0].raster().unwrap().width() < 100);
    let after = render::region(&merged, 100, 40, [14_990., 14_980.], [1., 1.]).unwrap();
    for ((x, y, actual), expected) in after.enumerate_pixels().zip(before.pixels()) {
        assert_eq!(actual[3], expected[3], "Merged alpha ({x},{y})");
        if expected[3] > 0 {
            assert_eq!(actual, expected, "Merged visible pixel ({x},{y})");
        }
    }
    merged.validate().unwrap();
}

#[test]
fn canvas_geometry_changes_allow_sparse_sizes_but_full_raster_operations_reject_them() {
    let mut doc = Document::new(100, 100).unwrap();
    canvas_size::resize(&mut doc, [30_000, 30_000], [0.5, 0.5], None).unwrap();
    assert!(doc.layers[0].raster().is_none());
    image_resize::resize(
        &mut doc,
        30_000,
        30_000,
        300.,
        compositor::geometry::Sampling::High,
    )
    .unwrap();
    assert_eq!(doc.resolution, 300.);
    let original = doc.clone();
    let dir = tempfile::tempdir().unwrap();
    assert!(image_io::export(&doc, &dir.path().join("too-large.png"), 90).is_err());
    assert!(!dir.path().join("too-large.png").exists());
    assert!(clipboard::copy(&doc, true, false).is_err());
    assert_eq!(doc, original);
    edits::crop(&mut doc, [0., 0.], [25_000., 20_000.]).unwrap();
    doc.validate().unwrap();
    let before = doc.clone();
    assert!(canvas_size::resize(&mut doc, [30_000, 30_000], [0., 0.], Some([255; 4])).is_err());
    assert_eq!(doc, before);
    assert!(Document::new(30_001, 1).is_err());
}
