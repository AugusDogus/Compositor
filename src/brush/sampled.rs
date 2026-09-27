//! Immutable sampled tip shapes. File colors are deliberately reduced to alpha.
mod gbr;
pub(super) mod segment;
use crate::{Result, invalid};
pub use gbr::read;
use image::GrayImage;
use std::sync::Arc;

pub const MAX_TIP_PIXELS: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct Tip {
    name: String,
    pixels: GrayImage,
    spacing: f64,
    colored: bool,
}
impl Tip {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn pixels(&self) -> &GrayImage {
        &self.pixels
    }
    pub fn spacing(&self) -> f64 {
        self.spacing
    }
    pub fn embedded_colors(&self) -> bool {
        self.colored
    }
}

#[derive(Clone, Debug, Default)]
pub enum Shape {
    #[default]
    Round,
    Sampled(Sampled),
}

#[derive(Clone, Debug)]
pub struct Sampled {
    tip: Arc<Tip>,
    spacing: f64,
}
impl Sampled {
    pub fn new(tip: Arc<Tip>) -> Self {
        let spacing = tip.spacing;
        Self { tip, spacing }
    }
    pub fn tip(&self) -> &Arc<Tip> {
        &self.tip
    }
    pub fn spacing(&self) -> f64 {
        self.spacing
    }
    pub fn set_spacing(&mut self, fraction: f64) -> Result<()> {
        if !(0.01..=10.).contains(&fraction) {
            return Err(invalid(
                "Sampled tip spacing must be 1 to 1000% of its diameter.",
            ));
        }
        self.spacing = fraction;
        Ok(())
    }
}

pub(super) struct State {
    pub brush: Sampled,
    /// Distance until the next stamp, measured in brush diameters.
    pub next: f64,
}
impl State {
    pub fn kernel(&self, diameter: f64) -> super::coverage::Kernel {
        super::coverage::Kernel::Sampled {
            tip: self.brush.tip.clone(),
            diameter,
            first: self.next * diameter,
            spacing: self.brush.spacing * diameter,
        }
    }
    pub fn advance(&mut self, length: f64, diameter: f64) {
        let length = length / diameter;
        if self.next <= length {
            self.next += ((length - self.next) / self.brush.spacing).floor().max(0.)
                * self.brush.spacing
                + self.brush.spacing;
        }
        self.next = (self.next - length).max(0.);
    }
}
