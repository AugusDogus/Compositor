use crate::{Result, gradient::stops::MAX_STOPS, invalid};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpacityStop {
    pub position: f64,
    pub opacity: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct OpacityStops(Vec<OpacityStop>);

impl<'de> Deserialize<'de> for OpacityStops {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        Self::new(Vec::<OpacityStop>::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl Default for OpacityStops {
    fn default() -> Self {
        Self(vec![
            OpacityStop {
                position: 0.,
                opacity: 1.,
            },
            OpacityStop {
                position: 1.,
                opacity: 1.,
            },
        ])
    }
}
impl OpacityStops {
    pub fn new(mut stops: Vec<OpacityStop>) -> Result<Self> {
        if !(2..=MAX_STOPS).contains(&stops.len()) {
            return Err(invalid("A gradient needs between 2 and 32 opacity stops."));
        }
        for stop in &stops {
            validate(*stop)?;
        }
        stops.sort_by(|a, b| a.position.total_cmp(&b.position));
        Ok(Self(stops))
    }
    pub(super) fn retained_bytes(&self) -> usize {
        self.0.capacity() * std::mem::size_of::<OpacityStop>()
    }
    pub fn as_slice(&self) -> &[OpacityStop] {
        &self.0
    }
    /// Independently interpolate opacity. The last coincident stop wins.
    pub fn sample(&self, position: f64) -> f64 {
        let right = self.0.partition_point(|stop| stop.position <= position);
        if right == 0 {
            return self.0[0].opacity;
        }
        let left = self.0[right - 1];
        let Some(next) = self.0.get(right) else {
            return left.opacity;
        };
        let t = (position - left.position) / (next.position - left.position);
        left.opacity + (next.opacity - left.opacity) * t
    }
    pub fn insert(&mut self, position: f64) -> Result<usize> {
        if self.0.len() >= MAX_STOPS {
            return Err(invalid("A gradient can have at most 32 opacity stops."));
        }
        if !(0. ..=1.).contains(&position) {
            return Err(invalid(
                "Opacity stop positions must be between 0% and 100%.",
            ));
        }
        let stop = OpacityStop {
            position,
            opacity: self.sample(position),
        };
        let index = self.0.partition_point(|s| s.position <= position);
        self.0.insert(index, stop);
        Ok(index)
    }
    pub fn update(&mut self, index: usize, stop: OpacityStop) -> Result<usize> {
        if index >= self.0.len() {
            return Err(invalid("Select an opacity stop before changing it."));
        }
        validate(stop)?;
        if self.0[index].position == stop.position {
            self.0[index] = stop;
            return Ok(index);
        }
        self.0.remove(index);
        let index = self.0.partition_point(|s| s.position <= stop.position);
        self.0.insert(index, stop);
        Ok(index)
    }
    pub fn remove(&mut self, index: usize) -> Result<usize> {
        if self.0.len() <= 2 {
            return Err(invalid("Keep at least two opacity stops."));
        }
        if index >= self.0.len() {
            return Err(invalid("Select an opacity stop before removing it."));
        }
        self.0.remove(index);
        Ok(index.min(self.0.len() - 1))
    }
}
fn validate(stop: OpacityStop) -> Result<()> {
    if !(0. ..=1.).contains(&stop.position) || !(0. ..=1.).contains(&stop.opacity) {
        return Err(invalid(
            "Gradient opacity and stop positions must be between 0% and 100%.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opacity_edits_are_independent_precise_and_validated_before_mutation() {
        let mut stops = OpacityStops::new(vec![
            OpacityStop {
                position: 0.,
                opacity: 0.123456789,
            },
            OpacityStop {
                position: 1.,
                opacity: 0.987654321,
            },
        ])
        .unwrap();
        let expected = stops.sample(0.37);
        let index = stops.insert(0.37).unwrap();
        assert_eq!(stops.as_slice()[index].opacity, expected);
        assert!(
            (stops.sample(0.2) - (0.123456789 + (0.987654321 - 0.123456789) * 0.2)).abs() < 1e-12
        );
        for value in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
            let before = stops.clone();
            assert!(
                stops
                    .update(
                        index,
                        OpacityStop {
                            position: 0.5,
                            opacity: value
                        }
                    )
                    .is_err()
            );
            assert!(stops.insert(value).is_err());
            assert_eq!(stops, before);
        }
        stops.remove(index).unwrap();
        assert!(stops.remove(0).is_err());
        assert_eq!(
            serde_json::from_str::<OpacityStops>(&serde_json::to_string(&stops).unwrap()).unwrap(),
            stops
        );
    }
    #[test]
    fn opacity_coincident_stops_keep_hard_edges() {
        let stops = OpacityStops::new(vec![
            OpacityStop {
                position: 0.5,
                opacity: 0.,
            },
            OpacityStop {
                position: 0.5,
                opacity: 1.,
            },
        ])
        .unwrap();
        assert_eq!(stops.sample(0.49), 0.);
        assert_eq!(stops.sample(0.5), 1.);
        assert_eq!(stops.sample(0.51), 1.);
        assert!(
            serde_json::from_str::<OpacityStops>(
                r#"[{"position":0,"opacity":0},{"position":1,"opacity":1.1}]"#
            )
            .is_err()
        );
    }
}
