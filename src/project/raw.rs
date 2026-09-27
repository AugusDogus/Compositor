//! Linux RAW metadata is separate from the upstream manifest, whose PNG assets
//! remain usable by readers that do not understand editable camera sources.
use super::*;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

#[cfg(test)]
mod tests;

const NAME: &str = "linux-raw.json";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Sources {
    version: u32,
    layers: Vec<Record>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    layer: Uuid,
    source: Uuid,
    asset: crate::raw::RawAsset,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    binding: Option<[u8; 32]>,
}

/// Bind authoring data to the cached raster it can replace. Placement, masks and
/// layer effects deliberately stay outside this digest: they do not change the
/// developed source. This detects stale/corrupt sidecars, not malicious authors.
/// RawAsset's serialized schema is part of v2's digest format. Changes to its
/// serialized fields/defaults require a versioned migration before loading v2.
fn binding(
    document: Uuid,
    layer: &Layer,
    asset: &crate::raw::RawAsset,
    source_digest: &[u8; 32],
) -> Result<[u8; 32]> {
    let pixels = layer
        .raster()
        .ok_or_else(|| invalid("The RAW layer has no cached pixels."))?;
    let mut hash = Sha256::new();
    hash.update(b"compositor-linux-raw-v2\0");
    hash.update(document.as_bytes());
    hash.update(layer.id.as_bytes());
    hash.update(source_digest);
    hash.update(serde_json::to_vec(asset)?);
    hash.update(pixels.width().to_le_bytes());
    hash.update(pixels.height().to_le_bytes());
    hash.update(pixels.as_raw());
    Ok(hash.finalize().into())
}

fn source_path(root: &Path, source: Uuid) -> PathBuf {
    root.join("raw").join(format!("{source}.raw"))
}

pub(super) fn load(doc: &mut Document, path: &Path, root: &Path) -> Result<()> {
    let metadata = path.join(NAME);
    match fs::symlink_metadata(&metadata) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        result => {
            result?;
        }
    }
    let mut bytes = Vec::new();
    checked_file(root, &metadata, MANIFEST_LIMIT)?
        .take(MANIFEST_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MANIFEST_LIMIT {
        return Err(invalid(
            "The project's RAW settings exceed 4 MiB. The project is unchanged.",
        ));
    }
    let sources: Sources = serde_json::from_slice(&bytes)?;
    if !matches!(sources.version, 1 | 2) {
        return Err(invalid(
            "This project's RAW settings version is unsupported. Update Compositor before opening it.",
        ));
    }
    let mut cache: HashMap<Uuid, Arc<Vec<u8>>> = HashMap::new();
    let mut digests: HashMap<Uuid, [u8; 32]> = HashMap::new();
    let mut seen = HashSet::new();
    let mut total = 0_u64;
    for record in sources.layers {
        if !seen.insert(record.layer) {
            return Err(invalid(
                "The project repeats RAW settings for the same layer.",
            ));
        }
        let layer = doc
            .layers
            .iter_mut()
            .find(|layer| layer.id == record.layer)
            .ok_or_else(|| invalid("The project's RAW settings refer to a missing layer."))?;
        record.asset.settings.validate()?;
        let data = if let Some(data) = cache.get(&record.source) {
            data.clone()
        } else {
            let file = checked_file(
                root,
                &source_path(path, record.source),
                crate::raw::MAX_RAW_BYTES,
            )?;
            let remaining = crate::raw::MAX_RAW_BYTES.saturating_sub(total);
            if file.metadata()?.len() > remaining {
                return Err(invalid(
                    "The project's embedded RAW sources exceed 512 MiB.",
                ));
            }
            let mut bytes = Vec::new();
            file.take(remaining + 1).read_to_end(&mut bytes)?;
            total += bytes.len() as u64;
            if total > crate::raw::MAX_RAW_BYTES {
                return Err(invalid(
                    "The project's embedded RAW sources exceed 512 MiB.",
                ));
            }
            let data = Arc::new(bytes);
            cache.insert(record.source, data.clone());
            data
        };
        let mut asset = record.asset;
        asset.bytes = data;
        asset.validate()?;
        if sources.version == 2 {
            let source_digest = digests
                .entry(record.source)
                .or_insert_with(|| Sha256::digest(asset.bytes.as_slice()).into());
            if record.binding != Some(binding(doc.id, layer, &asset, source_digest)?) {
                return Err(invalid(
                    "The project's RAW source or settings no longer match its cached image. No files were changed. Restore a matching backup, or remove linux-raw.json from a copy of the .comp folder to open only its cached pixels.",
                ));
            }
        }
        layer.raw = Some(Arc::new(asset));
    }
    Ok(())
}

pub(super) fn validate_metadata(doc: &Document) -> Result<()> {
    // UUIDs have fixed length. A maximal digest bounds JSON bytes without
    // hashing every embedded source and pixel cache just to estimate storage.
    let layers = doc
        .layers
        .iter()
        .filter_map(|layer| {
            Some(Record {
                layer: layer.id,
                source: layer.id,
                asset: layer.raw.as_ref()?.as_ref().clone(),
                binding: Some([255; 32]),
            })
        })
        .collect();
    if serde_json::to_vec(&Sources { version: 2, layers })?.len() as u64 > MANIFEST_LIMIT {
        return Err(invalid(
            "The project's RAW settings would exceed 4 MiB. Choose fewer size variants or simplify the embedded RAW metadata; the document is unchanged.",
        ));
    }
    Ok(())
}

pub(super) fn save(doc: &Document, path: &Path) -> Result<()> {
    let mut sources = HashMap::new();
    let mut digests: HashMap<Uuid, [u8; 32]> = HashMap::new();
    let mut records = Vec::new();
    let mut total = 0_u64;
    for layer in &doc.layers {
        let Some(asset) = &layer.raw else {
            continue;
        };
        let pointer = Arc::as_ptr(&asset.bytes);
        let source = if let Some(source) = sources.get(&pointer) {
            *source
        } else {
            total += asset.bytes.len() as u64;
            if total > crate::raw::MAX_RAW_BYTES {
                return Err(invalid(
                    "The project's embedded RAW sources exceed 512 MiB. The previous save is preserved.",
                ));
            }
            fs::create_dir_all(path.join("raw"))?;
            let mut file = File::create(source_path(path, layer.id))?;
            file.write_all(&asset.bytes)?;
            file.sync_all()?;
            sources.insert(pointer, layer.id);
            layer.id
        };
        let source_digest = digests
            .entry(source)
            .or_insert_with(|| Sha256::digest(asset.bytes.as_slice()).into());
        records.push(Record {
            layer: layer.id,
            source,
            asset: asset.as_ref().clone(),
            binding: Some(binding(doc.id, layer, asset, source_digest)?),
        });
    }
    if records.is_empty() {
        return Ok(());
    }
    let bytes = serde_json::to_vec(&Sources {
        version: 2,
        layers: records,
    })?;
    if bytes.len() as u64 > MANIFEST_LIMIT {
        return Err(invalid(
            "The project's RAW settings exceed 4 MiB. The previous save is preserved.",
        ));
    }
    let mut file = File::create(path.join(NAME))?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    File::open(path.join("raw"))?.sync_all()?;
    Ok(())
}
