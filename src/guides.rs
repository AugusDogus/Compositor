use crate::{
    Result,
    document::Document,
    geometry::{Point, Transform},
    invalid,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    Horizontal,
    Vertical,
}
impl Axis {
    pub fn index(self) -> usize {
        match self {
            Self::Vertical => 0,
            Self::Horizontal => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Guide {
    pub id: Uuid,
    pub axis: Axis,
    pub position: f64,
}

pub fn validate(guides: &[Guide]) -> Result<()> {
    let mut ids = std::collections::HashSet::new();
    if guides.len() > 1000
        || guides
            .iter()
            .any(|g| !ids.insert(g.id) || !g.position.is_finite() || g.position.abs() > 1_000_000.)
    {
        return Err(invalid(
            "Project guides must have unique IDs, finite positions within one million pixels, and at most 1,000 entries.",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub grid: bool,
    pub guides: bool,
    pub rulers: bool,
    pub locked: bool,
    pub snap: bool,
    pub to_grid: bool,
    pub to_guides: bool,
    pub to_layers: bool,
    pub to_bounds: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            grid: false,
            guides: true,
            rulers: false,
            locked: false,
            snap: true,
            to_grid: true,
            to_guides: true,
            to_layers: true,
            to_bounds: true,
        }
    }
}

impl Settings {
    pub fn targets(
        self,
        doc: &Document,
        moving: &std::collections::HashSet<Uuid>,
        centers: bool,
    ) -> [Vec<f64>; 2] {
        let mut targets: [Vec<f64>; 2] = Default::default();
        if !self.snap {
            return targets;
        }
        if self.to_bounds {
            for (axis, length) in [doc.width, doc.height].into_iter().enumerate() {
                targets[axis].extend([0., length as f64]);
                if centers {
                    targets[axis].push(length as f64 / 2.);
                }
            }
        }
        if self.to_layers {
            for layer in doc.layers.iter().filter(|l| {
                l.raster().is_some() && doc.layer_is_visible(l.id) && !moving.contains(&l.id)
            }) {
                let b = layer.transform.bounds();
                for axis in 0..2 {
                    targets[axis].extend([b[axis], b[axis + 2]]);
                    if centers {
                        targets[axis].push((b[axis] + b[axis + 2]) / 2.);
                    }
                }
            }
        }
        if self.to_grid && self.grid {
            for (axis, length) in [doc.width, doc.height].into_iter().enumerate() {
                targets[axis].extend((0..=length).step_by(8).map(f64::from));
            }
        }
        if self.to_guides && self.guides {
            for guide in &doc.guides {
                targets[guide.axis.index()].push(guide.position);
            }
        }
        targets
    }

    pub fn snap_move(
        self,
        doc: &Document,
        original: Transform,
        delta: Point,
        tolerance: f64,
    ) -> (Point, [Option<f64>; 2]) {
        let targets = self.targets(doc, &crate::transform::target_ids(doc), true);
        crate::transform::snap_to_targets(original, delta, tolerance, &targets)
    }
}
