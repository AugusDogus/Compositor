//! Immutable sampled tip shapes. File colors are deliberately reduced to alpha.
pub mod abr;
mod gbr;
pub mod gih;
pub(super) mod segment;
mod source;
mod state;
use crate::{Result, invalid};
pub use gbr::read;
use image::GrayImage;
pub(super) use source::{Source, Stamp};
pub(super) use state::State;
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
    source: Source,
    spacing: f64,
    counter: Arc<std::sync::atomic::AtomicU32>,
}
impl Sampled {
    pub fn new(tip: Arc<Tip>) -> Self {
        let spacing = tip.spacing;
        Self {
            source: Source::Tip(tip),
            spacing,
            counter: Arc::default(),
        }
    }
    pub fn tip(&self) -> &Arc<Tip> {
        match &self.source {
            Source::Tip(tip) => tip,
            Source::Hose(hose) => &hose.cells()[0],
        }
    }
    pub fn from_hose(hose: Arc<gih::Hose>) -> Self {
        let spacing = hose.spacing();
        Self {
            source: Source::Hose(hose),
            spacing,
            counter: Arc::default(),
        }
    }
    pub fn hose(&self) -> Option<&Arc<gih::Hose>> {
        match &self.source {
            Source::Hose(hose) => Some(hose),
            Source::Tip(_) => None,
        }
    }
    pub fn name(&self) -> &str {
        self.hose()
            .map_or_else(|| self.tip().name(), |hose| hose.name())
    }
    pub fn pixel_count(&self) -> usize {
        self.source
            .cells()
            .iter()
            .map(|tip| tip.pixels().len())
            .sum()
    }
    pub fn embedded_colors(&self) -> bool {
        self.source.cells().iter().any(|tip| tip.embedded_colors())
    }
    pub fn same_source(&self, other: &Self) -> bool {
        self.source.same(&other.source)
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
