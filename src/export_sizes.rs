//! Render a document into several frames, publishing the batch as one new directory.
use crate::{Result, document::Document, image_io, invalid, render};
use image::RgbaImage;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
mod artboards;
pub use artboards::Source;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// Keep the entire canvas; center it on transparent padding.
    Contain,
    /// Fill the target frame; crop the canvas centrally.
    Cover,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg { quality: u8 },
}
impl Format {
    fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg { .. } => "jpg",
        }
    }
    fn quality(self) -> u8 {
        match self {
            Self::Png => 100,
            Self::Jpeg { quality } => quality,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Batch {
    sizes: Vec<[u32; 2]>,
    fit: Fit,
    format: Format,
}
impl Batch {
    pub fn new(sizes: Vec<[u32; 2]>, fit: Fit, format: Format) -> Result<Self> {
        if sizes.is_empty() || sizes.len() > 16 {
            return Err(invalid("Choose between 1 and 16 export sizes."));
        }
        let mut seen = HashSet::new();
        let mut total = 0_u64;
        for &[width, height] in &sizes {
            crate::document::validate_size(width, height)?;
            if !seen.insert([width, height]) {
                return Err(invalid(format!(
                    "Export size {width} × {height} is repeated. Use each size once."
                )));
            }
            total += u64::from(width) * u64::from(height);
        }
        if total > 800_000_000 || format.quality() > 100 {
            return Err(invalid(
                "An export batch can contain at most 800 million pixels; JPEG quality must be 0 to 100.",
            ));
        }
        Ok(Self { sizes, fit, format })
    }

    /// Example: `1920x1080, 1080x1080`. Dimensions are pixels.
    pub fn parse(sizes: &str, fit: Fit, format: Format) -> Result<Self> {
        if sizes.len() > 1024 {
            return Err(invalid(
                "The export size list is too long. Choose at most 16 sizes.",
            ));
        }
        let sizes = sizes.split(',').map(|size| {
            let (width, height) = size.trim().split_once(['x', 'X', '×'])
                .ok_or_else(|| invalid("Enter export sizes as width × height, separated by commas, such as 1920x1080, 1080x1080."))?;
            let parse = |value: &str| value.trim().parse::<u32>()
                .map_err(|_| invalid("Export dimensions must be positive whole numbers, such as 1920x1080."));
            Ok([parse(width)?, parse(height)?])
        }).collect::<Result<Vec<_>>>()?;
        Self::new(sizes, fit, format)
    }

    /// All files are staged first. Existing folders, files and symlinks are never replaced.
    pub fn export(&self, document: &Document, parent: &Path, title: &str) -> Result<PathBuf> {
        document.validate()?;
        let stem = crate::export_batch::safe_stem(title);
        crate::export_batch::publish(parent, &format!("{stem}-exports"), |directory| {
            let mut cache = render::DownsampleCache::default();
            for &[width, height] in &self.sizes {
                let pixels = render_frame(document, [width, height], self.fit, &mut cache)?;
                let path = directory.join(format!(
                    "{stem}-{width}x{height}.{}",
                    self.format.extension()
                ));
                image_io::export_pixels(pixels, document.resolution, &path, self.format.quality())?;
            }
            Ok(())
        })
    }
}

fn render_frame(
    document: &Document,
    size: [u32; 2],
    fit: Fit,
    cache: &mut render::DownsampleCache,
) -> Result<RgbaImage> {
    let [width, height] = size;
    let ratios = [
        f64::from(width) / f64::from(document.width),
        f64::from(height) / f64::from(document.height),
    ];
    match fit {
        Fit::Cover => {
            let scale = ratios[0].max(ratios[1]);
            let origin = [
                (f64::from(document.width) - f64::from(width) / scale) / 2.,
                (f64::from(document.height) - f64::from(height) / scale) / 2.,
            ];
            render::region_accelerated(document, width, height, origin, [1. / scale; 2], cache)
        }
        Fit::Contain => {
            let scale = ratios[0].min(ratios[1]);
            let fitted = [
                (f64::from(document.width) * scale)
                    .round()
                    .clamp(1., f64::from(width)) as u32,
                (f64::from(document.height) * scale)
                    .round()
                    .clamp(1., f64::from(height)) as u32,
            ];
            let image = render::region_accelerated(
                document,
                fitted[0],
                fitted[1],
                [0.; 2],
                [
                    f64::from(document.width) / f64::from(fitted[0]),
                    f64::from(document.height) / f64::from(fitted[1]),
                ],
                cache,
            )?;
            if fitted == size {
                return Ok(image);
            }
            let mut padded = RgbaImage::new(width, height);
            image::imageops::replace(
                &mut padded,
                &image,
                i64::from((width - fitted[0]) / 2),
                i64::from((height - fitted[1]) / 2),
            );
            Ok(padded)
        }
    }
}

#[cfg(test)]
mod tests;
