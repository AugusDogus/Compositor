use crate::{
    document::Document,
    geometry::{Point, Transform},
    transform::{self, Handle},
};

#[derive(Clone, Copy)]
pub enum Mode {
    Create,
    Move,
    Resize(usize),
}

pub struct Drag {
    pub start: Point,
    pub original: Transform,
    pub mode: Mode,
    targets: [Vec<f64>; 2],
}

pub fn rounded(mut frame: Transform) -> Transform {
    for axis in 0..2 {
        let right = (frame.origin[axis] + frame.size[axis]).round();
        frame.origin[axis] = frame.origin[axis].round();
        frame.size[axis] = (right - frame.origin[axis]).max(1.);
    }
    frame
}

pub fn valid(frame: Transform) -> bool {
    frame.valid() && frame.size.iter().all(|v| *v <= 30_000.)
}

impl Drag {
    pub fn new(doc: &Document, start: Point, original: Transform, mode: Mode) -> Self {
        let mut targets = [vec![0., doc.width as f64], vec![0., doc.height as f64]];
        for layer in doc
            .layers
            .iter()
            .filter(|l| l.visible && l.raster().is_some())
        {
            let mut parent = layer.parent;
            let mut visible = true;
            while let Some(id) = parent {
                let Some(group) = doc.layer(id) else {
                    break;
                };
                visible &= group.visible;
                parent = group.parent;
            }
            if !visible {
                continue;
            }
            let b = layer.transform.bounds();
            for axis in 0..2 {
                targets[axis].extend([b[axis].round(), b[axis + 2].round()]);
            }
        }
        Self {
            start,
            original,
            mode,
            targets,
        }
    }

    pub fn updated(
        &self,
        point: Point,
        ratio: Option<f64>,
        symmetric: bool,
        tolerance: f64,
    ) -> (Transform, [Option<f64>; 2]) {
        let mut frame = match self.mode {
            Mode::Create => {
                let mut delta = [point[0] - self.start[0], point[1] - self.start[1]];
                if let Some(ratio) = ratio {
                    if delta[0].abs() > delta[1].abs() * ratio {
                        delta[1] = (if delta[1] < 0. { -1. } else { 1. }) * delta[0].abs() / ratio;
                    } else {
                        delta[0] = (if delta[0] < 0. { -1. } else { 1. }) * delta[1].abs() * ratio;
                    }
                }
                Transform {
                    origin: std::array::from_fn(|i| {
                        if symmetric {
                            self.start[i] - delta[i].abs()
                        } else {
                            self.start[i].min(self.start[i] + delta[i])
                        }
                    }),
                    size: delta.map(|d| d.abs() * if symmetric { 2. } else { 1. }),
                    ..Transform::new(1, 1)
                }
            }
            Mode::Move => Transform {
                origin: std::array::from_fn(|i| self.original.origin[i] + point[i] - self.start[i]),
                ..self.original
            },
            Mode::Resize(index) => transform::drag(
                self.original,
                self.start,
                point,
                Handle::Resize(index),
                ratio.is_some(),
                false,
                symmetric,
            ),
        };
        frame = rounded(frame);
        let mut guides = [None; 2];
        if tolerance <= 0. || (ratio.is_some() && !matches!(self.mode, Mode::Move)) {
            return (frame, guides);
        }
        for axis in 0..2 {
            let nearest = |value: f64| {
                self.targets[axis]
                    .iter()
                    .copied()
                    .filter(|t| (t - value).abs() <= tolerance)
                    .min_by(|a, b| (a - value).abs().total_cmp(&(b - value).abs()))
            };
            let left = frame.origin[axis];
            let right = left + frame.size[axis];
            if matches!(self.mode, Mode::Move) {
                let shift = [left, right]
                    .into_iter()
                    .filter_map(|edge| nearest(edge).map(|target| (target - edge, target)))
                    .min_by(|a, b| a.0.abs().total_cmp(&b.0.abs()));
                if let Some((delta, guide)) = shift {
                    frame.origin[axis] += delta;
                    guides[axis] = Some(guide);
                }
                continue;
            }
            if let Mode::Resize(index) = self.mode
                && Transform::HANDLES.get(index).is_none_or(|p| p[axis] == 0.5)
            {
                continue;
            }
            let mut edges = [left, right];
            let edge = usize::from((point[axis] - left).abs() > (point[axis] - right).abs());
            if let Some(target) = nearest(edges[edge])
                && ((edge == 0 && target < right) || (edge == 1 && target > left))
            {
                edges[edge] = target;
                guides[axis] = Some(target);
            }
            if symmetric {
                let center = if matches!(self.mode, Mode::Create) {
                    self.start[axis]
                } else {
                    self.original.origin[axis] + self.original.size[axis] / 2.
                };
                let half = if point[axis] >= center {
                    edges[1] - center
                } else {
                    center - edges[0]
                };
                if half >= 0.5 {
                    edges = [center - half, center + half];
                }
            }
            frame.origin[axis] = edges[0];
            frame.size[axis] = edges[1] - edges[0];
        }
        (frame, guides)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn symmetric_crop_snaps_both_edges_and_ratio_stays_constrained() {
        let doc = Document::new(100, 80).unwrap();
        let drag = Drag::new(&doc, [50., 40.], Transform::new(100, 80), Mode::Create);
        let (frame, guides) = drag.updated([98., 60.], None, true, 3.);
        assert_eq!(frame.origin, [0., 20.]);
        assert_eq!(frame.size, [100., 40.]);
        assert_eq!(guides[0], Some(100.));
        let (frame, guides) = drag.updated([98., 60.], Some(2.), true, 3.);
        assert_eq!(frame.size, [96., 48.]);
        assert_eq!(guides, [None, None]);
    }
    #[test]
    fn moving_crop_snaps_without_changing_size() {
        let doc = Document::new(100, 80).unwrap();
        let frame = Transform {
            origin: [10., 10.],
            ..Transform::new(20, 30)
        };
        let drag = Drag::new(&doc, [15., 15.], frame, Mode::Move);
        let (moved, _) = drag.updated([84., 54.], Some(2. / 3.), false, 3.);
        assert_eq!(moved.origin, [80., 50.]);
        assert_eq!(moved.size, [20., 30.]);
    }
}
