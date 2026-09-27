//! GIMP image pipes: bounded text parameters followed by complete GBR cells.
//! Format: https://developer.gimp.org/core/standards/gih/
mod parameters;
pub(super) mod selection;
use super::{Tip, gbr};
use crate::{Result, invalid};
pub use selection::{Dynamics, Selection};
use std::{io::Read, path::Path, sync::Arc};

pub const MAX_CELLS: usize = 512;
pub const MAX_PIXELS: usize = 16 * 1024 * 1024;
const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct Hose {
    name: String,
    cells: Vec<Arc<Tip>>,
    dimensions: Vec<selection::Dimension>,
}
impl Hose {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn cells(&self) -> &[Arc<Tip>] {
        &self.cells
    }
    /// GIMP takes painting spacing from the first embedded GBR, not `step:`.
    pub fn spacing(&self) -> f64 {
        self.cells[0].spacing()
    }
    pub fn pixels(&self) -> usize {
        self.cells.iter().map(|c| c.pixels().len()).sum()
    }
    pub fn embedded_colors(&self) -> bool {
        self.cells.iter().any(|c| c.embedded_colors())
    }
    pub fn selections(&self) -> impl Iterator<Item = (u32, Selection)> + '_ {
        self.dimensions.iter().map(|d| (d.rank, d.selection))
    }

    pub fn read(path: &Path) -> Result<Self> {
        let metadata = std::fs::metadata(path)?;
        if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES as u64 {
            return Err(invalid(
                "Choose a regular GIH file no larger than 64 MiB. No brush was loaded.",
            ));
        }
        let file = std::fs::File::open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES as u64 {
            return Err(invalid(
                "The GIH file changed or exceeds 64 MiB. No brush was loaded.",
            ));
        }
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        Self::from_bytes(&bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_FILE_BYTES {
            return Err(invalid(
                "The GIH file exceeds 64 MiB. Export a smaller brush; no tips were loaded.",
            ));
        }
        let (name, rest) = line(bytes)?;
        let (parameters, mut rest) = line(rest)?;
        let name = String::from_utf8_lossy(name).trim().to_string();
        if name.is_empty() || name.contains('\0') {
            return Err(invalid(
                "The GIH brush has no valid name. Export it again from GIMP.",
            ));
        }
        let (count, dimensions) = parameters::parse(parameters)?;
        let mut cells = Vec::with_capacity(count);
        let mut remaining = MAX_PIXELS;
        for index in 0..count {
            let (tip, consumed) = gbr::parse_one(rest, remaining).map_err(|error| {
                invalid(format!(
                    "Could not load GIH cell {}: {error}. No brush was loaded.",
                    index + 1
                ))
            })?;
            remaining -= tip.pixels().len();
            cells.push(Arc::new(tip));
            rest = &rest[consumed..];
        }
        if !rest.is_empty() {
            return Err(invalid(
                "The GIH cell count does not match its contents. Export the brush again from GIMP; no brush was loaded.",
            ));
        }
        Ok(Self {
            name,
            cells,
            dimensions,
        })
    }
}

fn line(bytes: &[u8]) -> Result<(&[u8], &[u8])> {
    let length = bytes.iter().take(1024).position(|b| *b == b'\n')
        .ok_or_else(|| invalid("The GIH text header is missing or exceeds 1023 bytes per line. Export the brush again from GIMP."))?;
    Ok((&bytes[..length], &bytes[length + 1..]))
}

#[cfg(test)]
mod tests;
