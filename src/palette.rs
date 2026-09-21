use crate::{Result, invalid};

pub fn parse_hex(text: &str) -> Result<[u8; 4]> {
    let text = text.trim();
    let text = text.strip_prefix('#').unwrap_or(text);
    if !text.is_ascii() || !matches!(text.len(), 3 | 6) {
        return Err(invalid(
            "Enter a three- or six-digit hexadecimal color, such as #F80 or #FF8800.",
        ));
    }
    let value = u32::from_str_radix(text, 16)
        .map_err(|_| invalid("Colors use hexadecimal digits 0 to 9 and A to F."))?;
    Ok(if text.len() == 3 {
        [
            ((value >> 8) & 15) as u8 * 17,
            ((value >> 4) & 15) as u8 * 17,
            (value & 15) as u8 * 17,
            255,
        ]
    } else {
        [(value >> 16) as u8, (value >> 8) as u8, value as u8, 255]
    })
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Hsb {
    pub hue: f64,
    pub saturation: f64,
    pub brightness: f64,
}

impl Hsb {
    pub fn new(rgb: [u8; 4]) -> Self {
        let mut value = Self::default();
        value.set_rgb(rgb);
        value
    }

    /// Preserve hue through grays, and saturation through black, while dragging the picker.
    pub fn set_rgb(&mut self, rgb: [u8; 4]) {
        let [r, g, b, _] = rgb.map(|v| v as f64 / 255.);
        let high = r.max(g).max(b);
        let low = r.min(g).min(b);
        let delta = high - low;
        self.brightness = high;
        if high > 0. {
            self.saturation = delta / high;
        }
        if delta == 0. {
            return;
        }
        self.hue = (60.
            * if high == r {
                (g - b) / delta
            } else if high == g {
                (b - r) / delta + 2.
            } else {
                (r - g) / delta + 4.
            })
        .rem_euclid(360.);
    }

    pub fn rgb(self) -> [u8; 4] {
        let h = self.hue.rem_euclid(360.) / 60.;
        let c = self.brightness * self.saturation;
        let x = c * (1. - (h.rem_euclid(2.) - 1.).abs());
        let m = self.brightness - c;
        let rgb = match h as u8 {
            0 => [c, x, 0.],
            1 => [x, c, 0.],
            2 => [0., c, x],
            3 => [0., x, c],
            4 => [x, 0., c],
            _ => [c, 0., x],
        }
        .map(|v| ((v + m).clamp(0., 1.) * 255.).round() as u8);
        [rgb[0], rgb[1], rgb[2], 255]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hex_and_hsb_round_trip_eight_bit_colors() {
        for text in [
            "000000", "FFFFFF", "FF0000", "00FF00", "0000FF", "FF8000", "7F3FA2", "123456",
        ] {
            let rgb = parse_hex(text).unwrap();
            assert_eq!(Hsb::new(rgb).rgb(), rgb);
        }
        assert_eq!(parse_hex("  #0f0 ").unwrap(), [0, 255, 0, 255]);
        for text in ["12345", "GGGGGG", "##FFFFFF", "é00"] {
            assert!(parse_hex(text).is_err());
        }
    }
    #[test]
    fn neutral_colors_retain_previous_hue_and_black_retains_saturation() {
        let mut hsb = Hsb::new([255, 128, 0, 255]);
        let hue = hsb.hue;
        hsb.set_rgb([128, 128, 128, 255]);
        assert_eq!(hsb.hue, hue);
        assert_eq!(hsb.saturation, 0.);
        hsb.saturation = 0.5;
        hsb.set_rgb([0, 0, 0, 255]);
        assert_eq!(hsb.hue, hue);
        assert_eq!(hsb.saturation, 0.5);
        assert_eq!(hsb.brightness, 0.);
    }
}
