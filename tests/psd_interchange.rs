use compositor::{
    blend::Blend,
    document::{Document, Layer, LayerContent, Mask},
    geometry::Transform,
    psd, render,
};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use std::sync::Arc;
fn fixture() -> Document {
    let mut doc = Document::new(4, 3).unwrap();
    doc.resolution = 300.;
    doc.guides.push(compositor::guides::Guide {
        id: uuid::Uuid::new_v4(),
        axis: compositor::guides::Axis::Vertical,
        position: 1.5,
    });
    doc.layers[0].name = "Bottom".into();
    doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        4,
        3,
        Rgba([20, 80, 160, 255]),
    ))));
    let mut folder = Layer::blank("Folder", 4, 3);
    folder.content = LayerContent::Group;
    folder.opacity = 0.7;
    let parent = folder.id;
    doc.add(folder).unwrap();
    let mut top = Layer::blank("Top λ", 2, 2);
    top.parent = Some(parent);
    top.opacity = 0.8;
    top.blend = Blend::Multiply;
    top.transform.origin = [1., 1.];
    top.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        2,
        2,
        Rgba([220, 40, 80, 255]),
    ))));
    top.mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(2, 2, Luma([128]))),
        enabled: true,
        linked: true,
        placement: None,
    });
    let base = top.id;
    doc.add(top).unwrap();
    let mut clipped = Layer::blank("Clipped", 2, 2);
    clipped.parent = Some(parent);
    clipped.clip_source = Some(base);
    clipped.visible = false;
    clipped.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        2,
        2,
        Rgba([10, 20, 30, 255]),
    ))));
    doc.add(clipped).unwrap();
    doc
}
#[test]
fn psd_roundtrip_preserves_stack_groups_clipping_and_pixels() {
    let doc = fixture();
    let bytes = psd::encode(&doc).unwrap();
    let imported = psd::decode(&bytes).unwrap();
    let copy = imported.document;
    assert_eq!(copy.resolution, 300.);
    assert_eq!(copy.guides[0].position, 1.5);
    assert_eq!(
        copy.layers
            .iter()
            .map(|l| l.name.as_str())
            .collect::<Vec<_>>(),
        ["Bottom", "Folder", "Top λ", "Clipped"]
    );
    assert_eq!(copy.layers[2].parent, Some(copy.layers[1].id));
    assert_eq!(copy.layers[3].clip_source, Some(copy.layers[2].id));
    assert!(!copy.layers[3].visible);
    assert_eq!(copy.layers[2].blend, Blend::Multiply);
    let a = render::render(&doc, 4, 3).unwrap();
    let b = render::render(&copy, 4, 3).unwrap();
    for (a, b) in a.as_raw().iter().zip(b.as_raw()) {
        assert!(a.abs_diff(*b) <= 2, "{a} versus {b}");
    }
}
#[test]
fn unsupported_headers_and_every_truncation_are_rejected() {
    let bytes = psd::encode(&Document::new(2, 2).unwrap()).unwrap();
    for (offset, value) in [(5, 2), (23, 16), (25, 4)] {
        let mut malformed = bytes.clone();
        malformed[offset] = value;
        assert!(psd::decode(&malformed).is_err());
    }
    for length in 0..bytes.len() {
        assert!(
            psd::decode(&bytes[..length]).is_err(),
            "accepted truncation at {length}/{}",
            bytes.len()
        );
    }
}

#[test]
fn unknown_photoshop_resources_are_skipped_without_relaxing_validation() {
    let mut doc = fixture();
    doc.resolution = 144.;
    let original = psd::encode(&doc).unwrap();
    let color_length = u32::from_be_bytes(original[26..30].try_into().unwrap()) as usize;
    let length_offset = 30 + color_length;
    let length = u32::from_be_bytes(
        original[length_offset..length_offset + 4]
            .try_into()
            .unwrap(),
    );
    let mut resource = b"8BIM".to_vec();
    resource.extend_from_slice(&1083u16.to_be_bytes()); // Photoshop print settings, unhandled by ag-psd
    resource.extend_from_slice(&[0, 0]); // padded empty Pascal name
    resource.extend_from_slice(&3u32.to_be_bytes());
    resource.extend_from_slice(&[1, 2, 3, 0]); // odd payload with padding
    let mut bytes = original[..length_offset].to_vec();
    bytes.extend_from_slice(&(length + resource.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&resource);
    bytes.extend_from_slice(&original[length_offset + 4..]);
    let imported = psd::decode(&bytes).unwrap().document;
    assert_eq!(imported.resolution, 144.);
    assert_eq!(imported.guides[0].position, 1.5);
    assert_eq!(imported.layers.len(), doc.layers.len());
    assert_eq!(
        render::render(&imported, 4, 3).unwrap(),
        render::render(&psd::decode(&original).unwrap().document, 4, 3).unwrap()
    );

    let mut bad_signature = bytes.clone();
    bad_signature[length_offset + 4] = b'!';
    assert!(psd::decode(&bad_signature).is_err());
    let mut bad_length = bytes.clone();
    bad_length[length_offset + 12..length_offset + 16].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(psd::decode(&bad_length).is_err());
    for length in 0..bytes.len() {
        assert!(
            psd::decode(&bytes[..length]).is_err(),
            "accepted truncation at {length}"
        );
    }
}
#[test]
fn canvas_and_layer_budgets_are_checked_before_channel_decode() {
    let bytes = psd::encode(&Document::new(2, 2).unwrap()).unwrap();
    let mut huge = bytes.clone();
    huge[14..18].copy_from_slice(&30_000u32.to_be_bytes());
    huge[18..22].copy_from_slice(&30_000u32.to_be_bytes());
    assert!(
        psd::decode(&huge)
            .err()
            .unwrap()
            .to_string()
            .contains("200 million")
    );
}
#[test]
fn rotated_pixels_are_baked_and_conversion_is_reported() {
    let mut doc = Document::new(4, 4).unwrap();
    doc.layers[0].transform = Transform::new(2, 1);
    doc.layers[0].transform.origin = [1., 1.];
    doc.layers[0].transform.rotation = 90.;
    doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        2,
        1,
        Rgba([200, 10, 0, 255]),
    ))));
    assert!(!psd::export_report(&doc).changes.is_empty());
    let imported = psd::decode(&psd::encode(&doc).unwrap()).unwrap();
    assert_eq!(imported.document.layers[0].transform.rotation, 0.);
}

#[test]
fn oversized_layer_bounds_are_rejected_before_decoding_pixels() {
    let mut bytes = psd::encode(&Document::new(2, 2).unwrap()).unwrap();
    fn length(bytes: &[u8], offset: usize) -> usize {
        u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize
    }
    let mut offset = 26;
    offset += 4 + length(&bytes, offset); // color mode data
    offset += 4 + length(&bytes, offset); // image resources
    offset += 4 + 4 + 2; // layer/mask length, layer-info length, layer count
    bytes[offset + 8..offset + 12].copy_from_slice(&30_000u32.to_be_bytes());
    bytes[offset + 12..offset + 16].copy_from_slice(&30_000u32.to_be_bytes());
    let error = psd::decode(&bytes).err().unwrap().to_string();
    assert!(error.contains("200 million"), "{error}");
}

#[test]
fn flattened_grayscale_psd_decodes_real_channel_values() {
    let mut bytes = b"8BPS".to_vec();
    bytes.extend_from_slice(&1u16.to_be_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u16.to_be_bytes());
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&8u16.to_be_bytes());
    bytes.extend_from_slice(&1u16.to_be_bytes());
    bytes.extend_from_slice(&[0; 12]);
    bytes.extend_from_slice(&0u16.to_be_bytes());
    bytes.extend_from_slice(&[40, 200]);
    let doc = psd::decode(&bytes).unwrap().document;
    assert_eq!(
        doc.layers[0].raster().unwrap().as_raw(),
        &[40, 40, 40, 255, 200, 200, 200, 255]
    );
}

#[test]
fn editable_text_effects_and_guides_survive_project_save_and_psd_reports_rasterization() {
    use compositor::{
        effects::{ColorOverlayEffect, LayerEffects},
        project,
        text::{Text, TextRenderer},
    };
    let mut document = Document::new(180, 100).unwrap();
    let text = Text {
        content: "Hello".into(),
        font_size: 20.,
        ..Text::default()
    };
    let raster = TextRenderer::default().render(&text).unwrap();
    document.layers[0].transform = Transform::new(raster.width(), raster.height());
    document.layers[0].content = LayerContent::Raster(Some(Arc::new(raster)));
    document.layers[0].text = Some(text);
    document.layers[0].effects = Some(LayerEffects {
        color_overlay: Some(ColorOverlayEffect {
            red: 1.,
            green: 0.,
            blue: 0.,
            ..Default::default()
        }),
        ..Default::default()
    });
    document.guides.push(compositor::guides::Guide {
        id: uuid::Uuid::new_v4(),
        axis: compositor::guides::Axis::Horizontal,
        position: 50.,
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Editable.comp");
    project::save(&document, &path).unwrap();
    let restored = project::load(&path).unwrap();
    assert_eq!(restored.layers, document.layers);
    assert_eq!(restored.guides, document.guides);
    let report = psd::export_report(&restored).description();
    assert!(report.contains("text is rasterized"));
    assert!(report.contains("effects"));
    let converted = psd::decode(&psd::encode(&restored).unwrap())
        .unwrap()
        .document;
    assert!(converted.layers[0].text.is_none());
    assert!(converted.layers[0].effects.is_none());
    assert_eq!(converted.guides[0].position, 50.);
    assert_eq!(
        render::render(&restored, 180, 100).unwrap(),
        render::render(&converted, 180, 100).unwrap()
    );
}

#[test]
fn photoshop_folder_blends_are_converted_explicitly_instead_of_rejecting_the_stack() {
    use ag_psd::psd::{BlendMode, Layer, PixelData, Psd, ReadOptions, WriteOptions};
    let mut folder = Layer {
        children: Some(vec![]),
        blend_mode: Some(BlendMode::Multiply),
        ..Default::default()
    };
    folder.additional_info.name = Some("Multiply folder".into());
    let data = ag_psd::write_psd(
        &Psd {
            width: 1.,
            height: 1.,
            children: Some(vec![folder]),
            image_data: Some(PixelData {
                width: 1,
                height: 1,
                data: vec![0; 4],
            }),
            ..Default::default()
        },
        &WriteOptions::default(),
    );
    assert!(ag_psd::read_psd(&data, &ReadOptions::default()).is_ok());
    let imported = psd::decode(&data).unwrap();
    assert!(imported.document.layers[0].is_group());
    assert_eq!(imported.document.layers[0].blend, Blend::Normal);
    assert!(imported.report.description().contains("folder blend"));
}

#[test]
fn scaled_soft_masks_preserve_sampling_on_psd_export() {
    let mut document = Document::new(4, 1).unwrap();
    document.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        2,
        1,
        Rgba([200, 90, 50, 255]),
    ))));
    document.layers[0].mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_raw(2, 1, vec![0, 255]).unwrap()),
        enabled: true,
        linked: true,
        placement: None,
    });
    let original = render::render(&document, 4, 1).unwrap();
    assert_eq!(
        original.pixels().map(|p| p[3]).collect::<Vec<_>>(),
        [0, 64, 191, 255]
    );
    let restored = psd::decode(&psd::encode(&document).unwrap())
        .unwrap()
        .document;
    assert_eq!(render::render(&restored, 4, 1).unwrap(), original);
}

#[test]
fn project_save_accepts_the_case_insensitive_extension_offered_by_the_file_dialog() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Uppercase.COMP");
    let document = Document::new(2, 2).unwrap();
    compositor::project::save(&document, &path).unwrap();
    assert_eq!(
        compositor::project::load(&path).unwrap().layers,
        document.layers
    );
}

fn photoshop_fixture(layers: Vec<ag_psd::psd::Layer>) -> Vec<u8> {
    use ag_psd::psd as ps;
    ag_psd::write_psd(
        &ps::Psd {
            width: 64.,
            height: 48.,
            children: Some(layers),
            image_data: Some(ps::PixelData {
                width: 64,
                height: 48,
                data: vec![0; 64 * 48 * 4],
            }),
            ..Default::default()
        },
        &ps::WriteOptions {
            no_background: Some(true),
            ..Default::default()
        },
    )
}
fn photoshop_shape(kind: f64) -> ag_psd::psd::Layer {
    use ag_psd::psd as ps;
    let px = |value| ps::UnitsValue {
        units: ps::Units::Pixels,
        value,
    };
    let mut layer = ps::Layer::default();
    layer.additional_info.name = Some("Editable shape".into());
    layer.additional_info.vector_fill = Some(ps::VectorContent::Color(ps::Color::Rgb(ps::Rgb {
        r: 220.,
        g: 60.,
        b: 30.,
    })));
    layer.additional_info.vector_origination = Some(ps::VectorOrigination {
        key_descriptor_list: vec![ps::KeyDescriptorItem {
            key_origin_type: Some(kind),
            key_origin_shape_bounding_box: Some(ps::UnitsBounds {
                left: px(4.),
                top: px(6.),
                right: px(24.),
                bottom: px(22.),
            }),
            key_origin_r_rect_radii: Some(ps::RRectRadii {
                top_left: px(3.),
                top_right: px(3.),
                bottom_left: px(3.),
                bottom_right: px(3.),
            }),
            ..Default::default()
        }],
    });
    layer
}
#[test]
fn photoshop_primitives_without_cached_pixels_remain_editable_after_project_save() {
    use compositor::document::ShapeGeometry;
    for (kind, geometry, radius) in [
        (1., ShapeGeometry::Rectangle, 0.),
        (2., ShapeGeometry::Rectangle, 3.),
        (5., ShapeGeometry::Ellipse, 0.),
    ] {
        let imported = psd::decode(&photoshop_shape_fixture(photoshop_shape(kind))).unwrap();
        let layer = &imported.document.layers[0];
        let shape = layer
            .shape
            .expect("Photoshop primitive should remain editable");
        assert_eq!(shape.geometry, geometry);
        assert_eq!(shape.corner_radius, radius);
        assert_eq!(layer.transform.origin, [4., 6.]);
        assert_eq!(layer.transform.size, [20., 16.]);
        let pixels = render::render(&imported.document, 64, 48).unwrap();
        assert_eq!(pixels[(14, 14)].0, [220, 60, 30, 255]);
        if kind == 5. {
            assert_eq!(pixels[(4, 6)][3], 0);
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shape.comp");
        compositor::project::save(&imported.document, &path).unwrap();
        let mut saved = compositor::project::load(&path).unwrap();
        assert_eq!(saved.layers[0].shape, Some(shape));
        let old = saved.layers[0].transform;
        let mut resized = old;
        resized.size = [40., 32.];
        compositor::transform::apply(&mut saved, old, resized, false).unwrap();
        assert_eq!(saved.layers[0].shape, Some(shape));
        assert_eq!(saved.layers[0].raster().unwrap().dimensions(), (40, 32));
        assert!(
            !imported
                .report
                .changes
                .iter()
                .any(|note| note.contains("saved pixels"))
        );
    }
}
#[test]
fn vector_descriptor_allocation_is_bounded_before_rasterization() {
    use ag_psd::psd as ps;
    let mut layer = photoshop_shape(1.);
    let origin = layer.additional_info.vector_origination.as_mut().unwrap();
    let bounds = origin.key_descriptor_list[0]
        .key_origin_shape_bounding_box
        .as_mut()
        .unwrap();
    bounds.right = ps::UnitsValue {
        units: ps::Units::Pixels,
        value: 100_000.,
    };
    let error = psd::decode(&photoshop_shape_fixture(layer))
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.contains("30,000") || error.contains("30000") || error.contains("dimensions"),
        "{error}"
    );
}
#[test]
fn supported_adjustments_preserve_editable_values_and_rendered_result() {
    use compositor::adjustment::{
        Adjustment, ColorRange, CurvePoint, HueBand, HueSaturation, Kind, RangeAdjustment,
    };
    let mut doc = fixture();
    let mut levels = Adjustment::new(Kind::Levels);
    levels.levels.ranges[0].black = 12.;
    levels.levels.ranges[0].gamma = 1.35;
    levels.levels.ranges[1].white = 241.;
    levels.levels.ranges[2].output_black = 19.;
    levels.levels.ranges[3].output_white = 232.;
    let mut curves = Adjustment::new(Kind::Curves);
    curves.curves.channels[2] = vec![
        CurvePoint { x: 0., y: 0. },
        CurvePoint { x: 128., y: 160. },
        CurvePoint { x: 255., y: 255. },
    ];
    let mut hue = Adjustment::new(Kind::HueSaturation);
    hue.hsv_settings = Some(HueSaturation {
        adjustments: vec![
            (
                ColorRange::Master,
                RangeAdjustment {
                    hue: 14.,
                    saturation: 5.,
                    lightness: -3.,
                },
            ),
            (
                ColorRange::Blues,
                RangeAdjustment {
                    hue: -8.,
                    saturation: 20.,
                    lightness: 2.,
                },
            ),
        ],
        bands: vec![(
            ColorRange::Blues,
            HueBand {
                falloff_start: 190.,
                range_start: 210.,
                range_end: 270.,
                falloff_end: 295.,
            },
        )],
        ..Default::default()
    });
    for adjustment in [&levels, &curves, &hue] {
        let mut layer = Layer::blank(format!("{:?}", adjustment.kind), doc.width, doc.height);
        layer.content = LayerContent::Adjustment(Box::new(adjustment.clone()));
        doc.add(layer).unwrap();
    }
    let bytes = psd::encode(&doc).unwrap();
    let imported = psd::decode(&bytes).unwrap();
    let adjustments: Vec<_> = imported
        .document
        .layers
        .iter()
        .filter_map(|l| {
            if let LayerContent::Adjustment(a) = &l.content {
                Some(a)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(adjustments.len(), 3);
    assert_eq!(adjustments[0].levels, levels.levels);
    assert_eq!(adjustments[1].curves, curves.curves);
    let actual = adjustments[2].hsv_settings.as_ref().unwrap();
    assert_eq!(
        actual.band(ColorRange::Blues),
        hue.hsv_settings.as_ref().unwrap().band(ColorRange::Blues)
    );
    let a = render::render(&doc, 4, 3).unwrap();
    let b = render::render(&imported.document, 4, 3).unwrap();
    for (a, b) in a.as_raw().iter().zip(b.as_raw()) {
        assert!(a.abs_diff(*b) <= 2, "{a} versus {b}");
    }
    assert!(
        !psd::export_report(&doc)
            .changes
            .iter()
            .any(|note| note.contains("adjustment layer is omitted"))
    );
    let parsed = ag_psd::read_psd(&bytes, &Default::default()).unwrap();
    assert_eq!(
        parsed
            .children
            .unwrap()
            .iter()
            .filter(|l| l.additional_info.adjustment.is_some())
            .count(),
        3
    );
}
#[test]
fn photoshop_colorize_flag_and_values_follow_specification_layout() {
    use ag_psd::psd as ps;
    let mut layer = ps::Layer::default();
    layer.additional_info.adjustment = Some(ps::AdjustmentLayer::HueSaturation(
        ps::HueSaturationAdjustment {
            master: Some(ps::HueSaturationAdjustmentChannel {
                a: 256.,
                b: 210.,
                c: 50.,
                d: 8.,
                ..Default::default()
            }),
            ..Default::default()
        },
    ));
    let bytes = photoshop_fixture(vec![layer]);
    let offset = bytes.windows(4).position(|w| w == b"hue2").unwrap() + 8;
    assert_eq!(
        &bytes[offset..offset + 10],
        &[0, 2, 1, 0, 0, 210, 0, 50, 0, 8]
    );
    let imported = psd::decode(&bytes).unwrap();
    let LayerContent::Adjustment(a) = &imported.document.layers[0].content else {
        panic!("missing adjustment")
    };
    assert!(a.hsv_settings.as_ref().unwrap().colorize);
    assert_eq!(a.hue, 210.);
    assert_eq!(a.saturation, 50.);
    assert_eq!(a.lightness, 8.);
    let bytes = psd::encode(&imported.document).unwrap();
    let offset = bytes.windows(4).position(|w| w == b"hue2").unwrap() + 8;
    assert_eq!(
        &bytes[offset..offset + 10],
        &[0, 2, 1, 0, 0, 210, 0, 50, 0, 8]
    );
}
#[test]
fn bezier_fill_without_cached_pixels_is_rasterized() {
    use ag_psd::psd as ps;
    let mut layer = photoshop_shape(1.);
    layer.additional_info.vector_origination = None;
    layer.additional_info.vector_mask = Some(ps::LayerVectorMask {
        paths: vec![ps::BezierPath {
            open: false,
            operation: Some(ps::BooleanOperation::Combine),
            fill_rule: ps::FillRule::NonZero,
            knots: [[5., 5.], [35., 5.], [20., 35.]]
                .map(|[x, y]| ps::BezierKnot {
                    linked: false,
                    points: vec![x / 64., y / 48., x / 64., y / 48., x / 64., y / 48.],
                })
                .to_vec(),
        }],
        ..Default::default()
    });
    let imported = psd::decode(&photoshop_fixture(vec![layer])).unwrap();
    assert!(imported.document.layers[0].shape.is_none());
    let pixels = render::render(&imported.document, 64, 48).unwrap();
    assert_eq!(pixels[(20, 15)].0, [220, 60, 30, 255]);
    assert_eq!(pixels[(6, 30)][3], 0);
    assert!(
        imported
            .report
            .changes
            .iter()
            .any(|note| note.contains("Bézier"))
    );
}

// ag-psd 0.3.0 writes an empty vogk descriptor. Build the actual Photoshop
// descriptor independently, and replace that block in a one-layer fixture.
fn photoshop_shape_fixture(layer: ag_psd::psd::Layer) -> Vec<u8> {
    use ag_psd::descriptor::{Descriptor, DescriptorValue as V, UnitDoubleValue};
    let origin = &layer
        .additional_info
        .vector_origination
        .as_ref()
        .unwrap()
        .key_descriptor_list[0];
    let unit = |v: ag_psd::psd::UnitsValue| {
        V::UnitDouble(UnitDoubleValue {
            units: "Pixels".into(),
            value: v.value,
        })
    };
    let mut item = Descriptor::new("", "null");
    item.set(
        "keyOriginType",
        V::Integer(origin.key_origin_type.unwrap() as i32),
    );
    let b = origin.key_origin_shape_bounding_box.unwrap();
    let mut bounds = Descriptor::new("", "classFloatRect");
    for (key, value) in [
        ("Left", b.left),
        ("Top ", b.top),
        ("Rght", b.right),
        ("Btom", b.bottom),
    ] {
        bounds.set(key, unit(value));
    }
    item.set("keyOriginShapeBBox", V::Descriptor(bounds));
    if let Some(r) = origin.key_origin_r_rect_radii {
        let mut radii = Descriptor::new("", "radii");
        for (key, value) in [
            ("topLeft", r.top_left),
            ("topRight", r.top_right),
            ("bottomLeft", r.bottom_left),
            ("bottomRight", r.bottom_right),
        ] {
            radii.set(key, unit(value));
        }
        item.set("keyOriginRRectRadii", V::Descriptor(radii));
    }
    let mut desc = Descriptor::new("", "null");
    desc.set("keyDescriptorList", V::List(vec![V::Descriptor(item)]));
    let mut writer = ag_psd::writer::create_writer_default();
    ag_psd::writer::write_int32(&mut writer, 1);
    ag_psd::descriptor::write_version_and_descriptor(&mut writer, &desc);
    let mut payload = ag_psd::writer::get_writer_buffer(&writer);
    if !payload.len().is_multiple_of(2) {
        payload.push(0);
    }
    let mut bytes = photoshop_fixture(vec![layer]);
    let u32_at = |bytes: &[u8], offset: usize| {
        u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize
    };
    let mut pos = 26;
    for _ in 0..2 {
        pos += 4 + u32_at(&bytes, pos);
    }
    let section_size = pos;
    let layer_size = pos + 4;
    pos += 10 + 16;
    let channels = u16::from_be_bytes(bytes[pos..pos + 2].try_into().unwrap()) as usize;
    let extra_size = pos + 2 + channels * 6 + 12;
    let key = bytes.windows(4).position(|v| v == b"vogk").unwrap();
    let old_length = u32_at(&bytes, key + 4);
    let difference = payload.len() as i64 - old_length as i64;
    for offset in [section_size, layer_size, extra_size] {
        let size = (u32_at(&bytes, offset) as i64 + difference) as u32;
        bytes[offset..offset + 4].copy_from_slice(&size.to_be_bytes());
    }
    bytes[key + 4..key + 8].copy_from_slice(&(payload.len() as u32).to_be_bytes());
    bytes.splice(key + 8..key + 8 + old_length, payload);
    bytes
}

#[test]
fn adjustment_bases_do_not_create_invalid_clipping_links() {
    use ag_psd::psd as ps;
    for adjustment in [
        ps::AdjustmentLayer::Levels(Default::default()),
        ps::AdjustmentLayer::Exposure(Default::default()),
    ] {
        let mut base = ps::Layer::default();
        base.additional_info.adjustment = Some(adjustment);
        let clipped = ps::Layer {
            clipping: Some(true),
            image_data: Some(ps::PixelData {
                width: 1,
                height: 1,
                data: vec![255; 4],
            }),
            ..Default::default()
        };
        let imported = psd::decode(&photoshop_fixture(vec![base, clipped])).unwrap();
        imported.document.validate().unwrap();
        assert!(
            imported
                .document
                .layers
                .last()
                .unwrap()
                .clip_source
                .is_none()
        );
        assert!(
            imported
                .report
                .changes
                .iter()
                .any(|note| note.contains("clipping has no base"))
        );
    }
}
#[test]
fn unsupported_adjustments_and_inverted_hue_ranges_are_explicitly_reported() {
    use compositor::adjustment::{Adjustment, HueSaturation, Kind};
    let mut doc = Document::new(2, 2).unwrap();
    let mut unsupported = Adjustment::new(Kind::HueSaturation);
    unsupported.hsv_settings = Some(HueSaturation {
        invert_range: true,
        ..Default::default()
    });
    for adjustment in [
        unsupported,
        Adjustment::new(Kind::Exposure),
        Adjustment::new(Kind::GradientMap),
        Adjustment::new(Kind::Grain),
    ] {
        let mut layer = Layer::blank(format!("{:?}", adjustment.kind), 2, 2);
        layer.content = LayerContent::Adjustment(Box::new(adjustment));
        doc.add(layer).unwrap();
    }
    let report = psd::export_report(&doc);
    assert_eq!(
        report
            .changes
            .iter()
            .filter(|note| note.contains("adjustment layer is omitted"))
            .count(),
        4
    );
    let imported = psd::decode(&psd::encode(&doc).unwrap()).unwrap();
    assert_eq!(imported.document.layers.len(), 1);
}

#[test]
fn malformed_supported_adjustment_is_not_silently_lost() {
    use ag_psd::psd as ps;
    let mut layer = ps::Layer::default();
    layer.additional_info.adjustment = Some(ps::AdjustmentLayer::HueSaturation(Default::default()));
    let mut bytes = photoshop_fixture(vec![layer]);
    let offset = bytes.windows(4).position(|v| v == b"hue2").unwrap() + 8;
    bytes[offset..offset + 2].copy_from_slice(&99_u16.to_be_bytes());
    let error = psd::decode(&bytes).err().unwrap().to_string();
    assert!(error.contains("hue2 adjustment data is invalid"), "{error}");
}

#[test]
fn photoshop_color_adjustments_remain_editable() {
    use compositor::adjustment::{Adjustment, BlackWhite, ColorBalance, Kind};
    let mut doc = Document::new(3, 2).unwrap();
    doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        3,
        2,
        Rgba([80, 120, 150, 255]),
    ))));
    for kind in [Kind::Invert, Kind::BlackWhite, Kind::ColorBalance] {
        let mut adjustment = Adjustment::new(kind);
        if kind == Kind::BlackWhite {
            adjustment.black_white_settings = Some(BlackWhite {
                reds: 75.,
                blues: 42.,
                ..Default::default()
            });
        }
        if kind == Kind::ColorBalance {
            adjustment.color_balance_settings = Some(ColorBalance {
                shadow_cyan_red: 25.,
                mid_yellow_blue: -30.,
                preserve_luminosity: false,
                ..Default::default()
            });
        }
        let mut layer = Layer::blank(format!("{kind:?}"), 3, 2);
        layer.content = LayerContent::Adjustment(Box::new(adjustment));
        doc.add(layer).unwrap();
    }
    assert!(psd::export_report(&doc).changes.is_empty());
    let imported = psd::decode(&psd::encode(&doc).unwrap()).unwrap().document;
    for (original, restored) in doc
        .layers
        .iter()
        .skip(1)
        .zip(imported.layers.iter().skip(1))
    {
        let (LayerContent::Adjustment(a), LayerContent::Adjustment(b)) =
            (&original.content, &restored.content)
        else {
            panic!("adjustment rasterized")
        };
        assert_eq!(a.kind, b.kind);
        let left = a.apply([0.3, 0.4, 0.5, 1.], [0., 0.]);
        let right = b.apply([0.3, 0.4, 0.5, 1.], [0., 0.]);
        assert!(
            left.into_iter()
                .zip(right)
                .all(|(a, b)| (a - b).abs() < 0.001)
        );
    }
}
