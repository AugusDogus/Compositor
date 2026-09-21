use crate::adjustment::HueBand;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HueHandle {
    FalloffStart,
    RangeStart,
    RangeEnd,
    FalloffEnd,
}

impl HueBand {
    pub fn handles(self) -> [(HueHandle, f64); 4] {
        [
            (HueHandle::FalloffStart, self.falloff_start),
            (HueHandle::RangeStart, self.range_start),
            (HueHandle::RangeEnd, self.range_end),
            (HueHandle::FalloffEnd, self.falloff_end),
        ]
    }

    pub fn nearest_handle(self, degrees: f64) -> HueHandle {
        let distance = |angle: f64| {
            let gap = (angle - degrees).rem_euclid(360.);
            gap.min(360. - gap)
        };
        self.handles()
            .into_iter()
            .fold(
                (HueHandle::FalloffStart, f64::INFINITY),
                |best, (handle, angle)| {
                    let gap = distance(angle);
                    if gap < best.1 { (handle, gap) } else { best }
                },
            )
            .0
    }

    /// Reject crossings and full-circle bands, matching Swift's spectrum control.
    pub fn set_handle(&mut self, handle: HueHandle, degrees: f64) -> bool {
        if !degrees.is_finite() {
            return false;
        }
        let value = degrees.rem_euclid(360.);
        let mut next = *self;
        match handle {
            HueHandle::FalloffStart => next.falloff_start = value,
            HueHandle::RangeStart => next.range_start = value,
            HueHandle::RangeEnd => next.range_end = value,
            HueHandle::FalloffEnd => next.falloff_end = value,
        }
        let span = (next.falloff_end - next.falloff_start).rem_euclid(360.);
        let start = (next.range_start - next.falloff_start).rem_euclid(360.);
        let end = (next.range_end - next.falloff_start).rem_euclid(360.);
        if span <= 1. || span > 350. || start > end || end > span {
            return false;
        }
        let changed = next != *self;
        *self = next;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adjustment::ColorRange;
    #[test]
    fn handles_wrap_without_crossing_or_expanding_past_a_circle() {
        let mut band = ColorRange::Reds.default_band();
        assert_eq!(band.nearest_handle(359.), HueHandle::RangeStart);
        assert!(band.set_handle(HueHandle::RangeStart, 360.));
        assert_eq!(band.range_start, 0.);
        let before = band;
        assert!(!band.set_handle(HueHandle::RangeStart, 30.));
        assert!(!band.set_handle(HueHandle::FalloffEnd, 315.));
        assert!(!band.set_handle(HueHandle::FalloffEnd, f64::NAN));
        assert_eq!(band, before);
        assert!(band.set_handle(HueHandle::FalloffStart, -60.));
        assert_eq!(band.falloff_start, 300.);
        assert_eq!(band.weight(330.), 0.5);
    }
}
