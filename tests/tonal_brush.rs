use compositor::{
    brush::{
        Brush, Input, PaintMode, Stroke, Tip,
        tonal::{Range, Tonal},
    },
    document::{Document, LayerContent, Mask},
    selection::Selection,
};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use std::sync::Arc;

fn document() -> Document {
    let mut doc = Document::new(80, 80).unwrap();
    doc.layers[0].content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(80, 80, |x, y| {
            Rgba([120, 80, 160, ((x + y) * 255 / 158) as u8])
        }))));
    doc
}

#[test]
fn tonal_strokes_respect_selection_pressure_and_do_not_accumulate_on_retrace() {
    for mode in [
        Tonal::Dodge(Range::All),
        Tonal::Burn(Range::All),
        Tonal::Saturate,
        Tonal::Desaturate,
    ] {
        let mut doc = document();
        let original = doc.layers[0].raster().unwrap().clone();
        doc.selection = Some(Selection::rectangle(80, 80, [30., 0.], [80., 80.], false));
        let brush = Brush {
            diameter: 40.,
            hardness: 1.,
            opacity: 0.5,
            color: [255, 0, 0, 255],
        };
        let mut stroke = Stroke::start_input(
            &mut doc,
            Input {
                point: [40., 40.],
                tip: Some(Tip::new(Some(0.5), None)),
            },
            brush,
            PaintMode::Tonal(mode),
            false,
            false,
        )
        .unwrap();
        stroke.finish(&mut doc).unwrap();
        let dab = doc.layers[0].raster().unwrap().clone();
        assert_ne!(dab[(40, 40)], original[(40, 40)]);
        assert_eq!(
            dab[(55, 40)],
            original[(55, 40)],
            "pressure must reduce radius"
        );
        stroke
            .to_input(
                &mut doc,
                Input {
                    point: [45., 40.],
                    tip: Some(Tip::new(Some(0.5), None)),
                },
            )
            .unwrap();
        stroke
            .to_input(
                &mut doc,
                Input {
                    point: [40., 40.],
                    tip: Some(Tip::new(Some(0.5), None)),
                },
            )
            .unwrap();
        stroke.finish(&mut doc).unwrap();
        let pixels = doc.layers[0].raster().unwrap();
        assert_eq!(
            pixels[(40, 40)],
            dab[(40, 40)],
            "retracing cannot exceed stroke strength"
        );
        for (x, y, p) in pixels.enumerate_pixels() {
            assert_eq!(p[3], original[(x, y)][3]);
            if x < 30 {
                assert_eq!(*p, original[(x, y)]);
            }
        }
    }
}

#[test]
fn tonal_strokes_reject_masks_without_mutating_them() {
    let mut doc = document();
    doc.layers[0].mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(1, 1, Luma([255]))),
        enabled: true,
        linked: true,
        placement: None,
    });
    let before = doc.clone();
    assert!(
        Stroke::start(
            &mut doc,
            [40., 40.],
            Brush::default(),
            PaintMode::Tonal(Tonal::Dodge(Range::All)),
            true,
            false
        )
        .is_err()
    );
    assert_eq!(doc, before);
}

#[test]
fn ineffective_tonal_stroke_preserves_editable_shape_metadata() {
    use compositor::document::{Shape, ShapeGeometry};
    let mut doc = Document::new(80, 80).unwrap();
    compositor::edits::shape(
        &mut doc,
        [10., 10.],
        [70., 70.],
        Shape {
            geometry: ShapeGeometry::Rectangle,
            red: 1.,
            green: 1.,
            blue: 1.,
            corner_radius: 0.,
        },
    )
    .unwrap();
    let original = doc.clone();
    let mut stroke = Stroke::start(
        &mut doc,
        [40., 40.],
        Brush::default(),
        PaintMode::Tonal(Tonal::Dodge(Range::All)),
        false,
        false,
    )
    .unwrap();
    stroke.finish(&mut doc).unwrap();
    assert_eq!(doc, original);
}

#[test]
fn tonal_strokes_reject_unallocated_blank_layers_without_mutation() {
    for mode in [
        Tonal::Dodge(Range::All),
        Tonal::Burn(Range::All),
        Tonal::Saturate,
        Tonal::Desaturate,
    ] {
        let mut doc = Document::new(8, 8).unwrap();
        let original = doc.clone();
        assert!(
            Stroke::start(
                &mut doc,
                [4., 4.],
                Brush::default(),
                PaintMode::Tonal(mode),
                false,
                false
            )
            .is_err()
        );
        assert_eq!(doc, original);
    }
}
