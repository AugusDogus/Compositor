use compositor::{
    brush::{
        Brush, Input, PaintMode, Stroke, Tip,
        sampled::{Sampled, Shape, gih::Hose},
    },
    document::Document,
};
use std::sync::Arc;

fn pipe(mode: &str) -> Sampled {
    let mut bytes = format!("Test\n4 dim:1 rank0:4 sel0:{mode}\n").into_bytes();
    for (i, alpha) in [40, 100, 160, 220].into_iter().enumerate() {
        let w = i as u32 + 1;
        for value in [30u32, 2, w, 1, 1, 0x47494d50, 200] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes.extend_from_slice(b"x\0");
        bytes.extend(std::iter::repeat_n(alpha, w as usize));
    }
    Sampled::from_hose(Arc::new(Hose::from_bytes(&bytes).unwrap()))
}
fn draw(sampled: Sampled, points: &[[f64; 2]], tip: Option<Tip>) -> Document {
    let mut doc = Document::new(180, 100).unwrap();
    let mut stroke = Stroke::start_shaped_input(
        &mut doc,
        Input {
            point: points[0],
            tip,
        },
        Brush {
            diameter: 20.,
            opacity: 1.,
            ..Brush::default()
        },
        PaintMode::Paint,
        false,
        false,
        Shape::Sampled(sampled),
    )
    .unwrap();
    for point in &points[1..] {
        stroke
            .to_input(&mut doc, Input { point: *point, tip })
            .unwrap();
    }
    stroke.finish(&mut doc).unwrap();
    doc
}
fn alpha(doc: &Document, point: [u32; 2]) -> u8 {
    doc.layers[0].raster().unwrap()[(point[0], point[1])][3]
}
#[test]
fn mixed_size_cells_cycle_across_strokes_without_consuming_preview_dabs() {
    let sparse = draw(pipe("incremental"), &[[30., 50.], [150., 50.]], None);
    let dense = draw(
        pipe("incremental"),
        &[
            [30., 50.],
            [45., 50.],
            [60., 50.],
            [80., 50.],
            [95., 50.],
            [110., 50.],
            [130., 50.],
            [150., 50.],
        ],
        None,
    );
    assert_eq!(sparse.layers[0].raster(), dense.layers[0].raster());
    assert_eq!(
        [30, 70, 110, 150].map(|x| alpha(&sparse, [x, 50])),
        [100, 160, 220, 40]
    );
    let sampled = pipe("incremental");
    let first = draw(sampled.clone(), &[[50., 50.]], None);
    let second = draw(sampled.clone(), &[[50., 50.]], None);
    assert_eq!(alpha(&first, [50, 50]), 100);
    assert_eq!(alpha(&second, [50, 50]), 160);
}
#[test]
fn angular_hoses_skip_clicks_and_follow_document_direction_with_tilt() {
    let doc = draw(pipe("angular"), &[[50., 50.]], None);
    assert!(
        doc.layers[0].raster().is_none(),
        "no-motion angular dab must preserve an empty layer"
    );
    let right = draw(
        pipe("angular"),
        &[[30., 50.], [70., 50.]],
        Some(Tip::new(Some(1.), Some([45., 0.]))),
    );
    let left = draw(
        pipe("angular"),
        &[[70., 50.], [30., 50.]],
        Some(Tip::new(Some(1.), Some([45., 0.]))),
    );
    assert_eq!(alpha(&right, [30, 50]), 100);
    assert_eq!(alpha(&left, [70, 50]), 220);
}
#[test]
fn pressure_and_tilt_choose_cells_using_validated_tablet_axes() {
    for (pressure, expected) in [(0.25, 100), (0.5, 160), (1., 220)] {
        let doc = draw(
            pipe("pressure"),
            &[[50., 50.]],
            Some(Tip::new(Some(pressure), None)),
        );
        assert_eq!(alpha(&doc, [50, 50]), expected);
    }
    for (tilt, expected) in [(-45., 100), (0., 160), (45., 220)] {
        let doc = draw(
            pipe("xtilt"),
            &[[50., 50.]],
            Some(Tip::new(Some(1.), Some([tilt, 0.]))),
        );
        assert_eq!(alpha(&doc, [50, 50]), expected);
    }
}
