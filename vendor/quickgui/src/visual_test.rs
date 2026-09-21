use std::{fmt, fs::File, io::BufWriter, path::Path, sync::Arc};

use image_codecs::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use thiserror::Error;

use crate::{Image, ImageError};

/// Largest physical width or height accepted by one visual-test capture.
pub const MAX_VISUAL_TEST_DIMENSION: u32 = 4_096;
/// Largest tightly packed RGBA allocation returned by one visual-test capture.
pub const MAX_VISUAL_TEST_BYTES: u64 = 64 * 1024 * 1024;

/// Immutable, cheaply cloned RGBA8 output from the production WGPU pipelines.
///
/// Pixels are tightly packed in top-to-bottom row order. Clones share the same bounded storage.
#[derive(Clone)]
pub struct VisualSnapshot {
    image: Image,
}

impl VisualSnapshot {
    pub(crate) fn from_rgba(
        width: u32,
        height: u32,
        rgba: impl Into<Arc<[u8]>>,
    ) -> Result<Self, VisualTestError> {
        validate_snapshot_dimensions(width, height)?;
        Ok(Self {
            image: Image::from_rgba(width, height, rgba)?,
        })
    }

    /// Load a bounded PNG baseline from disk.
    pub fn open_png(path: impl AsRef<Path>) -> Result<Self, VisualTestError> {
        let image = Image::open(path)?;
        validate_snapshot_dimensions(image.width(), image.height())?;
        Ok(Self { image })
    }

    /// Write this snapshot as an RGBA PNG.
    pub fn write_png(&self, path: impl AsRef<Path>) -> Result<(), VisualTestError> {
        let file = File::create(path).map_err(VisualTestError::Write)?;
        PngEncoder::new(BufWriter::new(file))
            .write_image(
                self.rgba(),
                self.width(),
                self.height(),
                ExtendedColorType::Rgba8,
            )
            .map_err(|error| VisualTestError::Encode(error.to_string()))
    }

    pub fn width(&self) -> u32 {
        self.image.width()
    }

    pub fn height(&self) -> u32 {
        self.image.height()
    }

    pub fn byte_len(&self) -> usize {
        self.image.byte_len()
    }

    pub fn rgba(&self) -> &[u8] {
        self.image.rgba()
    }

    /// Read one physical RGBA8 pixel, returning `None` outside the snapshot.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width() || y >= self.height() {
            return None;
        }
        let offset =
            (u64::from(y) * u64::from(self.width()) + u64::from(x)).checked_mul(4)? as usize;
        self.rgba().get(offset..offset + 4)?.try_into().ok()
    }

    /// Compare this actual snapshot with an expected snapshot without allocating a diff image.
    pub fn difference(
        &self,
        expected: &Self,
        tolerance: VisualTolerance,
    ) -> Result<VisualDifference, VisualTestError> {
        if self.width() != expected.width() || self.height() != expected.height() {
            return Err(VisualTestError::DimensionMismatch {
                actual: (self.width(), self.height()),
                expected: (expected.width(), expected.height()),
            });
        }

        let mut differing_pixels = 0_u64;
        let mut maximum_channel_delta = 0_u8;
        let mut total_channel_delta = 0_u64;
        for (actual, expected) in self
            .rgba()
            .as_chunks::<4>()
            .0
            .iter()
            .zip(expected.rgba().as_chunks::<4>().0.iter())
        {
            let mut pixel_delta = 0_u8;
            for channel in 0..4 {
                let delta = actual[channel].abs_diff(expected[channel]);
                pixel_delta = pixel_delta.max(delta);
                maximum_channel_delta = maximum_channel_delta.max(delta);
                total_channel_delta += u64::from(delta);
            }
            if pixel_delta > tolerance.maximum_channel_delta {
                differing_pixels += 1;
            }
        }

        Ok(VisualDifference {
            total_pixels: u64::from(self.width()) * u64::from(self.height()),
            differing_pixels,
            maximum_channel_delta,
            total_channel_delta,
        })
    }

    /// Assert that this actual snapshot matches an expected snapshot within explicit tolerances.
    pub fn assert_matches(
        &self,
        expected: &Self,
        tolerance: VisualTolerance,
    ) -> Result<VisualDifference, VisualTestError> {
        let difference = self.difference(expected, tolerance)?;
        if difference.differing_pixels > tolerance.maximum_differing_pixels {
            return Err(VisualTestError::SnapshotMismatch {
                differing_pixels: difference.differing_pixels,
                maximum_differing_pixels: tolerance.maximum_differing_pixels,
                maximum_channel_delta: difference.maximum_channel_delta,
                allowed_channel_delta: tolerance.maximum_channel_delta,
            });
        }
        Ok(difference)
    }

    /// Load and compare against one bounded PNG baseline.
    pub fn assert_matches_png(
        &self,
        expected: impl AsRef<Path>,
        tolerance: VisualTolerance,
    ) -> Result<VisualDifference, VisualTestError> {
        self.assert_matches(&Self::open_png(expected)?, tolerance)
    }
}

impl fmt::Debug for VisualSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VisualSnapshot")
            .field("width", &self.width())
            .field("height", &self.height())
            .field("byte_len", &self.byte_len())
            .finish()
    }
}

/// Explicit per-channel and changed-pixel tolerance for screenshot comparisons.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct VisualTolerance {
    pub maximum_channel_delta: u8,
    pub maximum_differing_pixels: u64,
}

impl VisualTolerance {
    pub const EXACT: Self = Self {
        maximum_channel_delta: 0,
        maximum_differing_pixels: 0,
    };

    pub const fn new(maximum_channel_delta: u8, maximum_differing_pixels: u64) -> Self {
        Self {
            maximum_channel_delta,
            maximum_differing_pixels,
        }
    }
}

/// Allocation-free screenshot comparison statistics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VisualDifference {
    pub total_pixels: u64,
    pub differing_pixels: u64,
    pub maximum_channel_delta: u8,
    pub total_channel_delta: u64,
}

#[derive(Debug, Error)]
pub enum VisualTestError {
    #[error(
        "visual snapshot dimensions must be between 1 and {MAX_VISUAL_TEST_DIMENSION} physical pixels per axis"
    )]
    InvalidDimensions,
    #[error(
        "visual snapshot requires {bytes} RGBA bytes, exceeding the {MAX_VISUAL_TEST_BYTES}-byte limit"
    )]
    TooLarge { bytes: u64 },
    #[error("could not initialize an offscreen GPU adapter: {0}")]
    Adapter(String),
    #[error("could not initialize an offscreen GPU device: {0}")]
    Device(String),
    #[error("offscreen rendering failed: {0}")]
    Render(String),
    #[error("offscreen GPU readback failed: {0}")]
    Readback(String),
    #[error(
        "native NSView pixels cannot be captured by the headless WGPU target; assert its host geometry separately"
    )]
    NativeViewUnsupported,
    #[error("could not decode visual baseline: {0}")]
    Image(#[from] ImageError),
    #[error("could not create visual baseline: {0}")]
    Write(#[source] std::io::Error),
    #[error("could not encode visual baseline: {0}")]
    Encode(String),
    #[error("snapshot dimensions differ: actual {actual:?}, expected {expected:?}")]
    DimensionMismatch {
        actual: (u32, u32),
        expected: (u32, u32),
    },
    #[error(
        "snapshot differs in {differing_pixels} pixels (allowed {maximum_differing_pixels}); maximum channel delta {maximum_channel_delta} (allowed {allowed_channel_delta})"
    )]
    SnapshotMismatch {
        differing_pixels: u64,
        maximum_differing_pixels: u64,
        maximum_channel_delta: u8,
        allowed_channel_delta: u8,
    },
}

pub(crate) fn validate_snapshot_dimensions(width: u32, height: u32) -> Result<(), VisualTestError> {
    if width == 0
        || height == 0
        || width > MAX_VISUAL_TEST_DIMENSION
        || height > MAX_VISUAL_TEST_DIMENSION
    {
        return Err(VisualTestError::InvalidDimensions);
    }
    let bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(VisualTestError::TooLarge { bytes: u64::MAX })?;
    if bytes > MAX_VISUAL_TEST_BYTES {
        return Err(VisualTestError::TooLarge { bytes });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visual_differences_are_bounded_and_apply_both_tolerances() {
        let expected = VisualSnapshot::from_rgba(2, 1, [10, 20, 30, 255, 1, 2, 3, 255]).unwrap();
        let actual = VisualSnapshot::from_rgba(2, 1, [12, 20, 30, 255, 9, 2, 3, 255]).unwrap();

        let difference = actual
            .difference(&expected, VisualTolerance::new(2, 0))
            .unwrap();
        assert_eq!(difference.total_pixels, 2);
        assert_eq!(difference.differing_pixels, 1);
        assert_eq!(difference.maximum_channel_delta, 8);
        assert_eq!(difference.total_channel_delta, 10);
        assert!(
            actual
                .assert_matches(&expected, VisualTolerance::new(2, 0))
                .is_err()
        );
        assert!(
            actual
                .assert_matches(&expected, VisualTolerance::new(2, 1))
                .is_ok()
        );
    }

    #[test]
    fn visual_snapshots_share_pixels_and_check_coordinates() {
        let snapshot = VisualSnapshot::from_rgba(1, 1, [7, 11, 13, 255]).unwrap();
        let clone = snapshot.clone();
        assert_eq!(snapshot.pixel(0, 0), Some([7, 11, 13, 255]));
        assert_eq!(snapshot.pixel(1, 0), None);
        assert_eq!(snapshot.rgba().as_ptr(), clone.rgba().as_ptr());
    }

    #[test]
    fn png_baselines_round_trip_exact_rgba() {
        let snapshot = VisualSnapshot::from_rgba(2, 1, [7, 11, 13, 255, 17, 19, 23, 127]).unwrap();
        let path = std::env::temp_dir().join(format!(
            "quickgui-visual-snapshot-{}.png",
            std::process::id()
        ));
        snapshot.write_png(&path).unwrap();
        let decoded = VisualSnapshot::open_png(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(decoded.rgba(), snapshot.rgba());
    }
}
