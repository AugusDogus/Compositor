//! Separate allocation limits for one surface and the retained document stack.
use std::sync::OnceLock;

pub const MAX_SURFACE_PIXELS: u64 = 200_000_000;

pub fn document_pixel_budget() -> u64 {
    static BUDGET: OnceLock<u64> = OnceLock::new();
    *BUDGET.get_or_init(|| {
        let memory = std::fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|text| physical_memory(&text))
            .unwrap_or(0);
        budget_for_memory(memory)
    })
}

fn budget_for_memory(bytes: u64) -> u64 {
    (bytes / 16).clamp(MAX_SURFACE_PIXELS, 800_000_000)
}
fn physical_memory(meminfo: &str) -> Option<u64> {
    let line = meminfo.lines().find(|line| line.starts_with("MemTotal:"))?;
    let mut fields = line.split_whitespace();
    fields.next()?;
    let kib = fields.next()?.parse::<u64>().ok()?;
    (fields.next()? == "kB").then_some(kib)?.checked_mul(1024)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn budget_scales_with_memory_and_is_bounded() {
        assert_eq!(budget_for_memory(0), 200_000_000);
        assert_eq!(budget_for_memory(2 * 1024 * 1024 * 1024), 200_000_000);
        assert_eq!(budget_for_memory(8 * 1024 * 1024 * 1024), 536_870_912);
        assert_eq!(budget_for_memory(16 * 1024 * 1024 * 1024), 800_000_000);
        assert_eq!(budget_for_memory(u64::MAX), 800_000_000);
    }
    #[test]
    fn linux_memory_reader_checks_units_and_overflow() {
        assert_eq!(
            physical_memory("MemTotal: 8388608 kB\nMemFree: 12 kB"),
            Some(8 * 1024 * 1024 * 1024)
        );
        for invalid in [
            "",
            "MemTotal: 2 MB",
            "MemTotal: x kB",
            "MemTotal: 18446744073709551615 kB",
        ] {
            assert_eq!(physical_memory(invalid), None);
        }
    }
}
