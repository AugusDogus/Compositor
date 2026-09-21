//! Photoshop is an interchange format. Conversion is staged before UI approval.
mod adjustments;
mod export;
mod import;
mod preflight;
mod resources;
mod vector;
mod vector_metadata;

use crate::{Result, document::Document, invalid};
pub use export::{encode, export_report};
pub use import::decode;
use std::{io::Read, path::Path};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConversionReport {
    pub changes: Vec<String>,
}
impl ConversionReport {
    pub(super) fn note(&mut self, text: impl Into<String>) {
        let text = text.into();
        if !self.changes.contains(&text) {
            self.changes.push(text);
        }
    }
    pub fn description(&self) -> String {
        if self.changes.is_empty() {
            "Layers, folders, opacity, supported blends, masks, and clipping will be preserved. Save continues to use Compositor projects.".into()
        } else {
            format!(
                "{}\n\nThe source file and editable Compositor project are unchanged. Continue with this conversion?",
                self.changes.join("\n\n")
            )
        }
    }
}
pub struct Imported {
    pub document: Document,
    pub report: ConversionReport,
}
pub fn is_psd(path: &Path) -> Result<bool> {
    if path.is_dir() {
        return Ok(false);
    }
    let mut signature = [0; 4];
    let count = std::fs::File::open(path)?.read(&mut signature)?;
    Ok((count == 4 && &signature == b"8BPS")
        || path
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("psd") || s.eq_ignore_ascii_case("psb")))
}
pub fn load(path: &Path) -> Result<Imported> {
    let file = std::fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(invalid("PSD input must be a regular file."));
    }
    let mut bytes = Vec::new();
    file.take(512 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    decode(&bytes)
}
