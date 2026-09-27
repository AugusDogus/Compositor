use ag_psd::psd as ps;
use compositor::psd;

fn linear_rgb() -> Vec<u8> {
    let xy = |x, y| lcms2::CIExyY { x, y, Y: 1. };
    let curve = lcms2::ToneCurve::new(1.);
    lcms2::Profile::new_rgb(
        &xy(0.3127, 0.3290),
        &lcms2::CIExyYTRIPLE {
            Red: xy(0.64, 0.33),
            Green: xy(0.30, 0.60),
            Blue: xy(0.15, 0.06),
        },
        &[&curve, &curve, &curve],
    )
    .unwrap()
    .icc()
    .unwrap()
}

fn with_profile(bytes: &[u8], profile: &[u8]) -> Vec<u8> {
    let offset = 30 + u32::from_be_bytes(bytes[26..30].try_into().unwrap()) as usize;
    let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let mut resource = b"8BIM".to_vec();
    resource.extend_from_slice(&1039u16.to_be_bytes());
    resource.extend_from_slice(&[0, 0]);
    resource.extend_from_slice(&(profile.len() as u32).to_be_bytes());
    resource.extend_from_slice(profile);
    if !profile.len().is_multiple_of(2) {
        resource.push(0);
    }
    let mut result = bytes[..offset].to_vec();
    result.extend_from_slice(&(length + resource.len() as u32).to_be_bytes());
    result.extend_from_slice(&resource);
    result.extend_from_slice(&bytes[offset + 4..]);
    result
}

fn pixels() -> ps::PixelData {
    ps::PixelData {
        width: 1,
        height: 1,
        data: vec![64, 64, 64, 128],
    }
}

#[test]
fn embedded_profile_converts_editable_effect_colors() {
    use compositor::{
        document::{Document, LayerContent},
        effects::{ColorOverlayEffect, LayerEffects},
    };
    let mut doc = Document::new(1, 1).unwrap();
    doc.layers[0].content = LayerContent::Raster(Some(std::sync::Arc::new(
        image::RgbaImage::from_pixel(1, 1, image::Rgba([64, 64, 64, 255])),
    )));
    doc.layers[0].effects = Some(LayerEffects {
        color_overlay: Some(ColorOverlayEffect {
            red: 64. / 255.,
            green: 64. / 255.,
            blue: 64. / 255.,
            ..Default::default()
        }),
        ..Default::default()
    });
    let bytes = with_profile(&psd::encode(&doc).unwrap(), &linear_rgb());
    let imported = psd::decode(&bytes).unwrap().document;
    let color = imported.layers[0]
        .effects
        .as_ref()
        .unwrap()
        .color_overlay
        .as_ref()
        .unwrap();
    for channel in [color.red, color.green, color.blue] {
        assert!((channel * 255. - 137.).abs() <= 1.);
    }
    assert_eq!(color.opacity, 1.);
}

#[test]
fn embedded_profile_converts_layer_and_flattened_pixels_in_psd_and_psb() {
    // Linear-light 64/255 becomes about 137/255 in sRGB; alpha and masks are coverage.
    for large in [false, true] {
        for layered in [false, true] {
            let mut composite = pixels();
            composite.data[3] = 255;
            let bytes = ag_psd::write_psd(
                &ps::Psd {
                    width: 1.,
                    height: 1.,
                    image_data: Some(composite),
                    children: layered.then(|| {
                        vec![ps::Layer {
                            image_data: Some(pixels()),
                            additional_info: ps::LayerAdditionalInfo {
                                mask: Some(ps::LayerMaskData {
                                    image_data: Some(pixels()),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            },
                            ..Default::default()
                        }]
                    }),
                    ..Default::default()
                },
                &ps::WriteOptions {
                    psb: Some(large),
                    no_background: Some(true),
                    ..Default::default()
                },
            );
            let imported = psd::decode(&with_profile(&bytes, &linear_rgb())).unwrap();
            let layer = &imported.document.layers[0];
            let pixel = layer.raster().unwrap()[(0, 0)];
            assert!((136..=138).contains(&pixel[0]), "{pixel:?}");
            assert_eq!(pixel[3], if layered { 128 } else { 255 });
            if layered {
                assert_eq!(layer.mask.as_ref().unwrap().pixels[(1, 1)][0], 64);
            }
        }
    }
}

#[test]
fn damaged_embedded_profile_is_rejected_instead_of_silently_reinterpreted() {
    let bytes = ag_psd::write_psd(
        &ps::Psd {
            width: 1.,
            height: 1.,
            image_data: Some(pixels()),
            ..Default::default()
        },
        &Default::default(),
    );
    assert!(psd::decode(&with_profile(&bytes, b"damaged profile")).is_err());
    let gray = lcms2::Profile::new_gray(lcms2::CIExyY::d50(), &lcms2::ToneCurve::new(1.))
        .unwrap()
        .icc()
        .unwrap();
    assert!(psd::decode(&with_profile(&bytes, &gray)).is_err());
    assert!(
        psd::decode(&with_profile(
            &with_profile(&bytes, &linear_rgb()),
            &linear_rgb()
        ))
        .is_err()
    );
}

#[test]
fn grayscale_profile_converts_tone_without_colorizing_masks() {
    let mut bytes = b"8BPS".to_vec();
    bytes.extend_from_slice(&1u16.to_be_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u16.to_be_bytes());
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&8u16.to_be_bytes());
    bytes.extend_from_slice(&1u16.to_be_bytes());
    bytes.extend_from_slice(&[0; 12]);
    bytes.extend_from_slice(&0u16.to_be_bytes());
    bytes.push(64);
    let profile = lcms2::Profile::new_gray(lcms2::CIExyY::d50(), &lcms2::ToneCurve::new(1.))
        .unwrap()
        .icc()
        .unwrap();
    let imported = psd::decode(&with_profile(&bytes, &profile)).unwrap();
    let pixel = imported.document.layers[0].raster().unwrap()[(0, 0)];
    assert!((136..=138).contains(&pixel[0]), "{pixel:?}");
    assert_eq!(pixel[3], 255);
}

#[test]
fn editable_text_color_stays_converted_after_native_save_and_retyping() {
    let bytes = ag_psd::write_psd(
        &ps::Psd {
            width: 100.,
            height: 100.,
            children: Some(vec![ps::Layer {
                additional_info: ps::LayerAdditionalInfo {
                    text: Some(ps::LayerTextData {
                        text: "ICC".into(),
                        transform: Some(vec![1., 0., 0., 1., 0., 30.]),
                        style: Some(ps::TextStyle {
                            font: Some(ps::Font {
                                name: "Inter Variable".into(),
                                ..Default::default()
                            }),
                            font_size: Some(20.),
                            fill_color: Some(ps::Color::Rgb(ps::Rgb {
                                r: 64.,
                                g: 64.,
                                b: 64.,
                            })),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                ..Default::default()
            }]),
            ..Default::default()
        },
        &Default::default(),
    );
    let doc = psd::decode(&with_profile(&bytes, &linear_rgb()))
        .unwrap()
        .document;
    let text = doc.layers[0].text.as_ref().unwrap();
    assert!((136. / 255. ..=138. / 255.).contains(&text.red));
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("converted.comp");
    compositor::project::save(&doc, &path).unwrap();
    let reopened = compositor::project::load(&path).unwrap();
    let text = reopened.layers[0].text.as_ref().unwrap();
    let pixels = compositor::text::TextRenderer::default()
        .render(text)
        .unwrap();
    let solid = pixels.pixels().find(|pixel| pixel[3] == 255).unwrap();
    assert!((136..=138).contains(&solid[0]));
}

#[test]
fn editable_shape_fill_and_cached_pixels_use_the_same_converted_color() {
    let bytes = ag_psd::write_psd(
        &ps::Psd {
            width: 20.,
            height: 20.,
            children: Some(vec![ps::Layer {
                additional_info: ps::LayerAdditionalInfo {
                    vector_fill: Some(ps::VectorContent::Color(ps::Color::Rgb(ps::Rgb {
                        r: 64.,
                        g: 64.,
                        b: 64.,
                    }))),
                    vector_mask: Some(ps::LayerVectorMask {
                        paths: vec![ps::BezierPath {
                            open: false,
                            operation: Some(ps::BooleanOperation::Combine),
                            fill_rule: ps::FillRule::NonZero,
                            knots: [[0., 0.], [0.5, 0.], [0.5, 0.5], [0., 0.5]]
                                .map(|[x, y]| ps::BezierKnot {
                                    linked: false,
                                    points: vec![x, y, x, y, x, y],
                                })
                                .to_vec(),
                        }],
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                ..Default::default()
            }]),
            ..Default::default()
        },
        &Default::default(),
    );
    let imported = psd::decode(&with_profile(&bytes, &linear_rgb())).unwrap();
    let layer = &imported.document.layers[0];
    let shape = layer.shape.unwrap();
    assert!((136. / 255. ..=138. / 255.).contains(&shape.red));
    assert_eq!(
        layer.raster().unwrap()[(5, 5)][0],
        (shape.red * 255.).round() as u8
    );
}
