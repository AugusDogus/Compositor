use std::collections::BTreeSet;
use std::path::Path;

const SEEDS: [&str; 9] = [
    "patt_long_gray",
    "patt_oversized_channel",
    "patt_oversized_name",
    "patt_short_gray",
    "tip_bitmap_dim_overflow",
    "v2_rle_overflow",
    "v2_rle_underflow",
    "wellformed_v6_min",
    "wellformed_v6_patt",
];

#[test]
fn corpus_files_parse_without_panic() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let dir = Path::new(&manifest_dir).join("../../fuzz/corpus/abr_parse");
    let mut replayed = BTreeSet::new();
    for entry in std::fs::read_dir(&dir).expect("seed corpus dir must exist") {
        let path = entry.unwrap().path();
        if !path.is_file() {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let _ = brushkit_abr::parse_abr(&bytes);
        if let Ok(deferred) = brushkit_abr::parse_abr_all_deferred_without_patterns(&bytes) {
            for i in 0..deferred.pack.brushes.len() {
                let _ = deferred.decode_tip(i);
            }
        }
        replayed.insert(path.file_name().unwrap().to_string_lossy().into_owned());
    }

    let names: Vec<&str> = replayed.iter().map(String::as_str).collect();
    println!("replayed {} file(s): {}", names.len(), names.join(", "));

    let missing: Vec<&str> = SEEDS
        .iter()
        .copied()
        .filter(|seed| !replayed.contains(*seed))
        .collect();
    assert!(
        missing.is_empty(),
        "committed fuzz seed(s) gone from fuzz/corpus/abr_parse/: {} — \
         restore the file(s) under their original names, or drop them from SEEDS if \
         the removal was deliberate",
        missing.join(", ")
    );
}
