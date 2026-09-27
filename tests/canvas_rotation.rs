use compositor::{
    canvas_rotation::{QuarterTurn, rotate},
    document::{Document, LayerContent, Mask},
    geometry::Transform,
    guides::{Axis, Guide},
    render,
    selection::Selection,
};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use std::sync::Arc;

#[test]
fn rotates_pixels_masks_guides_and_selection_without_replacing_source_pixels() {
    for turn in [QuarterTurn::Clockwise, QuarterTurn::CounterClockwise] {
        let mut doc = Document::new(4, 3).unwrap();
        let source = Arc::new(RgbaImage::from_fn(4, 3, |x, y| {
            Rgba([(x * 30) as u8, (y * 70) as u8, 90, 255])
        }));
        doc.layers[0].content = LayerContent::Raster(Some(source.clone()));
        doc.layers[0].mask = Some(Mask {
            pixels: Arc::new(GrayImage::from_pixel(2, 2, Luma([128]))),
            placement: Some(Transform {
                origin: [1., 0.],
                ..Transform::new(2, 2)
            }),
            enabled: true,
            linked: false,
        });
        doc.guides = vec![
            Guide {
                id: uuid::Uuid::new_v4(),
                axis: Axis::Vertical,
                position: 1.,
            },
            Guide {
                id: uuid::Uuid::new_v4(),
                axis: Axis::Horizontal,
                position: 2.,
            },
        ];
        doc.selection = Some(Selection::rectangle(4, 3, [1., 0.], [3., 2.], false));
        let before = render::render(&doc, 4, 3).unwrap();
        rotate(&mut doc, turn).unwrap();
        assert_eq!([doc.width, doc.height], [3, 4]);
        assert!(Arc::ptr_eq(doc.layers[0].raster().unwrap(), &source));
        let expected = match turn {
            QuarterTurn::Clockwise => image::imageops::rotate90(&before),
            QuarterTurn::CounterClockwise => image::imageops::rotate270(&before),
        };
        assert_eq!(render::render(&doc, 3, 4).unwrap(), expected);
        assert_eq!(doc.guides[0].axis, Axis::Horizontal);
        assert_eq!(doc.guides[1].axis, Axis::Vertical);
        let (positions, inside, outside) = match turn {
            QuarterTurn::Clockwise => ([1., 1.], [1.5, 1.5], [0.5, 0.5]),
            QuarterTurn::CounterClockwise => ([3., 2.], [0.5, 1.5], [2.5, 0.5]),
        };
        assert_eq!([doc.guides[0].position, doc.guides[1].position], positions);
        assert_eq!(doc.selection.as_ref().unwrap().coverage(inside), 1.);
        assert_eq!(doc.selection.as_ref().unwrap().coverage(outside), 0.);
    }
}

#[test]
fn clockwise_then_counterclockwise_preserves_editable_layers_and_placements() {
    let mut doc = Document::new(30, 20).unwrap();
    doc.layers[0].transform.rotation = 23.;
    doc.layers[0].transform.flip_x = true;
    doc.layers[0].shape = Some(compositor::document::Shape {
        geometry: compositor::document::ShapeGeometry::Rectangle,
        red: 1.,
        green: 0.,
        blue: 0.,
        corner_radius: 0.,
    });
    doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        30,
        20,
        Rgba([255, 0, 0, 255]),
    ))));
    let before = doc.clone();
    rotate(&mut doc, QuarterTurn::Clockwise).unwrap();
    rotate(&mut doc, QuarterTurn::CounterClockwise).unwrap();
    assert_eq!(doc, before);
}

#[test]
fn rotation_keeps_motion_blur_direction_and_rejects_invalid_guide_placement_atomically() {
    use compositor::{
        adjustment::{Adjustment, Kind},
        document::Layer,
    };
    let mut doc = Document::new(10, 20).unwrap();
    let mut layer = Layer::blank("Motion Blur", 10, 20);
    let mut adjustment = Adjustment::new(Kind::MotionBlur);
    adjustment.motion_angle = Some(25.);
    layer.content = LayerContent::Adjustment(Box::new(adjustment));
    doc.add(layer).unwrap();
    rotate(&mut doc, QuarterTurn::Clockwise).unwrap();
    let LayerContent::Adjustment(adjustment) = &doc.layers[1].content else {
        panic!("lost adjustment")
    };
    assert_eq!(adjustment.motion_angle, Some(-65.));
    doc.guides.push(Guide {
        id: uuid::Uuid::new_v4(),
        axis: Axis::Horizontal,
        position: -1_000_000.,
    });
    let before = doc.clone();
    assert!(rotate(&mut doc, QuarterTurn::Clockwise).is_err());
    assert_eq!(doc, before);
}

#[test]
fn rotation_rejects_generated_noise_without_changes_even_after_reopening() {
    use compositor::{
        adjustment::{Adjustment, Kind},
        document::Layer,
        project,
    };
    for kind in [Kind::Grain, Kind::AddNoise] {
        let mut doc = Document::new(8, 6).unwrap();
        doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
            8,
            6,
            Rgba([127, 127, 127, 255]),
        ))));
        let mut layer = Layer::blank("Noise", 8, 6);
        layer.content = LayerContent::Adjustment(Box::new(Adjustment::new(kind)));
        doc.add(layer).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("noise.comp");
        project::save(&doc, &path).unwrap();
        let reopened = project::load(&path).unwrap();
        for mut document in [doc, reopened] {
            let before = document.clone();
            let pixels = render::render(&document, 8, 6).unwrap();
            for turn in [QuarterTurn::Clockwise, QuarterTurn::CounterClockwise] {
                let error = rotate(&mut document, turn).unwrap_err().to_string();
                assert!(error.contains("Merge those adjustments"));
                assert_eq!(document, before);
                assert_eq!(render::render(&document, 8, 6).unwrap(), pixels);
            }
            compositor::layer_ops::merge(&mut document, true).unwrap();
            rotate(&mut document, QuarterTurn::Clockwise).unwrap();
            assert_eq!(
                render::render(&document, 6, 8).unwrap(),
                image::imageops::rotate90(&pixels)
            );
        }
    }
}

#[test]
fn inactive_noise_does_not_prevent_canvas_rotation() {
    use compositor::{
        adjustment::{Adjustment, Grain, Kind},
        document::Layer,
    };
    for kind in [Kind::Grain, Kind::AddNoise] {
        for hidden_parent in [false, true] {
            for invisible in [false, true] {
                let mut doc = Document::new(8, 6).unwrap();
                let mut group = Layer::blank("Folder", 8, 6);
                group.content = LayerContent::Group;
                let mut noise = Layer::blank("Noise", 8, 6);
                noise.parent = Some(group.id);
                noise.content = LayerContent::Adjustment(Box::new(Adjustment::new(kind)));
                let inactive = if hidden_parent {
                    &mut group
                } else {
                    &mut noise
                };
                if invisible {
                    inactive.visible = false;
                } else {
                    inactive.opacity = 0.;
                }
                doc.add(group).unwrap();
                doc.add(noise).unwrap();
                rotate(&mut doc, QuarterTurn::Clockwise).unwrap();
                assert_eq!([doc.width, doc.height], [6, 8]);
            }
        }
    }
    let mut doc = Document::new(8, 6).unwrap();
    let mut adjustment = Adjustment::new(Kind::Grain);
    adjustment.grain_settings = Some(Grain {
        amount: 0.,
        ..Grain::default()
    });
    doc.layers[0].content = LayerContent::Adjustment(Box::new(adjustment));
    rotate(&mut doc, QuarterTurn::Clockwise).unwrap();
}
