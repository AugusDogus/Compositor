//! Optional controls only. The upstream manifest is authoritative for rendering.
use super::*;
use crate::adjustment::EditorHint;
use std::collections::{HashMap, HashSet};

pub(super) const NAME: &str = "linux-editors.json";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Editors {
    version: u32,
    layers: Vec<EditorRecord>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditorRecord {
    layer: Uuid,
    editor: EditorHint,
}

pub(super) fn load(doc: &mut Document, path: &Path, root: &Path) -> Result<()> {
    let path = path.join(NAME);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        result => {
            result?;
        }
    }
    let mut bytes = Vec::new();
    checked_file(root, &path, MANIFEST_LIMIT)?
        .take(MANIFEST_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MANIFEST_LIMIT {
        return Err(invalid(
            "The project's Linux editor hints exceed 4 MiB. Native layers are preserved. Remove linux-editors.json from a copy of the project to open its native controls.",
        ));
    }
    let editors: Editors = serde_json::from_slice(&bytes).map_err(|error| invalid(format!(
        "The project's Linux editor hints are invalid: {error}. Native layers are preserved. Remove linux-editors.json from a copy of the project to open its native controls."
    )))?;
    if editors.version != 1 {
        return Err(invalid(
            "This project's Linux editor hints use an unsupported version. Update Compositor, or remove linux-editors.json from a copy to open its native controls.",
        ));
    }
    let mut seen = HashSet::new();
    let mut layers: HashMap<_, _> = doc
        .layers
        .iter_mut()
        .map(|layer| (layer.id, layer))
        .collect();
    for record in editors.layers {
        if !seen.insert(record.layer) {
            return Err(invalid(
                "The project repeats Linux editor hints for a layer. Native layers are preserved. Remove linux-editors.json from a copy to open its native controls.",
            ));
        }
        let Some(layer) = layers.get_mut(&record.layer) else {
            continue;
        };
        if let LayerContent::Adjustment(settings) = &mut layer.content {
            settings.editor_hint = Some(record.editor);
            settings.editor_hint = settings.valid_editor_hint();
        }
    }
    Ok(())
}

pub(super) fn save(doc: &Document, path: &Path) -> Result<()> {
    let layers: Vec<_> = doc
        .layers
        .iter()
        .filter_map(|layer| {
            let LayerContent::Adjustment(settings) = &layer.content else {
                return None;
            };
            Some(EditorRecord {
                layer: layer.id,
                editor: settings.valid_editor_hint()?,
            })
        })
        .collect();
    if layers.is_empty() {
        return Ok(());
    }
    let bytes = serde_json::to_vec_pretty(&Editors { version: 1, layers })?;
    if bytes.len() as u64 > MANIFEST_LIMIT {
        return Err(invalid(
            "The project's Linux editor hints exceed 4 MiB. The previous save is preserved.",
        ));
    }
    let mut file = File::create(path.join(NAME))?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests;
