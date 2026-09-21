//! Known-pixel regressions for upstream 1.2.0 fixes, using Linux's straight RGBA storage.
use compositor::{
    adjustment::{Adjustment, Kind},
    blend::Blend,
    document::{Document, Layer, LayerContent},
    effects::{ColorOverlayEffect, LayerEffects, StrokeEffect},
    pixel_adjustment, project, render,
};
use image::{Rgba, RgbaImage};
use std::sync::Arc;

fn raster(name: &str, pixels: RgbaImage) -> Layer {
    let mut layer = Layer::blank(name, pixels.width(), pixels.height());
    layer.content = LayerContent::Raster(Some(Arc::new(pixels)));
    layer
}

#[test]
fn dodge_and_burn_render_known_srgb_values() {
    // These are the 40% backdrop / 80% source values from upstream's regression.
    let mut doc = Document::new(1, 1).unwrap();
    doc.layers[0] = raster(
        "Bottom",
        RgbaImage::from_pixel(1, 1, Rgba([102, 102, 102, 255])),
    );
    doc.add(raster(
        "Top",
        RgbaImage::from_pixel(1, 1, Rgba([204, 204, 204, 255])),
    ))
    .unwrap();
    for (blend, full, half) in [(Blend::ColorDodge, 255, 179), (Blend::ColorBurn, 64, 83)] {
        doc.layers[1].blend = blend;
        for (opacity, expected) in [(1., full), (0.5, half)] {
            doc.layers[1].opacity = opacity;
            assert_eq!(
                render::render(&doc, 1, 1).unwrap()[(0, 0)],
                Rgba([expected, expected, expected, 255]),
                "{blend:?}, {opacity}"
            );
        }
    }
}

#[test]
fn levels_adjusts_color_once_at_every_alpha() {
    let alphas = [0, 1, 64, 128, 254, 255];
    let pixels = RgbaImage::from_fn(alphas.len() as u32, 1, |x, _| {
        Rgba([128, 64, 0, alphas[x as usize]])
    });
    let mut layer = raster("Soft edge", pixels.clone());
    let mut levels = Adjustment::new(Kind::Levels);
    levels.levels.ranges[0].output_black = 255.;
    levels.levels.ranges[0].output_white = 0.;
    pixel_adjustment::apply(&mut layer, &levels, None).unwrap();
    for (x, alpha) in alphas.into_iter().enumerate() {
        let expected = if alpha == 0 {
            [128, 64, 0, 0]
        } else {
            [127, 191, 255, alpha]
        };
        assert_eq!(layer.raster().unwrap()[(x as u32, 0)], Rgba(expected));
    }

    let mut doc = Document::new(alphas.len() as u32, 1).unwrap();
    doc.layers[0] = raster("Soft edge", pixels);
    let mut adjustment = Layer::blank("Levels", doc.width, doc.height);
    adjustment.content = LayerContent::Adjustment(Box::new(levels));
    doc.add(adjustment).unwrap();
    let result = render::render(&doc, doc.width, doc.height).unwrap();
    for (x, alpha) in alphas.into_iter().enumerate().skip(1) {
        assert_eq!(result[(x as u32, 0)], Rgba([127, 191, 255, alpha]));
    }
}

#[test]
fn hiding_one_effect_preserves_siblings_and_project_snapshot() {
    let mut doc = Document::new(9, 9).unwrap();
    doc.layers[0] = raster(
        "Effects",
        RgbaImage::from_pixel(3, 3, Rgba([0, 0, 255, 255])),
    );
    doc.layers[0].transform.origin = [3., 3.];
    doc.active = Some(doc.layers[0].id);
    doc.selected = std::collections::HashSet::from([doc.layers[0].id]);
    doc.layers[0].effects = Some(LayerEffects {
        stroke: Some(StrokeEffect {
            size: 1.,
            red: 1.,
            ..Default::default()
        }),
        color_overlay: Some(ColorOverlayEffect {
            green: 1.,
            ..Default::default()
        }),
        ..Default::default()
    });
    let before = render::render(&doc, 9, 9).unwrap();
    assert_eq!(before[(2, 4)], Rgba([255, 0, 0, 255]));
    assert_eq!(before[(4, 4)], Rgba([0, 255, 0, 255]));
    doc.layers[0]
        .effects
        .as_mut()
        .unwrap()
        .stroke
        .as_mut()
        .unwrap()
        .enabled = Some(false);
    let hidden = render::render(&doc, 9, 9).unwrap();
    assert_eq!(hidden[(2, 4)][3], 0);
    assert_eq!(hidden[(4, 4)], before[(4, 4)]);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("effects.comp");
    project::save(&doc, &path).unwrap();
    let reopened = project::load(&path).unwrap();
    assert_eq!(reopened.layers[0].effects, doc.layers[0].effects);
    assert_eq!(render::render(&reopened, 9, 9).unwrap(), hidden);
}
