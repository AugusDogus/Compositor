use std::{
    fmt,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

use resvg::{
    tiny_skia::{Pixmap, Transform},
    usvg,
};
use thiserror::Error;

use crate::{Rect, Size, Vector};

/// Largest compressed or plain SVG source accepted by [`Svg::from_bytes`] and [`Svg::open`].
pub const MAX_SVG_SOURCE_BYTES: u64 = 4 * 1024 * 1024;
/// Largest width or height rasterized for one SVG cache entry.
pub const MAX_SVG_RASTER_DIMENSION: u32 = 4096;
/// Largest one-channel mask retained for one SVG cache entry.
pub const MAX_SVG_RASTER_PIXELS: u64 = 16 * 1024 * 1024;

const MAX_SVG_TRANSFORM_SCALE: f32 = 64.0;
const MAX_SVG_TRANSLATION: f32 = 32_768.0;

static NEXT_SVG_ID: AtomicU64 = AtomicU64::new(1);
static SYSTEM_FONT_DATABASE: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct SvgId(u64);

/// A parsed, immutable SVG document.
///
/// Parsing is explicit and synchronous, while clones are cheap and retain a stable identity.
/// Rendering uses that identity plus physical output size to cache a one-channel GPU mask, so
/// changing an icon's inherited color does not parse, rasterize, or upload it again.
#[derive(Clone)]
pub struct Svg(Arc<SvgData>);

struct SvgData {
    id: SvgId,
    tree: usvg::Tree,
    size: Size,
    source_bytes: usize,
}

impl Svg {
    /// Parse an SVG or SVGZ document from memory with strict source-size limits.
    ///
    /// System fonts are loaded lazily only when the document may contain a `<text>` element.
    /// External and embedded raster images are deliberately ignored, keeping parsing and
    /// rasterization bounded for UI icons.
    pub fn from_bytes(source: impl AsRef<[u8]>) -> Result<Self, SvgError> {
        let source = source.as_ref();
        validate_source_length(source.len() as u64)?;
        let source_bytes = source.len();
        let decoded = decode_svgz_bounded(source)?;
        let document = decoded.as_deref().unwrap_or(source);

        let mut options = usvg::Options {
            image_href_resolver: usvg::ImageHrefResolver {
                resolve_data: Box::new(|_, _, _| None),
                resolve_string: Box::new(|_, _| None),
            },
            ..usvg::Options::default()
        };
        if might_contain_text(document) {
            options.fontdb = system_font_database().clone();
        }

        let tree = usvg::Tree::from_data(document, &options)?;
        let intrinsic = tree.size();
        let size = Size::new(intrinsic.width(), intrinsic.height());
        if size.is_empty() || !size.width.is_finite() || !size.height.is_finite() {
            return Err(SvgError::InvalidSize);
        }

        Ok(Self(Arc::new(SvgData {
            id: SvgId(NEXT_SVG_ID.fetch_add(1, Ordering::Relaxed)),
            tree,
            size,
            source_bytes,
        })))
    }

    /// Parse an SVG document from UTF-8 text.
    pub fn from_svg(source: impl AsRef<str>) -> Result<Self, SvgError> {
        Self::from_bytes(source.as_ref().as_bytes())
    }

    /// Read and parse an SVG or SVGZ file synchronously.
    ///
    /// Applications should call this during setup or on their own background executor rather
    /// than from latency-sensitive input handling.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, SvgError> {
        let path = path.as_ref();
        let metadata = std::fs::metadata(path).map_err(|source| SvgError::Open {
            path: path.to_path_buf(),
            source,
        })?;
        validate_source_length(metadata.len())?;
        let source = std::fs::read(path).map_err(|source| SvgError::Open {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_bytes(source)
    }

    pub fn width(&self) -> f32 {
        self.0.size.width
    }

    pub fn height(&self) -> f32 {
        self.0.size.height
    }

    pub fn size(&self) -> Size {
        self.0.size
    }

    pub fn source_byte_len(&self) -> usize {
        self.0.source_bytes
    }

    pub(crate) fn id(&self) -> SvgId {
        self.0.id
    }

    pub(crate) fn rasterize_mask(&self, width: u32, height: u32) -> Result<SvgMask, SvgError> {
        validate_raster_size(width, height)?;
        let mut pixmap =
            Pixmap::new(width, height).ok_or(SvgError::RasterAllocation { width, height })?;
        let transform =
            Transform::from_scale(width as f32 / self.width(), height as f32 / self.height());
        resvg::render(&self.0.tree, transform, &mut pixmap.as_mut());
        let alpha = pixmap
            .data()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| pixel[3])
            .collect();
        Ok(SvgMask {
            width,
            height,
            alpha,
        })
    }
}

impl From<&Svg> for Svg {
    fn from(svg: &Svg) -> Self {
        svg.clone()
    }
}

impl fmt::Debug for Svg {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Svg")
            .field("width", &self.width())
            .field("height", &self.height())
            .field("source_bytes", &self.source_byte_len())
            .finish_non_exhaustive()
    }
}

impl PartialEq for Svg {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

impl Eq for Svg {}

pub(crate) struct SvgMask {
    pub width: u32,
    pub height: u32,
    pub alpha: Vec<u8>,
}

/// A render-only transform applied around the center of an SVG element.
///
/// Like CSS transforms, this affects pixels but not flexbox layout or hit testing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SvgTransform {
    scale: [f32; 2],
    translation: Vector,
    rotation: f32,
}

impl SvgTransform {
    pub const IDENTITY: Self = Self {
        scale: [1.0, 1.0],
        translation: Vector::ZERO,
        rotation: 0.0,
    };

    pub const fn new() -> Self {
        Self::IDENTITY
    }

    pub fn scale(mut self, scale: f32) -> Self {
        let scale = sanitize_scale(scale);
        self.scale = [scale, scale];
        self
    }

    pub fn scale_xy(mut self, x: f32, y: f32) -> Self {
        self.scale = [sanitize_scale(x), sanitize_scale(y)];
        self
    }

    pub fn translate(mut self, x: f32, y: f32) -> Self {
        self.translation = Vector::new(sanitize_translation(x), sanitize_translation(y));
        self
    }

    pub fn rotate(mut self, radians: f32) -> Self {
        self.rotation = if radians.is_finite() {
            radians.rem_euclid(std::f32::consts::TAU)
        } else {
            0.0
        };
        self
    }

    pub const fn scale_factors(self) -> [f32; 2] {
        self.scale
    }

    pub const fn translation(self) -> Vector {
        self.translation
    }

    pub const fn rotation(self) -> f32 {
        self.rotation
    }

    pub(crate) fn transformed_bounds(self, rect: Rect) -> Rect {
        let center_x = rect.x + rect.width * 0.5 + self.translation.x;
        let center_y = rect.y + rect.height * 0.5 + self.translation.y;
        let half_width = rect.width * self.scale[0].abs() * 0.5;
        let half_height = rect.height * self.scale[1].abs() * 0.5;
        let (sin, cos) = self.rotation.sin_cos();
        let extent_x = cos.abs() * half_width + sin.abs() * half_height;
        let extent_y = sin.abs() * half_width + cos.abs() * half_height;
        Rect::new(
            center_x - extent_x,
            center_y - extent_y,
            extent_x * 2.0,
            extent_y * 2.0,
        )
    }
}

impl Default for SvgTransform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

fn sanitize_scale(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-MAX_SVG_TRANSFORM_SCALE, MAX_SVG_TRANSFORM_SCALE)
    } else {
        1.0
    }
}

fn sanitize_translation(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-MAX_SVG_TRANSLATION, MAX_SVG_TRANSLATION)
    } else {
        0.0
    }
}

fn validate_source_length(bytes: u64) -> Result<(), SvgError> {
    if bytes > MAX_SVG_SOURCE_BYTES {
        Err(SvgError::TooLarge {
            bytes,
            maximum: MAX_SVG_SOURCE_BYTES,
        })
    } else {
        Ok(())
    }
}

fn decode_svgz_bounded(source: &[u8]) -> Result<Option<Vec<u8>>, SvgError> {
    if !source.starts_with(&[0x1f, 0x8b]) {
        return Ok(None);
    }
    let mut decoded = Vec::new();
    flate2::read::GzDecoder::new(Cursor::new(source))
        .take(MAX_SVG_SOURCE_BYTES + 1)
        .read_to_end(&mut decoded)
        .map_err(SvgError::Decompress)?;
    validate_source_length(decoded.len() as u64)?;
    Ok(Some(decoded))
}

fn validate_raster_size(width: u32, height: u32) -> Result<(), SvgError> {
    let pixels = u64::from(width).saturating_mul(u64::from(height));
    if width == 0
        || height == 0
        || width > MAX_SVG_RASTER_DIMENSION
        || height > MAX_SVG_RASTER_DIMENSION
        || pixels > MAX_SVG_RASTER_PIXELS
    {
        Err(SvgError::RasterTooLarge {
            width,
            height,
            maximum_dimension: MAX_SVG_RASTER_DIMENSION,
            maximum_pixels: MAX_SVG_RASTER_PIXELS,
        })
    } else {
        Ok(())
    }
}

fn might_contain_text(source: &[u8]) -> bool {
    source.windows(5).any(|window| {
        window.eq_ignore_ascii_case(b"<text") || window.eq_ignore_ascii_case(b":text")
    })
}

fn system_font_database() -> &'static Arc<usvg::fontdb::Database> {
    SYSTEM_FONT_DATABASE.get_or_init(|| {
        let mut database = usvg::fontdb::Database::new();
        database.load_system_fonts();
        Arc::new(database)
    })
}

#[derive(Debug, Error)]
pub enum SvgError {
    #[error("SVG source is {bytes} bytes; the maximum is {maximum} bytes")]
    TooLarge { bytes: u64, maximum: u64 },
    #[error("could not open SVG file {path}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not decompress SVGZ data: {0}")]
    Decompress(#[source] std::io::Error),
    #[error("could not parse SVG: {0}")]
    Parse(#[from] usvg::Error),
    #[error("SVG has an empty or non-finite intrinsic size")]
    InvalidSize,
    #[error(
        "SVG raster size {width}x{height} exceeds {maximum_dimension}px per axis or {maximum_pixels} pixels"
    )]
    RasterTooLarge {
        width: u32,
        height: u32,
        maximum_dimension: u32,
        maximum_pixels: u64,
    },
    #[error("could not allocate SVG raster surface {width}x{height}")]
    RasterAllocation { width: u32, height: u32 },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const RECT: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="white"/></svg>"#;

    #[test]
    fn parses_intrinsic_size_and_clones_identity() {
        let svg = Svg::from_svg(RECT).unwrap();
        assert_eq!(svg.size(), Size::new(20.0, 10.0));
        assert_eq!(svg, svg.clone());
        assert_ne!(svg, Svg::from_svg(RECT).unwrap());
    }

    #[test]
    fn rasterizes_a_single_channel_alpha_mask() {
        let svg = Svg::from_svg(RECT).unwrap();
        let mask = svg.rasterize_mask(40, 20).unwrap();
        assert_eq!((mask.width, mask.height), (40, 20));
        assert_eq!(mask.alpha.len(), 800);
        assert_eq!(mask.alpha[10 * 40 + 20], 255);
    }

    #[test]
    fn source_and_raster_limits_are_enforced() {
        assert!(matches!(
            Svg::from_bytes(vec![b' '; MAX_SVG_SOURCE_BYTES as usize + 1]),
            Err(SvgError::TooLarge { .. })
        ));
        let svg = Svg::from_svg(RECT).unwrap();
        assert!(matches!(
            svg.rasterize_mask(MAX_SVG_RASTER_DIMENSION + 1, 1),
            Err(SvgError::RasterTooLarge { .. })
        ));
    }

    #[test]
    fn svgz_is_supported_without_unbounded_expansion() {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(RECT.as_bytes()).unwrap();
        let encoded = encoder.finish().unwrap();
        assert_eq!(
            Svg::from_bytes(encoded).unwrap().size(),
            Size::new(20.0, 10.0)
        );

        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let expanded = vec![b' '; MAX_SVG_SOURCE_BYTES as usize + 1];
        encoder.write_all(&expanded).unwrap();
        let encoded = encoder.finish().unwrap();
        assert!(matches!(
            Svg::from_bytes(encoded),
            Err(SvgError::TooLarge { .. })
        ));
    }

    #[test]
    fn transform_bounds_include_rotation_scale_and_translation() {
        let bounds = SvgTransform::new()
            .scale_xy(2.0, 1.0)
            .rotate(std::f32::consts::FRAC_PI_2)
            .translate(5.0, -3.0)
            .transformed_bounds(Rect::new(10.0, 20.0, 40.0, 20.0));
        assert!((bounds.x - 25.0).abs() < 0.001);
        assert!((bounds.y + 13.0).abs() < 0.001);
        assert!((bounds.width - 20.0).abs() < 0.001);
        assert!((bounds.height - 80.0).abs() < 0.001);
    }

    #[test]
    fn non_finite_transform_values_are_sanitized() {
        let transform = SvgTransform::new()
            .scale_xy(f32::NAN, f32::INFINITY)
            .translate(f32::NEG_INFINITY, f32::NAN)
            .rotate(f32::NAN);
        assert_eq!(transform, SvgTransform::IDENTITY);
    }
}
