//! Repeating RGBA pattern tiles and bounded Adobe Photoshop PAT imports.
mod adobe;
mod overlay;
use crate::{Result, invalid};
use image::RgbaImage;
pub use overlay::{Overlay, Settings};
use std::{io::Read, path::Path, sync::Arc};

pub const MAX_PATTERN_PIXELS: u64 = 16 * 1024 * 1024;
pub const MAX_PACK_PIXELS: u64 = 32 * 1024 * 1024;
pub const MAX_PATTERNS: usize = 256;
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct Pattern {
    id: String,
    name: String,
    pixels: Arc<RgbaImage>,
    digest: String,
}

impl Pattern {
    pub fn from_pixels(name: &str, pixels: RgbaImage) -> Result<Self> {
        Self::new(uuid::Uuid::new_v4().to_string(), name.to_owned(), pixels)
    }

    fn new(id: String, name: String, pixels: RgbaImage) -> Result<Self> {
        Self::from_shared(id, name, Arc::new(pixels))
    }

    pub(crate) fn from_shared(id: String, name: String, pixels: Arc<RgbaImage>) -> Result<Self> {
        validate_size(pixels.width(), pixels.height())?;
        validate_labels(&id, &name)?;
        let digest = pixel_digest(&pixels);
        Ok(Self {
            id,
            name,
            pixels,
            digest,
        })
    }

    pub(crate) fn relabel(&self, id: String, name: String) -> Result<Self> {
        validate_labels(&id, &name)?;
        Ok(Self {
            id,
            name,
            pixels: self.pixels.clone(),
            digest: self.digest.clone(),
        })
    }

    pub(crate) fn digest(&self) -> &str {
        &self.digest
    }

    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn pixels(&self) -> &Arc<RgbaImage> {
        &self.pixels
    }
}

pub(crate) fn validate_labels(id: &str, name: &str) -> Result<()> {
    if name.trim().is_empty() || name.len() > 16_384 || name.chars().any(char::is_control) {
        return Err(invalid(
            "A pattern name must contain 1 to 16384 bytes without control characters.",
        ));
    }
    if id.is_empty() || id.len() > 255 || !id.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(invalid(
            "The pattern identifier must contain 1 to 255 printable ASCII characters.",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct Pack {
    patterns: Vec<Pattern>,
}

impl Pack {
    pub fn patterns(&self) -> &[Pattern] {
        &self.patterns
    }

    pub fn read(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
            return Err(invalid(
                "Choose a regular Photoshop PAT file no larger than 64 MiB. No patterns were imported.",
            ));
        }
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
        Self::from_bytes(&bytes)
    }

    /// Reject the whole pack on malformed or unsupported entries. The caller
    /// receives no partial library that could hide omitted patterns.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(invalid(
                "The PAT file exceeds 64 MiB. Export a smaller pattern pack; no patterns were imported.",
            ));
        }
        Ok(Self {
            patterns: adobe::decode(bytes)?,
        })
    }
}

pub(crate) fn validate_size(width: u32, height: u32) -> Result<()> {
    if width == 0
        || height == 0
        || width > 30_000
        || height > 30_000
        || u64::from(width) * u64::from(height) > MAX_PATTERN_PIXELS
    {
        return Err(invalid(
            "A pattern tile must have 1 to 30000 pixels per side and at most 16 megapixels. Export a smaller tile; no patterns were imported.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;

fn pixel_digest(pixels: &RgbaImage) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(b"compositor-pattern-v1\0");
    hash.update(pixels.width().to_le_bytes());
    hash.update(pixels.height().to_le_bytes());
    hash.update(pixels.as_raw());
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
