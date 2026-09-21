use compositor::{
    document::{Document, LayerContent, Mask},
    floating::FloatingPixels,
    geometry::{Sampling, Transform},
    project, render,
    selection::Selection,
    session::Session,
};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use std::sync::Arc;

#[test]
fn moving_pixels_from_rotated_flipped_layers_preserves_source_mask_and_history() {
    for rotation in [0., 37., 90., 180., 270.] {
        for duplicate in [false, true] {
            let mut doc = Document::new(160, 120).unwrap();
            let layer = &mut doc.layers[0];
            layer.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
                40,
                40,
                Rgba([80, 140, 200, 255]),
            ))));
            layer.transform = Transform {
                origin: [10., 10.],
                size: [40., 40.],
                rotation,
                flip_x: true,
                sampling: Sampling::Nearest,
                ..Transform::new(40, 40)
            };
            let original_transform = layer.transform;
            layer.mask = Some(Mask {
                pixels: Arc::new(GrayImage::from_pixel(40, 40, Luma([255]))),
                enabled: true,
                linked: true,
                placement: Some(original_transform),
            });
            let original_mask = layer.mask.as_ref().unwrap().pixels.clone();
            doc.selection = Some(Selection::rectangle(
                160,
                120,
                [24., 24.],
                [36., 36.],
                false,
            ));
            let original = doc.clone();
            let source = FloatingPixels::lift(&doc).unwrap();
            let mut destination = source.placement;
            destination.origin[0] += 80.;
            destination.origin[1] += 40.;
            let moved = source.preview(destination, duplicate).unwrap();
            moved.validate().unwrap();
            let layer = &moved.layers[0];
            assert_eq!(layer.transform.rotation, rotation);
            assert!(layer.transform.flip_x);
            let mask = layer.mask.as_ref().unwrap();
            assert!(Arc::ptr_eq(&mask.pixels, &original_mask));
            assert_eq!(mask.placement, Some(original_transform));
            let pixels = render::render(&moved, 160, 120);
            assert_eq!(
                pixels[(110, 70)],
                Rgba([80, 140, 200, 255]),
                "rotation {rotation}"
            );
            assert_eq!(pixels[(30, 30)][3], if duplicate { 255 } else { 0 });
            // Source pixels outside both the cut and pasted regions retain their mapping.
            let original_pixels = original.layers[0].raster().unwrap();
            for (x, y, expected) in original_pixels.enumerate_pixels() {
                let point =
                    original_transform.point([(x as f64 + 0.5) / 40., (y as f64 + 0.5) / 40.]);
                if (23. ..37.).contains(&point[0]) && (23. ..37.).contains(&point[1]) {
                    continue;
                }
                let unit = layer.transform.unit(point);
                let raster = layer.raster().unwrap();
                let pixel = raster[(
                    (unit[0] * raster.width() as f64).floor() as u32,
                    (unit[1] * raster.height() as f64).floor() as u32,
                )];
                assert_eq!(pixel, *expected, "rotation {rotation}, source ({x}, {y})");
            }
            assert_eq!(source.preview(source.placement, false).unwrap(), original);
            let mut session = Session::new(original.clone(), None);
            session
                .edit("Move selection", |doc| {
                    *doc = moved.clone();
                    Ok(())
                })
                .unwrap();
            session.undo();
            assert_eq!(session.document, original);
            session.redo();
            assert_eq!(session.document, moved);
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("transformed.comp");
            project::save(&session.document, &path).unwrap();
            assert_eq!(
                render::render(&project::load(&path).unwrap(), 160, 120),
                pixels
            );
        }
    }
}

#[test]
fn scaling_rotating_and_distorting_selected_pixels_preserves_transformed_source_edits() {
    for rotation in [37., 90.] {
        for duplicate in [false, true] {
            for perspective in [false, true] {
                let mut doc = Document::new(180, 120).unwrap();
                let layer = &mut doc.layers[0];
                let color = Rgba([80, 140, 200, 255]);
                layer.content =
                    LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(40, 40, color))));
                layer.transform = Transform {
                    origin: [10., 10.],
                    rotation,
                    flip_x: true,
                    sampling: Sampling::Nearest,
                    ..Transform::new(40, 40)
                };
                let placement = layer.transform;
                layer.mask = Some(Mask {
                    pixels: Arc::new(GrayImage::from_pixel(40, 40, Luma([255]))),
                    enabled: true,
                    linked: false,
                    placement: Some(placement),
                });
                doc.selection = Some(Selection::rectangle(
                    180,
                    120,
                    [24., 24.],
                    [36., 36.],
                    false,
                ));
                let source = FloatingPixels::lift(&doc).unwrap();
                let transformed = if perspective {
                    source
                        .preview_distorted(
                            [[100., 30.], [145., 25.], [148., 80.], [105., 90.]],
                            duplicate,
                        )
                        .unwrap()
                } else {
                    source
                        .preview(
                            Transform {
                                origin: [100., 35.],
                                size: [44., 44.],
                                rotation: -19.,
                                flip_y: true,
                                ..source.placement
                            },
                            duplicate,
                        )
                        .unwrap()
                };
                transformed.validate().unwrap();
                let rendered = render::render(&transformed, 180, 120);
                assert_eq!(rendered[(123, 55)], color);
                assert_eq!(rendered[(30, 30)][3], if duplicate { 255 } else { 0 });
                assert_eq!(transformed.layers[0].mask, doc.layers[0].mask);
                assert_eq!(transformed.layers[0].transform.rotation, rotation);
                assert!(transformed.layers[0].transform.flip_x);
                let selection = transformed.selection.as_ref().unwrap();
                assert_eq!(selection.coverage([123.5, 55.5]), 1.);
                assert_eq!(selection.coverage([30.5, 30.5]), 0.);
                // A new preview always comes from the original snapshot, not the
                // preceding scaled/perspective result. Cancel is lossless.
                assert_eq!(source.preview(source.placement, false).unwrap(), doc);
                let mut session = Session::new(doc.clone(), None);
                session
                    .edit("Transform selection", |document| {
                        *document = transformed.clone();
                        Ok(())
                    })
                    .unwrap();
                session.undo();
                assert_eq!(session.document, doc);
                session.redo();
                assert_eq!(session.document, transformed);
                let directory = tempfile::tempdir().unwrap();
                let path = directory.path().join("transformed.comp");
                project::save(&session.document, &path).unwrap();
                assert_eq!(
                    render::render(&project::load(&path).unwrap(), 180, 120),
                    rendered
                );
            }
        }
    }
}
