use compositor::{
    brush::{Brush, Input, PaintMode, Stroke, Tip},
    document::Document,
};
fn dab(pressure: Option<f64>, tilt: Option<[f64; 2]>) -> Document {
    let mut doc = Document::new(100, 100).unwrap();
    let mut stroke = Stroke::start_input(
        &mut doc,
        Input {
            point: [50., 50.],
            tip: Some(Tip::new(pressure, tilt)),
        },
        Brush {
            diameter: 40.,
            ..Brush::default()
        },
        PaintMode::Paint,
        false,
        false,
    )
    .unwrap();
    stroke.finish(&mut doc).unwrap();
    doc
}
fn alpha(doc: &Document, x: u32, y: u32) -> u8 {
    doc.layers[0].raster().unwrap()[(x, y)][3]
}
#[test]
fn pressure_scales_size_and_missing_pressure_keeps_full_mouse_size() {
    let soft = dab(Some(0.25), None);
    let firm = dab(Some(1.), None);
    let unknown = dab(None, None);
    assert!(alpha(&soft, 52, 50) > 200);
    assert_eq!(alpha(&soft, 60, 50), 0);
    assert!(alpha(&firm, 60, 50) > 200);
    assert_eq!(firm.layers[0].raster(), unknown.layers[0].raster());
    assert!(
        dab(Some(0.), None).layers[0].raster().is_none(),
        "Zero pressure must leave the blank layer unallocated"
    );
}
#[test]
fn tilt_orients_an_elliptical_tip_without_expanding_its_diameter() {
    let horizontal = dab(Some(1.), Some([60., 0.]));
    let vertical = dab(Some(1.), Some([0., 60.]));
    assert!(alpha(&horizontal, 65, 50) > 200);
    assert_eq!(alpha(&horizontal, 50, 65), 0);
    assert!(alpha(&vertical, 50, 65) > 200);
    assert_eq!(alpha(&vertical, 65, 50), 0);
    assert_eq!(alpha(&horizontal, 72, 50), 0);
}
#[test]
fn changing_pressure_keeps_earlier_marks_and_does_not_exceed_stroke_opacity() {
    let mut doc = Document::new(200, 100).unwrap();
    let sample = |point, pressure| Input {
        point,
        tip: Some(Tip::new(Some(pressure), None)),
    };
    let mut stroke = Stroke::start_input(
        &mut doc,
        sample([30., 50.], 1.),
        Brush {
            diameter: 40.,
            opacity: 0.4,
            ..Brush::default()
        },
        PaintMode::Paint,
        false,
        false,
    )
    .unwrap();
    for (point, pressure) in [([80., 50.], 0.25), ([120., 50.], 0.75), ([150., 50.], 0.15)] {
        stroke.to_input(&mut doc, sample(point, pressure)).unwrap();
    }
    stroke.finish(&mut doc).unwrap();
    assert!(
        alpha(&doc, 30, 62) > 90,
        "Later pressure must not rewrite the starting dab"
    );
    assert!(alpha(&doc, 150, 50) > 90);
    assert_eq!(alpha(&doc, 150, 60), 0);
    assert!(
        doc.layers[0]
            .raster()
            .unwrap()
            .pixels()
            .all(|p| p[3] <= 103)
    );
}
