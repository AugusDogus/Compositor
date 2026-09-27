use compositor::{
    document::{Document, Mask},
    edits,
    gradient::Gradient,
    selection::Selection,
    text,
};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use std::sync::Arc;

fn document() -> Document {
    let mut doc = Document::new(32, 32).unwrap();
    doc.add(
        text::new_layer(
            text::Text::default(),
            RgbaImage::from_pixel(8, 8, Rgba([100, 120, 140, 0])),
            [4., 4.],
        )
        .unwrap(),
    )
    .unwrap();
    doc
}

#[test]
fn empty_selection_preserves_editable_content_extent_and_hidden_rgb() {
    for gradient in [false, true] {
        let mut doc = document();
        doc.selection = Some(Selection::rectangle(32, 32, [0., 0.], [0., 0.], false));
        let before = doc.clone();
        if gradient {
            Gradient::default()
                .apply(
                    &mut doc,
                    [0., 0.],
                    [32., 32.],
                    [255, 0, 0, 255],
                    [0; 4],
                    false,
                )
                .unwrap();
        } else {
            edits::fill(&mut doc, [255, 0, 0, 255], false, false).unwrap();
        }
        assert_eq!(doc, before);
    }
}

#[test]
fn transparent_fill_preserves_blank_layers_and_editable_text() {
    for mut doc in [document(), Document::new(32, 32).unwrap()] {
        // A selection requests a raster fill rather than changing text color.
        doc.selection = Some(Selection::rectangle(32, 32, [0., 0.], [32., 32.], false));
        let before = doc.clone();
        edits::fill(&mut doc, [50, 60, 70, 0], false, false).unwrap();
        assert_eq!(doc, before);
    }
}

#[test]
fn ineffective_mask_fill_and_gradient_preserve_uniform_mask() {
    for gradient in [false, true] {
        let mut doc = document();
        doc.active_layer_mut().unwrap().mask = Some(Mask {
            pixels: Arc::new(GrayImage::from_pixel(1, 1, Luma([255]))),
            enabled: true,
            linked: true,
            placement: None,
        });
        doc.selection = Some(Selection::rectangle(32, 32, [0., 0.], [32., 32.], false));
        let before = doc.clone();
        if gradient {
            Gradient {
                opacity: 0.,
                ..Gradient::default()
            }
            .apply(&mut doc, [0., 0.], [32., 32.], [0, 0, 0, 255], [0; 4], true)
            .unwrap();
        } else {
            edits::fill(&mut doc, [255; 4], false, true).unwrap();
        }
        assert_eq!(doc, before);
    }
}

#[test]
fn actual_selected_fill_rasterizes_text_without_touching_excluded_hidden_rgb() {
    for gradient in [false, true] {
        let mut doc = document();
        doc.selection = Some(Selection::rectangle(32, 32, [6., 6.], [8., 8.], false));
        if gradient {
            Gradient::default()
                .apply(
                    &mut doc,
                    [0., 0.],
                    [32., 32.],
                    [255, 0, 0, 255],
                    [0; 4],
                    false,
                )
                .unwrap();
        } else {
            edits::fill(&mut doc, [255, 0, 0, 255], false, false).unwrap();
        }
        let layer = doc.active_layer().unwrap();
        assert!(layer.text.is_none());
        let pixels = layer.raster().unwrap();
        assert_eq!(pixels[(4, 4)], Rgba([100, 120, 140, 0]));
        assert!(pixels[(6, 6)][3] > 0);
        assert_eq!(pixels[(6, 6)][0], 255);
    }
}

#[test]
fn transparent_fill_and_zero_opacity_gradient_skip_large_canvas_allocation() {
    let mut doc = Document::new(30_000, 30_000).unwrap();
    let before = doc.clone();
    edits::fill(&mut doc, [255, 0, 0, 0], false, false).unwrap();
    Gradient {
        opacity: 0.,
        ..Gradient::default()
    }
    .apply(
        &mut doc,
        [0., 0.],
        [30_000., 30_000.],
        [255; 4],
        [0; 4],
        false,
    )
    .unwrap();
    assert_eq!(doc, before);
}
