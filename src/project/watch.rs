//! Detect package replacement and in-place asset changes without decoding images.
use super::*;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    os::unix::fs::MetadataExt,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fingerprint(u64);

pub fn fingerprint(path: &Path) -> Result<Option<Fingerprint>> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
        Ok(metadata) if !metadata.is_dir() => {
            return Err(invalid(
                "The project package is no longer a directory. Your open edits are preserved. Use Save As to keep a separate copy.",
            ));
        }
        Ok(_) => {}
    }
    let mut hash = DefaultHasher::new();
    let root = path.canonicalize()?;
    let mut data = Vec::new();
    checked_file(&root, &path.join("manifest.json"), MANIFEST_LIMIT)?
        .take(MANIFEST_LIMIT + 1)
        .read_to_end(&mut data)?;
    if data.len() as u64 > MANIFEST_LIMIT {
        return Err(invalid("The project manifest exceeds 4 MiB."));
    }
    data.hash(&mut hash);
    for name in ["linux-raw.json", super::editors::NAME] {
        let settings = path.join(name);
        match fs::symlink_metadata(&settings) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            result => {
                result?;
            }
        }
        data.clear();
        checked_file(&root, &settings, MANIFEST_LIMIT)?
            .take(MANIFEST_LIMIT + 1)
            .read_to_end(&mut data)?;
        if data.len() as u64 > MANIFEST_LIMIT {
            return Err(invalid(format!("Project settings {name} exceed 4 MiB.")));
        }
        name.hash(&mut hash);
        data.hash(&mut hash);
    }
    for folder in ["images", "raw"] {
        let entries = match fs::read_dir(path.join(folder)) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        let mut entries = entries.take(32769).collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        if entries.len() > 32768 {
            return Err(invalid("Project asset count exceeds the monitoring limit."));
        }
        for entry in entries {
            entry.file_name().hash(&mut hash);
            let metadata = fs::symlink_metadata(entry.path())?;
            if !metadata.is_file() {
                return Err(invalid(
                    "A project asset is not a regular file. Your open edits are preserved.",
                ));
            }
            metadata.len().hash(&mut hash);
            metadata.mtime().hash(&mut hash);
            metadata.mtime_nsec().hash(&mut hash);
            metadata.ctime().hash(&mut hash);
            metadata.ctime_nsec().hash(&mut hash);
            metadata.ino().hash(&mut hash);
        }
    }
    hash_authoring(path, &mut hash)?;
    Ok(Some(Fingerprint(hash.finish())))
}

// Malformed optional editing sources still get a stable fingerprint, allowing
// an explicit rendered copy without following symlinks or reading huge files.
fn hash_authoring(path: &Path, hash: &mut impl Hasher) -> Result<()> {
    hash_optional_entry(&path.join(super::authoring::NAME), hash, true)?;
    let source = path.join("authoring");
    if !hash_optional_entry(&source, hash, false)?.is_some_and(|m| m.is_dir()) {
        return Ok(());
    }
    for name in [
        "manifest.json",
        "adjustments.json",
        "linux-raw.json",
        "linux-editors.json",
    ] {
        name.hash(hash);
        hash_optional_entry(&source.join(name), hash, true)?;
    }
    for name in ["images", "raw", "patterns"] {
        name.hash(hash);
        let directory = source.join(name);
        if !hash_optional_entry(&directory, hash, false)?.is_some_and(|m| m.is_dir()) {
            continue;
        }
        let mut entries = fs::read_dir(directory)?
            .take(32769)
            .collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        entries.len().hash(hash);
        for entry in entries {
            entry.file_name().hash(hash);
            hash_optional_entry(&entry.path(), hash, false)?;
        }
    }
    Ok(())
}
fn hash_optional_entry(
    path: &Path,
    hash: &mut impl Hasher,
    content: bool,
) -> Result<Option<fs::Metadata>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            false.hash(hash);
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    };
    true.hash(hash);
    metadata.is_file().hash(hash);
    metadata.is_dir().hash(hash);
    metadata.len().hash(hash);
    metadata.mtime().hash(hash);
    metadata.mtime_nsec().hash(hash);
    metadata.ctime().hash(hash);
    metadata.ctime_nsec().hash(hash);
    metadata.ino().hash(hash);
    if content && metadata.is_file() && metadata.len() <= MANIFEST_LIMIT {
        let mut bytes = Vec::new();
        File::open(path)?
            .take(MANIFEST_LIMIT + 1)
            .read_to_end(&mut bytes)?;
        bytes.hash(hash);
    }
    Ok(Some(metadata))
}

pub fn load_verified(path: &Path) -> Result<(Document, Fingerprint)> {
    let before = fingerprint(path)?
        .ok_or_else(|| invalid("The project was removed before it could be loaded."))?;
    let document = load(path)?;
    if fingerprint(path)?.as_ref() != Some(&before) {
        return Err(invalid(
            "The project changed while loading. Your open edits are preserved. Wait for the other writer to finish, then open it again.",
        ));
    }
    Ok((document, before))
}

pub fn save_if_unchanged(
    document: &Document,
    path: &Path,
    expected: Option<&Fingerprint>,
) -> Result<Fingerprint> {
    save_checked(document, path, |destination| {
        if fingerprint(destination)?.as_ref() != expected {
            return Err(invalid(
                "The project changed outside Compositor. Your edits and the external project are preserved. Open the external version or use Save As to keep your edits separately.",
            ));
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn competing_save_is_rejected_and_both_documents_survive() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Test.comp");
        let mut doc = Document::new(3, 2).unwrap();
        save(&doc, &path).unwrap();
        let baseline = fingerprint(&path).unwrap();
        doc.resolution = 144.;
        save(&doc, &path).unwrap();
        assert!(
            save_if_unchanged(&Document::new(3, 2).unwrap(), &path, baseline.as_ref()).is_err()
        );
        assert_eq!(load(&path).unwrap().resolution, 144.);
    }
    #[test]
    fn same_size_asset_edit_is_detected_but_manifest_rewrite_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Test.comp");
        let mut doc = Document::new(3, 2).unwrap();
        crate::edits::fill(&mut doc, [20, 60, 90, 255], false, false).unwrap();
        save(&doc, &path).unwrap();
        let before = fingerprint(&path).unwrap();
        let manifest = path.join("manifest.json");
        fs::write(&manifest, fs::read(&manifest).unwrap()).unwrap();
        assert_eq!(before, fingerprint(&path).unwrap());
        let asset = fs::read_dir(path.join("images"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        fs::write(&asset, fs::read(&asset).unwrap()).unwrap();
        assert_ne!(before, fingerprint(&path).unwrap());
    }
    #[test]
    fn external_save_in_final_exchange_race_is_preserved_separately() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Race.comp");
        let mut original = Document::new(3, 2).unwrap();
        save(&original, &path).unwrap();
        let expected = fingerprint(&path).unwrap();
        original.resolution = 144.;
        let external = original.clone();
        original.resolution = 300.;
        let first = std::cell::Cell::new(true);
        let result = save_checked(&original, &path, |destination| {
            if first.replace(false) {
                assert_eq!(fingerprint(destination)?, expected);
                save(&external, &path)?;
                Ok(())
            } else if fingerprint(destination)? != expected {
                Err(invalid("Concurrent writer detected"))
            } else {
                Ok(())
            }
        });
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("competing external version is preserved")
        );
        assert_eq!(load(&path).unwrap().resolution, 300.);
        let preserved = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|candidate| candidate != &path)
            .unwrap();
        assert_eq!(load(&preserved).unwrap().resolution, 144.);
    }
}
