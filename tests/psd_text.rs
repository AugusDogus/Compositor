use ag_psd::psd as ps;
use compositor::{project, psd};

fn type_layer(matrix: [f64; 6]) -> ps::Layer {
    ps::Layer {
        top: Some(10.),
        left: Some(20.),
        bottom: Some(11.),
        right: Some(21.),
        image_data: Some(ps::PixelData {
            width: 1,
            height: 1,
            data: vec![9, 8, 7, 255],
        }),
        additional_info: ps::LayerAdditionalInfo {
            name: Some("Title".into()),
            text: Some(ps::LayerTextData {
                text: "Hello Photoshop".into(),
                transform: Some(matrix.to_vec()),
                style: Some(ps::TextStyle {
                    font: Some(ps::Font {
                        name: "Inter Variable".into(),
                        ..Default::default()
                    }),
                    font_size: Some(20.),
                    tracking: Some(100.),
                    leading: Some(30.),
                    auto_leading: Some(false),
                    fill_color: Some(ps::Color::Rgb(ps::Rgb {
                        r: 51.,
                        g: 102.,
                        b: 153.,
                    })),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    }
}
fn decode(layer: ps::Layer, large: bool) -> psd::Imported {
    let bytes = ag_psd::write_psd(
        &ps::Psd {
            width: 512.,
            height: 256.,
            children: Some(vec![layer]),
            ..Default::default()
        },
        &ps::WriteOptions {
            psb: Some(large),
            ..Default::default()
        },
    );
    psd::decode(&bytes).unwrap()
}

#[test]
fn psd_and_psb_text_preserves_editable_style_and_uniform_scale() {
    for large in [false, true] {
        let imported = decode(type_layer([2., 0., 0., 2., 100., 80.]), large);
        let layer = &imported.document.layers[0];
        let text = layer.text.as_ref().unwrap();
        assert_eq!(text.content, "Hello Photoshop");
        assert_eq!(text.font_size, 40.);
        assert_eq!(text.tracking, 4.);
        assert_eq!(text.leading, 60.);
        assert_eq!([text.red, text.green, text.blue], [0.2, 0.4, 0.6]);
        assert_eq!(layer.transform.origin[0], 88.);
        assert!(layer.transform.origin[1] < 80.);
        assert!(
            !imported
                .report
                .description()
                .contains("without editable text")
        );
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("text.comp");
        project::save(&imported.document, &path).unwrap();
        assert_eq!(
            project::load(&path).unwrap().layers[0].text.as_ref(),
            Some(text)
        );
    }
}

#[test]
fn rotated_flipped_type_remains_editable_and_unsupported_geometry_retains_pixels() {
    let imported = decode(type_layer([0., 2., 2., 0., 100., 80.]), false);
    let layer = &imported.document.layers[0];
    assert!(layer.text.is_some());
    assert_eq!(layer.transform.rotation, 90.);
    assert!(layer.transform.flip_y);
    for matrix in [[1., 0., 0., 2., 100., 80.], [1., 0.4, 0., 1., 100., 80.]] {
        let imported = decode(type_layer(matrix), false);
        assert!(imported.document.layers[0].text.is_none());
        assert_eq!(
            imported.document.layers[0].raster().unwrap().as_raw(),
            &[9, 8, 7, 255]
        );
        assert!(imported.report.description().contains("cannot be retyped"));
    }
    let mut source = type_layer([1., 0., 0., 1., 100., 80.]);
    source.additional_info.text.as_mut().unwrap().orientation = Some(ps::Orientation::Vertical);
    assert!(decode(source, false).document.layers[0].text.is_none());
}

#[test]
fn paragraph_text_retains_frame_and_alignment() {
    let mut source = type_layer([2., 0., 0., 2., 100., 80.]);
    let text = source.additional_info.text.as_mut().unwrap();
    text.shape_type = Some(ps::TextShapeType::Box);
    text.box_bounds = Some(vec![5., 10., 105., 60.]);
    text.paragraph_style = Some(ps::ParagraphStyle {
        justification: Some(ps::Justification::Right),
        ..Default::default()
    });
    let imported = decode(source, false);
    let layer = &imported.document.layers[0];
    let text = layer.text.as_ref().unwrap();
    assert_eq!(text.box_size, Some([224., 124.]));
    assert_eq!(text.alignment, compositor::text::Alignment::Right);
    assert_eq!(layer.transform.origin, [98., 88.]);
}

#[test]
fn text_conversion_reports_font_fallback_and_omitted_warp() {
    let mut source = type_layer([1., 0., 0., 1., 100., 80.]);
    let text = source.additional_info.text.as_mut().unwrap();
    text.style.as_mut().unwrap().font.as_mut().unwrap().name = "Missing-Font-Fixture-123456".into();
    text.warp = Some(ps::Warp {
        style: Some(ps::WarpStyle::Arc),
        ..Default::default()
    });
    let imported = decode(source, false);
    assert!(imported.document.layers[0].text.is_some());
    let report = imported.report.description();
    assert!(report.contains("not installed"));
    assert!(report.contains("warp is omitted"));
}
