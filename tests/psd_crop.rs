//! Oversized layer recovery uses compressed fixtures, never source-sized images.
use compositor::psd;

fn u16be(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}
fn u32be(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}
fn length(out: &mut Vec<u8>, value: usize, large: bool) {
    if large {
        out.extend_from_slice(&(value as u64).to_be_bytes());
    } else {
        u32be(out, value as u32);
    }
}
fn bounds(out: &mut Vec<u8>, rect: [i32; 4]) {
    for value in rect {
        out.extend_from_slice(&value.to_be_bytes());
    }
}
fn rle(width: usize, height: usize, value: u8, large: bool) -> Vec<u8> {
    let mut row = Vec::new();
    let mut left = width;
    while left > 0 {
        let run = left.min(128);
        row.push((1i16 - run as i16) as u8);
        row.push(value);
        left -= run;
    }
    let mut data = Vec::new();
    u16be(&mut data, 1);
    for _ in 0..height {
        if large {
            u32be(&mut data, row.len() as u32);
        } else {
            u16be(&mut data, row.len() as u16);
        }
    }
    for _ in 0..height {
        data.extend_from_slice(&row);
    }
    data
}
fn oversized(large: bool, mask_outside: bool) -> Vec<u8> {
    let (width, height) = (30_000, 7_000);
    let channels = [
        rle(width, height, 90, large),
        rle(width, height, 255, large),
        rle(width, height, 128, large),
    ];
    let mut layer = Vec::new();
    bounds(&mut layer, [-6998, -29998, 2, 2]);
    u16be(&mut layer, 3);
    for (id, data) in [0i16, -1, -2].into_iter().zip(&channels) {
        u16be(&mut layer, id as u16);
        length(&mut layer, data.len(), large);
    }
    layer.extend_from_slice(b"8BIMnorm");
    layer.extend_from_slice(&[255, 0, 0, 0]);
    let mut extra = Vec::new();
    u32be(&mut extra, 20);
    // Relative masks must retain their original absolute placement when the
    // layer origin changes during cropping.
    if mask_outside {
        bounds(&mut extra, [20_000, 40_000, 27_000, 70_000]);
    } else {
        bounds(&mut extra, [0, 0, 7000, 30_000]);
    }
    extra.extend_from_slice(&[0, 1, 0, 0]);
    u32be(&mut extra, 0);
    extra.extend_from_slice(&[3, b'B', b'i', b'g']);
    u32be(&mut layer, extra.len() as u32);
    layer.extend(extra);
    for data in channels {
        layer.extend(data);
    }
    let mut info = Vec::new();
    u16be(&mut info, 1);
    info.extend(layer);
    if info.len() % 2 != 0 {
        info.push(0);
    }
    let mut section = Vec::new();
    length(&mut section, info.len(), large);
    section.extend(info);
    u32be(&mut section, 0);
    let mut out = b"8BPS".to_vec();
    u16be(&mut out, if large { 2 } else { 1 });
    out.extend_from_slice(&[0; 6]);
    u16be(&mut out, 1);
    u32be(&mut out, 4);
    u32be(&mut out, 4);
    u16be(&mut out, 8);
    u16be(&mut out, 1);
    u32be(&mut out, 0);
    u32be(&mut out, 0);
    length(&mut out, section.len(), large);
    out.extend(section);
    u16be(&mut out, 0);
    out.extend_from_slice(&[0; 16]);
    out
}
#[test]
fn oversized_psd_and_psb_crop_before_allocating_and_preserve_relative_mask_position() {
    for large in [false, true] {
        let imported = psd::decode(&oversized(large, false)).unwrap();
        let layer = &imported.document.layers[0];
        assert_eq!(layer.transform.origin, [0., 0.]);
        assert_eq!(layer.transform.size, [2., 2.]);
        assert_eq!(layer.raster().unwrap().dimensions(), (2, 2));
        assert_eq!(layer.raster().unwrap()[(0, 0)].0, [90, 90, 90, 255]);
        let mask = layer.mask.as_ref().unwrap();
        assert_eq!(mask.placement.unwrap().origin, [-1., -1.]);
        assert_eq!(mask.pixels.dimensions(), (4, 4));
        assert_eq!(mask.pixels[(1, 1)].0, [128]);
        assert_eq!(mask.pixels[(0, 0)].0, [0]);
        assert!(
            imported
                .report
                .changes
                .iter()
                .any(|s| s.contains("cropped to the canvas"))
        );
    }
}
#[test]
fn cropped_away_mask_keeps_its_default_tone_instead_of_becoming_unmasked() {
    let imported = psd::decode(&oversized(false, true)).unwrap();
    let mask = imported.document.layers[0].mask.as_ref().unwrap();
    assert!(mask.pixels.pixels().all(|p| p.0 == [0]));
}
#[test]
fn truncated_oversized_channel_is_rejected() {
    let mut bytes = oversized(false, false);
    bytes.truncate(bytes.len() / 2);
    assert!(psd::decode(&bytes).is_err());
}
