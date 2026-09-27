use super::Hose;
use crate::{Result, invalid};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    Constant,
    Incremental,
    Random,
    Angular,
    Pressure,
    XTilt,
    YTilt,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Dimension {
    pub rank: u32,
    pub stride: u32,
    pub selection: Selection,
}

#[derive(Clone, Copy, Debug)]
pub struct Dynamics {
    pressure: f64,
    tilt: [f64; 2],
    direction: Option<f64>,
}
impl Dynamics {
    /// Pressure 0..1, tilt in degrees -90..90, direction in document coordinates
    /// (positive y points down). Missing tablet axes use neutral mouse values.
    pub fn new(pressure: Option<f64>, tilt: Option<[f64; 2]>, direction: [f64; 2]) -> Result<Self> {
        let pressure = pressure.unwrap_or(1.);
        let tilt = tilt.unwrap_or([0.; 2]);
        if !pressure.is_finite() || tilt.iter().chain(direction.iter()).any(|v| !v.is_finite()) {
            return Err(invalid(
                "The brush input contains non-finite pressure, tilt or direction. The stroke was not changed.",
            ));
        }
        Ok(Self {
            pressure: pressure.clamp(0., 1.),
            tilt: tilt.map(|v| v.clamp(-90., 90.) / 90.),
            direction: (direction != [0.; 2]).then(|| {
                (-direction[1])
                    .atan2(direction[0])
                    .rem_euclid(std::f64::consts::TAU)
                    / std::f64::consts::TAU
            }),
        })
    }
}

impl Hose {
    /// Absolute dab index and fixed stroke seed make provisional-tail replay and
    /// CPU/GPU coverage independent of how pointer events split a stroke.
    pub fn cell_index(&self, dab: u32, seed: u32, dynamics: Dynamics) -> Option<usize> {
        let index = select(self.plan(dynamics)?, dab, seed);
        Some(index.min(self.cells.len() - 1))
    }
    /// GPU-compatible rows: mode (fixed/cycle/random), rank, stride, fixed index.
    /// Resolve floating-point tablet axes here so CPU and GPU choose identical cells.
    pub(in crate::brush) fn plan(&self, dynamics: Dynamics) -> Option<[[u32; 4]; 4]> {
        let mut plan = [[0, 1, 0, 0]; 4];
        for (axis, dimension) in self.dimensions.iter().enumerate() {
            let rank = dimension.rank;
            let (mode, selected) = match dimension.selection {
                Selection::Constant => (0, 0),
                Selection::Incremental => (1, 0),
                Selection::Random => (2, 0),
                Selection::Angular => (
                    0,
                    ((1. - dynamics.direction? + 0.25) * f64::from(rank)).round_ties_even() as u32
                        % rank,
                ),
                Selection::Pressure => (
                    0,
                    (dynamics.pressure * f64::from(rank - 1)).round_ties_even() as u32,
                ),
                Selection::XTilt | Selection::YTilt => {
                    let axis = usize::from(dimension.selection == Selection::YTilt);
                    (
                        0,
                        ((dynamics.tilt[axis] * 0.5 * f64::from(rank)).round_ties_even() as i32
                            + (rank / 2) as i32)
                            .clamp(0, rank as i32 - 1) as u32,
                    )
                }
            };
            plan[axis] = [mode, rank, dimension.stride, selected];
        }
        Some(plan)
    }
}

pub(in crate::brush) fn select(plan: [[u32; 4]; 4], dab: u32, seed: u32) -> usize {
    let mut index = 0;
    for (axis, [mode, rank, stride, fixed]) in plan.into_iter().enumerate() {
        let selected = match mode {
            1 => dab.wrapping_add(1) % rank,
            2 => hash(dab ^ seed ^ (axis as u32).wrapping_mul(0x9e37_79b9)) % rank,
            _ => fixed,
        };
        index += stride * selected;
    }
    index as usize
}

fn hash(mut value: u32) -> u32 {
    value = (value ^ (value >> 16)).wrapping_mul(0x7feb_352d);
    value = (value ^ (value >> 15)).wrapping_mul(0x846c_a68b);
    value ^ (value >> 16)
}
