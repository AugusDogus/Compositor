//! Native bitmap equivalents of EditorCanvas.swift's outlined vector cursors.
use compositor::{Result, invalid};
use quickgui::{CursorImage, Image};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum Badge {
    New,
    Add,
    Subtract,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum Glyph {
    Rectangle(Badge),
    Ellipse(Badge),
    Lasso(Badge),
    Polygon(Badge),
    Wand(Badge),
    Object(Badge),
    Eyedropper,
    ZoomIn,
    ZoomOut,
    Rotate,
    Move,
    Duplicate,
    Distort,
    MoveSelection,
    MovePixels,
    LoadSelection,
    CreateClipping,
    ReleaseClipping,
}

#[derive(Default)]
pub(super) struct Atlas {
    scale: f32,
    images: HashMap<Glyph, CursorImage>,
}

impl Atlas {
    pub fn image(&mut self, glyph: Glyph, scale: f32) -> Result<CursorImage> {
        if self.scale != scale {
            self.images.clear();
            self.scale = scale;
        }
        if let Some(image) = self.images.get(&glyph) {
            return Ok(image.clone());
        }
        let image = rasterize(glyph, scale)?;
        self.images.insert(glyph, image.clone());
        Ok(image)
    }
}

const ARROW: &str = "M0 0V16.5L3.9 12.8 6.6 19 9.2 17.9 6.6 11.8H11.8Z";

fn arrow(offset: u8, inverse: bool) -> String {
    let (fill, outline) = if inverse {
        ("white", "black")
    } else {
        ("black", "white")
    };
    format!(
        r#"<g filter="url(#shadow)"><path transform="translate({},{})" d="{ARROW}" stroke="{outline}" fill="{fill}" stroke-width="2.2" stroke-linejoin="round" paint-order="stroke fill"/></g>"#,
        4 + offset,
        3 + offset
    )
}

fn outlined(path: &str, outer: f32, inner: f32) -> String {
    format!(
        r#"<g fill="none" stroke-linecap="round" stroke-linejoin="round"><path d="{path}" stroke="white" stroke-width="{outer}"/><path d="{path}" stroke="black" stroke-width="{inner}"/></g>"#
    )
}

fn badge(badge: Badge, x: f32, y: f32) -> String {
    if badge == Badge::New {
        return String::new();
    }
    let mut path = format!("M{} {y}h6", x - 3.);
    if badge == Badge::Add {
        path.push_str(&format!("M{x} {}v6", y - 3.));
    }
    outlined(&path, 3.2, 1.2)
}

// Reuse the rail's project-drawn glyphs in the same twelve-point boxes as Swift.
fn icon(source: &str, grid: u32, x: f32, y: f32, size: f32) -> String {
    let source = source.replacen(
        "<svg ",
        &format!("<svg width=\"{grid}\" height=\"{grid}\" "),
        1,
    );
    format!(
        r#"<g transform="translate({x},{y}) scale({})" color="black" filter="url(#outline)">{source}</g>"#,
        size / grid as f32
    )
}

fn drawing(glyph: Glyph) -> ([u32; 2], [u16; 2], String) {
    use Glyph::*;
    match glyph {
        Rectangle(mode) | Ellipse(mode) | Lasso(mode) | Polygon(mode) => {
            let cross = outlined("M3 10H17M10 3V17", 3.2, 1.2);
            let tool = match glyph {
                Rectangle(_) => r#"<rect x="18" y="19" width="11" height="8" stroke="white" stroke-width="2.7"/><rect x="18" y="19" width="11" height="8" stroke="black" stroke-width="1.2" stroke-dasharray="2 1.5"/>"#.into(),
                Ellipse(_) => r#"<ellipse cx="23" cy="23" rx="5.5" ry="5.5" stroke="white" stroke-width="2.7"/><ellipse cx="23" cy="23" rx="5.5" ry="5.5" stroke="black" stroke-width="1.2" stroke-dasharray="2 1.5"/>"#.into(),
                Lasso(_) => icon(include_str!("../../assets/icons/compositor-lasso.svg"), 20, 17., 17., 12.),
                _ => icon(include_str!("../../assets/icons/compositor-polygonal-lasso.svg"), 18, 17., 17., 12.),
            };
            (
                [44, 36],
                [10, 10],
                format!(
                    "{cross}<g fill=\"none\">{tool}</g>{}",
                    badge(mode, 34., 23.)
                ),
            )
        }
        Object(mode) => {
            let tool = outlined(
                "M4 10V4H10M18 4H24V10M24 18V24H18M10 24H4V18M10 14H18M14 10V18",
                3.2,
                1.2,
            );
            (
                [36, 32],
                [14, 14],
                format!("{tool}{}", badge(mode, 29., 25.)),
            )
        }
        Wand(mode) => {
            let marks = "M7 4.5V1M7 9.5V13M4.5 7H1M9.5 7H13";
            // Draw both white outlines before either black line, as in Swift.
            let drawing = format!(
                r#"<g stroke-linecap="round"><path d="M13 13L27 27" stroke="white" stroke-width="5"/><path d="{marks}" stroke="white" stroke-width="3.2"/><path d="M13 13L27 27" stroke="black" stroke-width="2.4"/><path d="{marks}" stroke="black" stroke-width="1.2"/></g>{}"#,
                badge(mode, 24., 12.)
            );
            ([34, 34], [7, 7], drawing)
        }
        Eyedropper => (
            [24, 24],
            [3, 21],
            icon(
                include_str!("../../assets/icons/compositor-eyedropper.svg"),
                20,
                2.,
                2.,
                20.,
            ),
        ),
        ZoomIn | ZoomOut => {
            let plus = if glyph == ZoomIn { "M10 7V13" } else { "" };
            let drawing = format!(
                r#"<g stroke-linecap="round"><path d="M15 15L21 21" stroke="white" stroke-width="4.5"/><circle cx="10" cy="10" r="6.5" stroke="white" stroke-width="4.5" fill="white"/><path d="M15 15L21 21" stroke="black" stroke-width="2"/><circle cx="10" cy="10" r="6.5" stroke="black" stroke-width="2" fill="white"/><path d="M7 10H13{plus}" stroke="black" stroke-width="1.3"/></g>"#
            );
            ([24, 24], [10, 10], drawing)
        }
        Rotate => (
            [24, 24],
            [12, 12],
            outlined(
                "M4 10A8 8 0 0 1 18 6M18 2V6H14M20 14A8 8 0 0 1 6 18M6 22V18H10",
                4.5,
                2.,
            ),
        ),
        Duplicate => (
            [28, 32],
            [4, 3],
            format!("{}{}", arrow(5, true), arrow(0, false)),
        ),
        Distort => ([28, 32], [4, 3], arrow(0, true)),
        LoadSelection => {
            let hand = "M7 16V4a2 2 0 0 1 4 0v7-2a2 2 0 0 1 4 0v2a2 2 0 0 1 4 0v2a2 2 0 0 1 4 0v6c0 5-3 8-8 8h-2c-3 0-5-2-7-5l-3-5c-1-2 1-4 3-2l3 4";
            let hand = format!(
                r#"<path d="{hand}" fill="white" stroke="white" stroke-width="3.5" stroke-linejoin="round"/><path d="{hand}" fill="white" stroke="black" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>"#
            );
            let box_badge = r#"<g fill="none"><rect x="19.5" y="18.5" width="8" height="6" stroke="white" stroke-width="2.5"/><rect x="19.5" y="18.5" width="8" height="6" stroke="black" stroke-width="1" stroke-dasharray="2 1.5"/></g>"#;
            ([36, 36], [8, 4], format!("{hand}{box_badge}"))
        }
        CreateClipping | ReleaseClipping => {
            let arrow = outlined("M3 2V7Q3 12 8 12H16M12 8L16 12L12 16", 4., 2.);
            let rectangle = outlined("M12 12H25V24H12Z", 3.5, 1.5);
            let vertical = if glyph == CreateClipping {
                "M25 20V26"
            } else {
                ""
            };
            let badge = format!(
                r#"<circle cx="25" cy="23" r="4" fill="white" stroke="white" stroke-width="3"/><circle cx="25" cy="23" r="4" fill="white" stroke="black" stroke-width="1.2"/><path d="M22 23H28{vertical}" fill="none" stroke="black" stroke-width="1.2" stroke-linecap="round"/>"#
            );
            ([30, 28], [3, 3], format!("{arrow}{rectangle}{badge}"))
        }
        Move | MoveSelection | MovePixels => {
            let detail = match glyph {
                Move => {
                    let mut points = Vec::new();
                    for turn in 0..4 {
                        for (mut x, mut y) in [(-0.75, -4.), (-2., -4.), (0., -6.5), (2., -4.), (0.75, -4.), (0.75, -0.75)] {
                            for _ in 0..turn { (x, y) = (-y, x); }
                            points.push(format!("{},{}", 18.5 + x, 20.5 + y));
                        }
                    }
                    format!(r#"<polygon points="{}" stroke="white" stroke-width="1.6" stroke-linejoin="round" fill="black" paint-order="stroke fill"/>"#, points.join(" "))
                }
                MoveSelection => r#"<g fill="none"><rect x="13.5" y="16.5" width="8" height="6" stroke="white" stroke-width="2.5"/><rect x="13.5" y="16.5" width="8" height="6" stroke="black" stroke-width="1" stroke-dasharray="2 1.5"/></g>"#.into(),
                _ => outlined("M16 20L24 26M16 25L24 17M14 19a2 2 0 1 0 4 0a2 2 0 1 0-4 0M14 26a2 2 0 1 0 4 0a2 2 0 1 0-4 0", 3.5, 1.2),
            };
            ([36, 36], [4, 3], format!("{}{detail}", arrow(0, false)))
        }
    }
}

fn rasterize(glyph: Glyph, scale: f32) -> Result<CursorImage> {
    if !scale.is_finite() || !(0.25..=16.).contains(&scale) {
        return Err(invalid(
            "The cursor's display scale is outside the supported range.",
        ));
    }
    let (size, hotspot, drawing) = drawing(glyph);
    let [width, height] = size.map(|v| (v as f32 * scale).ceil() as u32);
    let source = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}"><defs><filter id="shadow" x="-50%" y="-30%" width="200%" height="180%"><feDropShadow dx="0" dy="1" stdDeviation="0.75" flood-color="black" flood-opacity="0.35"/></filter><filter id="outline" x="-30%" y="-30%" width="160%" height="160%"><feMorphology in="SourceAlpha" operator="dilate" radius="1.25" result="edge"/><feFlood flood-color="white"/><feComposite in2="edge" operator="in"/><feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge></filter></defs>{drawing}</svg>"#,
        size[0], size[1], size[0], size[1]
    );
    let tree = resvg::usvg::Tree::from_str(&source, &resvg::usvg::Options::default())
        .map_err(|error| invalid(format!("Could not parse the tool cursor: {error}")))?;
    let mut pixels = tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| invalid("Could not allocate the tool cursor image."))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixels.as_mut(),
    );
    let rgba: Vec<u8> = pixels
        .pixels()
        .iter()
        .flat_map(|p| {
            let p = p.demultiply();
            [p.red(), p.green(), p.blue(), p.alpha()]
        })
        .collect();
    let image =
        Image::from_rgba(width, height, rgba).map_err(|error| invalid(error.to_string()))?;
    CursorImage::new(
        image,
        hotspot.map(|v| (f32::from(v) * scale).round() as u16),
    )
    .map_err(|error| invalid(error.to_string()))
}
