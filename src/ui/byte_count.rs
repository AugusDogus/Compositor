//! English byte-count labels using Foundation's adaptive File/Memory defaults.
//! Bytes and KB have no decimals, MB has at most one, larger units at most two.

pub(super) enum Style {
    File,
    Memory,
}

impl Style {
    pub(super) fn format(&self, bytes: u64) -> String {
        if bytes == 0 {
            return "Zero KB".into();
        }
        if bytes == 1 {
            return "1 byte".into();
        }
        let base = match self {
            Self::File => 1000_u128,
            Self::Memory => 1024_u128,
        };
        let units = ["bytes", "KB", "MB", "GB", "TB", "PB", "EB"];
        let mut unit = 0;
        let mut divisor = 1_u128;
        while unit + 1 < units.len() && u128::from(bytes) >= divisor * base {
            divisor *= base;
            unit += 1;
        }
        let decimals = match unit {
            0 | 1 => 0,
            2 => 1,
            _ => 2,
        };
        let precision = 10_u128.pow(decimals);
        let scaled = u128::from(bytes) * precision;
        let mut rounded = scaled / divisor;
        // NumberFormatter defaults to half-even rounding. Integer arithmetic keeps
        // exact ties and unit boundaries independent of floating-point conversion.
        let remainder = (scaled % divisor) * 2;
        if remainder > divisor || (remainder == divisor && rounded % 2 == 1) {
            rounded += 1;
        }
        let digits = (rounded / precision).to_string();
        let mut number = String::new();
        for (index, digit) in digits.chars().enumerate() {
            if index > 0 && (digits.len() - index).is_multiple_of(3) {
                number.push(',');
            }
            number.push(digit);
        }
        if decimals > 0 {
            let fraction = format!("{:0width$}", rounded % precision, width = decimals as usize);
            let fraction = fraction.trim_end_matches('0');
            if !fraction.is_empty() {
                number.push('.');
                number.push_str(fraction);
            }
        }
        format!("{number} {}", units[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::Style;

    #[test]
    fn adaptive_byte_labels_distinguish_files_from_memory() {
        for (bytes, file, memory) in [
            (0, "Zero KB", "Zero KB"),
            (1, "1 byte", "1 byte"),
            (999, "999 bytes", "999 bytes"),
            (1000, "1 KB", "1,000 bytes"),
            (1024, "1 KB", "1 KB"),
            (2500, "2 KB", "2 KB"),
            (3500, "4 KB", "3 KB"),
            (184_900, "185 KB", "181 KB"),
            (999_999, "1,000 KB", "977 KB"),
            (1_000_000, "1 MB", "977 KB"),
            (1_048_576, "1 MB", "1 MB"),
            (4_194_304, "4.2 MB", "4 MB"),
            (3_600_000_000, "3.6 GB", "3.35 GB"),
            (u64::MAX, "18.45 EB", "16 EB"),
        ] {
            assert_eq!(Style::File.format(bytes), file, "file size {bytes}");
            assert_eq!(Style::Memory.format(bytes), memory, "memory size {bytes}");
        }
    }
}
