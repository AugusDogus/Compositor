use compositor::{
    brush::{Brush, PaintMode, Stroke},
    document::Document,
    filters::Healing,
    selection::Selection,
    text,
};
use image::{Rgba, RgbaImage};

fn document() -> Document {
    let mut doc = Document::new(32, 32).unwrap();
    doc.add(
        text::new_layer(
            text::Text::default(),
            RgbaImage::from_pixel(16, 16, Rgba([100, 120, 140, 0])),
            [4., 4.],
        )
        .unwrap(),
    )
    .unwrap();
    doc
}

fn stroke(doc: &mut Document, point: [f64; 2], brush: Brush, mode: PaintMode) {
    let mut stroke = Stroke::start(doc, point, brush, mode, false, false).unwrap();
    stroke.finish(doc).unwrap();
}

#[test]
fn excluded_strokes_preserve_editable_content_and_hidden_rgb() {
    for mode in [
        PaintMode::Paint,
        PaintMode::Erase,
        PaintMode::Blur,
        PaintMode::Heal(Healing::Proximity),
    ] {
        let mut doc = document();
        doc.selection = Some(Selection::rectangle(32, 32, [25., 25.], [32., 32.], false));
        let original = doc.clone();
        stroke(
            &mut doc,
            [10., 10.],
            Brush {
                diameter: 4.,
                ..Brush::default()
            },
            mode,
        );
        assert_eq!(doc, original, "{mode:?}");
    }
}

#[test]
fn zero_opacity_strokes_preserve_editable_content_and_hidden_rgb() {
    for mode in [
        PaintMode::Paint,
        PaintMode::Erase,
        PaintMode::Blur,
        PaintMode::Heal(Healing::Proximity),
    ] {
        let mut doc = document();
        let original = doc.clone();
        stroke(
            &mut doc,
            [10., 10.],
            Brush {
                diameter: 4.,
                opacity: 0.,
                ..Brush::default()
            },
            mode,
        );
        assert_eq!(doc, original, "{mode:?}");
    }
}

#[test]
fn excluded_strokes_do_not_expand_the_source() {
    let mut doc = document();
    doc.selection = Some(Selection::rectangle(32, 32, [4., 4.], [8., 8.], false));
    let original = doc.clone();
    stroke(
        &mut doc,
        [28., 28.],
        Brush {
            diameter: 4.,
            ..Brush::default()
        },
        PaintMode::Paint,
    );
    assert_eq!(doc, original);
}

#[test]
fn transparent_paint_and_unchanged_erase_preserve_editable_content() {
    for mode in [PaintMode::Paint, PaintMode::Erase] {
        let mut doc = document();
        let original = doc.clone();
        stroke(
            &mut doc,
            [10., 10.],
            Brush {
                diameter: 4.,
                color: [20, 30, 40, 0],
                ..Brush::default()
            },
            mode,
        );
        assert_eq!(doc, original, "{mode:?}");
    }
}

#[test]
fn healing_an_unchanged_area_preserves_editable_text() {
    let mut doc = document();
    doc.active_layer_mut().unwrap().content = compositor::document::LayerContent::Raster(Some(
        std::sync::Arc::new(RgbaImage::from_pixel(16, 16, Rgba([100, 120, 140, 255]))),
    ));
    let original = doc.clone();
    stroke(
        &mut doc,
        [10., 10.],
        Brush {
            diameter: 4.,
            ..Brush::default()
        },
        PaintMode::Heal(Healing::Proximity),
    );
    assert_eq!(doc, original);
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn gpu_no_op_strokes_preserve_editable_text() {
    compositor::brush::initialize_gpu().unwrap();
    for (mode, color) in [
        (PaintMode::Erase, [100, 120, 140, 0]),
        (PaintMode::Paint, [100, 120, 140, 255]),
    ] {
        let mut doc = Document::new(600, 600).unwrap();
        doc.add(
            text::new_layer(
                text::Text::default(),
                RgbaImage::from_pixel(600, 600, Rgba(color)),
                [0., 0.],
            )
            .unwrap(),
        )
        .unwrap();
        let original = doc.clone();
        stroke(
            &mut doc,
            [300., 300.],
            Brush {
                diameter: 520.,
                color,
                ..Brush::default()
            },
            mode,
        );
        assert_eq!(doc, original, "{mode:?}");
    }
}

#[test]
fn selection_keeps_hard_brush_antialias_fringe_on_fractional_layer() {
    let mut doc = document();
    let layer = doc.active_layer_mut().unwrap();
    layer.transform.origin = [0.6, 0.];
    layer.content = compositor::document::LayerContent::Raster(Some(std::sync::Arc::new(
        RgbaImage::new(16, 16),
    )));
    doc.selection = Some(Selection::rectangle(32, 32, [11., 0.], [12., 32.], false));
    // The selected pixel center is x=11.1. It is 0.9px from a 0.5px-radius
    // hard tip, inside its 0.5px antialias fringe despite lying outside the disk.
    stroke(
        &mut doc,
        [10.2, 10.5],
        Brush {
            diameter: 1.,
            hardness: 1.,
            color: [255, 0, 0, 255],
            ..Brush::default()
        },
        PaintMode::Paint,
    );
    let pixels = doc.active_layer().unwrap().raster().unwrap();
    assert!(
        pixels[(10, 10)][3] > 0,
        "The selected antialias fringe must remain paintable"
    );
    assert_eq!(
        pixels[(9, 10)][3],
        0,
        "Selection still excludes the brush center"
    );
}

#[test]
fn zero_opacity_or_excluded_strokes_do_not_materialize_blank_layers() {
    for excluded in [false, true] {
        let mut doc = Document::new(32, 32).unwrap();
        if excluded {
            doc.selection = Some(Selection::rectangle(32, 32, [25., 25.], [32., 32.], false));
        }
        let original = doc.clone();
        stroke(
            &mut doc,
            [10., 10.],
            Brush {
                diameter: 4.,
                opacity: if excluded { 1. } else { 0. },
                ..Brush::default()
            },
            PaintMode::Paint,
        );
        assert_eq!(doc, original, "excluded={excluded}");
    }
}

#[test]
fn zero_opacity_or_excluded_strokes_do_not_expand_uniform_masks() {
    for excluded in [false, true] {
        let mut doc = document();
        doc.active_layer_mut().unwrap().mask = Some(compositor::document::Mask {
            pixels: std::sync::Arc::new(image::GrayImage::from_pixel(1, 1, image::Luma([255]))),
            enabled: true,
            linked: true,
            placement: None,
        });
        if excluded {
            doc.selection = Some(Selection::rectangle(32, 32, [25., 25.], [32., 32.], false));
        }
        let original = doc.clone();
        let mut brush = Stroke::start(
            &mut doc,
            [10., 10.],
            Brush {
                diameter: 4.,
                opacity: if excluded { 1. } else { 0. },
                ..Brush::default()
            },
            PaintMode::Paint,
            true,
            false,
        )
        .unwrap();
        brush.finish(&mut doc).unwrap();
        assert_eq!(doc, original, "excluded={excluded}");
    }
}

#[test]
fn stroke_inside_selection_hole_restores_source_extent() {
    let mut doc = document();
    doc.selection = Some(
        Selection::rasterize([0., 0., 32., 32.], |point| {
            if (24. ..32.).contains(&point[0]) && (24. ..32.).contains(&point[1]) {
                0.
            } else {
                1.
            }
        })
        .unwrap(),
    );
    let original = doc.clone();
    stroke(
        &mut doc,
        [28., 28.],
        Brush {
            diameter: 4.,
            ..Brush::default()
        },
        PaintMode::Paint,
    );
    assert_eq!(doc, original);
}

#[test]
fn provisional_chord_alone_does_not_commit_a_raster_or_mask_edit() {
    for target in ["blank", "text", "mask"] {
        let mut doc = Document::new(300, 120).unwrap();
        if target == "text" {
            doc.add(
                text::new_layer(
                    text::Text::default(),
                    RgbaImage::from_pixel(300, 120, Rgba([100, 120, 140, 0])),
                    [0., 0.],
                )
                .unwrap(),
            )
            .unwrap();
        }
        if target == "mask" {
            doc.active_layer_mut().unwrap().mask = Some(compositor::document::Mask {
                pixels: std::sync::Arc::new(image::GrayImage::from_pixel(1, 1, image::Luma([255]))),
                enabled: true,
                linked: true,
                placement: None,
            });
        }
        doc.selection = Some(Selection::rectangle(
            300,
            120,
            [215., 40.],
            [216., 41.],
            false,
        ));
        let original = doc.clone();
        let mut stroke = Stroke::start(
            &mut doc,
            [20., 60.],
            Brush {
                diameter: 8.,
                ..Brush::default()
            },
            PaintMode::Paint,
            target == "mask",
            false,
        )
        .unwrap();
        stroke.to(&mut doc, [150., 20.]).unwrap();
        stroke.to(&mut doc, [280., 60.]).unwrap();
        let layer = doc.active_layer().unwrap();
        if target == "mask" {
            assert_eq!(layer.mask.as_ref().unwrap().pixels[(215, 40)][0], 0);
        } else {
            assert_eq!(layer.raster().unwrap()[(215, 40)][3], 255);
        }
        stroke.finish(&mut doc).unwrap();
        assert!(
            doc == original,
            "Provisional {target} edit must be discarded"
        );
    }
}
