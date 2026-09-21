use crate::{
    Result,
    adjustment::Adjustment,
    blend::Blend,
    document::{Document, Layer, LayerContent, Mask, Shape},
    geometry::Transform,
    image_io, invalid,
};
use image::{DynamicImage, ImageFormat};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};
use uuid::Uuid;

const MANIFEST_LIMIT: u64 = 4 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    format: String,
    version: u32,
    color_space: String,
    #[serde(rename = "documentID")]
    document_id: Uuid,
    width: u32,
    height: u32,
    resolution: Option<f64>,
    #[serde(rename = "activeLayerID")]
    active_layer_id: Option<Uuid>,
    layers: Vec<Record>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    id: Uuid,
    name: String,
    is_visible: bool,
    transform: Transform,
    #[serde(skip_serializing_if = "Option::is_none")]
    image_file: Option<String>,
    #[serde(rename = "parentID", skip_serializing_if = "Option::is_none")]
    parent_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_group: Option<bool>,
    opacity: Option<f64>,
    blend_mode: Option<Blend>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mask_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mask_enabled: Option<bool>,
    #[serde(rename = "maskSourceID", skip_serializing_if = "Option::is_none")]
    mask_source_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mask_placement: Option<Transform>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mask_linked: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    adjustment: Option<Adjustment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shape: Option<Shape>,
}

fn asset_name(id: Uuid, mask: bool) -> String {
    format!(
        "{}{}.png",
        id.to_string().to_uppercase(),
        if mask { ".mask" } else { "" }
    )
}

fn checked_file(root: &Path, path: &Path, limit: u64) -> Result<File> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > limit || !path.canonicalize()?.starts_with(root) {
        return Err(invalid(format!(
            "Project asset '{}' is oversized, not a regular file, or outside the package.",
            path.display()
        )));
    }
    Ok(File::open(path)?)
}

pub fn load(path: &Path) -> Result<Document> {
    let root = path.canonicalize()?;
    let mut bytes = Vec::new();
    checked_file(&root, &path.join("manifest.json"), MANIFEST_LIMIT)?
        .take(MANIFEST_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MANIFEST_LIMIT {
        return Err(invalid("Project manifest exceeds 4 MiB."));
    }
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    if manifest.format != "com.compositor.project" || manifest.color_space != "sRGB" {
        return Err(invalid("This is not an sRGB Compositor project."));
    }
    if !(1..=7).contains(&manifest.version) {
        return Err(invalid(format!(
            "Project version {} is unsupported. Supported versions: 1 through 7.",
            manifest.version
        )));
    }
    let mut doc = Document::new(manifest.width, manifest.height)?;
    doc.id = manifest.document_id;
    doc.resolution = manifest.resolution.unwrap_or(72.);
    doc.active = manifest.active_layer_id;
    doc.selected = manifest.active_layer_id.into_iter().collect();
    doc.layers.clear();
    // Validate metadata and references before decoding large assets.
    for record in &manifest.layers {
        let group = record.is_group.unwrap_or(false);
        if record
            .image_file
            .as_ref()
            .is_some_and(|s| s != &asset_name(record.id, false))
            || record
                .mask_file
                .as_ref()
                .is_some_and(|s| s != &asset_name(record.id, true))
            || (group && (record.image_file.is_some() || record.adjustment.is_some()))
            || (record.adjustment.is_some()
                && (manifest.version < 7 || record.image_file.is_some()))
            || (manifest.version < 2 && (group || record.parent_id.is_some()))
            || (manifest.version < 3
                && (record.opacity.unwrap_or(1.) != 1.
                    || record.blend_mode.unwrap_or_default() != Blend::Normal))
            || (record.mask_file.is_some() && manifest.version < if group { 6 } else { 4 })
            || (manifest.version < 5 && record.mask_source_id.is_some())
            || (record.mask_file.is_none()
                && (record.mask_enabled.is_some()
                    || record.mask_placement.is_some()
                    || record.mask_linked.is_some()))
        {
            return Err(invalid(format!(
                "Layer '{}' contains unsafe asset paths or metadata incompatible with project version {}.",
                record.name, manifest.version
            )));
        }
        doc.layers.push(Layer {
            id: record.id,
            name: record.name.clone(),
            visible: record.is_visible,
            parent: record.parent_id,
            transform: record.transform,
            opacity: record.opacity.unwrap_or(1.),
            blend: record.blend_mode.unwrap_or_default(),
            clip_source: record.mask_source_id,
            mask: None,
            shape: None,
            content: if group {
                LayerContent::Group
            } else if let Some(adjustment) = &record.adjustment {
                LayerContent::Adjustment(Box::new(adjustment.clone()))
            } else {
                LayerContent::Raster(None)
            },
        });
    }
    doc.validate()?;
    let mut source_pixels = 0_u64;
    let mut mask_pixels = 0_u64;
    for (layer, record) in doc.layers.iter_mut().zip(manifest.layers) {
        for (filename, is_mask) in [(record.image_file, false), (record.mask_file, true)] {
            let Some(filename) = filename else {
                continue;
            };
            let file = path.join("images").join(filename);
            checked_file(&root, &file, 512 * 1024 * 1024)?;
            let reader = image::ImageReader::open(&file)?.with_guessed_format()?;
            if reader.format() != Some(ImageFormat::Png) {
                return Err(invalid("Embedded project assets must be PNG images."));
            }
            let (width, height) = reader.into_dimensions()?;
            let used = if is_mask {
                &mut mask_pixels
            } else {
                &mut source_pixels
            };
            *used += u64::from(width) * u64::from(height);
            if *used > crate::document::MAX_PIXELS {
                return Err(invalid(
                    "Project exceeds 100 million source or mask pixels.",
                ));
            }
            let image = image_io::read_image(&file)?;
            if is_mask {
                if image
                    .pixels()
                    .any(|p| p[0] != p[1] || p[1] != p[2] || p[3] != 255)
                {
                    return Err(invalid(
                        "Project masks must contain opaque grayscale coverage.",
                    ));
                }
                layer.mask = Some(Mask {
                    pixels: Arc::new(DynamicImage::ImageRgba8(image).into_luma8()),
                    enabled: record.mask_enabled.unwrap_or(true),
                    linked: record.mask_linked.unwrap_or(true),
                    placement: record.mask_placement,
                });
            } else {
                layer.content = LayerContent::Raster(Some(Arc::new(image)));
            }
        }
        layer.shape = record.shape;
    }
    doc.validate()?;
    Ok(doc)
}

pub fn save(document: &Document, path: &Path) -> Result<()> {
    document.validate()?;
    if path.extension().is_none_or(|ext| ext != "comp") {
        return Err(invalid(
            "Use a .comp folder name to save a Compositor project.",
        ));
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if path.exists() {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_dir() || !path.join("manifest.json").is_file() {
            return Err(invalid(
                "The destination already exists and is not a Compositor project. Choose another name.",
            ));
        }
    }
    let staged = tempfile::Builder::new()
        .prefix(".compositor-save-")
        .tempdir_in(parent)?;
    fs::create_dir(staged.path().join("images"))?;
    let mut records = Vec::new();
    for layer in &document.layers {
        let image_file = layer.raster().map(|_| asset_name(layer.id, false));
        let mask_file = layer.mask.as_ref().map(|_| asset_name(layer.id, true));
        if let (Some(image), Some(filename)) = (layer.raster(), &image_file) {
            let mut file = File::create(staged.path().join("images").join(filename))?;
            DynamicImage::ImageRgba8(image.as_ref().clone())
                .write_to(&mut file, ImageFormat::Png)?;
            file.sync_all()?;
        }
        if let (Some(mask), Some(filename)) = (&layer.mask, &mask_file) {
            let mut file = File::create(staged.path().join("images").join(filename))?;
            DynamicImage::ImageLuma8(mask.pixels.as_ref().clone())
                .write_to(&mut file, ImageFormat::Png)?;
            file.sync_all()?;
        }
        records.push(Record {
            id: layer.id,
            name: layer.name.clone(),
            is_visible: layer.visible,
            transform: layer.transform,
            image_file,
            parent_id: layer.parent,
            is_group: Some(layer.is_group()),
            opacity: Some(layer.opacity),
            blend_mode: Some(layer.blend),
            mask_file,
            mask_enabled: layer.mask.as_ref().map(|m| m.enabled),
            mask_source_id: layer.clip_source,
            mask_placement: layer.mask.as_ref().and_then(|m| m.placement),
            mask_linked: layer.mask.as_ref().map(|m| m.linked),
            shape: layer.shape,
            adjustment: match &layer.content {
                LayerContent::Adjustment(a) => Some(a.as_ref().clone()),
                _ => None,
            },
        });
    }
    let manifest = Manifest {
        format: "com.compositor.project".into(),
        version: 7,
        color_space: "sRGB".into(),
        document_id: document.id,
        width: document.width,
        height: document.height,
        resolution: Some(document.resolution),
        active_layer_id: document.active,
        layers: records,
    };
    let data = serde_json::to_vec_pretty(&manifest)?;
    if data.len() as u64 > MANIFEST_LIMIT {
        return Err(invalid(
            "Project manifest exceeds 4 MiB. The previous save is preserved.",
        ));
    }
    let mut metadata = File::create(staged.path().join("manifest.json"))?;
    metadata.write_all(&data)?;
    metadata.sync_all()?;
    File::open(staged.path().join("images"))?.sync_all()?;
    File::open(staged.path())?.sync_all()?;
    if path.exists() {
        // Linux atomically exchanges complete directories. The old package is removed only after success.
        rustix::fs::renameat_with(
            rustix::fs::CWD,
            staged.path(),
            rustix::fs::CWD,
            path,
            rustix::fs::RenameFlags::EXCHANGE,
        )
        .map_err(std::io::Error::from)?;
    } else {
        fs::rename(staged.path(), path)?;
    }
    File::open(parent)?.sync_all()?;
    Ok(())
}

pub fn with_extension(mut path: PathBuf) -> PathBuf {
    if path.extension().is_none() {
        path.set_extension("comp");
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GrayImage, Luma, Rgba, RgbaImage};

    #[test]
    fn save_roundtrips_pixels_transform_mask_and_adjustment() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sample.comp");
        let mut doc = Document::new(2, 3).unwrap();
        doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
            2,
            3,
            Rgba([34, 56, 78, 90]),
        ))));
        doc.layers[0].transform.rotation = 37.;
        doc.layers[0].mask = Some(Mask {
            pixels: Arc::new(GrayImage::from_pixel(1, 1, Luma([128]))),
            enabled: true,
            linked: false,
            placement: Some(Transform::new(2, 3)),
        });
        let mut adjustment = Layer::blank("Exposure", 2, 3);
        adjustment.content =
            LayerContent::Adjustment(Box::new(Adjustment::new(crate::adjustment::Kind::Exposure)));
        doc.add(adjustment).unwrap();
        save(&doc, &path).unwrap();
        assert_eq!(load(&path).unwrap(), doc);
        doc.layers[0].name = "Renamed".into();
        save(&doc, &path).unwrap();
        assert_eq!(load(&path).unwrap(), doc);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn invalid_save_does_not_replace_existing_package() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("safe.comp");
        let mut doc = Document::new(1, 1).unwrap();
        save(&doc, &path).unwrap();
        let before = fs::read(path.join("manifest.json")).unwrap();
        doc.layers[0].clip_source = Some(doc.layers[0].id);
        assert!(save(&doc, &path).is_err());
        assert_eq!(fs::read(path.join("manifest.json")).unwrap(), before);
    }

    #[test]
    fn failure_after_staging_pixels_preserves_the_saved_package_and_cleans_up() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("safe.comp");
        let mut original = Document::new(2, 2).unwrap();
        crate::edits::fill(&mut original, [255, 0, 0, 255], false, false).unwrap();
        save(&original, &path).unwrap();
        let manifest = fs::read(path.join("manifest.json")).unwrap();
        let asset = path
            .join("images")
            .join(asset_name(original.layers[0].id, false));
        let pixels = fs::read(&asset).unwrap();
        let mut edited = original.clone();
        crate::edits::fill(&mut edited, [0, 0, 255, 255], false, false).unwrap();
        for _ in 0..300 {
            edited.add(Layer::blank("x".repeat(16_000), 2, 2)).unwrap();
        }
        edited.validate().unwrap();
        let error = save(&edited, &path).unwrap_err().to_string();
        assert!(error.contains("manifest exceeds"));
        assert_eq!(fs::read(path.join("manifest.json")).unwrap(), manifest);
        assert_eq!(fs::read(asset).unwrap(), pixels);
        assert_eq!(load(&path).unwrap(), original);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn interrupted_staging_does_not_hide_the_last_committed_project() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("safe.comp");
        let original = Document::new(2, 2).unwrap();
        save(&original, &path).unwrap();
        let interrupted = directory.path().join(".compositor-save-interrupted");
        fs::create_dir(&interrupted).unwrap();
        fs::write(interrupted.join("manifest.json"), b"partial manifest").unwrap();
        assert_eq!(load(&path).unwrap(), original);
        let mut updated = original.clone();
        updated.layers[0].name = "Updated".into();
        save(&updated, &path).unwrap();
        assert_eq!(load(&path).unwrap(), updated);
        assert!(interrupted.exists());
    }

    #[test]
    fn rejects_escaping_asset_paths_before_reading() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.comp");
        save(&Document::new(1, 1).unwrap(), &path).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(path.join("manifest.json")).unwrap()).unwrap();
        value["layers"][0]["imageFile"] = "../../outside.png".into();
        fs::write(
            path.join("manifest.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        assert!(load(&path).is_err());
    }

    #[test]
    fn rejects_cyclic_groups() {
        let mut doc = Document::new(1, 1).unwrap();
        doc.layers[0].content = LayerContent::Group;
        doc.layers[0].parent = Some(doc.layers[0].id);
        assert!(doc.validate().is_err());
    }
}
