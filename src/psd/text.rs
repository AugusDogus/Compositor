//! Photoshop type conversion uses the native text engine and explicit placement anchors.
use super::ConversionReport;
use crate::{
    Result,
    geometry::Transform,
    invalid,
    text::{Alignment, Text, TextRenderer},
};
use ag_psd::psd as ps;
use image::RgbaImage;

pub(super) struct ImportedText {
    pub style: Text,
    pub transform: Transform,
    pub pixels: RgbaImage,
}

pub(super) fn import(
    source: &ps::LayerTextData,
    budget: &mut u64,
    report: &mut ConversionReport,
    name: &str,
) -> Result<Option<ImportedText>> {
    let Some((style, matrix, frame)) = settings(source, report, name) else {
        report.note(format!("{name}: unsupported Photoshop text geometry or styling is kept as saved pixels; it cannot be retyped."));
        return Ok(None);
    };
    let mut renderer = TextRenderer::default();
    if !renderer
        .font_names()
        .iter()
        .any(|font| font.eq_ignore_ascii_case(&style.font_name))
    {
        report.note(format!(
            "{name}: font '{}' is not installed; text uses the bundled Inter fallback.",
            style.font_name
        ));
    }
    let (pixels, baseline) = renderer.render_with_baseline(&style)?;
    *budget += u64::from(pixels.width()) * u64::from(pixels.height());
    crate::document::validate_pixel_budget(*budget)?;
    let [xx, xy, yx, yy, tx, ty] = matrix;
    let scale = xx.hypot(yx);
    let anchor = if let Some([left, top]) = frame {
        [xx * left + xy * top + tx, yx * left + yy * top + ty]
    } else {
        [tx, ty]
    };
    let image_anchor = if frame.is_some() {
        [12., 12.]
    } else {
        [
            match style.alignment {
                Alignment::Left => 12.,
                Alignment::Center => f64::from(pixels.width()) / 2.,
                Alignment::Right => f64::from(pixels.width()) - 12.,
            },
            baseline,
        ]
    };
    let mut transform = Transform::new(pixels.width(), pixels.height());
    transform.rotation = yx.atan2(xx).to_degrees();
    transform.flip_y = (xx * yy - yx * xy) / scale < 0.;
    let current = transform.point([
        image_anchor[0] / transform.size[0],
        image_anchor[1] / transform.size[1],
    ]);
    transform.origin = [anchor[0] - current[0], anchor[1] - current[1]];
    if !transform.valid() {
        return Err(invalid(
            "Photoshop text placement exceeds supported coordinates.",
        ));
    }
    report.note(format!(
        "{name}: editable text is preserved; font shaping may differ from Photoshop."
    ));
    Ok(Some(ImportedText {
        style,
        transform,
        pixels,
    }))
}

type Settings = (Text, [f64; 6], Option<[f64; 2]>);
fn settings(
    source: &ps::LayerTextData,
    report: &mut ConversionReport,
    name: &str,
) -> Option<Settings> {
    if source.orientation == Some(ps::Orientation::Vertical) || source.text_path.is_some() {
        return None;
    }
    let matrix: [f64; 6] = source
        .transform
        .as_deref()
        .unwrap_or(&[1., 0., 0., 1., 0., 0.])
        .try_into()
        .ok()?;
    if !matrix.iter().all(|v| v.is_finite()) {
        return None;
    }
    let [xx, xy, yx, yy, _, _] = matrix;
    let scale = xx.hypot(yx);
    if scale < 1e-6 {
        return None;
    }
    let shear = (xx * xy + yx * yy) / scale;
    let scale_y = ((xx * yy - yx * xy) / scale).abs();
    if scale_y < 1e-6
        || shear.abs() > 0.02 * scale.max(scale_y)
        || (scale - scale_y).abs() > 0.02 * scale.max(scale_y)
    {
        return None;
    }
    let first = source
        .style_runs
        .as_ref()
        .and_then(|runs| runs.first())
        .map(|r| &r.style);
    let default = ps::TextStyle::default();
    let base = source.style.as_ref().unwrap_or(&default);
    let run = first.unwrap_or(base);
    let size = run.font_size.or(base.font_size).unwrap_or(12.) * scale;
    let rgb = color(run.fill_color.or(base.fill_color))?;
    let paragraph = source
        .paragraph_style_runs
        .as_ref()
        .and_then(|r| r.first())
        .map(|r| &r.style)
        .or(source.paragraph_style.as_ref());
    let alignment = match paragraph.and_then(|p| p.justification) {
        Some(ps::Justification::Center) => Alignment::Center,
        Some(ps::Justification::Right) => Alignment::Right,
        Some(ps::Justification::Left) | None => Alignment::Left,
        _ => {
            report.note(format!(
                "{name}: full justification is converted to left alignment."
            ));
            Alignment::Left
        }
    };
    let content = source
        .text
        .trim_start_matches(['\u{feff}', '\0'])
        .trim_end_matches('\0')
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    if content.is_empty() {
        return None;
    }
    let mut style = Text {
        content,
        font_name: run
            .font
            .as_ref()
            .or(base.font.as_ref())
            .map_or("Inter Variable", |font| font.name.as_str())
            .into(),
        font_size: size.clamp(1., 2000.),
        red: rgb[0],
        green: rgb[1],
        blue: rgb[2],
        alignment,
        tracking: (run.tracking.or(base.tracking).unwrap_or(0.) * size / 1000.).clamp(-100., 1000.),
        leading: if run.auto_leading.or(base.auto_leading) == Some(false) {
            (run.leading.or(base.leading).unwrap_or(0.) * scale).clamp(0., 5000.)
        } else {
            0.
        },
        box_size: None,
    };
    if !size.is_finite() || size <= 0. {
        return None;
    }
    let frame = paragraph_frame(source)?;
    if let Some([left, top, right, bottom]) = frame {
        style.box_size = Some([(right - left) * scale + 24., (bottom - top) * scale + 24.]);
    }
    style.validate().ok()?;
    if source.style_runs.as_ref().is_some_and(|r| r.len() > 1)
        || source
            .paragraph_style_runs
            .as_ref()
            .is_some_and(|r| r.len() > 1)
    {
        report.note(format!(
            "{name}: only the first Photoshop text and paragraph style is retained."
        ));
    }
    if source
        .warp
        .as_ref()
        .and_then(|w| w.style)
        .is_some_and(|s| s != ps::WarpStyle::None)
    {
        report.note(format!("{name}: Photoshop text warp is omitted."));
    }
    if run.faux_bold.or(base.faux_bold) == Some(true)
        || run.faux_italic.or(base.faux_italic) == Some(true)
    {
        report.note(format!("{name}: faux bold or italic is omitted."));
    }
    if run
        .horizontal_scale
        .or(base.horizontal_scale)
        .is_some_and(|v| v != 1.)
        || run
            .vertical_scale
            .or(base.vertical_scale)
            .is_some_and(|v| v != 1.)
        || run
            .baseline_shift
            .or(base.baseline_shift)
            .is_some_and(|v| v != 0.)
    {
        report.note(format!(
            "{name}: per-character scale and baseline shift are omitted."
        ));
    }
    Some((style, matrix, frame.map(|r| [r[0], r[1]])))
}

fn paragraph_frame(source: &ps::LayerTextData) -> Option<Option<[f64; 4]>> {
    if source.shape_type == Some(ps::TextShapeType::Box) {
        if let Some(bounds) = source.box_bounds.as_deref() {
            return Some(Some(bounds.try_into().ok()?));
        }
        let bounds = source.bounds?;
        return Some(Some([
            bounds.left.value,
            bounds.top.value,
            bounds.right.value,
            bounds.bottom.value,
        ]));
    }
    // Older Photoshop files expose only descriptor bounds, without a shape type.
    if let (Some(bounds), Some(glyphs)) = (source.bounds, source.bounding_box)
        && bounds.right.value - bounds.left.value > glyphs.right.value - glyphs.left.value + 4.
        && bounds.bottom.value - bounds.top.value > glyphs.bottom.value - glyphs.top.value + 4.
    {
        return Some(Some([
            bounds.left.value,
            bounds.top.value,
            bounds.right.value,
            bounds.bottom.value,
        ]));
    }
    Some(None)
}
fn color(color: Option<ps::Color>) -> Option<[f64; 3]> {
    Some(match color {
        None => [0.; 3],
        Some(ps::Color::Rgb(v)) => [v.r / 255., v.g / 255., v.b / 255.],
        Some(ps::Color::Rgba(v)) if v.a == 255. => [v.r / 255., v.g / 255., v.b / 255.],
        Some(ps::Color::Frgb(v)) => [v.fr, v.fg, v.fb],
        Some(ps::Color::Grayscale(v)) => [v.k / 255.; 3],
        _ => return None,
    })
}
