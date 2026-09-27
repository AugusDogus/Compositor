use compositor::{
    document::{Document, Shape, ShapeGeometry},
    edits,
    filters::{self, Filter},
    text,
};
use image::{Rgba, RgbaImage};

#[test]
fn filters_that_leave_pixels_unchanged_preserve_editable_content() {
    let mut shape = Document::new(16, 16).unwrap();
    edits::shape(
        &mut shape,
        [2., 2.],
        [14., 14.],
        Shape {
            geometry: ShapeGeometry::Rectangle,
            red: 0.3,
            green: 0.5,
            blue: 0.7,
            corner_radius: 0.,
        },
    )
    .unwrap();
    let mut text = Document::new(16, 16).unwrap();
    text.add(
        text::new_layer(
            text::Text::default(),
            RgbaImage::from_pixel(8, 8, Rgba([100, 120, 140, 255])),
            [2., 2.],
        )
        .unwrap(),
    )
    .unwrap();
    for original in [shape, text] {
        for filter in [
            Filter::UnsharpMask {
                amount: 0.,
                radius: 2.,
                threshold: 0.,
            },
            Filter::UnsharpMask {
                amount: 100.,
                radius: 2.,
                threshold: 255.,
            },
            Filter::Bloom {
                amount: 0.,
                radius: 2.,
            },
        ] {
            let mut doc = original.clone();
            filters::apply(&mut doc, filter, false).unwrap();
            assert_eq!(doc, original);
        }
    }
}
