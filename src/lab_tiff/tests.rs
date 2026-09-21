use super::*;
use tiff::{
    encoder::{TiffEncoder, colortype},
    tags::Tag,
};

#[test]
fn lab_tiff_import_converts_cie_and_icc_channels_at_both_depths() {
    for photometric in [8_u16, 9] {
        for bits in [8, 16] {
            let mut encoded = Cursor::new(Vec::new());
            let mut encoder = TiffEncoder::new(&mut encoded).unwrap();
            let offset = if photometric == 8 { 0 } else { 128 };
            if bits == 8 {
                let samples = [
                    0,
                    offset,
                    offset,
                    255,
                    offset,
                    offset,
                    128,
                    offset,
                    offset,
                    138,
                    80 + offset,
                    70 + offset,
                ];
                let mut image = encoder.new_image::<colortype::RGB8>(4, 1).unwrap();
                image
                    .encoder()
                    .write_tag(Tag::PhotometricInterpretation, photometric)
                    .unwrap();
                image.write_data(&samples).unwrap();
            } else {
                let middle = u16::from(offset) * 256;
                let samples = [
                    0,
                    middle,
                    middle,
                    if photometric == 8 { 65535 } else { 65280 },
                    middle,
                    middle,
                    if photometric == 8 { 32768 } else { 32640 },
                    middle,
                    middle,
                    if photometric == 8 { 35465 } else { 35328 },
                    80 * 256 + middle,
                    70 * 256 + middle,
                ];
                let mut image = encoder.new_image::<colortype::RGB16>(4, 1).unwrap();
                image
                    .encoder()
                    .write_tag(Tag::PhotometricInterpretation, photometric)
                    .unwrap();
                image.write_data(&samples).unwrap();
            }
            let pixels = crate::image_io::read_encoded(encoded.get_ref()).unwrap();
            assert_eq!(pixels.dimensions(), (4, 1));
            assert_eq!(pixels[(0, 0)], image::Rgba([0, 0, 0, 255]));
            assert!(pixels[(1, 0)].0[..3].iter().all(|v| *v >= 254));
            assert!(
                pixels[(2, 0)].0[..3]
                    .iter()
                    .all(|v| (118..=120).contains(v))
            );
            assert!(
                pixels[(3, 0)][0] > 250 && pixels[(3, 0)][1] < 10 && pixels[(3, 0)][2] < 10,
                "{photometric} {bits}: {:?}",
                pixels[(3, 0)]
            );
        }
    }
}

#[test]
fn lab_alpha_profile_orientation_and_bigtiff_are_preserved() {
    let mut encoded = Cursor::new(Vec::new());
    let mut encoder = TiffEncoder::new_big(&mut encoded).unwrap();
    let profile =
        lcms2::Profile::new_lab4_context(lcms2::GlobalContext::default(), lcms2::CIExyY::d50())
            .unwrap()
            .icc()
            .unwrap();
    let mut image = encoder.new_image::<colortype::RGBA8>(2, 1).unwrap();
    image
        .encoder()
        .write_tag(Tag::PhotometricInterpretation, 8_u16)
        .unwrap();
    image
        .encoder()
        .write_tag(Tag::ExtraSamples, &[2_u16][..])
        .unwrap();
    image.encoder().write_tag(Tag::Orientation, 6_u16).unwrap();
    image
        .encoder()
        .write_tag(Tag::IccProfile, profile.as_slice())
        .unwrap();
    image.write_data(&[255, 0, 0, 128, 0, 0, 0, 255]).unwrap();
    let pixels = crate::image_io::read_encoded(encoded.get_ref()).unwrap();
    assert_eq!(pixels.dimensions(), (1, 2));
    assert_eq!(pixels[(0, 0)][3], 128);
    assert!(pixels[(0, 0)][0] >= 254);
    assert_eq!(pixels[(0, 1)], image::Rgba([0, 0, 0, 255]));
}

#[test]
fn inconsistent_lab_profile_and_truncated_headers_are_rejected() {
    let mut encoded = Cursor::new(Vec::new());
    let mut encoder = TiffEncoder::new(&mut encoded).unwrap();
    let mut image = encoder.new_image::<colortype::RGB8>(1, 1).unwrap();
    image
        .encoder()
        .write_tag(Tag::PhotometricInterpretation, 8_u16)
        .unwrap();
    image
        .encoder()
        .write_tag(
            Tag::IccProfile,
            lcms2::Profile::new_srgb().icc().unwrap().as_slice(),
        )
        .unwrap();
    image.write_data(&[255, 0, 0]).unwrap();
    assert!(
        crate::image_io::read_encoded(encoded.get_ref())
            .unwrap_err()
            .to_string()
            .contains("different color-space profile")
    );
    for size in 0..16 {
        assert!(header::raw_channels(&mut vec![0; size], 8).is_err());
    }
}

#[test]
fn profiled_cielab_matches_independent_pillow_reference() {
    let imported = crate::image_io::read_encoded(include_bytes!(
        "../../tests/fixtures/lab/pillow-cielab.tiff"
    ))
    .unwrap();
    let expected =
        image::load_from_memory(include_bytes!("../../tests/fixtures/lab/pillow-srgb.png"))
            .unwrap()
            .into_rgba8();
    assert_eq!(imported.dimensions(), expected.dimensions());
    for (a, b) in imported.as_raw().iter().zip(expected.as_raw()) {
        assert!(a.abs_diff(*b) <= 1, "{a} vs {b}");
    }
}

#[test]
fn signed_chroma_and_icc16_lightness_follow_adobe_encoding() {
    // The same quantized Lab colors in all four standard encodings.
    let values = [[96, -50, 40], [180, 25, -70], [128, 0, 0]];
    let mut reference = None;
    for photometric in [8_u16, 9] {
        for bits in [8, 16] {
            let mut encoded = Cursor::new(Vec::new());
            let mut encoder = TiffEncoder::new(&mut encoded).unwrap();
            let offset = if photometric == 9 { 128 } else { 0 };
            if bits == 8 {
                let samples: Vec<u8> = values
                    .iter()
                    .flat_map(|p| [p[0] as u8, (p[1] + offset) as u8, (p[2] + offset) as u8])
                    .collect();
                let mut image = encoder.new_image::<colortype::RGB8>(3, 1).unwrap();
                image
                    .encoder()
                    .write_tag(Tag::PhotometricInterpretation, photometric)
                    .unwrap();
                image.write_data(&samples).unwrap();
            } else {
                let l_scale = if photometric == 8 { 257 } else { 256 };
                let samples: Vec<u16> = values
                    .iter()
                    .flat_map(|p| {
                        [
                            (p[0] * l_scale) as u16,
                            ((p[1] + offset) * 256) as u16,
                            ((p[2] + offset) * 256) as u16,
                        ]
                    })
                    .collect();
                let mut image = encoder.new_image::<colortype::RGB16>(3, 1).unwrap();
                image
                    .encoder()
                    .write_tag(Tag::PhotometricInterpretation, photometric)
                    .unwrap();
                image.write_data(&samples).unwrap();
            }
            let pixels = crate::image_io::read_encoded(encoded.get_ref()).unwrap();
            if let Some(expected) = &reference {
                assert_eq!(&pixels, expected, "photometric {photometric}, bits {bits}");
            } else {
                reference = Some(pixels);
            }
        }
    }
}

#[test]
fn declared_d65_white_point_and_associated_alpha_are_converted() {
    let mut encoded = Cursor::new(Vec::new());
    let mut encoder = TiffEncoder::new(&mut encoded).unwrap();
    let mut image = encoder.new_image::<colortype::RGB16>(1, 1).unwrap();
    image
        .encoder()
        .write_tag(Tag::PhotometricInterpretation, 8_u16)
        .unwrap();
    image
        .encoder()
        .write_tag(
            Tag::Unknown(318),
            &[
                tiff::encoder::Rational { n: 3127, d: 10000 },
                tiff::encoder::Rational { n: 3290, d: 10000 },
            ][..],
        )
        .unwrap();
    // sRGB green expressed in D65 Lab, including negative a*.
    image
        .write_data(&[57497, (-22063_i16) as u16, 21294])
        .unwrap();
    let pixels = crate::image_io::read_encoded(encoded.get_ref()).unwrap();
    assert!(
        pixels[(0, 0)][0] <= 2 && pixels[(0, 0)][1] >= 254 && pixels[(0, 0)][2] <= 2,
        "{:?}",
        pixels[(0, 0)]
    );
    for photometric in [8_u16, 9] {
        let mut encoded = Cursor::new(Vec::new());
        let mut encoder = TiffEncoder::new(&mut encoded).unwrap();
        let mut image = encoder.new_image::<colortype::RGBA8>(2, 1).unwrap();
        image
            .encoder()
            .write_tag(Tag::PhotometricInterpretation, photometric)
            .unwrap();
        image
            .encoder()
            .write_tag(Tag::ExtraSamples, &[1_u16][..])
            .unwrap();
        let neutral = if photometric == 8 { 0 } else { 128 };
        image
            .write_data(&[128, neutral, neutral, 128, 0, neutral, neutral, 0])
            .unwrap();
        let pixels = crate::image_io::read_encoded(encoded.get_ref()).unwrap();
        assert_eq!(pixels[(0, 0)], image::Rgba([255, 255, 255, 128]));
        assert_eq!(pixels[(1, 0)], image::Rgba([0, 0, 0, 0]));
    }
}

#[test]
fn planar_lab_and_both_byte_orders_decode_the_same_colors() {
    // Two pixels in separate L/a/b strips. A hand-built TIFF independently
    // exercises endian-aware IFD adaptation and the decoder's plane layout.
    for little in [false, true] {
        let u16_bytes = |v: u16| {
            if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        };
        let u32_bytes = |v: u32| {
            if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        };
        let mut bytes = Vec::from(if little { b"II" } else { b"MM" });
        bytes.extend(u16_bytes(42));
        bytes.extend(u32_bytes(8));
        let tags = [
            (256_u16, 4_u16, 1_u32, 2_u32),
            (257, 4, 1, 1),
            (258, 3, 3, 134),
            (259, 3, 1, 1),
            (262, 3, 1, 8),
            (273, 4, 3, 140),
            (277, 3, 1, 3),
            (278, 4, 1, 1),
            (279, 4, 3, 152),
            (284, 3, 1, 2),
        ];
        bytes.extend(u16_bytes(tags.len() as u16));
        for (tag, kind, count, value) in tags {
            bytes.extend(u16_bytes(tag));
            bytes.extend(u16_bytes(kind));
            bytes.extend(u32_bytes(count));
            if kind == 3 && count == 1 {
                bytes.extend(u16_bytes(value as u16));
                bytes.extend([0, 0]);
            } else {
                bytes.extend(u32_bytes(value));
            }
        }
        bytes.extend(u32_bytes(0));
        for n in [8_u16; 3] {
            bytes.extend(u16_bytes(n));
        }
        for n in [164_u32, 166, 168, 2, 2, 2] {
            bytes.extend(u32_bytes(n));
        }
        bytes.extend([0, 255, 0, 0, 0, 0]);
        let pixels = crate::image_io::read_encoded(&bytes).unwrap();
        assert_eq!(pixels[(0, 0)], image::Rgba([0, 0, 0, 255]));
        assert_eq!(pixels[(1, 0)], image::Rgba([255, 255, 255, 255]));
    }
}
