//! Linux editing sources accompany a native compatibility document. Bindings detect
//! stale sidecars and interrupted/manual edits, not a malicious package author.
use super::*;
use crate::adjustment::ExtendedAdjustment;
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub(super) const NAME: &str = "linux-editing.json";
const SOURCE: &str = "authoring";
const SETTINGS: &str = "adjustments.json";
const ASSET_LIMIT: u64 = 512 * 1024 * 1024;
const ENTRY_LIMIT: usize = 32_768;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Index {
    version: u32,
    purpose: Purpose,
    files: Vec<BoundFile>,
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) enum Purpose {
    Project,
    Recovery,
}
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundFile {
    name: String,
    length: u64,
    sha256: [u8; 32],
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    version: u32,
    layers: Vec<ExtendedLayer>,
    #[serde(default)]
    paths: Vec<crate::vector_path::SavedPath>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtendedLayer {
    layer: Uuid,
    adjustment: ExtendedAdjustment,
}

pub enum OpenResult {
    Editable {
        document: Document,
        fingerprint: Fingerprint,
    },
    RenderedCopyAvailable {
        reason: String,
        fingerprint: Fingerprint,
    },
}

pub(super) fn needed(document: &Document) -> bool {
    !document.paths.is_empty() || rendered_projection_needed(document)
}
fn rendered_projection_needed(document: &Document) -> bool {
    document
        .layers
        .iter()
        .any(|layer| matches!(layer.content, LayerContent::ExtendedAdjustment(_)))
}
pub(super) fn present(path: &Path) -> Result<bool> {
    for name in [NAME, SOURCE] {
        match fs::symlink_metadata(path.join(name)) {
            Ok(_) => return Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(false)
}

fn projection_size(document: &Document) -> (u32, u32) {
    let pixels = u64::from(document.width) * u64::from(document.height);
    if pixels <= crate::document::MAX_SURFACE_PIXELS {
        return (document.width, document.height);
    }
    let scale = (crate::document::MAX_SURFACE_PIXELS as f64 / pixels as f64).sqrt();
    (
        (f64::from(document.width) * scale).floor().max(1.) as u32,
        (f64::from(document.height) * scale).floor().max(1.) as u32,
    )
}
pub fn compatibility_notice(document: &Document) -> Option<String> {
    if !needed(document) {
        return None;
    }
    if !rendered_projection_needed(document) {
        return Some("Saved paths remain editable in Linux Compositor. Other Compositor versions open the native editable layers without these paths; saving there discards the saved paths.".into());
    }
    let size = projection_size(document);
    let resolution = if size == (document.width, document.height) {
        "full resolution".into()
    } else {
        format!("{} × {} pixels", size.0, size.1)
    };
    Some(format!(
        "Linux editing sources are preserved at full resolution. Other Compositor versions open a rendered view at {resolution}; saving there discards Linux editing sources."
    ))
}

pub(super) fn write(document: &Document, path: &Path, purpose: Purpose) -> Result<()> {
    let source_path = path.join(SOURCE);
    fs::create_dir(&source_path)?;
    let mut source = document.clone();
    let paths = std::mem::take(&mut source.paths);
    let mut layers = Vec::new();
    for layer in &mut source.layers {
        if let LayerContent::ExtendedAdjustment(adjustment) = &layer.content {
            layers.push(ExtendedLayer {
                layer: layer.id,
                adjustment: **adjustment,
            });
            layer.content = LayerContent::Raster(None);
        }
    }
    write_native(&source, &source_path)?;
    write_json(
        &source_path.join(SETTINGS),
        &Settings {
            version: 2,
            layers,
            paths,
        },
    )?;
    if purpose == Purpose::Recovery {
        // Deliberately not a native manifest. A damaged recovery snapshot must
        // never be offered as an apparently successful blank rendered copy.
        write_json(
            &path.join("manifest.json"),
            &serde_json::json!({
                "format": "com.compositor.linux-recovery", "version": 1
            }),
        )?;
    } else if rendered_projection_needed(document) {
        let (width, height) = projection_size(document);
        let pixels = crate::render::render(document, width, height)?;
        let mut projection = Document::new(document.width, document.height)?;
        projection.id = document.id;
        projection.resolution = document.resolution;
        projection.guides = document.guides.clone();
        projection.layers[0].name = "Rendered Linux project".into();
        projection.layers[0].content = LayerContent::Raster(Some(Arc::new(pixels)));
        write_native(&projection, path)?;
    } else {
        write_native(&source, path)?;
    }
    let files = bound_files(path)?;
    write_json(
        &path.join(NAME),
        &Index {
            version: 1,
            purpose,
            files,
        },
    )?;
    File::open(source_path)?.sync_all()?;
    File::open(path)?.sync_all()?;
    Ok(())
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() as u64 > MANIFEST_LIMIT {
        return Err(invalid(
            "Linux editing metadata exceeds 4 MiB. The previous save is preserved.",
        ));
    }
    let mut file = File::create(path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}
fn read_json<T: serde::de::DeserializeOwned>(root: &Path, path: &Path) -> Result<T> {
    let mut bytes = Vec::new();
    checked_file(root, path, MANIFEST_LIMIT)?
        .take(MANIFEST_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MANIFEST_LIMIT {
        return Err(invalid("Linux editing metadata exceeds 4 MiB."));
    }
    Ok(serde_json::from_slice(&bytes)?)
}

pub(super) fn load(path: &Path) -> Result<Document> {
    let root = path.canonicalize()?;
    let index: Index = read_json(&root, &path.join(NAME))?;
    if index.version != 1 {
        return Err(invalid(
            "This project's Linux editing version is unsupported. Update Compositor, or open its rendered copy. The original files are preserved.",
        ));
    }
    if index.files.len() > ENTRY_LIMIT || index.files != bound_files(path)? {
        return Err(invalid(
            "The Linux editing sources no longer match this project's rendered view. No files were changed. Restore a matching backup, or open the rendered copy as a separate unsaved document.",
        ));
    }
    let source_path = path.join(SOURCE);
    let settings: Settings = read_json(&root, &source_path.join(SETTINGS))?;
    if !matches!(settings.version, 1 | 2)
        || (settings.version == 1 && !settings.paths.is_empty())
        || (settings.layers.is_empty() && settings.paths.is_empty())
        || settings.layers.len() > 10_000
    {
        return Err(invalid(
            "The Linux editing snapshot has an unsupported version or invalid source count.",
        ));
    }
    crate::vector_path::validate(&settings.paths)?;
    let mut seen = HashSet::new();
    for setting in &settings.layers {
        setting.adjustment.validate()?;
        if !seen.insert(setting.layer) {
            return Err(invalid("The Linux adjustment snapshot repeats a layer."));
        }
    }
    // No compatibility image is decoded: source pixels alone consume the
    // document allocation budget.
    let mut document = load_native(&source_path)?;
    document.paths = settings.paths;
    for setting in settings.layers {
        let layer = document
            .layers
            .iter_mut()
            .find(|layer| layer.id == setting.layer)
            .ok_or_else(|| invalid("A Linux adjustment refers to a missing source layer."))?;
        if !matches!(layer.content, LayerContent::Raster(None)) {
            return Err(invalid(
                "A Linux adjustment must reference an empty source slot.",
            ));
        }
        layer.content = LayerContent::ExtendedAdjustment(Box::new(setting.adjustment));
    }
    document.validate()?;
    Ok(document)
}

fn regular_directory(path: &Path) -> Result<()> {
    if !fs::symlink_metadata(path)?.is_dir() {
        return Err(invalid(
            "A Linux editing asset directory is not a regular directory.",
        ));
    }
    Ok(())
}

/// Fixed namespaces avoid arbitrary paths in the binding index. Every stored
/// file in these namespaces is included, so omitted or extra assets invalidate it.
fn file_names(path: &Path) -> Result<Vec<String>> {
    regular_directory(&path.join(SOURCE))?;
    let mut names = Vec::new();
    for prefix in ["", "authoring/"] {
        for name in ["manifest.json", "linux-raw.json", "linux-editors.json"] {
            let relative = format!("{prefix}{name}");
            match fs::symlink_metadata(path.join(&relative)) {
                Ok(_) => names.push(relative),
                Err(error)
                    if error.kind() == std::io::ErrorKind::NotFound && name != "manifest.json" => {}
                Err(error) => return Err(error.into()),
            }
        }
        for folder in ["images", "raw"] {
            let relative = format!("{prefix}{folder}");
            let directory = path.join(&relative);
            match fs::symlink_metadata(&directory) {
                Ok(_) => regular_directory(&directory)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            }
            for entry in fs::read_dir(&directory)? {
                let entry = entry?;
                let filename = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| invalid("A Linux editing asset has a non-UTF-8 filename."))?;
                names.push(format!("{relative}/{filename}"));
                if names.len() > ENTRY_LIMIT {
                    return Err(invalid(
                        "The project exceeds the Linux editing asset count limit.",
                    ));
                }
            }
        }
    }
    names.push(format!("{SOURCE}/{SETTINGS}"));
    names.sort();
    Ok(names)
}
fn bound_files(path: &Path) -> Result<Vec<BoundFile>> {
    let root = path.canonicalize()?;
    let mut files = Vec::new();
    let mut total = 0_u64;
    // Includes both encoded source pixels and the compatibility view, plus
    // embedded RAW data. A forged package cannot force unbounded hashing.
    let byte_budget =
        (crate::document::document_pixel_budget() + crate::document::MAX_SURFACE_PIXELS) * 8
            + crate::raw::MAX_RAW_BYTES * 2
            + MANIFEST_LIMIT * 8;
    for name in file_names(path)? {
        let limit = if name.ends_with(".json") {
            MANIFEST_LIMIT
        } else {
            ASSET_LIMIT
        };
        let mut file = checked_file(&root, &path.join(&name), limit)?;
        let mut hash = Sha256::new();
        hash.update(b"compositor-linux-editing-v1\0");
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        let length = file.metadata()?.len();
        total = total
            .checked_add(length)
            .filter(|total| *total <= byte_budget)
            .ok_or_else(|| {
                invalid("The Linux editing package exceeds the total encoded asset size limit.")
            })?;
        hash.update(length.to_le_bytes());
        let mut read = 0_u64;
        let mut buffer = [0; 64 * 1024];
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            read += count as u64;
            if read > limit {
                return Err(invalid(
                    "A Linux editing asset grew beyond its size limit while loading.",
                ));
            }
            hash.update(&buffer[..count]);
        }
        if read != length {
            return Err(invalid(
                "A Linux editing asset changed while reading. Retry after the other writer finishes.",
            ));
        }
        files.push(BoundFile {
            name,
            length,
            sha256: hash.finalize().into(),
        });
    }
    Ok(files)
}

pub fn inspect_open(path: &Path) -> Result<OpenResult> {
    let before = fingerprint(path)?
        .ok_or_else(|| invalid("The project was removed before it could be opened."))?;
    let result = match super::load(path) {
        Ok(document) => OpenResult::Editable {
            document,
            fingerprint: before.clone(),
        },
        Err(error) if present(path)? && native_projection(path)? => {
            OpenResult::RenderedCopyAvailable {
                reason: error.to_string(),
                fingerprint: before.clone(),
            }
        }
        Err(error) => return Err(error),
    };
    if fingerprint(path)?.as_ref() != Some(&before) {
        return Err(invalid(
            "The project changed while opening. Retry after the other writer finishes. Your edits are preserved.",
        ));
    }
    Ok(result)
}
fn native_projection(path: &Path) -> Result<bool> {
    let root = path.canonicalize()?;
    let value: serde_json::Value = read_json(&root, &path.join("manifest.json"))?;
    Ok(value.get("format").and_then(serde_json::Value::as_str) == Some("com.compositor.project"))
}
pub fn load_rendered_copy(path: &Path, expected: &Fingerprint) -> Result<Document> {
    if fingerprint(path)?.as_ref() != Some(expected) {
        return Err(invalid(
            "The project changed after the warning. Open it again to inspect the current version.",
        ));
    }
    let document = load_native(path)?;
    if fingerprint(path)?.as_ref() != Some(expected) {
        return Err(invalid(
            "The rendered project changed while loading. Open it again after the other writer finishes.",
        ));
    }
    Ok(document)
}

#[cfg(test)]
mod tests;
