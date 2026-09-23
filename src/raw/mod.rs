// Adapted from Xuan, copyright (c) 2026 Wonder Assembly LLC and Silver Ling.
// Distributed under the MIT license; see licenses/Xuan-MIT.txt.
//! Nondestructive camera RAW assets and a floating-point Develop pipeline.
use crate::render::gpu::raw as gpu;
mod process;
mod settings;
#[cfg(test)]
mod tests;

use std::{fs::File, io::Read, path::Path, sync::Arc};

use crate::{Result, invalid};
use image::{DynamicImage, ImageBuffer, Rgb32FImage, RgbaImage};
use rawler::{
    decoders::RawDecodeParams,
    imgop::{
        develop::{Intermediate, ProcessingStep, RawDevelop},
        matrix::{multiply, normalize, pseudo_inverse},
        xyz::{Illuminant, SRGB_TO_XYZ_D65},
    },
    rawimage::RawPhotometricInterpretation,
    rawsource::RawSource,
};
use serde::{Deserialize, Serialize};

use crate::document::validate_size;
pub use process::{auto_exposure, render, render_16, sample_white_balance, source_point};
pub use settings::{DevelopSettings, Overlay, OverlayKind, WhiteBalance};

/// Normalized coordinates in the oriented, uncropped camera image.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}
impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

pub const MAX_RAW_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawMetadata {
    pub camera: String,
    pub lens: String,
    pub iso: Option<u32>,
    pub aperture: Option<f32>,
    pub shutter: Option<f32>,
    pub focal_length: Option<f32>,
    pub width: u32,
    pub height: u32,
    /// Zero denotes an unknown bit depth.
    pub bits: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawAsset {
    pub filename: String,
    pub metadata: RawMetadata,
    pub settings: DevelopSettings,
    /// Stored separately in the project archive. Never overwrite the camera file.
    #[serde(skip)]
    pub bytes: Arc<Vec<u8>>,
}

impl RawAsset {
    pub fn validate(&self) -> Result<()> {
        ensure(
            !self.filename.is_empty() && self.filename.len() <= 16_384,
            "Invalid RAW filename",
        )?;
        ensure(
            !self.bytes.is_empty() && self.bytes.len() as u64 <= MAX_RAW_BYTES,
            "Missing or oversized RAW source",
        )?;
        validate_size(self.metadata.width, self.metadata.height)?;
        ensure(
            self.metadata.camera.len() <= 4096 && self.metadata.lens.len() <= 4096,
            "RAW camera or lens metadata is too long",
        )?;
        ensure(
            self.metadata.bits <= 32,
            "RAW bit depth must be between 0 and 32",
        )?;
        ensure(
            self.metadata
                .aperture
                .into_iter()
                .chain(self.metadata.shutter)
                .chain(self.metadata.focal_length)
                .all(|v| v.is_finite() && v > 0.),
            "RAW exposure metadata contains invalid values",
        )?;
        self.settings.validate()
    }
}

/// Demosaiced, oriented, black-level-normalized camera RGB. Values are not clipped
/// at 1.0 and have not had white balance, exposure, color conversion or gamma applied.
#[derive(Debug)]
pub struct DecodedRaw {
    pub camera: Rgb32FImage,
    pub as_shot: [f32; 3],
    pub camera_to_rgb: [[f32; 3]; 3],
    pub xyz_to_camera: [[f32; 3]; 3],
    pub metadata: RawMetadata,
}

impl DecodedRaw {
    pub fn validate(&self) -> Result<()> {
        validate_size(self.camera.width(), self.camera.height())?;
        validate_size(self.metadata.width, self.metadata.height)?;
        ensure(
            self.as_shot.iter().all(|v| v.is_finite() && *v > 0.)
                && self
                    .camera_to_rgb
                    .iter()
                    .flatten()
                    .chain(self.xyz_to_camera.iter().flatten())
                    .all(|v| v.is_finite()),
            "Invalid camera color calibration or white balance",
        )?;
        ensure(
            self.camera.as_raw().iter().all(|v| v.is_finite()),
            "RAW camera pixels contain invalid values",
        )
    }

    pub fn preview(&self, max_side: u32) -> Self {
        let scale =
            (max_side as f32 / self.camera.width().max(self.camera.height()) as f32).min(1.0);
        Self {
            camera: {
                let width = (self.camera.width() as f32 * scale).round().max(1.) as u32;
                let height = (self.camera.height() as f32 * scale).round().max(1.) as u32;
                // Generic image resizing clamps float samples to 1.0. Preserve
                // camera highlights above unity until exposure/tone development.
                ImageBuffer::from_fn(width, height, |x, y| {
                    image::Rgb(process::sample(
                        &self.camera,
                        (x as f32 + 0.5) * self.camera.width() as f32 / width as f32 - 0.5,
                        (y as f32 + 0.5) * self.camera.height() as f32 / height as f32 - 0.5,
                    ))
                })
            },
            as_shot: self.as_shot,
            camera_to_rgb: self.camera_to_rgb,
            xyz_to_camera: self.xyz_to_camera,
            metadata: self.metadata.clone(),
        }
    }
}

/// File extensions recognized by the native decoder. Individual camera encodings
/// and sensor layouts are checked during decoding.
pub fn extensions() -> &'static [&'static str] {
    rawler::decoders::supported_extensions()
}

pub fn is_raw(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| extensions().iter().any(|ext| s.eq_ignore_ascii_case(ext)))
}

pub fn open(path: &Path) -> Result<(RawAsset, DecodedRaw)> {
    let metadata = path.metadata()?;
    ensure(
        metadata.is_file() && metadata.len() <= MAX_RAW_BYTES,
        "RAW image must be a regular file no larger than 512 MiB.",
    )?;
    let file = File::open(path)?;
    ensure(
        file.metadata()?.len() <= MAX_RAW_BYTES,
        "RAW file exceeds 512 MiB",
    )?;
    let mut bytes = Vec::new();
    file.take(MAX_RAW_BYTES + 1).read_to_end(&mut bytes)?;
    let decoded = decode(&bytes)?;
    let asset = RawAsset {
        filename: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        metadata: decoded.metadata.clone(),
        settings: DevelopSettings::default(),
        bytes: Arc::new(bytes),
    };
    asset.validate()?;
    Ok((asset, decoded))
}

pub fn decode(bytes: &[u8]) -> Result<DecodedRaw> {
    ensure(
        !bytes.is_empty() && bytes.len() as u64 <= MAX_RAW_BYTES,
        "Empty or oversized RAW file",
    )?;
    // The external decoder has panic paths for unsupported encodings. Convert these
    // into import errors so a failed camera file cannot unwind through the editor.
    std::panic::catch_unwind(|| decode_inner(bytes))
        .map_err(|_| invalid("The RAW decoder could not process this camera file. The current document is preserved; try another camera file."))?
}

fn decode_inner(bytes: &[u8]) -> Result<DecodedRaw> {
    let source = RawSource::new_from_slice(bytes);
    let header = rawler::decode_dummy(&source).map_err(decode_error)?;
    validate_size(dimension(header.width)?, dimension(header.height)?)?;
    ensure(
        matches!(&header.photometric, RawPhotometricInterpretation::Cfa(c)
        if c.cfa.is_rgb() && c.cfa.width == 2 && c.cfa.height == 2)
            || (header.photometric == RawPhotometricInterpretation::LinearRaw && header.cpp == 3),
        "This RAW sensor layout is not supported; an RGB Bayer or linear RGB camera file is required. Export a TIFF from your camera software to import this image.",
    )?;
    let decoder = rawler::get_decoder(&source).map_err(decode_error)?;
    let params = RawDecodeParams::default();
    let metadata = decoder
        .raw_metadata(&source, &params)
        .map_err(decode_error)?;
    let raw = decoder
        .raw_image(&source, &params, false)
        .map_err(decode_error)?;
    validate_size(dimension(raw.width)?, dimension(raw.height)?)?;
    let matrix = raw
        .color_matrix
        .get(&Illuminant::D65)
        .or_else(|| raw.color_matrix.get(&Illuminant::A))
        .or_else(|| {
            raw.color_matrix
                .iter()
                .min_by_key(|(key, _)| **key as u16)
                .map(|(_, value)| value)
        })
        .ok_or_else(|| invalid("No camera color calibration is available for this RAW file."))?;
    ensure(matrix.len() == 9, "Unsupported camera color matrix")?;
    let xyz_to_camera = std::array::from_fn(|i| std::array::from_fn(|j| matrix[i * 3 + j]));
    let camera_to_rgb = pseudo_inverse(normalize(multiply(&xyz_to_camera, &SRGB_TO_XYZ_D65)));
    ensure(
        camera_to_rgb.iter().flatten().all(|v| v.is_finite()),
        "Invalid camera color calibration",
    )?;
    let as_shot = std::array::from_fn(|i| raw.wb_coeffs[i] / raw.wb_coeffs[1]);
    ensure(
        as_shot.iter().all(|v| v.is_finite() && *v > 0.0),
        "Invalid camera white balance",
    )?;
    let developer = RawDevelop {
        steps: vec![
            ProcessingStep::Rescale,
            ProcessingStep::Demosaic,
            ProcessingStep::CropActiveArea,
            ProcessingStep::CropDefault,
        ],
    };
    let Intermediate::ThreeColor(pixels) =
        developer.develop_intermediate(&raw).map_err(decode_error)?
    else {
        return Err(invalid("RAW decoder did not produce an RGB image."));
    };
    let camera = ImageBuffer::from_raw(
        pixels.width as u32,
        pixels.height as u32,
        pixels.data.into_iter().flatten().collect(),
    )
    .ok_or_else(|| invalid("Invalid decoded RAW dimensions."))?;
    let mut oriented = DynamicImage::ImageRgb32F(camera);
    oriented.apply_orientation(
        image::metadata::Orientation::from_exif(
            metadata
                .exif
                .orientation
                .unwrap_or(raw.orientation.to_u16()) as u8,
        )
        .unwrap_or(image::metadata::Orientation::NoTransforms),
    );
    let camera = oriented.into_rgb32f();
    let exif = metadata.exif;
    let metadata = RawMetadata {
        camera: format!("{} {}", raw.clean_make, raw.clean_model),
        lens: exif.lens_model.unwrap_or_default(),
        iso: exif.iso_speed.or(exif.iso_speed_ratings.map(u32::from)),
        aperture: exif
            .fnumber
            .map(|v| v.as_f32())
            .filter(|v| v.is_finite() && *v > 0.),
        shutter: exif
            .exposure_time
            .map(|v| v.as_f32())
            .filter(|v| v.is_finite() && *v > 0.),
        focal_length: exif
            .focal_length
            .map(|v| v.as_f32())
            .filter(|v| v.is_finite() && *v > 0.),
        width: camera.width(),
        height: camera.height(),
        bits: raw.bps,
    };
    Ok(DecodedRaw {
        camera,
        as_shot,
        camera_to_rgb,
        xyz_to_camera,
        metadata,
    })
}

/// Preserve layer placement, masks, blending, and identity when redeveloping.
pub fn update_layer(
    layer: &mut crate::document::Layer,
    asset: RawAsset,
    pixels: RgbaImage,
) -> Result<()> {
    ensure(
        layer.raw.is_some() && matches!(layer.content, crate::document::LayerContent::Raster(_)),
        "Select a RAW image layer",
    )?;
    asset.validate()?;
    validate_size(pixels.width(), pixels.height())?;
    // A new crop changes source bounds. Map it through the old placement so
    // uncropped content stays at its original document position and scale.
    let old_asset = layer
        .raw
        .as_ref()
        .ok_or_else(|| invalid("Select a RAW layer."))?;
    old_asset.validate()?;
    let old = gpu::crop(
        &old_asset.settings,
        [old_asset.metadata.width, old_asset.metadata.height],
    );
    let new = gpu::crop(
        &asset.settings,
        [asset.metadata.width, asset.metadata.height],
    );
    if old != new {
        let left = (f64::from(new[0]) - f64::from(old[0])) / f64::from(old[2] - old[0]);
        let top = (f64::from(new[1]) - f64::from(old[1])) / f64::from(old[3] - old[1]);
        let right = (f64::from(new[2]) - f64::from(old[0])) / f64::from(old[2] - old[0]);
        let bottom = (f64::from(new[3]) - f64::from(old[1])) / f64::from(old[3] - old[1]);
        let center = layer
            .transform
            .point([(left + right) / 2., (top + bottom) / 2.]);
        let size = [
            layer.transform.size[0] * (right - left),
            layer.transform.size[1] * (bottom - top),
        ];
        let transform = crate::geometry::Transform {
            size,
            origin: [center[0] - size[0] / 2., center[1] - size[1] / 2.],
            ..layer.transform
        };
        ensure(
            transform.valid(),
            "RAW crop would create an invalid layer size. Increase the crop area or layer scale.",
        )?;
        // Existing masks keep their document-space alignment through a RAW crop.
        if let Some(mask) = &mut layer.mask {
            mask.placement = Some(mask.placement.unwrap_or(layer.transform));
        }
        layer.transform = transform;
    }
    layer.raw = Some(Arc::new(asset));
    layer.content = crate::document::LayerContent::Raster(Some(Arc::new(pixels)));
    Ok(())
}

pub(crate) fn ensure(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(invalid(message))
    }
}
fn dimension(value: usize) -> Result<u32> {
    value
        .try_into()
        .map_err(|_| invalid("RAW image dimensions exceed supported limits."))
}
fn decode_error(error: impl std::fmt::Display) -> crate::Error {
    invalid(format!(
        "Cannot decode this RAW camera file: {error}. The current document is preserved."
    ))
}
