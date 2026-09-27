use ag_psd::psd as ps;
use compositor::{
    blend_if::{Range, Settings},
    psd,
};

fn file(ranges: ps::BlendingRanges, group: bool) -> Vec<u8> {
    let mut layer = ps::Layer {
        image_data: Some(ps::PixelData {
            width: 1,
            height: 1,
            data: vec![100, 120, 140, 255],
        }),
        ..Default::default()
    };
    layer.additional_info.name = Some("Conditional".into());
    layer.additional_info.blending_ranges = Some(ranges);
    if group {
        layer.children = Some(Vec::new());
        layer.image_data = None;
    }
    ag_psd::write_psd(
        &ps::Psd {
            width: 1.,
            height: 1.,
            children: Some(vec![layer]),
            ..Default::default()
        },
        &ps::WriteOptions {
            no_background: Some(true),
            ..Default::default()
        },
    )
}
fn ranges() -> ps::BlendingRanges {
    ps::BlendingRanges {
        composite_gray_blend_source: vec![10., 50., 200., 240.],
        composite_graph_blend_destination_range: vec![30., 80., 190., 220.],
        ranges: vec![],
    }
}

#[test]
fn gray_blend_if_psd_import_preserves_external_byte_ranges() {
    let imported = psd::decode(&file(ranges(), false)).unwrap();
    assert_eq!(
        imported.document.layers[0].blend_if,
        Some(Settings {
            enabled: true,
            source: Range::new([10, 50], [200, 240]).unwrap(),
            underlying: Range::new([30, 80], [190, 220]).unwrap()
        })
    );
    assert!(!imported.report.description().contains("ranges are omitted"));
}

#[test]
fn psd_nonidentity_color_channel_ranges_and_folder_ranges_are_explicitly_rejected() {
    let mut channels = ranges();
    channels.ranges.push(ps::BlendingRange {
        source_range: vec![10., 20., 255., 255.],
        dest_range: vec![0., 0., 255., 255.],
    });
    assert!(
        psd::decode(&file(channels, false))
            .err()
            .unwrap()
            .to_string()
            .contains("channel-specific Blend If")
    );
    assert!(
        psd::decode(&file(ranges(), true))
            .err()
            .unwrap()
            .to_string()
            .contains("Blend If")
    );
    let mut invalid = ranges();
    invalid.composite_gray_blend_source = vec![50., 10., 200., 240.];
    assert!(
        psd::decode(&file(invalid, false))
            .err()
            .unwrap()
            .to_string()
            .contains("split handles")
    );
}

#[test]
fn psd_identity_color_channel_ranges_do_not_block_gray_import() {
    let mut channels = ranges();
    for _ in 0..3 {
        channels.ranges.push(ps::BlendingRange {
            source_range: vec![0., 0., 255., 255.],
            dest_range: vec![0., 0., 255., 255.],
        });
    }
    assert!(
        psd::decode(&file(channels, false)).unwrap().document.layers[0]
            .blend_if
            .is_some()
    );
}
