use super::*;
const CELL: &[u8] = include_bytes!("../../../../tests/fixtures/gbr/pixel.gbr");
fn hose(parameters: &str, count: usize) -> Vec<u8> {
    let mut bytes = format!("Test\n{count} {parameters}\n").into_bytes();
    for _ in 0..count {
        bytes.extend_from_slice(CELL);
    }
    bytes
}
fn mouse(direction: [f64; 2]) -> Dynamics {
    Dynamics::new(None, None, direction).unwrap()
}

#[test]
fn real_gimp_hoses_preserve_spacing_and_partially_populated_rank_grids() {
    let fine = Hose::from_bytes(include_bytes!(
        "../../../../tests/fixtures/gih/fine-grain.gih"
    ))
    .unwrap();
    assert_eq!(fine.name(), "Fine Grain");
    assert_eq!(fine.cells().len(), 3);
    assert_eq!(fine.pixels(), 3 * 256 * 256);
    assert_eq!(fine.spacing(), 0.2); // Text step:100 is not painting spacing.
    assert_eq!(
        fine.selections().collect::<Vec<_>>(),
        [(3, Selection::Random)]
    );
    assert!(!fine.embedded_colors());
    assert!(
        fine.cells()
            .iter()
            .all(|cell| cell.pixels().pixels().any(|p| p[0] > 0))
    );
    let wood =
        Hose::from_bytes(include_bytes!("../../../../tests/fixtures/gih/wood1b.gih")).unwrap();
    assert_eq!(wood.cells().len(), 4);
    assert_eq!(
        wood.selections().collect::<Vec<_>>(),
        [(6, Selection::Random)]
    );
    assert!((0..100).all(|dab| wood.cell_index(dab, 1, mouse([1., 0.])).unwrap() < 4));
}

#[test]
fn selection_matches_gimp_incremental_rank_pressure_tilt_and_direction_rules() {
    let incremental = Hose::from_bytes(&hose("dim:1 rank0:3 sel0:incremental", 3)).unwrap();
    let selected: Vec<_> = (0..7)
        .map(|dab| incremental.cell_index(dab, 0, mouse([0.; 2])).unwrap())
        .collect();
    assert_eq!(selected, [1, 2, 0, 1, 2, 0, 1]);
    let legacy = Hose::from_bytes(&hose("", 3)).unwrap();
    assert_eq!(
        legacy.selections().collect::<Vec<_>>(),
        [(3, Selection::Incremental)]
    );
    let multidimensional = Hose::from_bytes(&hose(
        "dim:2 rank0:2 rank1:3 sel0:pressure sel1:incremental",
        6,
    ))
    .unwrap();
    assert_eq!(
        multidimensional.cell_index(0, 0, Dynamics::new(Some(0.), None, [0.; 2]).unwrap()),
        Some(1)
    );
    assert_eq!(
        multidimensional.cell_index(0, 0, Dynamics::new(Some(1.), None, [0.; 2]).unwrap()),
        Some(4)
    );
    let angular = Hose::from_bytes(&hose("dim:1 rank0:4 sel0:angular", 4)).unwrap();
    for (direction, expected) in [([1., 0.], 1), ([0., -1.], 0), ([-1., 0.], 3), ([0., 1.], 2)] {
        assert_eq!(angular.cell_index(0, 0, mouse(direction)), Some(expected));
    }
    assert_eq!(angular.cell_index(0, 0, mouse([0.; 2])), None);
    for mode in ["xtilt", "ytilt"] {
        let tilted = Hose::from_bytes(&hose(&format!("dim:1 rank0:5 sel0:{mode}"), 5)).unwrap();
        for (tilt, expected) in [(-90., 0), (0., 2), (90., 4)] {
            assert_eq!(
                tilted.cell_index(0, 0, Dynamics::new(None, Some([tilt; 2]), [0.; 2]).unwrap()),
                Some(expected)
            );
        }
    }
    let pressure = Hose::from_bytes(&hose("dim:1 rank0:3 sel0:pressure", 3)).unwrap();
    for (pressure_value, expected) in [(0., 0), (0.25, 0), (0.5, 1), (0.75, 2), (1., 2)] {
        assert_eq!(
            pressure.cell_index(
                0,
                0,
                Dynamics::new(Some(pressure_value), None, [0.; 2]).unwrap()
            ),
            Some(expected)
        );
    }
}

#[test]
fn random_selection_is_seeded_and_replays_identically_across_segment_boundaries() {
    let random = Hose::from_bytes(&hose("dim:1 rank0:5 sel0:random", 5)).unwrap();
    let whole: Vec<_> = (0..100)
        .map(|dab| random.cell_index(dab, 123, mouse([0.; 2])))
        .collect();
    let split: Vec<_> = (0..15)
        .chain(15..64)
        .chain(64..100)
        .map(|dab| random.cell_index(dab, 123, mouse([0.; 2])))
        .collect();
    assert_eq!(whole, split);
    assert_ne!(
        whole,
        (0..100)
            .map(|dab| random.cell_index(dab, 124, mouse([0.; 2])))
            .collect::<Vec<_>>()
    );
    for cell in 0..5 {
        assert!(whole.contains(&Some(cell)));
    }
}

#[test]
fn rejects_truncated_headers_cells_unsupported_dynamics_and_oversized_dimensions() {
    for params in [
        "dim:0",
        "dim:5",
        "dim:1 rank0:0",
        "rank0:513",
        "dim:4 rank0:512 rank1:512 rank2:512 rank3:512",
        "sel0:velocity",
        "sel0:unknown",
        "dim:1 dim:2",
        "missingcolon",
    ] {
        assert!(Hose::from_bytes(&hose(params, 1)).is_err(), "{params}");
    }
    let good = hose("dim:1 rank0:1 sel0:constant", 1);
    for len in 0..good.len() {
        assert!(Hose::from_bytes(&good[..len]).is_err(), "{len}");
    }
    let mut extra = good.clone();
    extra.push(0);
    assert!(Hose::from_bytes(&extra).is_err());
    assert!(Hose::from_bytes(&hose("", 0)).is_err());
    assert!(Hose::from_bytes(&hose("", 513)).is_err());
    assert!(Hose::from_bytes(format!("{}\n1\n", "x".repeat(1024)).as_bytes()).is_err());
    for pressure in [f64::NAN, f64::INFINITY] {
        assert!(Dynamics::new(Some(pressure), None, [1., 0.]).is_err());
    }
    assert!(Dynamics::new(None, Some([f64::NAN, 0.]), [1., 0.]).is_err());
    assert!(Dynamics::new(None, None, [f64::INFINITY, 0.]).is_err());
}

#[test]
fn supports_different_cell_sizes_and_rgba_coverage_without_silently_using_rgb() {
    let mut bytes = b"Mixed\n2 dim:1 rank0:2 sel0:incremental\n".to_vec();
    bytes.extend_from_slice(CELL);
    let mut larger = CELL.to_vec();
    larger[8..12].copy_from_slice(&2u32.to_be_bytes());
    larger[16..20].copy_from_slice(&4u32.to_be_bytes());
    larger.pop();
    larger.extend([10, 20, 30, 128, 90, 80, 70, 255]);
    bytes.extend(larger);
    let parsed = Hose::from_bytes(&bytes).unwrap();
    assert_eq!(parsed.cells()[0].pixels().dimensions(), (1, 1));
    assert_eq!(parsed.cells()[1].pixels().dimensions(), (2, 1));
    assert!(parsed.embedded_colors());
    assert_eq!(parsed.cells()[1].pixels().as_raw(), &[128, 255]);
}

#[test]
fn rejects_cumulative_cell_pixels_before_allocating_over_budget() {
    let mut cell = CELL.to_vec();
    cell[8..12].copy_from_slice(&1024u32.to_be_bytes());
    cell[12..16].copy_from_slice(&1024u32.to_be_bytes());
    let header = cell.len() - 1;
    cell.resize(header + 1024 * 1024, 255);
    let mut bytes = b"Large\n17 dim:1 rank0:17 sel0:random\n".to_vec();
    for _ in 0..17 {
        bytes.extend_from_slice(&cell);
    }
    let error = Hose::from_bytes(&bytes).unwrap_err().to_string();
    assert!(error.contains("16 million"), "{error}");
}

#[test]
#[cfg(unix)]
fn rejects_devices_and_fifos_without_blocking() {
    assert!(Hose::read(Path::new("/dev/zero")).is_err());
    let directory = tempfile::tempdir().unwrap();
    let fifo = directory.path().join("hose.gih");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    assert!(Hose::read(&fifo).is_err());
}
