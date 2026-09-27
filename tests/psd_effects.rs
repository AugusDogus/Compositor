use ag_psd::psd as ps;
use compositor::{
    document::{Document, Layer, LayerContent, Mask},
    effects::*,
    psd, render,
};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use std::sync::Arc;

fn document(effects: LayerEffects) -> Document {
    let mut doc = Document::new(40, 36).unwrap();
    doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        40,
        36,
        Rgba([30, 40, 50, 255]),
    ))));
    let mut layer = Layer::blank("Styled", 9, 7);
    layer.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(9, 7, |x, y| {
        Rgba([80, 120, 180, if x < 2 || y < 1 { 0 } else { 200 }])
    }))));
    layer.transform.origin = [14., 12.];
    layer.opacity = 0.8;
    layer.effects = Some(effects);
    doc.add(layer).unwrap();
    doc
}
fn effects() -> LayerEffects {
    LayerEffects {
        stroke: Some(StrokeEffect {
            size: 2.,
            red: 1.,
            green: 0.2,
            blue: 0.1,
            opacity: 0.6,
            ..Default::default()
        }),
        shadow: Some(ShadowEffect {
            angle: 90.,
            distance: 3.,
            blur: 1.,
            opacity: 0.5,
            ..Default::default()
        }),
        inner_shadow: Some(ShadowEffect {
            angle: 90.,
            distance: 1.,
            blur: 1.,
            opacity: 0.5,
            ..Default::default()
        }),
        color_overlay: Some(ColorOverlayEffect {
            red: 0.8,
            green: 0.2,
            blue: 0.1,
            opacity: 0.4,
            ..Default::default()
        }),
        outer_glow: Some(OuterGlowEffect {
            size: 2.,
            opacity: 0.5,
            ..Default::default()
        }),
        inner_glow: Some(InnerGlowEffect {
            size: 2.,
            opacity: 0.5,
            ..Default::default()
        }),
    }
}
fn parsed(bytes: &[u8]) -> ps::Psd {
    ag_psd::read_psd(
        bytes,
        &ps::ReadOptions {
            use_image_data: Some(true),
            strict: Some(true),
            ..Default::default()
        },
    )
    .unwrap()
}
fn assert_render_close(a: &Document, b: &Document) {
    let a = render::render(a, a.width, a.height).unwrap();
    let b = render::render(b, b.width, b.height).unwrap();
    let maximum = a
        .as_raw()
        .iter()
        .zip(b.as_raw())
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap();
    assert!(maximum <= 1, "maximum channel error {maximum}");
}

#[test]
fn styles_are_written_as_editable_descriptors_and_render_after_reimport() {
    let all = effects();
    for effects in [
        LayerEffects {
            stroke: all.stroke.clone(),
            ..Default::default()
        },
        LayerEffects {
            shadow: all.shadow.clone(),
            ..Default::default()
        },
        LayerEffects {
            inner_shadow: all.inner_shadow.clone(),
            ..Default::default()
        },
        LayerEffects {
            color_overlay: all.color_overlay.clone(),
            ..Default::default()
        },
        LayerEffects {
            outer_glow: all.outer_glow.clone(),
            ..Default::default()
        },
        LayerEffects {
            inner_glow: all.inner_glow.clone(),
            ..Default::default()
        },
        all,
    ] {
        let doc = document(effects);
        let before = doc.clone();
        let bytes = psd::encode(&doc).unwrap();
        assert!(bytes.windows(4).any(|v| v == b"lfx2"));
        let decoded = parsed(&bytes);
        let layer = &decoded.children.as_ref().unwrap()[1];
        assert!(layer.additional_info.effects.is_some());
        assert_eq!([layer.left.unwrap(), layer.top.unwrap()], [14., 12.]);
        // Export renders the layer's placement; RGB beneath zero alpha is immaterial.
        for (exported, source) in layer
            .image_data
            .as_ref()
            .unwrap()
            .data
            .chunks_exact(4)
            .zip(doc.layers[1].raster().unwrap().pixels())
        {
            assert_eq!(exported[3], source[3]);
            if source[3] != 0 {
                assert_eq!(exported, source.0);
            }
        }
        assert_eq!(
            decoded.image_data.unwrap().data,
            render::render(&doc, 40, 36).unwrap().into_raw()
        );
        let imported = psd::decode(&bytes).unwrap();
        assert!(imported.document.layers[1].effects.is_some());
        assert!(
            !imported
                .report
                .description()
                .contains("effects are omitted")
        );
        assert_render_close(&doc, &imported.document);
        assert_eq!(doc, before);
    }
}

#[test]
fn transformed_style_metrics_and_disabled_styles_remain_editable() {
    let mut doc = document(effects());
    let layer = &mut doc.layers[1];
    layer.transform.size = [18., 14.];
    layer.transform.rotation = 90.;
    layer.transform.flip_x = true;
    layer
        .effects
        .as_mut()
        .unwrap()
        .shadow
        .as_mut()
        .unwrap()
        .angle = 30.;
    layer
        .effects
        .as_mut()
        .unwrap()
        .color_overlay
        .as_mut()
        .unwrap()
        .enabled = Some(false);
    let bytes = psd::encode(&doc).unwrap();
    let parsed = parsed(&bytes);
    let styles = parsed.children.unwrap()[1]
        .additional_info
        .effects
        .clone()
        .unwrap();
    assert_eq!(styles.stroke.as_ref().unwrap()[0].size.unwrap().value, 4.);
    let shadow = &styles.drop_shadow.as_ref().unwrap()[0];
    assert_eq!(shadow.distance.unwrap().value, 6.);
    assert_eq!(shadow.size.unwrap().value, 2.);
    assert!((shadow.angle.unwrap() - 60.).abs() < 0.00001);
    assert_eq!(shadow.use_global_light, Some(false));
    assert_eq!(styles.solid_fill.unwrap()[0].enabled, Some(false));
    let imported = psd::decode(&bytes).unwrap().document;
    assert_eq!(
        imported.layers[1]
            .effects
            .as_ref()
            .unwrap()
            .color_overlay
            .as_ref()
            .unwrap()
            .enabled,
        Some(false)
    );
}

#[test]
fn mask_nonuniform_scale_blending_and_clipping_use_explicit_baked_fallback() {
    for mode in 0..5 {
        let mut doc = document(effects());
        match mode {
            0 => {
                doc.layers[1].mask = Some(Mask {
                    pixels: Arc::new(GrayImage::from_pixel(9, 7, Luma([128]))),
                    placement: None,
                    enabled: true,
                    linked: true,
                })
            }
            1 => doc.layers[1].transform.size[0] *= 1.5,
            2 => doc.layers[1].blend = compositor::blend::Blend::Multiply,
            3 => doc.layers[1].clip_source = Some(doc.layers[0].id),
            _ => {
                let mut clipped = Layer::blank("Clipped", 40, 36);
                clipped.clip_source = Some(doc.layers[1].id);
                clipped.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
                    40,
                    36,
                    Rgba([150, 80, 10, 128]),
                ))));
                doc.add(clipped).unwrap();
            }
        }
        assert!(
            psd::export_report(&doc)
                .description()
                .contains("baked into pixels because")
        );
        let bytes = psd::encode(&doc).unwrap();
        assert!(
            parsed(&bytes).children.unwrap()[1]
                .additional_info
                .effects
                .is_none()
        );
        assert_render_close(&doc, &psd::decode(&bytes).unwrap().document);
    }
}

#[test]
fn unsupported_style_settings_are_reported_instead_of_silently_remapped() {
    let doc = document(effects());
    let mut psd_file = parsed(&psd::encode(&doc).unwrap());
    psd_file.children.as_mut().unwrap()[1]
        .additional_info
        .effects
        .as_mut()
        .unwrap()
        .stroke
        .as_mut()
        .unwrap()[0]
        .position = Some(ps::StrokePosition::Center);
    let bytes = ag_psd::write_psd(
        &psd_file,
        &ps::WriteOptions {
            no_background: Some(true),
            ..Default::default()
        },
    );
    let imported = psd::decode(&bytes).unwrap();
    assert!(imported.document.layers[1].effects.is_none());
    assert!(
        imported
            .report
            .description()
            .contains("only solid inside or outside strokes")
    );
}
