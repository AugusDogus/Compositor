use compositor::{
    brush::{
        Brush, Input, PaintMode, Stroke, Tip,
        sampled::{self, Sampled, Shape},
    },
    document::{Document, Mask},
    selection::Selection,
};
use image::{GrayImage, Luma};
use std::{path::PathBuf, sync::Arc};
fn shape(spacing: f64) -> Shape {
    let tip = sampled::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gbr/pixel.gbr"),
    )
    .unwrap();
    let mut sampled = Sampled::new(Arc::new(tip));
    sampled.set_spacing(spacing).unwrap();
    Shape::Sampled(sampled)
}
fn draw(points: &[[f64; 2]], input_tip: Option<Tip>) -> Document {
    let mut doc = Document::new(160, 100).unwrap();
    let mut stroke = Stroke::start_shaped_input(
        &mut doc,
        Input {
            point: points[0],
            tip: input_tip,
        },
        Brush {
            diameter: 20.,
            hardness: 0.,
            opacity: 0.4,
            color: [210, 30, 160, 255],
        },
        PaintMode::Paint,
        false,
        false,
        shape(2.),
    )
    .unwrap();
    for point in &points[1..] {
        stroke
            .to_input(
                &mut doc,
                Input {
                    point: *point,
                    tip: input_tip,
                },
            )
            .unwrap();
    }
    stroke.finish(&mut doc).unwrap();
    doc
}
#[test]
fn sampled_square_spacing_is_event_invariant_and_uses_foreground_with_capped_opacity() {
    let sparse = draw(&[[30., 50.], [130., 50.]], None);
    let dense = draw(
        &[
            [30., 50.],
            [45., 50.],
            [60., 50.],
            [75., 50.],
            [90., 50.],
            [110., 50.],
            [130., 50.],
        ],
        None,
    );
    assert_eq!(sparse.layers[0].raster(), dense.layers[0].raster());
    let pixels = sparse.layers[0].raster().unwrap();
    assert_eq!(
        pixels[(21, 41)].0,
        [210, 30, 160, 102],
        "square corners must paint, unlike a round tip"
    );
    assert_eq!(pixels[(50, 50)][3], 0, "200% spacing leaves a gap");
    assert_eq!(pixels[(70, 50)][3], 102);
    assert!(pixels.pixels().all(|p| p[3] <= 102));
}
#[test]
fn sampled_pressure_and_tilt_control_footprint() {
    let small = draw(&[[50., 50.]], Some(Tip::new(Some(0.25), None)));
    assert_eq!(small.layers[0].raster().unwrap()[(57, 50)][3], 0);
    let tilted = draw(&[[50., 50.]], Some(Tip::new(Some(1.), Some([60., 0.]))));
    let pixels = tilted.layers[0].raster().unwrap();
    assert!(pixels[(57, 50)][3] > 0);
    assert_eq!(pixels[(50, 57)][3], 0);
}
#[test]
fn sampled_selection_erase_and_mask_keep_existing_semantics() {
    let mut doc = draw(&[[50., 50.]], None);
    doc.selection = Some(Selection::rectangle(
        160,
        100,
        [50., 0.],
        [160., 100.],
        false,
    ));
    let brush = Brush {
        diameter: 20.,
        opacity: 0.5,
        ..Brush::default()
    };
    let mut stroke = Stroke::start_shaped_input(
        &mut doc,
        Input {
            point: [50., 50.],
            tip: None,
        },
        brush,
        PaintMode::Erase,
        false,
        false,
        shape(0.5),
    )
    .unwrap();
    stroke.finish(&mut doc).unwrap();
    let pixels = doc.layers[0].raster().unwrap();
    assert_eq!(pixels[(45, 50)][3], 102);
    assert_eq!(pixels[(55, 50)][3], 51);
    doc.layers[0].mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(160, 100, Luma([255]))),
        enabled: true,
        linked: true,
        placement: None,
    });
    let mut stroke = Stroke::start_shaped_input(
        &mut doc,
        Input {
            point: [50., 50.],
            tip: None,
        },
        brush,
        PaintMode::Erase,
        true,
        false,
        shape(0.5),
    )
    .unwrap();
    stroke.finish(&mut doc).unwrap();
    let mask = &doc.layers[0].mask.as_ref().unwrap().pixels;
    assert_eq!(mask[(45, 50)][0], 255);
    assert_eq!(mask[(55, 50)][0], 128);
}
