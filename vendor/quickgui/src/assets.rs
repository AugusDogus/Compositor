use std::{
    collections::BTreeMap,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use thiserror::Error;

/// Maximum UTF-8 bytes accepted in one normalized application asset path.
pub const MAX_ASSET_PATH_BYTES: usize = 4 * 1024;
/// Maximum bytes returned for one application asset.
pub const MAX_ASSET_BYTES: usize = 64 * 1024 * 1024;
/// Maximum entries retained by [`BundledAssets`].
pub const MAX_BUNDLED_ASSETS: usize = 4_096;
/// Maximum aggregate payload bytes retained by [`BundledAssets`].
pub const MAX_BUNDLED_ASSET_BYTES: usize = 256 * 1024 * 1024;
/// Maximum entries accepted from one [`AssetSource::list`] result.
pub const MAX_ASSET_LIST_ENTRIES: usize = 4_096;
/// Maximum aggregate path bytes accepted from one [`AssetSource::list`] result.
pub const MAX_ASSET_LIST_PATH_BYTES: usize = 16 * 1024 * 1024;
/// Maximum custom font files registered by one application.
pub const MAX_CUSTOM_FONTS: usize = 64;
/// Maximum bytes retained for one custom font file.
pub const MAX_CUSTOM_FONT_BYTES: usize = 16 * 1024 * 1024;
/// Maximum aggregate custom font bytes retained by one application.
pub const MAX_CUSTOM_FONT_TOTAL_BYTES: usize = 64 * 1024 * 1024;
/// Maximum faces accepted from one custom font collection.
pub const MAX_CUSTOM_FONT_FACES_PER_FILE: usize = 32;
/// Maximum custom font faces registered by one application.
pub const MAX_CUSTOM_FONT_FACES: usize = 256;

static NEXT_ASSET_SOURCE_ID: AtomicU64 = AtomicU64::new(1);

/// A bounded application-asset failure.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum AssetError {
    #[error(
        "asset paths must be nonempty normalized relative paths without NUL, backslash, empty, `.` or `..` segments and at most {MAX_ASSET_PATH_BYTES} UTF-8 bytes"
    )]
    InvalidPath,
    #[error("asset `{path}` contains {bytes} bytes, above the {maximum}-byte limit")]
    AssetTooLarge {
        path: Arc<str>,
        bytes: usize,
        maximum: usize,
    },
    #[error("bundled assets cannot contain more than {MAX_BUNDLED_ASSETS} entries")]
    TooManyBundledAssets,
    #[error(
        "bundled assets would retain {bytes} bytes, above the {MAX_BUNDLED_ASSET_BYTES}-byte limit"
    )]
    BundledAssetsTooLarge { bytes: usize },
    #[error("bundled asset `{0}` was registered more than once")]
    DuplicateAsset(Arc<str>),
    #[error("application asset `{0}` does not exist")]
    NotFound(Arc<str>),
    #[error("application asset source failed: {0}")]
    Source(Arc<str>),
    #[error(
        "an asset listing cannot contain more than {MAX_ASSET_LIST_ENTRIES} entries or {MAX_ASSET_LIST_PATH_BYTES} aggregate path bytes"
    )]
    InvalidList,
    #[error("asset `{path}` could not be decoded: {message}")]
    Decode { path: Arc<str>, message: Arc<str> },
    #[error("an application cannot register more than {MAX_CUSTOM_FONTS} custom font files")]
    TooManyFonts,
    #[error(
        "custom font {index} contains {bytes} bytes, above the {MAX_CUSTOM_FONT_BYTES}-byte limit"
    )]
    FontTooLarge { index: usize, bytes: usize },
    #[error(
        "custom fonts would retain {bytes} bytes, above the {MAX_CUSTOM_FONT_TOTAL_BYTES}-byte limit"
    )]
    FontsTooLarge { bytes: usize },
    #[error(
        "custom font {index} declares {faces} faces, above the {MAX_CUSTOM_FONT_FACES_PER_FILE}-face per-file limit"
    )]
    TooManyFacesInFont { index: usize, faces: usize },
    #[error(
        "custom fonts declare {faces} faces, above the {MAX_CUSTOM_FONT_FACES}-face application limit"
    )]
    TooManyFontFaces { faces: usize },
    #[error("custom font {index} is not a valid supported OpenType font or collection")]
    InvalidFont { index: usize },
}

impl AssetError {
    /// Convert an application asset-source error without exposing a framework-specific error type.
    pub fn source(error: impl fmt::Display) -> Self {
        Self::Source(Arc::from(error.to_string()))
    }

    pub(crate) fn decode(path: Arc<str>, error: impl fmt::Display) -> Self {
        Self::Decode {
            path,
            message: Arc::from(error.to_string()),
        }
    }
}

#[derive(Clone)]
struct StaticBytes(&'static [u8]);

impl AsRef<[u8]> for StaticBytes {
    fn as_ref(&self) -> &[u8] {
        self.0
    }
}

#[derive(Clone)]
struct SharedBytes(Arc<[u8]>);

impl AsRef<[u8]> for SharedBytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// Immutable, cheaply cloned bytes returned by an [`AssetSource`].
///
/// Static data from `include_bytes!` is borrowed without copying. Owned data is retained behind one
/// shared allocation, so image workers and every application window can reuse the same payload.
#[derive(Clone)]
pub struct AssetBytes {
    backing: Arc<dyn AsRef<[u8]> + Send + Sync>,
}

impl AssetBytes {
    pub fn from_static(bytes: &'static [u8]) -> Self {
        Self {
            backing: Arc::new(StaticBytes(bytes)),
        }
    }

    pub fn from_shared(bytes: Arc<[u8]>) -> Self {
        Self {
            backing: Arc::new(SharedBytes(bytes)),
        }
    }

    pub fn len(&self) -> usize {
        self.as_ref().len()
    }

    pub fn is_empty(&self) -> bool {
        self.as_ref().is_empty()
    }

    pub(crate) fn backing(&self) -> Arc<dyn AsRef<[u8]> + Send + Sync> {
        self.backing.clone()
    }
}

impl AsRef<[u8]> for AssetBytes {
    fn as_ref(&self) -> &[u8] {
        self.backing.as_ref().as_ref()
    }
}

impl fmt::Debug for AssetBytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AssetBytes")
            .field("len", &self.len())
            .finish_non_exhaustive()
    }
}

impl From<Vec<u8>> for AssetBytes {
    fn from(bytes: Vec<u8>) -> Self {
        Self {
            backing: Arc::new(bytes),
        }
    }
}

impl From<Arc<[u8]>> for AssetBytes {
    fn from(bytes: Arc<[u8]>) -> Self {
        Self::from_shared(bytes)
    }
}

impl From<&'static [u8]> for AssetBytes {
    fn from(bytes: &'static [u8]) -> Self {
        Self::from_static(bytes)
    }
}

impl<const N: usize> From<&'static [u8; N]> for AssetBytes {
    fn from(bytes: &'static [u8; N]) -> Self {
        Self::from_static(bytes)
    }
}

/// Synchronous source of immutable application assets.
///
/// Sources are normally compiled-in maps. A source which performs filesystem or network I/O should
/// be called from QuickGUI's bounded background executor rather than a latency-sensitive event
/// callback. QuickGUI validates paths before calling the source and bounds every result before it is
/// retained by framework caches.
pub trait AssetSource: Send + Sync + 'static {
    fn load(&self, path: &str) -> Result<Option<AssetBytes>, AssetError>;

    fn list(&self, _prefix: &str) -> Result<Vec<Arc<str>>, AssetError> {
        Ok(Vec::new())
    }
}

impl AssetSource for () {
    fn load(&self, _path: &str) -> Result<Option<AssetBytes>, AssetError> {
        Ok(None)
    }
}

/// Cloneable identity for one application asset source.
#[derive(Clone)]
pub struct Assets {
    id: u64,
    source: Arc<dyn AssetSource>,
}

impl Assets {
    pub fn new(source: impl AssetSource) -> Self {
        Self {
            id: NEXT_ASSET_SOURCE_ID.fetch_add(1, Ordering::Relaxed).max(1),
            source: Arc::new(source),
        }
    }

    /// Load one bounded asset. `Ok(None)` means the normalized path is not present.
    pub fn load(&self, path: &str) -> Result<Option<AssetBytes>, AssetError> {
        validate_asset_path(path, false)?;
        let asset = self.source.load(path)?;
        if let Some(bytes) = &asset {
            validate_asset_length(path, bytes.len(), MAX_ASSET_BYTES)?;
        }
        Ok(asset)
    }

    /// Load one bounded asset, returning [`AssetError::NotFound`] when absent.
    pub fn load_required(&self, path: &str) -> Result<AssetBytes, AssetError> {
        self.load(path)?
            .ok_or_else(|| AssetError::NotFound(Arc::from(path)))
    }

    /// Return a deterministic, bounded listing from the source.
    pub fn list(&self, prefix: &str) -> Result<Vec<Arc<str>>, AssetError> {
        validate_asset_path(prefix, true)?;
        let mut paths = self.source.list(prefix)?;
        let path_bytes = paths.iter().try_fold(0_usize, |total, path| {
            validate_asset_path(path, false)?;
            total.checked_add(path.len()).ok_or(AssetError::InvalidList)
        })?;
        if paths.len() > MAX_ASSET_LIST_ENTRIES || path_bytes > MAX_ASSET_LIST_PATH_BYTES {
            return Err(AssetError::InvalidList);
        }
        paths.sort_unstable();
        paths.dedup();
        Ok(paths)
    }

    /// Build a stable background-decoded image resource from an application asset path.
    pub fn image(&self, path: impl Into<Arc<str>>) -> Result<crate::ImageResource, AssetError> {
        let path = path.into();
        validate_asset_path(&path, false)?;
        Ok(crate::ImageResource::from_asset(self.clone(), path))
    }

    /// Load and synchronously parse one bounded SVG application asset.
    pub fn svg(&self, path: &str) -> Result<crate::Svg, AssetError> {
        let path: Arc<str> = Arc::from(path);
        let bytes = self.load_required(&path)?;
        crate::Svg::from_bytes(bytes.as_ref()).map_err(|error| AssetError::decode(path, error))
    }

    pub(crate) fn id(&self) -> u64 {
        self.id
    }
}

impl Default for Assets {
    fn default() -> Self {
        Self::new(())
    }
}

impl fmt::Debug for Assets {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Assets")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// Bounded in-memory implementation of [`AssetSource`] for `include_bytes!` resources.
#[derive(Clone, Debug, Default)]
pub struct BundledAssets {
    entries: BTreeMap<Arc<str>, AssetBytes>,
    total_bytes: usize,
}

impl BundledAssets {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(
        &mut self,
        path: impl Into<Arc<str>>,
        bytes: impl Into<AssetBytes>,
    ) -> Result<(), AssetError> {
        let path = path.into();
        validate_asset_path(&path, false)?;
        if self.entries.contains_key(&path) {
            return Err(AssetError::DuplicateAsset(path));
        }
        if self.entries.len() == MAX_BUNDLED_ASSETS {
            return Err(AssetError::TooManyBundledAssets);
        }
        let bytes = bytes.into();
        validate_asset_length(&path, bytes.len(), MAX_ASSET_BYTES)?;
        let total_bytes = self
            .total_bytes
            .checked_add(bytes.len())
            .ok_or(AssetError::BundledAssetsTooLarge { bytes: usize::MAX })?;
        if total_bytes > MAX_BUNDLED_ASSET_BYTES {
            return Err(AssetError::BundledAssetsTooLarge { bytes: total_bytes });
        }
        self.entries.insert(path, bytes);
        self.total_bytes = total_bytes;
        Ok(())
    }

    pub fn with(
        mut self,
        path: impl Into<Arc<str>>,
        bytes: impl Into<AssetBytes>,
    ) -> Result<Self, AssetError> {
        self.insert(path, bytes)?;
        Ok(self)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }
}

impl AssetSource for BundledAssets {
    fn load(&self, path: &str) -> Result<Option<AssetBytes>, AssetError> {
        Ok(self.entries.get(path).cloned())
    }

    fn list(&self, prefix: &str) -> Result<Vec<Arc<str>>, AssetError> {
        Ok(self
            .entries
            .keys()
            .filter(|path| asset_has_prefix(path, prefix))
            .cloned()
            .collect())
    }
}

/// One custom font declaration accepted by [`crate::Application::font`].
#[derive(Clone, Debug)]
pub enum FontSource {
    Bytes(AssetBytes),
    Asset(Arc<str>),
}

impl FontSource {
    pub fn asset(path: impl Into<Arc<str>>) -> Self {
        Self::Asset(path.into())
    }
}

impl From<AssetBytes> for FontSource {
    fn from(bytes: AssetBytes) -> Self {
        Self::Bytes(bytes)
    }
}

impl From<Vec<u8>> for FontSource {
    fn from(bytes: Vec<u8>) -> Self {
        Self::Bytes(bytes.into())
    }
}

impl From<Arc<[u8]>> for FontSource {
    fn from(bytes: Arc<[u8]>) -> Self {
        Self::Bytes(bytes.into())
    }
}

impl From<&'static [u8]> for FontSource {
    fn from(bytes: &'static [u8]) -> Self {
        Self::Bytes(bytes.into())
    }
}

impl<const N: usize> From<&'static [u8; N]> for FontSource {
    fn from(bytes: &'static [u8; N]) -> Self {
        Self::Bytes(bytes.into())
    }
}

impl From<String> for FontSource {
    fn from(path: String) -> Self {
        Self::Asset(Arc::from(path))
    }
}

impl From<&str> for FontSource {
    fn from(path: &str) -> Self {
        Self::Asset(Arc::from(path))
    }
}

impl From<Arc<str>> for FontSource {
    fn from(path: Arc<str>) -> Self {
        Self::Asset(path)
    }
}

pub(crate) struct ResolvedFonts {
    pub(crate) sources: Vec<AssetBytes>,
    pub(crate) declared_faces: Vec<usize>,
}

pub(crate) fn resolve_fonts(
    assets: &Assets,
    fonts: &[FontSource],
) -> Result<ResolvedFonts, AssetError> {
    if fonts.len() > MAX_CUSTOM_FONTS {
        return Err(AssetError::TooManyFonts);
    }
    let mut sources = Vec::with_capacity(fonts.len());
    let mut declared_faces = Vec::with_capacity(fonts.len());
    let mut total_bytes = 0_usize;
    let mut total_faces = 0_usize;
    for (index, font) in fonts.iter().enumerate() {
        let source = match font {
            FontSource::Bytes(bytes) => bytes.clone(),
            FontSource::Asset(path) => assets.load_required(path)?,
        };
        if source.len() > MAX_CUSTOM_FONT_BYTES {
            return Err(AssetError::FontTooLarge {
                index,
                bytes: source.len(),
            });
        }
        total_bytes = total_bytes
            .checked_add(source.len())
            .ok_or(AssetError::FontsTooLarge { bytes: usize::MAX })?;
        if total_bytes > MAX_CUSTOM_FONT_TOTAL_BYTES {
            return Err(AssetError::FontsTooLarge { bytes: total_bytes });
        }
        let faces =
            declared_font_faces(source.as_ref()).ok_or(AssetError::InvalidFont { index })?;
        if faces > MAX_CUSTOM_FONT_FACES_PER_FILE {
            return Err(AssetError::TooManyFacesInFont { index, faces });
        }
        total_faces = total_faces
            .checked_add(faces)
            .ok_or(AssetError::TooManyFontFaces { faces: usize::MAX })?;
        if total_faces > MAX_CUSTOM_FONT_FACES {
            return Err(AssetError::TooManyFontFaces { faces: total_faces });
        }
        sources.push(source);
        declared_faces.push(faces);
    }
    Ok(ResolvedFonts {
        sources,
        declared_faces,
    })
}

fn validate_asset_path(path: &str, allow_empty: bool) -> Result<(), AssetError> {
    let valid = (allow_empty || !path.is_empty())
        && path.len() <= MAX_ASSET_PATH_BYTES
        && !path.starts_with('/')
        && !path.ends_with('/')
        && !path.contains(['\0', '\\'])
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..");
    if valid || (allow_empty && path.is_empty()) {
        Ok(())
    } else {
        Err(AssetError::InvalidPath)
    }
}

fn validate_asset_length(path: &str, bytes: usize, maximum: usize) -> Result<(), AssetError> {
    if bytes <= maximum {
        Ok(())
    } else {
        Err(AssetError::AssetTooLarge {
            path: Arc::from(path),
            bytes,
            maximum,
        })
    }
}

fn asset_has_prefix(path: &str, prefix: &str) -> bool {
    prefix.is_empty()
        || path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn declared_font_faces(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 4 {
        return None;
    }
    if &bytes[..4] != b"ttcf" {
        return Some(1);
    }
    let count = u32::from_be_bytes(bytes.get(8..12)?.try_into().ok()?);
    usize::try_from(count).ok().filter(|count| *count > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_assets_are_normalized_shared_and_deterministic() {
        static ICON: &[u8] = b"icon";
        let mut assets = BundledAssets::new();
        assets.insert("icons/z.svg", ICON).unwrap();
        assets.insert("icons/a.svg", b"a").unwrap();
        assets.insert("root.txt", b"root").unwrap();
        assert_eq!(assets.len(), 3);
        assert_eq!(assets.total_bytes(), 9);
        assert_eq!(
            assets
                .load("icons/z.svg")
                .unwrap()
                .unwrap()
                .as_ref()
                .as_ptr(),
            ICON.as_ptr()
        );
        assert_eq!(
            assets.list("icons").unwrap(),
            [Arc::<str>::from("icons/a.svg"), Arc::from("icons/z.svg")]
        );
        assert_eq!(
            assets.insert("icons/z.svg", b"duplicate"),
            Err(AssetError::DuplicateAsset(Arc::from("icons/z.svg")))
        );
    }

    #[test]
    fn asset_handles_reject_ambiguous_paths_and_oversized_results() {
        for path in ["", "/root", "a//b", "a/./b", "a/../b", "a\\b", "a\0b"] {
            assert_eq!(
                validate_asset_path(path, false),
                Err(AssetError::InvalidPath)
            );
        }
        assert!(validate_asset_path("images/猫.png", false).is_ok());
        assert_eq!(
            validate_asset_length("large", MAX_ASSET_BYTES + 1, MAX_ASSET_BYTES),
            Err(AssetError::AssetTooLarge {
                path: Arc::from("large"),
                bytes: MAX_ASSET_BYTES + 1,
                maximum: MAX_ASSET_BYTES,
            })
        );
    }

    #[test]
    fn font_collection_face_counts_are_checked_before_parsing() {
        assert_eq!(declared_font_faces(b""), None);
        assert_eq!(declared_font_faces(b"OTTO"), Some(1));
        let mut collection = b"ttcf\0\x01\0\0\0\0\0\x03".to_vec();
        collection.extend_from_slice(&[0; 12]);
        assert_eq!(declared_font_faces(&collection), Some(3));
    }
}
