//! Recover live primitives first; rasterize other solid Bézier paths when possible.
use super::ConversionReport;
use crate::{
    Result,
    document::{Shape, ShapeGeometry},
    geometry::Transform,
    invalid,
};
use ag_psd::psd as ps;
use image::RgbaImage;

pub(super) struct ImportedVector {
    pub shape: Option<Shape>,
    pub transform: Transform,
    pub pixels: RgbaImage,
}

pub(super) fn import(
    info: &ps::LayerAdditionalInfo,
    budget: &mut u64,
    report: &mut ConversionReport,
    name: &str,
) -> Result<Option<ImportedVector>> {
    let fill = info.vector_fill.as_ref().and_then(color);
    let enabled = info
        .vector_stroke
        .as_ref()
        .is_none_or(|s| s.fill_enabled != Some(false));
    let mask_usable = info
        .vector_mask
        .as_ref()
        .is_none_or(|m| m.invert != Some(true) && m.disable != Some(true));
    if enabled
        && mask_usable
        && let Some(rgb) = fill
        && let Some((geometry, radius, bounds)) = primitive(info)
    {
        let transform = placement(bounds, budget)?;
        let shape = Shape {
            geometry,
            corner_radius: radius,
            red: rgb[0],
            green: rgb[1],
            blue: rgb[2],
        };
        if info
            .vector_stroke
            .as_ref()
            .is_some_and(|s| s.stroke_enabled == Some(true))
        {
            report.note(format!(
                "{name}: editable shape fill is preserved; its Photoshop stroke is omitted."
            ));
        }
        let pixels =
            crate::edits::shape_pixels(transform.size[0] as u32, transform.size[1] as u32, shape);
        return Ok(Some(ImportedVector {
            shape: Some(shape),
            transform,
            pixels,
        }));
    }
    raster(info, budget, report, name)
}
fn color(content: &ps::VectorContent) -> Option<[f64; 3]> {
    let ps::VectorContent::Color(color) = content else {
        return None;
    };
    let rgb = match color {
        ps::Color::Rgb(v) => [v.r / 255., v.g / 255., v.b / 255.],
        ps::Color::Rgba(v) if v.a == 255. => [v.r / 255., v.g / 255., v.b / 255.],
        ps::Color::Frgb(v) => [v.fr, v.fg, v.fb],
        ps::Color::Grayscale(v) => [v.k / 255.; 3],
        _ => return None,
    };
    rgb.iter()
        .all(|v| v.is_finite() && (0. ..=1.).contains(v))
        .then_some(rgb)
}
fn pixels(value: ps::UnitsValue) -> Option<f64> {
    (value.units == ps::Units::Pixels && value.value.is_finite()).then_some(value.value)
}
fn primitive(info: &ps::LayerAdditionalInfo) -> Option<(ShapeGeometry, f64, [f64; 4])> {
    if let Some(origin) = &info.vector_origination {
        if origin.key_descriptor_list.len() != 1 {
            return None;
        }
        let item = &origin.key_descriptor_list[0];
        if item.key_shape_invalidated == Some(true)
            || item
                .transform
                .as_ref()
                .is_some_and(|t| t.as_slice() != [1., 0., 0., 1., 0., 0.])
        {
            return None;
        }
        let geometry = match item.key_origin_type? {
            1. | 2. => ShapeGeometry::Rectangle,
            5. => ShapeGeometry::Ellipse,
            _ => return None,
        };
        let bounds = item.key_origin_shape_bounding_box?;
        let bounds = [
            pixels(bounds.left)?,
            pixels(bounds.top)?,
            pixels(bounds.right)?,
            pixels(bounds.bottom)?,
        ];
        let radius = if item.key_origin_type == Some(2.) {
            let r = item.key_origin_r_rect_radii?;
            let values = [
                pixels(r.top_left)?,
                pixels(r.top_right)?,
                pixels(r.bottom_right)?,
                pixels(r.bottom_left)?,
            ];
            if values
                .iter()
                .any(|v| *v < 0. || (*v - values[0]).abs() > 0.5)
            {
                return None;
            }
            values[0]
        } else {
            0.
        };
        return Some((geometry, radius, bounds));
    }
    let paths = &info.vector_mask.as_ref()?.paths;
    if paths.len() != 1 || paths[0].open || paths[0].knots.len() != 4 {
        return None;
    }
    let mut anchors = Vec::new();
    for knot in &paths[0].knots {
        let [a, b, x, y, c, d] = knot.points.as_slice() else {
            return None;
        };
        if [a, b, x, y, c, d].iter().any(|v| !v.is_finite())
            || (a - x).abs() > 0.01
            || (c - x).abs() > 0.01
            || (b - y).abs() > 0.01
            || (d - y).abs() > 0.01
        {
            return None;
        }
        anchors.push([*x, *y]);
    }
    let bounds = anchors.iter().fold(
        [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ],
        |b, p| {
            [
                b[0].min(p[0]),
                b[1].min(p[1]),
                b[2].max(p[0]),
                b[3].max(p[1]),
            ]
        },
    );
    for i in 0..4 {
        let a = anchors[i];
        let b = anchors[(i + 1) % 4];
        if a == b || ((a[0] - b[0]).abs() > 0.01 && (a[1] - b[1]).abs() > 0.01) {
            return None;
        }
    }
    Some((ShapeGeometry::Rectangle, 0., bounds))
}
fn placement(bounds: [f64; 4], budget: &mut u64) -> Result<Transform> {
    if bounds
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
        || bounds[2] <= bounds[0]
        || bounds[3] <= bounds[1]
    {
        return Err(invalid(
            "PSD vector bounds are invalid or outside the supported coordinate range.",
        ));
    }
    let left = bounds[0].floor();
    let top = bounds[1].floor();
    let width = (bounds[2].ceil() - left) as u32;
    let height = (bounds[3].ceil() - top) as u32;
    crate::document::validate_size(width, height)?;
    *budget += u64::from(width) * u64::from(height);
    crate::document::validate_pixel_budget(*budget)?;
    let mut transform = Transform::new(width, height);
    transform.origin = [left, top];
    Ok(transform)
}
fn raster(
    info: &ps::LayerAdditionalInfo,
    budget: &mut u64,
    report: &mut ConversionReport,
    name: &str,
) -> Result<Option<ImportedVector>> {
    let Some(mask) = &info.vector_mask else {
        return Ok(None);
    };
    if mask.disable == Some(true)
        || mask.invert == Some(true)
        || mask.fill_starts_with_all_pixels == Some(true)
        || mask.paths.iter().any(|p| {
            matches!(
                p.operation,
                Some(ps::BooleanOperation::Intersect | ps::BooleanOperation::Subtract)
            )
        })
    {
        return Ok(None);
    }
    let fill = if info
        .vector_stroke
        .as_ref()
        .is_none_or(|s| s.fill_enabled != Some(false))
    {
        info.vector_fill.as_ref().and_then(color)
    } else {
        None
    };
    let stroke = info
        .vector_stroke
        .as_ref()
        .filter(|s| s.stroke_enabled == Some(true));
    let stroke_color = stroke.and_then(|s| s.content.as_ref()).and_then(color);
    if fill.is_none() && stroke_color.is_none() {
        return Ok(None);
    };
    let mut builder = tiny_skia::PathBuilder::new();
    for path in &mask.paths {
        if path.knots.is_empty() {
            continue;
        }
        if path.knots.iter().any(|k| {
            k.points.len() != 6
                || k.points
                    .iter()
                    .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
        }) {
            return Err(invalid("PSD vector path contains invalid control points."));
        }
        let first = &path.knots[0].points;
        builder.move_to(first[2] as f32, first[3] as f32);
        let count = path.knots.len();
        for i in 1..if path.open { count } else { count + 1 } {
            let prev = &path.knots[(i - 1) % count].points;
            let next = &path.knots[i % count].points;
            builder.cubic_to(
                prev[4] as f32,
                prev[5] as f32,
                next[0] as f32,
                next[1] as f32,
                next[2] as f32,
                next[3] as f32,
            );
        }
        if !path.open {
            builder.close();
        }
    }
    let Some(path) = builder.finish() else {
        return Ok(None);
    };
    let stroke_width = stroke
        .and_then(|s| s.line_width)
        .and_then(pixels)
        .unwrap_or(1.);
    if !(0. ..=30_000.).contains(&stroke_width) {
        return Err(invalid(
            "PSD vector stroke width is outside the supported range.",
        ));
    }
    let pad = if stroke_color.is_some() {
        stroke_width / 2. + 1.
    } else {
        0.
    };
    let bounds = path.bounds();
    let transform = placement(
        [
            f64::from(bounds.left()) - pad,
            f64::from(bounds.top()) - pad,
            f64::from(bounds.right()) + pad,
            f64::from(bounds.bottom()) + pad,
        ],
        budget,
    )?;
    let mut pixmap = tiny_skia::Pixmap::new(transform.size[0] as u32, transform.size[1] as u32)
        .ok_or_else(|| invalid("PSD vector raster allocation failed."))?;
    let translation = tiny_skia::Transform::from_translate(
        -transform.origin[0] as f32,
        -transform.origin[1] as f32,
    );
    let mut paint = tiny_skia::Paint::default();
    if let Some(rgb) = fill {
        paint.set_color_rgba8(
            (rgb[0] * 255.).round() as u8,
            (rgb[1] * 255.).round() as u8,
            (rgb[2] * 255.).round() as u8,
            255,
        );
        let rule = if mask.paths.iter().any(|p| {
            p.fill_rule == ps::FillRule::EvenOdd
                || p.operation == Some(ps::BooleanOperation::Exclude)
        }) {
            tiny_skia::FillRule::EvenOdd
        } else {
            tiny_skia::FillRule::Winding
        };
        pixmap.fill_path(&path, &paint, rule, translation, None);
    }
    if let Some(rgb) = stroke_color
        && let Some(s) = stroke
    {
        paint.set_color_rgba8(
            (rgb[0] * 255.).round() as u8,
            (rgb[1] * 255.).round() as u8,
            (rgb[2] * 255.).round() as u8,
            (s.opacity.unwrap_or(1.).clamp(0., 1.) * 255.).round() as u8,
        );
        let style = tiny_skia::Stroke {
            width: stroke_width as f32,
            line_cap: match s.line_cap_type {
                Some(ps::LineCapType::Round) => tiny_skia::LineCap::Round,
                Some(ps::LineCapType::Square) => tiny_skia::LineCap::Square,
                _ => tiny_skia::LineCap::Butt,
            },
            line_join: match s.line_join_type {
                Some(ps::LineJoinType::Round) => tiny_skia::LineJoin::Round,
                Some(ps::LineJoinType::Bevel) => tiny_skia::LineJoin::Bevel,
                _ => tiny_skia::LineJoin::Miter,
            },
            ..Default::default()
        };
        pixmap.stroke_path(&path, &paint, &style, translation, None);
        if s.line_alignment
            .is_some_and(|a| a != ps::LineAlignment::Center)
            || s.line_dash_set.as_ref().is_some_and(|d| !d.is_empty())
            || s.blend_mode.is_some_and(|b| b != ps::BlendMode::Normal)
        {
            report.note(format!("{name}: vector stroke uses a solid centered Normal stroke; Photoshop alignment, dashes, and blending may differ."));
        }
    }
    let data: Vec<u8> = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    let image = RgbaImage::from_raw(pixmap.width(), pixmap.height(), data)
        .ok_or_else(|| invalid("PSD vector raster dimensions are invalid."))?;
    report.note(format!(
        "{name}: Bézier vector paths are rasterized; their geometry is not editable."
    ));
    Ok(Some(ImportedVector {
        shape: None,
        transform,
        pixels: image,
    }))
}
