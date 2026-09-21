//! Linux RAW metadata is separate from the upstream manifest, whose PNG assets
//! remain usable by readers that do not understand editable camera sources.
use super::*;
use std::collections::{HashMap, HashSet};

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
    if sources.version != 1 {
        return Err(invalid(
            "This project's RAW settings version is unsupported. Update Compositor before opening it.",
        ));
    }
    let mut cache: HashMap<Uuid, Arc<Vec<u8>>> = HashMap::new();
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
        layer.raw = Some(Arc::new(asset));
    }
    Ok(())
}

pub(super) fn save(doc: &Document, path: &Path) -> Result<()> {
    let mut sources = HashMap::new();
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
        records.push(Record {
            layer: layer.id,
            source,
            asset: asset.as_ref().clone(),
        });
    }
    if records.is_empty() {
        return Ok(());
    }
    let bytes = serde_json::to_vec(&Sources {
        version: 1,
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
