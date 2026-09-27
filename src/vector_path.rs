//! Editable cubic paths in document coordinates. Curves remain geometry until
//! an explicit selection or painting operation requests a bounded polyline.
use crate::{Result, geometry::Point, invalid, selection::Selection};
use kurbo::{CubicBez, ParamCurve, ParamCurveExtrema};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

pub const MAX_ANCHORS: usize = 4096;
pub const MAX_PATHS: usize = 256;
pub const MAX_TOTAL_ANCHORS: usize = 16_384;
pub const MAX_FLATTENED_SEGMENTS: usize = 262_144;
const COORDINATE_LIMIT: f64 = 1_000_000.;
const MAX_SUBDIVISION_DEPTH: u8 = 32;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Anchor {
    pub point: Point,
    pub incoming: Option<Point>,
    pub outgoing: Option<Point>,
}
impl Anchor {
    pub fn corner(point: Point) -> Self {
        Self {
            point,
            incoming: None,
            outgoing: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Closure {
    #[default]
    Open,
    Closed,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BezierPath {
    pub anchors: Vec<Anchor>,
    pub closure: Closure,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedPath {
    pub id: Uuid,
    pub name: String,
    pub geometry: BezierPath,
}

impl SavedPath {
    pub fn new(name: impl Into<String>, geometry: BezierPath) -> Result<Self> {
        let path = Self {
            id: Uuid::new_v4(),
            name: name.into(),
            geometry,
        };
        path.validate()?;
        Ok(path)
    }

    pub fn validate(&self) -> Result<()> {
        if self.id.is_nil()
            || self.name.trim().is_empty()
            || self.name.len() > 256
            || self.name.chars().any(char::is_control)
        {
            return Err(invalid(
                "Saved paths need a nonzero ID and a name of 1 to 256 bytes without control characters.",
            ));
        }
        self.geometry.validate()
    }
}

pub fn validate(paths: &[SavedPath]) -> Result<()> {
    if paths.len() > MAX_PATHS {
        return Err(invalid(
            "A document can contain at most 256 saved paths. Remove an unused path before adding another.",
        ));
    }
    let mut ids = HashSet::new();
    let mut anchors = 0;
    for path in paths {
        path.validate()?;
        if !ids.insert(path.id) {
            return Err(invalid("Saved paths must have unique IDs."));
        }
        anchors += path.geometry.anchors.len();
        if anchors > MAX_TOTAL_ANCHORS {
            return Err(invalid(
                "Saved paths exceed 16384 total anchors. Simplify or remove unused paths.",
            ));
        }
    }
    Ok(())
}

/// Preflight the entire collection before a canvas operation changes any layer.
pub fn mapped(paths: &[SavedPath], map: impl Fn(Point) -> Point) -> Result<Vec<SavedPath>> {
    validate(paths)?;
    paths
        .iter()
        .map(|path| {
            Ok(SavedPath {
                id: path.id,
                name: path.name.clone(),
                geometry: path.geometry.mapped(&map)?,
            })
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Anchor(usize),
    Incoming(usize),
    Outgoing(usize),
}

#[derive(Clone, Copy, Debug)]
pub struct FlattenOptions {
    /// Maximum distance from the curve to its polyline, in document pixels.
    pub tolerance: f64,
    /// Global budget across every segment in this path, not per cubic.
    pub max_segments: usize,
}
impl Default for FlattenOptions {
    fn default() -> Self {
        Self {
            tolerance: 0.05,
            max_segments: 65_536,
        }
    }
}
impl FlattenOptions {
    fn validate(self) -> Result<()> {
        if !self.tolerance.is_finite()
            || !(0.0001..=1000.).contains(&self.tolerance)
            || !(1..=MAX_FLATTENED_SEGMENTS).contains(&self.max_segments)
        {
            return Err(invalid(
                "Path flattening needs a tolerance between 0.0001 and 1000 pixels and a segment budget between 1 and 262144.",
            ));
        }
        Ok(())
    }
}

fn valid_point(point: Point) -> bool {
    point
        .iter()
        .all(|value| value.is_finite() && value.abs() <= COORDINATE_LIMIT)
}
fn curve(points: [Point; 4]) -> CubicBez {
    let points = points.map(|point| kurbo::Point::new(point[0], point[1]));
    CubicBez::new(points[0], points[1], points[2], points[3])
}
fn point(point: kurbo::Point) -> Point {
    [point.x, point.y]
}
fn distance_squared(a: Point, b: Point) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}
fn distance_to_segment(p: Point, start: Point, end: Point) -> f64 {
    let delta = [end[0] - start[0], end[1] - start[1]];
    let length_squared = delta[0].powi(2) + delta[1].powi(2);
    let parameter = if length_squared == 0. {
        0.
    } else {
        (((p[0] - start[0]) * delta[0] + (p[1] - start[1]) * delta[1]) / length_squared)
            .clamp(0., 1.)
    };
    distance_squared(
        p,
        [
            start[0] + parameter * delta[0],
            start[1] + parameter * delta[1],
        ],
    )
    .sqrt()
}

impl BezierPath {
    pub fn validate(&self) -> Result<()> {
        if self.anchors.len() > MAX_ANCHORS {
            return Err(invalid(
                "A path can contain at most 4096 anchors. Simplify the path or split it into separate paths.",
            ));
        }
        if self.closure == Closure::Closed && self.anchors.len() < 2 {
            return Err(invalid("A closed path needs at least two anchors."));
        }
        if self.anchors.iter().any(|anchor| {
            [Some(anchor.point), anchor.incoming, anchor.outgoing]
                .into_iter()
                .flatten()
                .any(|point| !valid_point(point))
        }) {
            return Err(invalid(
                "Path anchors and handles must be finite coordinates within 1000000 pixels of the origin.",
            ));
        }
        Ok(())
    }

    /// Missing handles lie at their anchor, producing an ordinary line.
    pub fn segments(&self) -> impl Iterator<Item = [Point; 4]> + '_ {
        let count = if self.closure == Closure::Closed && self.anchors.len() >= 2 {
            self.anchors.len()
        } else {
            self.anchors.len().saturating_sub(1)
        };
        (0..count).map(|index| {
            let start = self.anchors[index];
            let end = self.anchors[(index + 1) % self.anchors.len()];
            [
                start.point,
                start.outgoing.unwrap_or(start.point),
                end.incoming.unwrap_or(end.point),
                end.point,
            ]
        })
    }

    /// Map all anchors and handles together. Invalid mappings leave the original
    /// untouched. Callers may use Transform::point/unit, including its flips
    /// and rotation, after converting local pixels to/from unit coordinates.
    pub fn mapped(&self, map: impl Fn(Point) -> Point) -> Result<Self> {
        self.validate()?;
        let result = Self {
            anchors: self
                .anchors
                .iter()
                .map(|anchor| Anchor {
                    point: map(anchor.point),
                    incoming: anchor.incoming.map(&map),
                    outgoing: anchor.outgoing.map(&map),
                })
                .collect(),
            closure: self.closure,
        };
        result.validate()?;
        Ok(result)
    }

    /// Tight cubic extrema, rather than the enclosing handle polygon. A single
    /// anchor has a zero-area bound; an empty path has no bounds.
    pub fn bounds(&self) -> Result<Option<[f64; 4]>> {
        self.validate()?;
        let Some(first) = self.anchors.first() else {
            return Ok(None);
        };
        let mut bounds = [
            first.point[0],
            first.point[1],
            first.point[0],
            first.point[1],
        ];
        for segment in self.segments() {
            let rect = curve(segment).bounding_box();
            bounds = [
                bounds[0].min(rect.x0),
                bounds[1].min(rect.y0),
                bounds[2].max(rect.x1),
                bounds[3].max(rect.y1),
            ];
        }
        Ok(Some(bounds))
    }

    /// Handles win over anchors, then the closest point wins. Tolerance is in
    /// document pixels; divide a screen-space hit radius by viewport zoom.
    pub fn hit(&self, position: Point, tolerance: f64) -> Result<Option<Hit>> {
        self.validate()?;
        if !valid_point(position) || !tolerance.is_finite() || tolerance < 0. {
            return Err(invalid(
                "Path hit testing requires a finite position and nonnegative tolerance.",
            ));
        }
        let nearest = |handles: bool| {
            let mut result = None;
            let mut distance = tolerance * tolerance;
            for (index, anchor) in self.anchors.iter().enumerate() {
                let candidates = if handles {
                    [
                        (anchor.incoming, Hit::Incoming(index)),
                        (anchor.outgoing, Hit::Outgoing(index)),
                    ]
                } else {
                    [
                        (Some(anchor.point), Hit::Anchor(index)),
                        (None, Hit::Anchor(index)),
                    ]
                };
                for (candidate, hit) in candidates {
                    let Some(candidate) = candidate else {
                        continue;
                    };
                    let squared = distance_squared(position, candidate);
                    if squared <= distance && (result.is_none() || squared < distance) {
                        distance = squared;
                        result = Some(hit);
                    }
                }
            }
            result
        };
        Ok(nearest(true).or_else(|| nearest(false)))
    }

    pub fn flatten(&self, options: FlattenOptions) -> Result<Vec<Point>> {
        self.validate()?;
        options.validate()?;
        let Some(first) = self.anchors.first() else {
            return Ok(Vec::new());
        };
        let mut output = vec![first.point];
        let mut stack = Vec::with_capacity(usize::from(MAX_SUBDIVISION_DEPTH) + 1);
        for segment in self.segments() {
            stack.push((curve(segment), 0_u8));
            while let Some((curve, depth)) = stack.pop() {
                let start = point(curve.p0);
                let end = point(curve.p3);
                // A cubic lies in its control polygon. Distance to the finite
                // chord also catches collinear reversals and closed loops.
                let deviation = distance_to_segment(point(curve.p1), start, end)
                    .max(distance_to_segment(point(curve.p2), start, end));
                if deviation <= options.tolerance {
                    if output.len() > options.max_segments {
                        return Err(invalid(
                            "This path exceeds the flattened segment budget. Simplify it or use a coarser tolerance. The original path is preserved.",
                        ));
                    }
                    output.push(end);
                } else {
                    if depth >= MAX_SUBDIVISION_DEPTH {
                        return Err(invalid(
                            "This path cannot meet the requested curve tolerance within the subdivision limit. Use a coarser tolerance. The original path is preserved.",
                        ));
                    }
                    let (left, right) = curve.subdivide();
                    stack.push((right, depth + 1));
                    stack.push((left, depth + 1));
                }
            }
        }
        Ok(output)
    }

    /// Filling implicitly closes an open path without changing its authoring
    /// state. Polygon coverage already supports canvases beyond raster limits.
    pub fn selection(&self, width: u32, height: u32, antialiased: bool) -> Result<Selection> {
        let points = self.flatten(FlattenOptions::default())?;
        if points.len() < 3 {
            return Err(invalid(
                "The path does not enclose an area. Add another anchor or curve its handles before making a selection.",
            ));
        }
        Selection::polygon(width, height, &points, antialiased)
    }
}

#[cfg(test)]
mod tests;
