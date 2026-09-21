//! One-point GPU selection strokes, with four-point dashes measured along the path.
use compositor::{Result, geometry::Point, invalid, selection::Selection};
use quickgui::{Path, PathBuilder, PathStyle, StrokeOptions};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Viewport {
    pub size: [f32; 2],
    pub zoom: f64,
    pub offset: Point,
}

#[derive(Clone, Debug)]
struct Polyline {
    points: Vec<Point>,
    distance: f64,
}

impl Polyline {
    fn length(&self) -> f64 {
        self.points.windows(2).map(|p| distance(p[0], p[1])).sum()
    }

    fn path(&self) -> Result<tiny_skia::Path> {
        let mut path = tiny_skia::PathBuilder::new();
        for (i, point) in self.points.iter().enumerate() {
            if i == 0 {
                path.move_to(point[0] as f32, point[1] as f32);
            } else {
                path.line_to(point[0] as f32, point[1] as f32);
            }
        }
        if self.points.first() == self.points.last() {
            path.close();
        }
        path.finish().ok_or_else(outline_error)
    }
}

fn distance(a: Point, b: Point) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

fn outline_error() -> compositor::Error {
    invalid(
        "The selection outline could not be drawn. The selection is preserved; zoom out or simplify its boundary and try again.",
    )
}

/// Retain the source distance when clipping, so panning does not reset dash phase.
fn clip_segment(a: Point, b: Point, size: [f32; 2]) -> Option<(f64, f64)> {
    let mut range = (0_f64, 1_f64);
    for axis in 0..2 {
        let delta = b[axis] - a[axis];
        // Half of the source miter limit, plus the antialiasing fringe.
        let bounds = (-6., f64::from(size[axis]) + 6.);
        if delta == 0. {
            if a[axis] < bounds.0 || a[axis] > bounds.1 {
                return None;
            }
        } else {
            let t0 = (bounds.0 - a[axis]) / delta;
            let t1 = (bounds.1 - a[axis]) / delta;
            range.0 = range.0.max(t0.min(t1));
            range.1 = range.1.min(t0.max(t1));
        }
    }
    (range.0 < range.1).then_some(range)
}

fn visible_parts(points: &[Point], origin: Point, view: Viewport) -> Vec<Polyline> {
    let map = |p: Point| {
        [
            (origin[0] + p[0]) * view.zoom + view.offset[0],
            (origin[1] + p[1]) * view.zoom + view.offset[1],
        ]
    };
    let mut parts: Vec<Polyline> = Vec::new();
    let mut travelled = 0.;
    let mut continuous = false;
    for (a, b) in points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
    {
        let (a, b) = (map(*a), map(*b));
        let length = distance(a, b);
        if length == 0. {
            continue;
        }
        if let Some((start, end)) = clip_segment(a, b, view.size) {
            let at = |t| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
            let first = if start == 0. { a } else { at(start) };
            let last = if end == 1. { b } else { at(end) };
            if continuous
                && start == 0.
                && let Some(part) = parts.last_mut()
                && part.points.last() == Some(&first)
            {
                part.points.push(last);
            } else {
                parts.push(Polyline {
                    points: vec![first, last],
                    distance: travelled + start * length,
                });
            }
            continuous = end == 1.;
        } else {
            continuous = false;
        }
        travelled += length;
    }
    parts
}

/// Join the two pieces at the original closed contour's seam when both are ink.
fn join_seam(parts: &mut Vec<Polyline>) {
    if parts.len() > 1
        && parts.first().and_then(|p| p.points.first())
            == parts.last().and_then(|p| p.points.last())
    {
        let first = parts.remove(0);
        if let Some(last) = parts.last_mut() {
            last.points.extend(first.points.into_iter().skip(1));
        }
    }
}

fn dashed(parts: &[Polyline], phase: u8) -> Result<Vec<Polyline>> {
    let mut result = Vec::new();
    for part in parts {
        let offset = (part.distance + f64::from(phase)).rem_euclid(8.);
        // Skia returns no path for a short piece contained entirely in a gap.
        if offset >= 4. && part.length() <= 8. - offset {
            continue;
        }
        let dash =
            tiny_skia::StrokeDash::new(vec![4., 4.], offset as f32).ok_or_else(outline_error)?;
        let path = part.path()?.dash(&dash, 1.).ok_or_else(outline_error)?;
        let mut current: Option<Polyline> = None;
        for segment in path.segments() {
            match segment {
                tiny_skia::PathSegment::MoveTo(p) => {
                    if let Some(part) = current.take() {
                        result.push(part);
                    }
                    current = Some(Polyline {
                        points: vec![[f64::from(p.x), f64::from(p.y)]],
                        distance: 0.,
                    });
                }
                tiny_skia::PathSegment::LineTo(p) => {
                    if let Some(part) = &mut current {
                        part.points.push([f64::from(p.x), f64::from(p.y)]);
                    }
                }
                tiny_skia::PathSegment::Close => {
                    if let Some(part) = &mut current
                        && let Some(first) = part.points.first().copied()
                        && part.points.last() != Some(&first)
                    {
                        part.points.push(first);
                    }
                }
                // Inputs are straight contour segments, so curves indicate an invalid conversion.
                _ => return Err(outline_error()),
            }
        }
        if let Some(part) = current {
            result.push(part);
        }
    }
    join_seam(&mut result);
    Ok(result)
}

fn gpu_paths(parts: &[Polyline]) -> Result<Vec<Path>> {
    let stroke = || {
        PathBuilder::stroke(1.).with_style(PathStyle::Stroke(
            StrokeOptions::default().with_miter_limit(10.),
        ))
    };
    let mut paths = Vec::new();
    let mut builder = stroke();
    let mut commands = 0;
    for part in parts {
        if part.points.len() < 2 {
            continue;
        }
        // Batch independent subpaths without splitting their joins.
        if commands > 0 && commands + part.points.len() > 16_000 {
            paths.push(builder.build().map_err(|error| invalid(format!(
                "Could not tessellate the selection outline: {error}. The selection is preserved; zoom out or simplify its boundary and try again."
            )))?);
            builder = stroke();
            commands = 0;
        }
        for (i, point) in part.points.iter().enumerate() {
            let point = quickgui::Point::new(point[0] as f32, point[1] as f32);
            if i == 0 {
                builder.move_to(point);
            } else {
                builder.line_to(point);
            }
        }
        if part.points.first() == part.points.last() {
            builder.close();
        }
        commands += part.points.len() + 1;
    }
    if commands > 0 {
        paths.push(builder.build().map_err(|error| invalid(format!(
                "Could not tessellate the selection outline: {error}. The selection is preserved; zoom out or simplify its boundary and try again."
            )))?);
    }
    Ok(paths)
}

pub(super) struct OutlinePaths {
    contours: Vec<Vec<Polyline>>,
    pub white: Arc<[Path]>,
    phases: [Option<Arc<[Path]>>; 8],
}

impl OutlinePaths {
    pub fn new(selection: &Selection, view: Viewport) -> Result<Option<Self>> {
        let contours: Vec<_> = selection
            .outline_contours()?
            .iter()
            .map(|points| visible_parts(points, selection.origin, view))
            .filter(|parts| !parts.is_empty())
            .collect();
        if contours.is_empty() {
            return Ok(None);
        }
        let mut white_parts = Vec::new();
        for parts in &contours {
            let mut joined = parts.clone();
            join_seam(&mut joined);
            white_parts.extend(joined);
        }
        let white = gpu_paths(&white_parts)?;
        Ok(Some(Self {
            contours,
            white: white.into(),
            phases: Default::default(),
        }))
    }

    pub fn black(&mut self, phase: u8) -> Result<Arc<[Path]>> {
        let phase = phase % 8;
        if let Some(paths) = &self.phases[usize::from(phase)] {
            return Ok(paths.clone());
        }
        let mut ink = Vec::new();
        for parts in &self.contours {
            ink.extend(dashed(parts, phase)?);
        }
        let paths: Arc<[Path]> = gpu_paths(&ink)?.into();
        self.phases[usize::from(phase)] = Some(paths.clone());
        Ok(paths)
    }
}

#[cfg(test)]
#[path = "selection_paths_tests.rs"]
mod tests;
