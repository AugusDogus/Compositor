use crate::{Result, geometry::Point, invalid, selection::SelectionMode};
use i_overlay::{
    core::{fill_rule::FillRule, overlay_rule::OverlayRule},
    float::{simplify::SimplifyShape, single::SingleFloatOverlay},
    mesh::{
        outline::offset::OutlineOffset,
        style::{LineJoin, OutlineStyle},
    },
};
use image::GrayImage;

/// Pixel-local contours retained so boolean edits rasterize once, without coverage seams.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SelectionGeometry {
    pub contours: Vec<Vec<Point>>,
    pub antialiased: bool,
}

impl SelectionGeometry {
    pub fn bounds(&self) -> Option<[f64; 4]> {
        let mut points = self.contours.iter().flatten();
        let first = points.next()?;
        Some(
            points.fold([first[0], first[1], first[0], first[1]], |b, p| {
                [
                    b[0].min(p[0]),
                    b[1].min(p[1]),
                    b[2].max(p[0]),
                    b[3].max(p[1]),
                ]
            }),
        )
    }

    pub fn polygon(points: &[Point], antialiased: bool) -> Self {
        Self {
            contours: if points.len() >= 3 {
                vec![points.to_vec()]
            } else {
                Vec::new()
            },
            antialiased,
        }
    }

    pub fn rectangle(bounds: [f64; 4], antialiased: bool) -> Self {
        let [l, t, r, b] = bounds;
        Self::polygon(&[[l, t], [r, t], [r, b], [l, b]], antialiased)
    }

    pub fn ellipse(bounds: [f64; 4], antialiased: bool) -> Self {
        let [l, t, r, b] = bounds;
        let radii = [(r - l) / 2., (b - t) / 2.];
        let center = [(r + l) / 2., (b + t) / 2.];
        // Inscribed segments deviate by at most 0.01 document pixel from the ellipse.
        let radius = radii[0].max(radii[1]);
        let count = (std::f64::consts::PI / (1. - 0.01 / radius).clamp(-1., 1.).acos())
            .ceil()
            .max(16.) as usize;
        let count = count.next_multiple_of(4);
        let points: Vec<_> = (0..count)
            .map(|i| {
                let angle = std::f64::consts::TAU * i as f64 / count as f64;
                [
                    center[0] + radii[0] * angle.cos(),
                    center[1] + radii[1] * angle.sin(),
                ]
            })
            .collect();
        Self::polygon(&points, antialiased)
    }

    pub fn translated(&self, delta: Point) -> Self {
        Self {
            contours: self
                .contours
                .iter()
                .map(|c| {
                    c.iter()
                        .map(|p| [p[0] + delta[0], p[1] + delta[1]])
                        .collect()
                })
                .collect(),
            antialiased: self.antialiased,
        }
    }

    pub fn combine(&self, other: &Self, mode: SelectionMode) -> Self {
        let rule = match mode {
            SelectionMode::Replace => return other.clone(),
            SelectionMode::Add => OverlayRule::Union,
            SelectionMode::Subtract => OverlayRule::Difference,
            SelectionMode::Intersect => OverlayRule::Intersect,
        };
        Self {
            contours: self
                .contours
                .overlay(&other.contours, rule, FillRule::NonZero)
                .into_iter()
                .flatten()
                .collect(),
            antialiased: other.antialiased,
        }
    }

    pub fn clipped(&self, width: u32, height: u32) -> Self {
        self.combine(
            &Self::rectangle([0., 0., width as f64, height as f64], self.antialiased),
            SelectionMode::Intersect,
        )
    }

    pub fn resized(&self, amount: f64) -> Self {
        let shapes = self.contours.simplify_shape(FillRule::NonZero);
        let style = OutlineStyle::new(amount).line_join(LineJoin::Round(0.01));
        Self {
            contours: shapes.outline(&style).into_iter().flatten().collect(),
            antialiased: self.antialiased,
        }
    }

    pub fn path(&self) -> Option<tiny_skia::Path> {
        let mut path = tiny_skia::PathBuilder::new();
        for contour in &self.contours {
            if let Some(first) = contour.first() {
                path.move_to(first[0] as f32, first[1] as f32);
                for point in &contour[1..] {
                    path.line_to(point[0] as f32, point[1] as f32);
                }
                path.close();
            }
        }
        path.finish()
    }

    pub fn rasterize(&self, width: u32, height: u32) -> Result<GrayImage> {
        let mut mask = tiny_skia::Mask::new(width, height)
            .ok_or_else(|| invalid("Cannot allocate selection coverage. Try a smaller canvas."))?;
        if let Some(path) = self.path() {
            mask.fill_path(
                &path,
                tiny_skia::FillRule::Winding,
                self.antialiased,
                tiny_skia::Transform::identity(),
            );
        }
        GrayImage::from_raw(width, height, mask.data().to_vec())
            .ok_or_else(|| invalid("Selection dimensions do not match its coverage."))
    }

    /// Match Swift's 50% mask tracing threshold. Coalesce row spans before unioning
    /// their rectangles, avoiding one contour per selected pixel on solid masks.
    pub fn trace(pixels: &GrayImage) -> Result<Self> {
        use std::collections::BTreeMap;
        let mut active = BTreeMap::<(u32, u32), u32>::new();
        let mut contours = Vec::new();
        for y in 0..=pixels.height() {
            let mut next = BTreeMap::new();
            if y < pixels.height() {
                let mut x = 0;
                while x < pixels.width() {
                    if pixels[(x, y)][0] < 128 {
                        x += 1;
                        continue;
                    }
                    let start = x;
                    while x < pixels.width() && pixels[(x, y)][0] >= 128 {
                        x += 1;
                    }
                    let top = active.remove(&(start, x)).unwrap_or(y);
                    next.insert((start, x), top);
                }
            }
            for ((l, r), t) in active {
                contours.push(vec![
                    [l as f64, t as f64],
                    [r as f64, t as f64],
                    [r as f64, y as f64],
                    [l as f64, y as f64],
                ]);
            }
            if contours.len() + next.len() > 1_000_000 {
                return Err(invalid(
                    "This mask is too complex to trace as a selection. Simplify the mask and try again. The current selection is preserved.",
                ));
            }
            active = next;
        }
        Ok(Self {
            contours: contours
                .simplify_shape(FillRule::NonZero)
                .into_iter()
                .flatten()
                .collect(),
            antialiased: true,
        })
    }
}
