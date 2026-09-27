//! Validated, ordered color stops for raster gradients.
use crate::{Result, invalid};

pub const MAX_STOPS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stop {
    pub position: f64,
    pub color: [u8; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stops(Vec<Stop>);

impl Stops {
    pub fn new(mut stops: Vec<Stop>) -> Result<Self> {
        if !(2..=MAX_STOPS).contains(&stops.len()) {
            return Err(invalid("A gradient needs between 2 and 32 color stops."));
        }
        if stops.iter().any(|stop| !valid_position(stop.position)) {
            return Err(invalid(
                "Gradient stop positions must be between 0 and 100%.",
            ));
        }
        stops.sort_by(|a, b| a.position.total_cmp(&b.position));
        Ok(Self(stops))
    }

    pub fn endpoints(start: [u8; 4], end: [u8; 4]) -> Self {
        Self(vec![
            Stop {
                position: 0.,
                color: start,
            },
            Stop {
                position: 1.,
                color: end,
            },
        ])
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        self.0.capacity() * std::mem::size_of::<Stop>()
    }

    pub fn as_slice(&self) -> &[Stop] {
        &self.0
    }

    /// Stops at the same position form a hard edge; the last one wins at that position.
    pub fn sample(&self, position: f64) -> [f64; 4] {
        let right = self.0.partition_point(|stop| stop.position <= position);
        let rgba = |stop: Stop| stop.color.map(|channel| f64::from(channel) / 255.);
        if right == 0 {
            return rgba(self.0[0]);
        }
        let left = self.0[right - 1];
        let Some(next) = self.0.get(right) else {
            return rgba(left);
        };
        let t = (position - left.position) / (next.position - left.position);
        let a = rgba(left);
        let b = rgba(*next);
        std::array::from_fn(|channel| a[channel] + (b[channel] - a[channel]) * t)
    }

    pub fn insert(&mut self, position: f64) -> Result<usize> {
        if self.0.len() >= MAX_STOPS {
            return Err(invalid("A gradient can have at most 32 color stops."));
        }
        if !valid_position(position) {
            return Err(invalid(
                "Gradient stop positions must be between 0 and 100%.",
            ));
        }
        let color = self
            .sample(position)
            .map(|channel| (channel * 255.).round() as u8);
        let index = self.0.partition_point(|stop| stop.position <= position);
        self.0.insert(index, Stop { position, color });
        Ok(index)
    }

    pub fn update(&mut self, index: usize, stop: Stop) -> Result<usize> {
        if index >= self.0.len() {
            return Err(invalid("Select a gradient stop before changing it."));
        }
        if !valid_position(stop.position) {
            return Err(invalid(
                "Gradient stop positions must be between 0 and 100%.",
            ));
        }
        if self.0[index].position == stop.position {
            self.0[index] = stop;
            return Ok(index);
        }
        self.0.remove(index);
        let index = self
            .0
            .partition_point(|entry| entry.position <= stop.position);
        self.0.insert(index, stop);
        Ok(index)
    }

    pub fn remove(&mut self, index: usize) -> Result<usize> {
        if self.0.len() <= 2 {
            return Err(invalid("Keep at least two gradient stops."));
        }
        if index >= self.0.len() {
            return Err(invalid("Select a gradient stop before removing it."));
        }
        self.0.remove(index);
        Ok(index.min(self.0.len() - 1))
    }
}

fn valid_position(position: f64) -> bool {
    (0. ..=1.).contains(&position)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn samples_multiple_stops_alpha_and_hard_edges() {
        let stops = Stops::new(vec![
            Stop {
                position: 1.,
                color: [0, 0, 255, 0],
            },
            Stop {
                position: 0.,
                color: [255, 0, 0, 255],
            },
            Stop {
                position: 0.5,
                color: [0, 255, 0, 255],
            },
        ])
        .unwrap();
        assert_eq!(stops.sample(0.25), [0.5, 0.5, 0., 1.]);
        assert_eq!(stops.sample(0.75), [0., 0.5, 0.5, 0.5]);
        let edge = Stops::new(vec![
            Stop {
                position: 0.5,
                color: [0; 4],
            },
            Stop {
                position: 0.5,
                color: [255; 4],
            },
        ])
        .unwrap();
        assert_eq!(edge.sample(0.49), [0.; 4]);
        assert_eq!(edge.sample(0.5), [1.; 4]);
    }
    #[test]
    fn edits_keep_stop_order_and_reject_invalid_changes_without_mutation() {
        let mut stops = Stops::endpoints([0; 4], [255; 4]);
        let mid = stops.insert(0.5).unwrap();
        assert_eq!(stops.as_slice()[mid].color, [128; 4]);
        let index = stops
            .update(
                mid,
                Stop {
                    position: 0.9,
                    color: [12; 4],
                },
            )
            .unwrap();
        assert_eq!(stops.as_slice()[index].position, 0.9);
        for position in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
            let before = stops.clone();
            assert!(stops.insert(position).is_err());
            assert!(
                stops
                    .update(
                        0,
                        Stop {
                            position,
                            color: [0; 4]
                        }
                    )
                    .is_err()
            );
            assert_eq!(stops, before);
        }
        stops.remove(index).unwrap();
        assert!(stops.remove(0).is_err());
        for _ in 2..MAX_STOPS {
            stops.insert(0.5).unwrap();
        }
        assert!(stops.insert(0.5).is_err());
    }
}
