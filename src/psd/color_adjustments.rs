//! Editable Photoshop color adjustments added alongside their native equivalents.
use crate::adjustment::{Adjustment, BlackWhite, ColorBalance, Kind, hsl_to_rgb, rgb_to_hsl};
use ag_psd::psd as ps;

pub(super) fn import(source: &ps::AdjustmentLayer) -> Option<Adjustment> {
    Some(match source {
        ps::AdjustmentLayer::Invert(_) => Adjustment::new(Kind::Invert),
        ps::AdjustmentLayer::ColorBalance(s) => {
            let zero = ps::ColorBalanceValues::default();
            let shadows = s.shadows.as_ref().unwrap_or(&zero);
            let midtones = s.midtones.as_ref().unwrap_or(&zero);
            let highlights = s.highlights.as_ref().unwrap_or(&zero);
            let mut out = Adjustment::new(Kind::ColorBalance);
            out.color_balance_settings = Some(ColorBalance {
                shadow_cyan_red: shadows.cyan_red,
                shadow_magenta_green: shadows.magenta_green,
                shadow_yellow_blue: shadows.yellow_blue,
                mid_cyan_red: midtones.cyan_red,
                mid_magenta_green: midtones.magenta_green,
                mid_yellow_blue: midtones.yellow_blue,
                highlight_cyan_red: highlights.cyan_red,
                highlight_magenta_green: highlights.magenta_green,
                highlight_yellow_blue: highlights.yellow_blue,
                preserve_luminosity: s.preserve_luminosity.unwrap_or(true),
            });
            out
        }
        ps::AdjustmentLayer::BlackAndWhite(s) => {
            let defaults = BlackWhite::default();
            let [hue, saturation, _] = match s.tint_color {
                Some(ps::Color::Rgb(c)) => rgb_to_hsl([c.r / 255., c.g / 255., c.b / 255.]),
                Some(ps::Color::Frgb(c)) => rgb_to_hsl([c.fr, c.fg, c.fb]),
                Some(ps::Color::Rgba(c)) => rgb_to_hsl([c.r / 255., c.g / 255., c.b / 255.]),
                None => [defaults.tint_hue, defaults.tint_saturation / 100., 0.5],
                _ if s.use_tint != Some(true) => {
                    [defaults.tint_hue, defaults.tint_saturation / 100., 0.5]
                }
                _ => return None,
            };
            let mut out = Adjustment::new(Kind::BlackWhite);
            out.black_white_settings = Some(BlackWhite {
                reds: s.reds.unwrap_or(defaults.reds),
                yellows: s.yellows.unwrap_or(defaults.yellows),
                greens: s.greens.unwrap_or(defaults.greens),
                cyans: s.cyans.unwrap_or(defaults.cyans),
                blues: s.blues.unwrap_or(defaults.blues),
                magentas: s.magentas.unwrap_or(defaults.magentas),
                tint: s.use_tint.unwrap_or(false),
                tint_hue: hue,
                tint_saturation: saturation * 100.,
            });
            out
        }
        _ => return None,
    })
}
pub(super) fn export(source: &Adjustment) -> Option<ps::AdjustmentLayer> {
    Some(match source.kind {
        Kind::Invert => ps::AdjustmentLayer::Invert(ps::InvertAdjustment),
        Kind::ColorBalance => {
            let s = source.color_balance_settings.unwrap_or_default();
            let [shadows, midtones, highlights] =
                s.ranges().map(|[cyan_red, magenta_green, yellow_blue]| {
                    Some(ps::ColorBalanceValues {
                        cyan_red,
                        magenta_green,
                        yellow_blue,
                    })
                });
            ps::AdjustmentLayer::ColorBalance(ps::ColorBalanceAdjustment {
                shadows,
                midtones,
                highlights,
                preserve_luminosity: Some(s.preserve_luminosity),
            })
        }
        Kind::BlackWhite => {
            let s = source.black_white_settings.unwrap_or_default();
            let [r, g, b] =
                hsl_to_rgb([s.tint_hue, s.tint_saturation / 100., 0.5]).map(|v| v * 255.);
            ps::AdjustmentLayer::BlackAndWhite(ps::BlackAndWhiteAdjustment {
                reds: Some(s.reds),
                yellows: Some(s.yellows),
                greens: Some(s.greens),
                cyans: Some(s.cyans),
                blues: Some(s.blues),
                magentas: Some(s.magentas),
                use_tint: Some(s.tint),
                tint_color: Some(ps::Color::Rgb(ps::Rgb { r, g, b })),
                ..Default::default()
            })
        }
        _ => return None,
    })
}
