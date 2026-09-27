use super::*;
#[test]
#[ignore = "Requires COMPOSITOR_ABR_FIXTURE pointing to a real Photoshop brush pack"]
fn real_pack_local_smoke() {
    let path = std::env::var_os("COMPOSITOR_ABR_FIXTURE")
        .expect("Set COMPOSITOR_ABR_FIXTURE to a local ABR pack");
    let pack = Pack::read(Path::new(&path)).unwrap();
    assert!(!pack.tips().is_empty());
    for (index, tip) in pack.tips().iter().enumerate() {
        if tip.unavailable().is_none() {
            assert!(!pack.decode(&[index]).unwrap()[0].pixels().is_empty());
        }
    }
}

#[test]
fn legacy_names_spacing_polarity_and_16_bit_coverage_are_preserved() {
    let pack =
        Pack::from_bytes(include_bytes!("../../../../tests/fixtures/abr/sampled-v2.abr").to_vec())
            .unwrap();
    assert_eq!(pack.tips.len(), 2);
    let tips = pack.decode(&[0, 1]).unwrap();
    assert_eq!(tips[0].name(), "Soft pair");
    assert_eq!(tips[0].spacing(), 1.25);
    assert_eq!(tips[0].pixels().as_raw(), &[0, 128]);
    assert_eq!(tips[1].name(), "Asymmetric dots");
    assert_eq!(tips[1].spacing(), 0.75);
    assert_eq!(tips[1].pixels().as_raw(), &[0, 128, 255, 64, 192, 0]);
    let legacy =
        Pack::from_bytes(include_bytes!("../../../../tests/fixtures/abr/sampled-v1.abr").to_vec())
            .unwrap()
            .decode(&[0])
            .unwrap();
    assert_eq!(legacy[0].spacing(), 0.5);
    assert_eq!(legacy[0].pixels(), tips[1].pixels());
}
#[test]
fn modern_sampled_data_uses_explicit_default_spacing_when_descriptors_are_absent() {
    let pack =
        Pack::from_bytes(include_bytes!("../../../../tests/fixtures/abr/sampled-v6.abr").to_vec())
            .unwrap();
    let tips = pack.decode(&[0]).unwrap();
    assert_eq!(tips[0].spacing(), 0.25);
    assert!(pack.report().contains("no spacing metadata"));
    assert_eq!(tips[0].pixels().as_raw(), &[0, 128, 255, 64, 192, 0]);
}
#[test]
fn truncation_duplicate_selection_and_invalid_indices_fail_without_loading() {
    let bytes = include_bytes!("../../../../tests/fixtures/abr/sampled-v2.abr");
    for end in 0..bytes.len() {
        assert!(Pack::from_bytes(bytes[..end].to_vec()).is_err(), "{end}");
    }
    let pack = Pack::from_bytes(bytes.to_vec()).unwrap();
    for selection in [vec![], vec![2], vec![0, 0], vec![0; 33]] {
        assert!(pack.decode(&selection).is_err());
    }
}
#[test]
fn oversized_tip_is_listed_but_never_decompressed() {
    let mut bytes = include_bytes!("../../../../tests/fixtures/abr/sampled-v1.abr").to_vec();
    // Legacy v1 rectangle starts after 4-byte file and 6-byte record headers,
    // then 6-byte miscellaneous/spacing fields, 1-byte AA and 8-byte short bounds.
    bytes[33..37].copy_from_slice(&5000_i32.to_be_bytes());
    bytes[37..41].copy_from_slice(&5000_i32.to_be_bytes());
    let pack = Pack::from_bytes(bytes).unwrap();
    assert!(pack.tips()[0].unavailable().is_some());
    assert!(
        pack.decode(&[0])
            .unwrap_err()
            .to_string()
            .contains("exceeds")
    );
}
#[test]
#[cfg(unix)]
fn devices_and_fifos_are_rejected_before_opening() {
    assert!(Pack::read(Path::new("/dev/zero")).is_err());
    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join("pack.abr");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    assert!(Pack::read(&fifo).is_err());
}

#[test]
fn zip_expansion_is_bounded_by_declared_tip_dimensions() {
    use std::io::Write;
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    // A few KiB on disk would expand to 8 MiB, despite a declared six-byte tip.
    for _ in 0..2048 {
        encoder.write_all(&[255; 4096]).unwrap();
    }
    let compressed = encoder.finish().unwrap();
    let pack = zip_pack(compressed);
    assert_eq!(pack.tips()[0].size, [3, 2]);
    let error = pack.decode(&[0]).unwrap_err().to_string();
    assert!(error.contains("7 bytes, expected 6"), "{error}");
}
#[test]
fn zip_bitmap_with_exact_declared_length_preserves_coverage() {
    use std::io::Write;
    let pixels = [0, 128, 255, 64, 192, 0];
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(&pixels).unwrap();
    let pack = zip_pack(encoder.finish().unwrap());
    assert_eq!(pack.decode(&[0]).unwrap()[0].pixels().as_raw(), &pixels);
}
fn zip_pack(compressed: Vec<u8>) -> Pack {
    let original = include_bytes!("../../../../tests/fixtures/abr/sampled-v6.abr");
    let body_start = 20;
    let mut body = original[body_start..body_start + 66].to_vec();
    body[65] = 2;
    body.extend(compressed);
    let mut block = (body.len() as u32).to_be_bytes().to_vec();
    block.extend(body);
    block.resize(block.len().next_multiple_of(4), 0);
    let mut bytes = vec![0, 6, 0, 1];
    bytes.extend(b"8BIMsamp");
    bytes.extend((block.len() as u32).to_be_bytes());
    bytes.extend(block);
    Pack::from_bytes(bytes).unwrap()
}
#[test]
fn aggregate_budget_is_checked_before_decoding_any_selected_tip() {
    let mut entry = include_bytes!("../../../../tests/fixtures/abr/sampled-v1.abr")[4..].to_vec();
    entry[29..33].copy_from_slice(&2048_i32.to_be_bytes());
    entry[33..37].copy_from_slice(&2048_i32.to_be_bytes());
    let mut bytes = vec![0, 1, 0, 5];
    for _ in 0..5 {
        bytes.extend_from_slice(&entry);
    }
    let pack = Pack::from_bytes(bytes).unwrap();
    assert!(pack.tips().iter().all(|t| t.unavailable().is_none()));
    let error = pack.decode(&[0, 1, 2, 3, 4]).unwrap_err().to_string();
    assert!(error.contains("16 million"), "{error}");
}

fn modern_blocks(blocks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut bytes = vec![0, 6, 0, 1];
    for (kind, body) in blocks {
        bytes.extend(b"8BIM");
        bytes.extend(*kind);
        bytes.extend((body.len() as u32).to_be_bytes());
        bytes.extend(body);
        bytes.resize(bytes.len().next_multiple_of(4), 0);
    }
    bytes
}
fn framed_tip(body: &[u8]) -> Vec<u8> {
    let mut frame = (body.len() as u32).to_be_bytes().to_vec();
    frame.extend(body);
    frame.resize(frame.len().next_multiple_of(4), 0);
    frame
}
fn recovered_sample_block(count: usize) -> Vec<u8> {
    // A single valid outer frame contains many recovered UUID records.
    // Its unanchored prefix forces the parser's compatibility recovery path.
    let cell = &include_bytes!("../../../../tests/fixtures/abr/sampled-v6.abr")[20..];
    let mut body = vec![0];
    for _ in 0..count {
        body.extend(cell);
    }
    framed_tip(&body)
}
#[test]
fn recovered_uuid_records_are_bounded_before_pairing_and_across_blocks() {
    let at_limit = modern_blocks(&[(b"samp", recovered_sample_block(2048))]);
    assert_eq!(Pack::from_bytes(at_limit).unwrap().tips().len(), 2048);
    for blocks in [
        vec![(b"samp", recovered_sample_block(2049))],
        vec![
            (b"samp", recovered_sample_block(1536)),
            (b"samp", recovered_sample_block(513)),
        ],
    ] {
        let error = Pack::from_bytes(modern_blocks(&blocks))
            .unwrap_err()
            .to_string();
        assert!(error.contains("UUID recovery anchors"), "{error}");
    }
}
#[test]
fn uuid_shaped_pixels_do_not_trigger_recovery_limits_in_framed_tips() {
    let mut body = include_bytes!("../../../../tests/fixtures/abr/sampled-v6.abr")[20..86].to_vec();
    body[55..59].copy_from_slice(&2049_i32.to_be_bytes());
    body[59..63].copy_from_slice(&38_i32.to_be_bytes());
    for _ in 0..2049 {
        body.extend(b"$a1b2c3d4-e5f6-7890-abcd-ef1234567890\0");
    }
    let pack = Pack::from_bytes(modern_blocks(&[(b"samp", framed_tip(&body))])).unwrap();
    assert_eq!(pack.tips().len(), 1);
    assert_eq!(
        pack.decode(&[0]).unwrap()[0].pixels().dimensions(),
        (38, 2049)
    );
}
fn descriptor_block(count: usize) -> Vec<u8> {
    let mut body = Vec::new();
    for word in [16_u32, 0, 0] {
        body.extend(word.to_be_bytes());
    }
    body.extend(b"null");
    body.extend(1_u32.to_be_bytes());
    body.extend(0_u32.to_be_bytes());
    body.extend(b"BrshVlLs");
    body.extend((count as u32).to_be_bytes());
    for _ in 0..count {
        body.extend(b"Objc");
        body.extend(0_u32.to_be_bytes());
        body.extend(0_u32.to_be_bytes());
        body.extend(b"null");
        body.extend(0_u32.to_be_bytes());
    }
    body
}
#[test]
fn descriptor_presets_are_bounded_before_collecting_and_across_blocks() {
    let at_limit = modern_blocks(&[(b"desc", descriptor_block(2048))]);
    assert!(Pack::from_bytes(at_limit).is_ok());
    for blocks in [
        vec![(b"desc", descriptor_block(2049))],
        vec![
            (b"desc", descriptor_block(1536)),
            (b"desc", descriptor_block(513)),
        ],
    ] {
        let error = Pack::from_bytes(modern_blocks(&blocks))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("descriptors exceed the 2048-record limit"),
            "{error}"
        );
    }
}

#[test]
fn duplicate_auxiliary_samples_share_long_owner_names_without_changing_imported_tips() {
    fn key(bytes: &mut Vec<u8>, value: &[u8]) {
        bytes.extend((value.len() as u32).to_be_bytes());
        bytes.extend(value);
    }
    fn text_value(bytes: &mut Vec<u8>, name: &[u8], value: &str) {
        key(bytes, name);
        bytes.extend(b"TEXT");
        bytes.extend((value.encode_utf16().count() as u32).to_be_bytes());
        for value in value.encode_utf16() {
            bytes.extend(value.to_be_bytes());
        }
    }
    fn object(bytes: &mut Vec<u8>, name: &[u8], class: &[u8], count: u32) {
        key(bytes, name);
        bytes.extend(b"Objc");
        bytes.extend(0_u32.to_be_bytes());
        key(bytes, class);
        bytes.extend(count.to_be_bytes());
    }
    let main_id = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
    let dual_id = "a1b2c3d4-e5f6-7890-abcd-ef1234567891";
    let mut cell = include_bytes!("../../../../tests/fixtures/abr/sampled-v6.abr")[20..].to_vec();
    cell[1..37].copy_from_slice(main_id.as_bytes());
    let mut samples = framed_tip(&cell);
    cell[1..37].copy_from_slice(dual_id.as_bytes());
    for _ in 0..127 {
        samples.extend(framed_tip(&cell));
    }
    let name = "N".repeat(256 * 1024);
    let mut descriptor = descriptor_block(1);
    let count = descriptor.len() - 4;
    descriptor[count..].copy_from_slice(&4_u32.to_be_bytes());
    text_value(&mut descriptor, b"Nm  ", &name);
    text_value(&mut descriptor, b"sampledData", main_id);
    object(&mut descriptor, b"Brsh", b"sampledBrush", 1);
    key(&mut descriptor, b"Spcn");
    descriptor.extend(b"UntF#Prc");
    descriptor.extend(75_f64.to_be_bytes());
    object(&mut descriptor, b"dualBrush", b"dualBrush", 2);
    key(&mut descriptor, b"useDualBrush");
    descriptor.extend(b"bool\x01");
    object(&mut descriptor, b"Brsh", b"sampledBrush", 1);
    text_value(&mut descriptor, b"sampledData", dual_id);
    let bytes = modern_blocks(&[(b"samp", samples), (b"desc", descriptor)]);
    let deferred = parse(&bytes).unwrap();
    let details = &deferred.pack.dropped_tip_details;
    assert_eq!(details.len(), 127);
    assert_eq!(details[0].owner_preset_names[0], name);
    assert!(
        details
            .iter()
            .all(|tip| Arc::ptr_eq(&details[0].owner_preset_names, &tip.owner_preset_names))
    );
    assert_eq!(deferred.pack.brushes[0].name, name);
    drop(deferred);
    let pack = Pack::from_bytes(bytes).unwrap();
    assert_eq!(pack.tips().len(), 1);
    let tips = pack.decode(&[0]).unwrap();
    assert_eq!(tips[0].name(), "N".repeat(256));
    assert_eq!(tips[0].spacing(), 0.75);
    assert_eq!(tips[0].pixels().as_raw(), &[0, 128, 255, 64, 192, 0]);
}
