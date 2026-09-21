//! Explicit executable updates from a build-time HTTPS feed.
mod download;
mod install;
mod manifest;
use crate::{Result, invalid};
pub use manifest::AvailableUpdate;
use quickgui::UpdateCancellation;
use std::path::PathBuf;

pub const RELEASES_URL: &str = "https://github.com/AugusDogus/Compositor/releases/latest";
const DEFAULT_ENDPOINT: &str =
    "https://github.com/AugusDogus/Compositor/releases/latest/download/linux-update.json";

/// AppImage runtimes expose the original image path while the executable is mounted read-only.
pub fn is_appimage() -> bool {
    std::env::var_os("APPIMAGE").is_some()
}

fn require_executable_installation() -> Result<()> {
    if is_appimage() {
        return Err(invalid(
            "Download the new AppImage from GitHub Releases, close Compositor, and replace the old AppImage. The current application and projects are unchanged.",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct Service {
    endpoint: String,
}

#[derive(Debug)]
pub struct Staged {
    // The temporary directory owns the artifact through installation or cancellation.
    _directory: tempfile::TempDir,
    artifact: PathBuf,
    update: AvailableUpdate,
}

impl Staged {
    pub fn version(&self) -> &str {
        &self.update.version
    }
}

impl Service {
    pub fn configured() -> Result<Self> {
        Self::new(option_env!("COMPOSITOR_UPDATE_URL").unwrap_or(DEFAULT_ENDPOINT))
    }

    pub fn new(endpoint: &str) -> Result<Self> {
        manifest::validate_url(endpoint)?;
        Ok(Self {
            endpoint: endpoint.into(),
        })
    }

    pub fn check(&self, cancellation: &UpdateCancellation) -> Result<Option<AvailableUpdate>> {
        let mut bytes = Vec::new();
        download::receive(&self.endpoint, &mut bytes, quickgui::MAX_UPDATE_MANIFEST_BYTES as u64, 30, cancellation)
            .map_err(|error| invalid(format!("Could not check for updates: {error}. Check the network connection and retry. The installed editor is unchanged.")))?;
        manifest::parse(&bytes, env!("CARGO_PKG_VERSION"), std::env::consts::ARCH).map_err(|error| invalid(format!("The update feed is invalid: {error}. Retry later or contact the publisher. The installed editor is unchanged.")))
    }

    pub fn stage(
        &self,
        update: AvailableUpdate,
        cancellation: &UpdateCancellation,
    ) -> Result<Staged> {
        require_executable_installation()?;
        let directory = tempfile::Builder::new()
            .prefix("compositor-update-")
            .tempdir()?;
        let artifact = directory.path().join("compositor");
        let mut file = std::fs::File::create(&artifact)?;
        download::receive(&update.url, &mut file, 512 * 1024 * 1024, 120, cancellation)
            .map_err(|error| invalid(format!("Could not download the update: {error}. Retry the download. The installed editor is unchanged.")))?;
        file.sync_all()?;
        validate_executable(&artifact)?;
        Ok(Staged {
            _directory: directory,
            artifact,
            update,
        })
    }

    pub fn install(&self, staged: Staged, cancellation: &UpdateCancellation) -> Result<String> {
        require_executable_installation()?;
        // Use the executable that is actually running, not a mutable APPIMAGE environment variable.
        let target = std::env::current_exe()?;
        install::replace(&staged.artifact, &target, cancellation, |path| {
            validate_executable(path)
        })?;
        Ok(staged.update.version)
    }
}

fn validate_executable(path: &std::path::Path) -> Result<()> {
    use std::io::Read;
    let mut header = [0; 64];
    std::fs::File::open(path)?.read_exact(&mut header)?;
    let machine = u16::from_le_bytes([header[18], header[19]]);
    let expected = match std::env::consts::ARCH {
        "x86_64" => 62,
        "aarch64" => 183,
        _ => 0,
    };
    if &header[..4] != b"\x7fELF"
        || header[4] != 2
        || header[5] != 1
        || header[6] != 1
        || !matches!(u16::from_le_bytes([header[16], header[17]]), 2 | 3)
        || expected == 0
        || machine != expected
    {
        return Err(invalid(
            "The download is not a compatible Linux executable. The installed editor is unchanged. Ask the publisher to correct the update artifact.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn executable_validation_rejects_invalid_downloads() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("download");
        std::fs::write(&path, [0; 64]).unwrap();
        assert!(validate_executable(&path).is_err());
        validate_executable(&std::env::current_exe().unwrap()).unwrap();
    }
}
