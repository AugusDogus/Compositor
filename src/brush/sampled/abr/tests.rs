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
