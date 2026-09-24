//! Pixel sampling helpers shared by the eyedropper and targeted grading tools.
use super::*;
pub fn sample(source: &RgbaImage, unit: [f64; 2]) -> Option<[f64; 3]> {
    if unit
        .iter()
        .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
    {
        return None;
    }
    let (x, y) = (
        (unit[0] * f64::from(source.width())).floor() as i64,
        (unit[1] * f64::from(source.height())).floor() as i64,
    );
    let mut sum = [0.; 3];
    let mut weight = 0.;
    for dy in -2..=2 {
        for dx in -2..=2 {
            let pixel = source[(
                (x + dx).clamp(0, i64::from(source.width()) - 1) as u32,
                (y + dy).clamp(0, i64::from(source.height()) - 1) as u32,
            )];
            let alpha = f64::from(pixel[3]) / 255.;
            weight += alpha;
            for c in 0..3 {
                sum[c] += f64::from(pixel[c]) / 255. * alpha;
            }
        }
    }
    (weight > 0.).then(|| sum.map(|v| v / weight))
}
pub fn white_balance(rgb: [f64; 3]) -> Result<[f64; 2]> {
    let linear = rgb.map(|v| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    });
    white_balance_linear(linear).ok_or_else(|| {
        invalid(
            "This pixel has too little color information for white balance. Pick a lighter neutral area.",
        )
    })
}
/// Solve the grade's temperature and tint gains from a linear RGB neutral.
pub(super) fn white_balance_linear([r, g, b]: [f64; 3]) -> Option<[f64; 2]> {
    let (a1, b1, c1) = (0.35 * r, 0.15 * r + 0.30 * g, g - r);
    let (a2, b2, c2) = (-0.35 * b, 0.15 * b + 0.30 * g, g - b);
    let determinant = a1 * b2 - a2 * b1;
    if r < 1e-4 || g < 1e-4 || b < 1e-4 || determinant.abs() < 1e-8 {
        return None;
    }
    Some([
        ((c1 * b2 - c2 * b1) / determinant * 100.).clamp(-100., 100.),
        ((a1 * c2 - a2 * c1) / determinant * 100.).clamp(-100., 100.),
    ])
}

pub fn hsl([r, g, b]: [f64; 3]) -> [f64; 3] {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.;
    let d = max - min;
    if d < 1e-6 {
        return [0., 0., l];
    }
    let s = d / (1. - (2. * l - 1.).abs());
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.)
    } else if max == g {
        (b - r) / d + 2.
    } else {
        (r - g) / d + 4.
    };
    [h * 60., s, l]
}
