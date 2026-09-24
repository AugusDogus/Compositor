//! Photoshop's supported editable adjustment subset. Values remain in PSD units.
use crate::{
    Result,
    adjustment::{
        Adjustment, ColorRange, CurvePoint, HueBand, HueSaturation, Kind, LevelRange,
        RangeAdjustment,
    },
    invalid,
};
use ag_psd::psd as ps;

const RANGES: [ColorRange; 7] = [
    ColorRange::Master,
    ColorRange::Reds,
    ColorRange::Yellows,
    ColorRange::Greens,
    ColorRange::Cyans,
    ColorRange::Blues,
    ColorRange::Magentas,
];

pub(super) fn import(source: &ps::AdjustmentLayer) -> Result<Option<Adjustment>> {
    let mut out = match source {
        ps::AdjustmentLayer::Levels(levels) => {
            let mut out = Adjustment::new(Kind::Levels);
            for (target, source) in out.levels.ranges.iter_mut().zip([
                &levels.rgb,
                &levels.red,
                &levels.green,
                &levels.blue,
            ]) {
                if let Some(v) = source {
                    *target = LevelRange {
                        black: v.shadow_input,
                        white: v.highlight_input,
                        output_black: v.shadow_output,
                        output_white: v.highlight_output,
                        gamma: v.midtone_input,
                    };
                }
            }
            out
        }
        ps::AdjustmentLayer::Curves(curves) => {
            let mut out = Adjustment::new(Kind::Curves);
            for (target, source) in out.curves.channels.iter_mut().zip([
                &curves.rgb,
                &curves.red,
                &curves.green,
                &curves.blue,
            ]) {
                if let Some(points) = source {
                    if points.is_empty()
                        || points
                            .iter()
                            .any(|p| !p.input.is_finite() || !p.output.is_finite())
                    {
                        return Err(invalid(
                            "PSD curve has no points or contains non-finite coordinates.",
                        ));
                    }
                    *target = points
                        .iter()
                        .map(|p| CurvePoint {
                            x: p.input.clamp(0., 255.),
                            y: p.output.clamp(0., 255.),
                        })
                        .collect();
                    target.sort_by(|a, b| a.x.total_cmp(&b.x));
                    if let Some(first) = target.first().copied()
                        && first.x != 0.
                    {
                        target.insert(0, CurvePoint { x: 0., y: first.y });
                    }
                    if let Some(last) = target.last().copied()
                        && last.x != 255.
                    {
                        target.push(CurvePoint { x: 255., y: last.y });
                    }
                }
            }
            out
        }
        ps::AdjustmentLayer::HueSaturation(hue) => {
            let mut out = Adjustment::new(Kind::HueSaturation);
            let mut settings = HueSaturation::default();
            for (range, channel) in RANGES.into_iter().zip([
                &hue.master,
                &hue.reds,
                &hue.yellows,
                &hue.greens,
                &hue.cyans,
                &hue.blues,
                &hue.magentas,
            ]) {
                if let Some(v) = channel {
                    settings.adjustments.push((
                        range,
                        RangeAdjustment {
                            hue: v.hue,
                            saturation: v.saturation,
                            lightness: v.lightness,
                        },
                    ));
                    if range != ColorRange::Master {
                        settings.bands.push((
                            range,
                            HueBand {
                                falloff_start: v.a,
                                range_start: v.b,
                                range_end: v.c,
                                falloff_end: v.d,
                            },
                        ));
                    }
                }
            }
            // ag-psd exposes the first eight hue2 bytes as master.a/b/c/d.
            // In the PSD specification these are the colorize flag + padding,
            // followed by colorize hue/saturation/lightness, not hue-band bounds.
            if let Some(master) = &hue.master
                && master.a != 0.
            {
                if master.a != 256. {
                    return Err(invalid("PSD Hue/Saturation colorize flag is invalid."));
                }
                settings.colorize = true;
                settings.adjustments = vec![(
                    ColorRange::Master,
                    RangeAdjustment {
                        hue: master.b,
                        saturation: master.c,
                        lightness: master.d,
                    },
                )];
            }
            out.hsv_settings = Some(settings);
            out
        }
        _ => match super::color_adjustments::import(source) {
            Some(value) => value,
            None => return Ok(None),
        },
    };
    out.validate()?;
    // Retain legacy master fields for other project consumers as well.
    if let Some(hsv) = &out.hsv_settings {
        out.colorize = hsv.colorize;
        if let Some((_, master)) = hsv
            .adjustments
            .iter()
            .find(|(range, _)| *range == ColorRange::Master)
        {
            out.hue = master.hue;
            out.saturation = master.saturation;
            out.lightness = master.lightness;
        }
    }
    Ok(Some(out))
}

pub(super) fn export(source: &Adjustment) -> Option<ps::AdjustmentLayer> {
    Some(match source.kind {
        Kind::Levels => {
            let [rgb, red, green, blue] = source.levels.ranges.map(|v| {
                Some(ps::LevelsAdjustmentChannel {
                    shadow_input: v.black,
                    highlight_input: v.white,
                    shadow_output: v.output_black,
                    highlight_output: v.output_white,
                    midtone_input: v.gamma,
                })
            });
            ps::AdjustmentLayer::Levels(ps::LevelsAdjustment {
                rgb,
                red,
                green,
                blue,
                ..Default::default()
            })
        }
        Kind::Curves => {
            let [rgb, red, green, blue] = source.curves.channels.each_ref().map(|points| {
                Some(
                    points
                        .iter()
                        .map(|p| ps::CurvesPoint {
                            input: p.x,
                            output: p.y,
                        })
                        .collect(),
                )
            });
            ps::AdjustmentLayer::Curves(ps::CurvesAdjustment {
                rgb,
                red,
                green,
                blue,
                ..Default::default()
            })
        }
        Kind::HueSaturation => {
            let fallback = HueSaturation {
                colorize: source.colorize,
                adjustments: vec![(
                    ColorRange::Master,
                    RangeAdjustment {
                        hue: source.hue,
                        saturation: source.saturation,
                        lightness: source.lightness,
                    },
                )],
                ..Default::default()
            };
            let settings = source.hsv_settings.as_ref().unwrap_or(&fallback);
            if settings.invert_range {
                return None;
            }
            let [mut master, reds, yellows, greens, cyans, blues, magentas] = RANGES.map(|range| {
                let v = settings
                    .adjustments
                    .iter()
                    .find(|(r, _)| *r == range)
                    .map_or(RangeAdjustment::default(), |(_, v)| *v);
                let band = settings.band(range);
                Some(ps::HueSaturationAdjustmentChannel {
                    a: band.falloff_start,
                    b: band.range_start,
                    c: band.range_end,
                    d: band.falloff_end,
                    hue: (v.hue + 180.).rem_euclid(360.) - 180.,
                    saturation: v.saturation,
                    lightness: v.lightness,
                })
            });
            if let Some(master) = &mut master {
                master.a = 0.;
                master.b = 0.;
                master.c = 0.;
                master.d = 0.;
                if settings.colorize {
                    let v = settings
                        .adjustments
                        .iter()
                        .find(|(r, _)| *r == settings.range)
                        .map_or(RangeAdjustment::default(), |(_, v)| *v);
                    master.a = 256.;
                    master.b = v.hue.rem_euclid(360.);
                    master.c = v.saturation.max(0.);
                    master.d = v.lightness;
                    master.hue = 0.;
                    master.saturation = 0.;
                    master.lightness = 0.;
                }
            }
            ps::AdjustmentLayer::HueSaturation(ps::HueSaturationAdjustment {
                master,
                reds,
                yellows,
                greens,
                cyans,
                blues,
                magentas,
                ..Default::default()
            })
        }
        _ => return super::color_adjustments::export(source),
    })
}

// ag-psd's document reader may swallow additional-info errors even in strict
// mode. Validate supported records explicitly before editable data can be lost.
pub(super) fn validate_record(key: &[u8], payload: &[u8]) -> Result<()> {
    let key = match key {
        b"levl" => "levl",
        b"curv" => "curv",
        b"hue2" => "hue2",
        b"blnc" => "blnc",
        b"blwh" => "blwh",
        b"nvrt" => "nvrt",
        _ => return Ok(()),
    };
    let mut reader = ag_psd::reader::PsdReader::new(payload, None, None);
    reader.strict = true;
    let mut info = ps::LayerAdditionalInfo::default();
    let options = ps::ReadOptions::default();
    let mut context = ag_psd::additional_info::ReadCtx {
        options: &options,
        large: false,
    };
    ag_psd::additional_info::read_additional_info_key(
        key,
        &mut reader,
        &mut info,
        &|r| payload.len().saturating_sub(r.offset),
        &mut context,
    )
    .map_err(|e| {
        invalid(format!(
            "PSD {key} adjustment data is invalid: {e}. The current document is unchanged."
        ))
    })?;
    if let Some(adjustment) = info.adjustment {
        import(&adjustment)?;
    }
    Ok(())
}
