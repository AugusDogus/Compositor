use compositor::{
    brush::{Brush, PaintMode, Stroke},
    document::Document,
};
use image::RgbaImage;

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn large_gpu_strokes_preserve_cancellation_history_and_saved_pixels() {
    use compositor::{
        document::{LayerContent, Mask},
        project,
        session::Session,
    };
    use image::{GrayImage, Luma, Rgba};
    use std::sync::Arc;

    compositor::brush::initialize_gpu().unwrap();
    for mask in [false, true] {
        for mode in [PaintMode::Paint, PaintMode::Erase] {
            let mut document = Document::new(1024, 768).unwrap();
            document.layers[0].content = LayerContent::Raster(Some(Arc::new(
                RgbaImage::from_pixel(1024, 768, Rgba([200, 100, 50, 255])),
            )));
            document.layers[0].mask = Some(Mask {
                pixels: Arc::new(GrayImage::from_pixel(1024, 768, Luma([255]))),
                enabled: true,
                linked: true,
                placement: None,
            });
            let mut session = Session::new(document.clone(), None);
            let brush = Brush {
                diameter: 400.,
                hardness: 0.,
                opacity: 0.4,
                color: [0, 0, 0, 255],
            };
            for cancel in [true, false] {
                session.begin("Brush").unwrap();
                let mut stroke = Stroke::start(
                    &mut session.document,
                    [240., 500.],
                    brush,
                    mode,
                    mask,
                    false,
                )
                .unwrap();
                for point in [[500., 250.], [800., 500.], [500., 250.], [800., 500.]] {
                    stroke.to(&mut session.document, point).unwrap();
                }
                assert_ne!(session.document, document);
                if cancel {
                    session.cancel();
                    assert_eq!(session.document, document);
                    assert_eq!(session.undo_label(), None);
                    continue;
                }
                stroke.finish(&mut session.document).unwrap();
                let finished = session.document.clone();
                stroke.finish(&mut session.document).unwrap();
                assert_eq!(session.document, finished);
                let layer = &session.document.layers[0];
                assert_eq!(layer.raster().unwrap()[(0, 0)], Rgba([200, 100, 50, 255]));
                assert_eq!(layer.mask.as_ref().unwrap().pixels[(0, 0)], Luma([255]));
                if mask {
                    assert_eq!(layer.raster(), document.layers[0].raster());
                    let value = layer.mask.as_ref().unwrap().pixels[(800, 500)][0];
                    assert!(
                        (153..200).contains(&value),
                        "overlap exceeded mask opacity: {value}"
                    );
                } else {
                    assert_eq!(layer.mask, document.layers[0].mask);
                    let pixel = layer.raster().unwrap()[(800, 500)];
                    if mode == PaintMode::Erase {
                        assert!(
                            (153..200).contains(&pixel[3]),
                            "overlap exceeded eraser opacity: {pixel:?}"
                        );
                    } else {
                        assert!(
                            (120..160).contains(&pixel[0]),
                            "overlap exceeded paint opacity: {pixel:?}"
                        );
                    }
                }
                session.commit().unwrap();
                assert_eq!(session.undo_label(), Some("Brush"));
                session.undo();
                assert_eq!(session.document, document);
                session.redo();
                assert_eq!(session.document, finished);
                let directory = tempfile::tempdir().unwrap();
                let path = directory.path().join("gpu-stroke.comp");
                project::save(&session.document, &path).unwrap();
                assert_eq!(project::load(&path).unwrap().layers, finished.layers);
            }
        }
    }
}

#[test]
fn hard_tip_edges_follow_the_transformed_pixel_grid() {
    use compositor::{document::LayerContent, geometry::Transform};
    use std::sync::Arc;
    for scale in [1., 2.] {
        for rotation in [0., 37.] {
            let mut doc = Document::new(80, 80).unwrap();
            let t = Transform {
                origin: [20., 20.],
                size: [20. * scale; 2],
                rotation,
                flip_x: true,
                ..Transform::new(20, 20)
            };
            doc.layers[0].transform = t;
            doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::new(20, 20))));
            Stroke::start(
                &mut doc,
                t.point([0.5, 0.5]),
                Brush {
                    diameter: 4. * scale,
                    ..Brush::default()
                },
                PaintMode::Paint,
                false,
                false,
            )
            .unwrap();
            let pixels = doc.layers[0].raster().unwrap();
            assert_eq!(pixels[(9, 9)][3], 255);
            assert!(
                (pixels[(8, 8)][3] as i16 - 97).abs() <= 1,
                "scale {scale}, rotation {rotation}"
            );
            assert_eq!(pixels[(7, 7)][3], 0);
        }
    }
}

#[test]
fn sparse_samples_follow_the_arc_between_pointer_positions() {
    let mut doc = Document::new(300, 300).unwrap();
    let circle = |degrees: f64| {
        let (sin, cos) = degrees.to_radians().sin_cos();
        [150. + 100. * cos, 150. + 100. * sin]
    };
    let mut stroke = Stroke::start(
        &mut doc,
        circle(0.),
        Brush {
            diameter: 4.,
            ..Brush::default()
        },
        PaintMode::Paint,
        false,
        false,
    )
    .unwrap();
    for degrees in (30..=180).step_by(30) {
        stroke.to(&mut doc, circle(degrees as f64)).unwrap();
    }
    stroke.finish(&mut doc).unwrap();
    let pixels = compositor::render::render(&doc, 300, 300);
    for degrees in [45., 75., 105., 135.] {
        let point = circle(degrees);
        assert!(
            pixels[(point[0] as u32, point[1] as u32)][3] > 0,
            "arc at {degrees} degrees"
        );
    }
}

#[test]
fn provisional_tail_reaches_pointer_and_leaves_no_chord_after_finish() {
    let mut doc = Document::new(300, 120).unwrap();
    let mut stroke = Stroke::start(
        &mut doc,
        [20., 60.],
        Brush {
            diameter: 8.,
            ..Brush::default()
        },
        PaintMode::Paint,
        false,
        false,
    )
    .unwrap();
    stroke.to(&mut doc, [150., 20.]).unwrap();
    stroke.to(&mut doc, [280., 60.]).unwrap();
    assert_eq!(doc.layers[0].raster().unwrap()[(278, 60)][3], 255);
    stroke.finish(&mut doc).unwrap();
    let pixels = compositor::render::render(&doc, 300, 120);
    assert_eq!(pixels[(215, 40)][3], 0);
    assert_eq!(pixels[(278, 60)][3], 255);
    let finished = doc.clone();
    stroke.finish(&mut doc).unwrap();
    assert_eq!(doc, finished);
}

#[test]
fn replacing_soft_tails_restores_existing_pixels_and_masks() {
    use compositor::document::{LayerContent, Mask};
    use image::{GrayImage, Luma, Rgba};
    use std::sync::Arc;
    for mask in [false, true] {
        for mode in [PaintMode::Paint, PaintMode::Erase] {
            let mut doc = Document::new(300, 120).unwrap();
            doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
                300,
                120,
                Rgba([100, 50, 25, 255]),
            ))));
            doc.layers[0].mask = Some(Mask {
                pixels: Arc::new(GrayImage::from_pixel(300, 120, Luma([255]))),
                enabled: true,
                linked: true,
                placement: None,
            });
            let original = doc.clone();
            let mut stroke = Stroke::start(
                &mut doc,
                [20., 60.],
                Brush {
                    diameter: 8.,
                    hardness: 0.,
                    opacity: 0.4,
                    ..Brush::default()
                },
                mode,
                mask,
                false,
            )
            .unwrap();
            stroke.to(&mut doc, [150., 20.]).unwrap();
            stroke.to(&mut doc, [280., 60.]).unwrap();
            stroke.finish(&mut doc).unwrap();
            assert_eq!(
                doc.layers[0].raster().unwrap()[(215, 40)],
                original.layers[0].raster().unwrap()[(215, 40)]
            );
            assert_eq!(
                doc.layers[0].mask.as_ref().unwrap().pixels[(215, 40)],
                Luma([255])
            );
            if mask {
                assert!(doc.layers[0].mask.as_ref().unwrap().pixels[(278, 60)][0] < 200);
            } else {
                assert_ne!(
                    doc.layers[0].raster().unwrap()[(278, 60)],
                    original.layers[0].raster().unwrap()[(278, 60)]
                );
            }
        }
    }
}

fn draw(points: &[[f64; 2]], event_spacing: f64, opacity: f64) -> RgbaImage {
    let mut doc = Document::new(200, 200).unwrap();
    let brush = Brush {
        diameter: 60.,
        hardness: 0.,
        opacity,
        color: [255; 4],
    };
    let mut stroke =
        Stroke::start(&mut doc, points[0], brush, PaintMode::Paint, false, false).unwrap();
    for pair in points.windows(2) {
        let [a, b] = [pair[0], pair[1]];
        let count = ((b[0] - a[0]).hypot(b[1] - a[1]) / event_spacing).ceil() as usize;
        for step in 1..=count {
            let t = step as f64 / count as f64;
            stroke
                .to(
                    &mut doc,
                    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t],
                )
                .unwrap();
        }
    }
    stroke.finish(&mut doc).unwrap();
    compositor::render::render(&doc, 200, 200)
}

#[test]
fn soft_brush_crossings_accumulate_coverage_with_a_stroke_wide_opacity_cap() {
    let vertical = draw(&[[100., 20.], [100., 180.]], 1000., 1.);
    let horizontal = draw(&[[180., 100.], [20., 100.]], 1000., 1.);
    let path = [
        [100., 20.],
        [100., 180.],
        [180., 180.],
        [180., 100.],
        [20., 100.],
    ];
    // Dense samples keep these crossing runs straight, isolating accumulation
    // from the curved interpolation tested separately above.
    let crossed = draw(&path, 1., 1.);
    for offset in [25, 27, 28] {
        let (x, y) = (100 + offset, 100 + offset);
        let a = vertical[(x, y)][3] as i32;
        let b = horizontal[(x, y)][3] as i32;
        let actual = crossed[(x, y)][3] as i32;
        let expected = 255 - (255 - a) * (255 - b) / 255;
        assert!(
            actual > a.max(b) + 10,
            "offset {offset}: {actual} vs {a}, {b}"
        );
        assert!(
            (actual - expected).abs() <= 3,
            "offset {offset}: {actual} vs {expected}"
        );
    }
    let capped = draw(&path, 1., 0.4);
    assert_eq!(capped[(100, 100)][3], 102);
    assert!(capped.pixels().all(|p| p[3] <= 102));
}

#[test]
fn soft_strokes_are_independent_of_pointer_event_density() {
    let path = [[20., 100.], [180., 100.]];
    let sparse = draw(&path, 1000., 1.);
    let dense = draw(&path, 1., 1.);
    assert!(
        sparse
            .pixels()
            .zip(dense.pixels())
            .all(|(a, b)| a[3].abs_diff(b[3]) <= 2)
    );
}

#[test]
fn strokes_expand_rotated_flipped_sources_without_moving_pixels_or_masks() {
    use compositor::{
        document::{LayerContent, Mask},
        geometry::{Sampling, Transform},
        session::Session,
    };
    use image::{GrayImage, Luma, Rgba};
    use std::sync::Arc;
    for rotation in [0., 37., 90.] {
        let mut document = Document::new(600, 200).unwrap();
        let layer = &mut document.layers[0];
        layer.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
            40,
            20,
            Rgba([255, 0, 0, 255]),
        ))));
        layer.transform = Transform {
            origin: [240., 80.],
            size: [80., 40.],
            rotation,
            flip_x: true,
            sampling: Sampling::Nearest,
            ..Transform::new(40, 20)
        };
        layer.mask = Some(Mask {
            pixels: Arc::new(GrayImage::from_pixel(1, 1, Luma([128]))),
            enabled: true,
            linked: true,
            placement: None,
        });
        let mut session = Session::new(document.clone(), None);
        session.begin("Paint").unwrap();
        let brush = Brush {
            diameter: 16.,
            color: [0, 255, 0, 255],
            ..Brush::default()
        };
        let mut stroke = Stroke::start(
            &mut session.document,
            [12., 12.],
            brush,
            PaintMode::Paint,
            false,
            false,
        )
        .unwrap();
        stroke.to(&mut session.document, [580., 12.]).unwrap();
        stroke.finish(&mut session.document).unwrap();
        session.commit().unwrap();
        let pixels = compositor::render::render(&session.document, 600, 200);
        assert_eq!(
            pixels[(280, 100)],
            Rgba([255, 0, 0, 128]),
            "rotation {rotation}"
        );
        assert!(
            pixels[(12, 12)][1] > 240 && pixels[(580, 12)][1] > 240,
            "rotation {rotation}"
        );
        assert_eq!(session.document.layers[0].transform.rotation, rotation);
        assert!(session.document.layers[0].transform.flip_x);
        session.undo();
        assert_eq!(session.document, document);
        session.redo();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("paint.comp");
        compositor::project::save(&session.document, &path).unwrap();
        let reopened = compositor::project::load(&path).unwrap();
        assert_eq!(compositor::render::render(&reopened, 600, 200), pixels);
    }
}
