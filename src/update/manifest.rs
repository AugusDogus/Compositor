use crate::{Result, invalid};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct AvailableUpdate {
    pub version: String,
    pub notes: Option<String>,
    pub(super) url: String,
}

#[derive(Deserialize)]
struct Manifest {
    version: String,
    notes: Option<String>,
    platforms: BTreeMap<String, Platform>,
}

#[derive(Deserialize)]
struct Platform {
    url: String,
}

pub(super) fn validate_url(value: &str) -> Result<()> {
    let parsed = url::Url::parse(value).map_err(|e| invalid(format!("Invalid update URL: {e}")))?;
    if value.len() > 16 * 1024
        || value.chars().any(|c| c.is_control() || c.is_whitespace())
        || parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
    {
        return Err(invalid(
            "Update URLs must use HTTPS with a host and no credentials, whitespace, or fragment. Correct the update feed and retry.",
        ));
    }
    Ok(())
}

pub(super) fn parse(
    bytes: &[u8],
    current: &str,
    architecture: &str,
) -> Result<Option<AvailableUpdate>> {
    if bytes.len() > quickgui::MAX_UPDATE_MANIFEST_BYTES {
        return Err(invalid("The update manifest exceeds the size limit."));
    }
    let manifest: Manifest = serde_json::from_slice(bytes)?;
    let version = semver::Version::parse(&manifest.version)
        .map_err(|e| invalid(format!("Invalid update version: {e}")))?;
    let current = semver::Version::parse(current)
        .map_err(|e| invalid(format!("Invalid installed version: {e}")))?;
    // Stable builds do not offer prereleases. Compare precedence, ignoring build metadata.
    if version.cmp_precedence(&current).is_le()
        || (current.pre.is_empty() && !version.pre.is_empty())
    {
        return Ok(None);
    }
    let platform = manifest.platforms.get(&format!("linux-{architecture}"))
        .ok_or_else(|| invalid(format!("This update has no Linux {architecture} executable. Retry after the publisher adds this architecture.")))?;
    validate_url(&platform.url)?;
    Ok(Some(AvailableUpdate {
        version: manifest.version,
        notes: manifest.notes,
        url: platform.url.clone(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn manifest(version: &str, url: &str) -> Vec<u8> {
        serde_json::to_vec(
            &serde_json::json!({"version": version, "platforms": {"linux-x86_64": {"url": url}}}),
        )
        .unwrap()
    }
    #[test]
    fn unsigned_updates_use_semantic_versions_and_matching_architecture() {
        let bytes = manifest("0.10.0", "https://example.com/editor.bin");
        assert_eq!(
            parse(&bytes, "0.9.0", "x86_64").unwrap().unwrap().version,
            "0.10.0"
        );
        assert!(parse(&bytes, "0.10.0", "x86_64").unwrap().is_none());
        assert!(parse(&bytes, "1.0.0", "x86_64").unwrap().is_none());
        assert!(parse(&bytes, "0.9.0", "aarch64").is_err());
        for version in ["0.9.0+build2", "1.0.0-beta.1"] {
            assert!(
                parse(
                    &manifest(version, "https://example.com/editor"),
                    "0.9.0",
                    "x86_64"
                )
                .unwrap()
                .is_none()
            );
        }
        assert!(
            parse(
                &manifest("bad", "https://example.com/editor"),
                "0.9.0",
                "x86_64"
            )
            .is_err()
        );
        assert!(parse(b"{}", "0.9.0", "x86_64").is_err());
    }
    #[test]
    fn malformed_and_insecure_urls_are_rejected() {
        for url in [
            "http://example.com/editor",
            "https://",
            "https://user:pass@example.com/editor",
            "https://example.com/\neditor",
            "https://example.com/editor#fragment",
        ] {
            assert!(validate_url(url).is_err(), "{url}");
            assert!(parse(&manifest("1.0.0", url), "0.9.0", "x86_64").is_err());
        }
        assert!(validate_url("https://example.com/editor.bin").is_ok());
    }
}
