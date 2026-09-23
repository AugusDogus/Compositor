use ag_psd::psd::{Layer, PixelData, Psd, WriteOptions};
use compositor::psd;

fn fixture(large: bool, layered: bool) -> Vec<u8> {
    let pixels = PixelData {
        width: 2,
        height: 1,
        data: vec![24, 81, 139, 255, 200, 70, 30, 255],
    };
    ag_psd::write_psd(
        &Psd {
            width: 2.,
            height: 1.,
            image_data: Some(pixels.clone()),
            children: layered.then(|| {
                vec![Layer {
                    top: Some(0.),
                    left: Some(0.),
                    bottom: Some(1.),
                    right: Some(2.),
                    image_data: Some(pixels),
                    ..Default::default()
                }]
            }),
            ..Default::default()
        },
        &WriteOptions {
            psb: Some(large),
            ..Default::default()
        },
    )
}

#[test]
fn psd_and_psb_import_layered_and_merged_backgrounds() {
    for large in [false, true] {
        for layered in [false, true] {
            let bytes = fixture(large, layered);
            let imported = psd::decode(&bytes).unwrap();
            assert_eq!(imported.document.layers.len(), 1);
            let pixels = imported.document.layers[0].raster().unwrap();
            assert_eq!(pixels.as_raw(), &[24, 81, 139, 255, 200, 70, 30, 255]);
        }
    }
}

#[test]
fn psb_rejects_truncated_sections_and_oversized_64_bit_lengths() {
    let bytes = fixture(true, true);
    for end in 0..bytes.len() {
        assert!(
            psd::decode(&bytes[..end]).is_err(),
            "accepted truncation {end}"
        );
    }
    let mut invalid = bytes;
    let color_len = u32::from_be_bytes(invalid[26..30].try_into().unwrap()) as usize;
    let resource_offset = 30 + color_len;
    let resource_len = u32::from_be_bytes(
        invalid[resource_offset..resource_offset + 4]
            .try_into()
            .unwrap(),
    ) as usize;
    let layer_length = resource_offset + 4 + resource_len;
    invalid[layer_length..layer_length + 8].copy_from_slice(&u64::MAX.to_be_bytes());
    assert!(psd::decode(&invalid).is_err());
}
