use super::*;
use image::Rgba;

fn word(out: &mut Vec<u8>, value: u32) {
    out.extend(value.to_be_bytes());
}
fn short(out: &mut Vec<u8>, value: u16) {
    out.extend(value.to_be_bytes());
}
fn bounds(out: &mut Vec<u8>, rect: [i32; 4]) {
    for value in rect {
        out.extend(value.to_be_bytes());
    }
}
fn plane(rect: [i32; 4], compression: u8, bytes: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    word(&mut body, 8);
    bounds(&mut body, rect);
    short(&mut body, 8);
    body.push(compression);
    body.extend(bytes);
    let mut result = Vec::new();
    word(&mut result, 1);
    word(&mut result, body.len() as u32);
    result.extend(body);
    result
}
fn record(
    mode: u32,
    rect: [i32; 4],
    name: &str,
    id: &str,
    colors: &[Option<Vec<u8>>],
    alpha: Option<Vec<u8>>,
) -> Vec<u8> {
    let mut out = Vec::new();
    word(&mut out, 1);
    word(&mut out, mode);
    short(&mut out, 0);
    short(&mut out, 0);
    let name: Vec<_> = name.encode_utf16().chain([0]).collect();
    word(&mut out, name.len() as u32);
    for unit in name {
        short(&mut out, unit);
    }
    out.push(id.len() as u8);
    out.extend(id.as_bytes());
    word(&mut out, 3);
    let mut data = Vec::new();
    bounds(&mut data, rect);
    word(&mut data, colors.len() as u32);
    for color in colors {
        match color {
            Some(bytes) => data.extend(bytes),
            None => word(&mut data, 0),
        }
    }
    word(&mut data, 0);
    match alpha {
        Some(bytes) => data.extend(bytes),
        None => word(&mut data, 0),
    }
    word(&mut out, data.len() as u32);
    out.extend(data);
    out
}
fn pack(records: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = b"8BPT".to_vec();
    short(&mut bytes, 1);
    word(&mut bytes, records.len() as u32);
    for record in records {
        bytes.extend(record);
    }
    bytes
}
fn gray() -> Vec<u8> {
    record(
        1,
        [0, 0, 1, 3],
        "Gray",
        "gray",
        &[Some(plane([0, 0, 1, 3], 0, &[0, 128, 255]))],
        None,
    )
}

#[test]
fn grayscale_rgb_and_transparency_preserve_straight_samples() {
    let rect = [0, 0, 1, 3];
    let rgb = record(
        3,
        rect,
        "Colors",
        "rgb",
        &[
            Some(plane(rect, 0, &[255, 128, 0])),
            Some(plane(rect, 0, &[0, 255, 128])),
            Some(plane(rect, 0, &[128, 0, 255])),
        ],
        Some(plane(rect, 0, &[0, 128, 255])),
    );
    let pack = Pack::from_bytes(&pack(&[gray(), rgb])).unwrap();
    assert_eq!(pack.patterns().len(), 2);
    assert_eq!(pack.patterns()[0].name(), "Gray");
    assert_eq!(pack.patterns()[0].id(), "gray");
    assert_eq!(
        pack.patterns()[0].pixels().as_raw(),
        &[0, 0, 0, 255, 128, 128, 128, 255, 255, 255, 255, 255]
    );
    assert_eq!(
        pack.patterns()[1].pixels().as_raw(),
        &[255, 0, 128, 0, 128, 255, 0, 128, 0, 128, 255, 255]
    );
}

#[test]
fn packbits_literals_runs_and_noops_are_row_bounded() {
    let mut rle = Vec::new();
    short(&mut rle, 6);
    short(&mut rle, 3);
    rle.extend([128, 3, 10, 20, 30, 40, 253, 80, 128]);
    let rect = [0, 0, 2, 4];
    let bytes = pack(&[record(
        1,
        rect,
        "RLE",
        "rle",
        &[Some(plane(rect, 1, &rle))],
        None,
    )]);
    let parsed = Pack::from_bytes(&bytes).unwrap();
    let actual: Vec<_> = parsed.patterns()[0]
        .pixels()
        .pixels()
        .map(|p| p[0])
        .collect();
    assert_eq!(actual, [10, 20, 30, 40, 80, 80, 80, 80]);
}

#[test]
fn cropped_channels_use_vmal_bounds_and_negative_origins() {
    let rect = [-10, -20, -8, -16];
    let bytes = pack(&[record(
        1,
        rect,
        "点 🖌",
        "offset",
        &[None, Some(plane([-10, -18, -8, -17], 0, &[100, 200]))],
        Some(plane([-10, -18, -9, -17], 0, &[128])),
    )]);
    let parsed = Pack::from_bytes(&bytes).unwrap();
    let pixels = parsed.patterns()[0].pixels();
    assert_eq!(pixels.dimensions(), (4, 2));
    assert_eq!(pixels[(2, 0)], Rgba([100, 100, 100, 128]));
    assert_eq!(pixels[(2, 1)], Rgba([200, 200, 200, 0]));
    assert_eq!(pixels[(0, 0)], Rgba([0; 4]));
    assert_eq!(parsed.patterns()[0].name(), "点 🖌");
}

#[test]
fn independent_ag_psd_writer_fixture_keeps_its_alpha_plane() {
    use ag_psd::{
        psd::{PatternBounds, PatternInfo},
        writer::{create_writer_default, get_writer_buffer, write_pattern},
    };
    let pixels = vec![
        255, 20, 0, 0, 40, 120, 200, 128, 0, 90, 255, 255, 130, 40, 60, 55,
    ];
    let mut writer = create_writer_default();
    write_pattern(
        &mut writer,
        &PatternInfo {
            id: "independent".into(),
            name: "Writer fixture".into(),
            x: 0.,
            y: 0.,
            bounds: PatternBounds {
                x: 4.,
                y: 8.,
                w: 2.,
                h: 2.,
            },
            data: pixels.clone(),
        },
    );
    let resource = get_writer_buffer(&writer);
    let body = &resource[4..]; // Standalone PAT omits the PSD resource's length and padding.
    let units = u32::from_be_bytes(body[12..16].try_into().unwrap()) as usize;
    let id_at = 16 + units * 2;
    let vm_at = id_at + 1 + usize::from(body[id_at]);
    let vm_len = u32::from_be_bytes(body[vm_at + 4..vm_at + 8].try_into().unwrap()) as usize;
    let parsed = Pack::from_bytes(&pack(&[body[..vm_at + 8 + vm_len].to_vec()])).unwrap();
    assert_eq!(*parsed.patterns()[0].pixels().as_raw(), pixels);
}

#[test]
fn truncation_lengths_and_extra_bytes_never_produce_partial_imports() {
    let bytes = pack(&[gray()]);
    for end in 0..bytes.len() {
        assert!(Pack::from_bytes(&bytes[..end]).is_err(), "prefix {end}");
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(Pack::from_bytes(&trailing).is_err());
    let mut bad_count = bytes.clone();
    bad_count[6..10].copy_from_slice(&2_u32.to_be_bytes());
    assert!(Pack::from_bytes(&bad_count).is_err());
    let mut second = gray();
    second[0..4].copy_from_slice(&7_u32.to_be_bytes());
    assert!(Pack::from_bytes(&pack(&[gray(), second])).is_err());
    assert!(Pack::from_bytes(&pack(&[gray(), gray()])).is_err());
}

#[test]
fn unsupported_modes_depth_compression_and_channels_report_actionable_errors() {
    for mode in [0_u32, 2, 4, 7, 9] {
        let mut entry = gray();
        entry[4..8].copy_from_slice(&mode.to_be_bytes());
        assert!(
            Pack::from_bytes(&pack(&[entry]))
                .unwrap_err()
                .to_string()
                .contains("RGB")
        );
    }
    let rect = [0, 0, 1, 1];
    for compression in [2, 3, 255] {
        assert!(
            Pack::from_bytes(&pack(&[record(
                1,
                rect,
                "Bad",
                "bad",
                &[Some(plane(rect, compression, &[0]))],
                None
            )]))
            .unwrap_err()
            .to_string()
            .contains("compression")
        );
    }
    let mut wrong_depth = plane(rect, 0, &[0]);
    wrong_depth[8..12].copy_from_slice(&16_u32.to_be_bytes());
    assert!(
        Pack::from_bytes(&pack(&[record(
            1,
            rect,
            "Bad",
            "bad",
            &[Some(wrong_depth)],
            None
        )]))
        .unwrap_err()
        .to_string()
        .contains("8-bit")
    );
    assert!(
        Pack::from_bytes(&pack(&[record(
            1,
            rect,
            "Bad",
            "bad",
            &[Some(plane(rect, 0, &[0])), Some(plane(rect, 0, &[1]))],
            None
        )]))
        .is_err()
    );
    assert!(
        Pack::from_bytes(&pack(&[record(
            3,
            rect,
            "Bad",
            "bad",
            &[Some(plane(rect, 0, &[0]))],
            None
        )]))
        .is_err()
    );
}

#[test]
fn malformed_raw_rle_and_outside_planes_are_rejected() {
    let rect = [0, 0, 1, 3];
    for (compression, data) in [
        (0, vec![0, 1]),
        (0, vec![0, 1, 2, 3]),
        (1, vec![0, 2, 252, 0]),
        (1, vec![0, 2, 255, 0]),
        (1, vec![0, 2, 3, 0]),
        (1, vec![0, 9, 0, 0]),
        (1, vec![]),
    ] {
        assert!(
            Pack::from_bytes(&pack(&[record(
                1,
                rect,
                "Bad",
                "bad",
                &[Some(plane(rect, compression, &data))],
                None
            )]))
            .is_err()
        );
    }
    for child in [[0, -1, 1, 2], [0, 2, 1, 5], [1, 0, 2, 3], [0, 0, -1, 3]] {
        assert!(
            Pack::from_bytes(&pack(&[record(
                1,
                rect,
                "Bad",
                "bad",
                &[Some(plane(child, 0, &[0, 1, 2]))],
                None
            )]))
            .is_err()
        );
    }
}

#[test]
fn labels_tile_sizes_and_pack_counts_are_bounded() {
    for name in ["", "bad\nname"] {
        assert!(Pattern::from_pixels(name, RgbaImage::new(1, 1)).is_err());
    }
    assert!(Pattern::from_pixels("empty", RgbaImage::new(0, 1)).is_err());
    let mut bytes = pack(&[gray()]);
    bytes[6..10].copy_from_slice(&257_u32.to_be_bytes());
    assert!(Pack::from_bytes(&bytes).is_err());
    assert!(Pack::from_bytes(&pack(&[])).is_err());
    let rect = [0, 0, 30_000, 30_000];
    assert!(Pack::from_bytes(&pack(&[record(1, rect, "Huge", "huge", &[None], None)])).is_err());
}

#[test]
fn file_import_and_custom_tiles_share_the_typed_domain() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("example.pat");
    std::fs::write(&file, pack(&[gray()])).unwrap();
    assert_eq!(
        Pack::read(&file).unwrap().patterns()[0]
            .pixels()
            .dimensions(),
        (3, 1)
    );
    assert!(Pack::read(temp.path()).is_err());
    let custom =
        Pattern::from_pixels("Custom", RgbaImage::from_pixel(1, 1, Rgba([1, 2, 3, 4]))).unwrap();
    assert_eq!(custom.name(), "Custom");
    assert!(!custom.id().is_empty());
    assert_eq!(custom.pixels()[(0, 0)], Rgba([1, 2, 3, 4]));
}

#[test]
fn aggregate_pixel_budget_is_checked_before_tile_allocation() {
    let rect = [0, 0, 4096, 4096];
    let mut rle = Vec::new();
    for _ in 0..4096 {
        short(&mut rle, 64);
    }
    for _ in 0..4096 * 32 {
        rle.extend([129, 0]);
    }
    let records: Vec<_> = (0..3)
        .map(|index| {
            record(
                1,
                rect,
                "Large",
                &format!("large-{index}"),
                &[Some(plane(rect, 1, &rle))],
                None,
            )
        })
        .collect();
    let error = Pack::from_bytes(&pack(&records)).unwrap_err().to_string();
    assert!(error.contains("32 megapixels"), "{error}");
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("TooBig.pat");
    std::fs::File::create(&path)
        .unwrap()
        .set_len(MAX_FILE_BYTES + 1)
        .unwrap();
    assert!(
        Pack::read(&path)
            .unwrap_err()
            .to_string()
            .contains("64 MiB")
    );
}

#[test]
fn adobe_resource_names_display_their_human_readable_label() {
    let rect = [0, 0, 1, 1];
    let input = pack(&[record(
        1,
        rect,
        "$$$/Patterns/Defaults/Paper=Paper ^C",
        "paper",
        &[Some(plane(rect, 0, &[255]))],
        None,
    )]);
    assert_eq!(
        Pack::from_bytes(&input).unwrap().patterns()[0].name(),
        "Paper ©"
    );
}
