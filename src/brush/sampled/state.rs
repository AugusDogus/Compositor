use super::{Sampled, Source, Stamp, gih};
use std::sync::atomic::{AtomicU32, Ordering};
static SEED: AtomicU32 = AtomicU32::new(0x6a09_e667);

pub(in crate::brush) struct State {
    pub brush: Sampled,
    /// Distance until the next stamp, measured in brush diameters.
    pub next: f64,
    pub dab: u32,
    seed: u32,
}
impl State {
    pub fn new(brush: Sampled) -> Self {
        let dab = brush.counter.load(Ordering::Relaxed);
        Self {
            brush,
            next: 0.,
            dab,
            seed: SEED.fetch_add(0x9e37_79b9, Ordering::Relaxed),
        }
    }
    pub fn commit(&self) {
        self.brush.counter.store(self.dab, Ordering::Relaxed);
    }
    pub fn kernel(
        &self,
        diameter: f64,
        dynamics: gih::Dynamics,
    ) -> Option<crate::brush::coverage::Kernel> {
        let plan = match &self.brush.source {
            Source::Tip(_) => [[0, 1, 0, 0]; 4],
            Source::Hose(hose) => hose.plan(dynamics)?,
        };
        Some(crate::brush::coverage::Kernel::Sampled {
            stamp: Stamp {
                source: self.brush.source.clone(),
                plan,
                dab: self.dab,
                seed: self.seed,
            },
            diameter,
            first: self.next * diameter,
            spacing: self.brush.spacing * diameter,
        })
    }
    pub fn advance(&mut self, length: f64, diameter: f64) {
        let length = length / diameter;
        if self.next <= length {
            let count = ((length - self.next) / self.brush.spacing).floor().max(0.) + 1.;
            self.next += count * self.brush.spacing;
            self.dab = self.dab.wrapping_add(count as u32);
        }
        self.next = (self.next - length).max(0.);
    }
}
