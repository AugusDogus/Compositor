use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{MAX_OPEN_URLS, MAX_OPEN_URLS_TOTAL_BYTES, MAX_PLATFORM_URL_BYTES, OpenUrls};

/// Maximum process arguments inspected for one initial or secondary deep-link launch.
pub const MAX_DEEP_LINK_ARGUMENTS: usize = 4_096;

#[cfg(not(target_os = "macos"))]
pub(super) fn initial_open_urls() -> Option<OpenUrls> {
    let arguments = std::env::args_os()
        .skip(1)
        .take(MAX_DEEP_LINK_ARGUMENTS)
        .map(|argument| Arc::<str>::from(argument.to_string_lossy().into_owned()))
        .collect::<Vec<_>>();
    let cwd = std::env::current_dir().ok()?;
    open_urls_from_arguments(&arguments, &cwd)
}

pub(super) fn open_urls_from_arguments(arguments: &[Arc<str>], cwd: &Path) -> Option<OpenUrls> {
    let mut urls = Vec::with_capacity(arguments.len().min(MAX_OPEN_URLS));
    let mut total_bytes = 0_usize;

    for argument in arguments.iter().take(MAX_DEEP_LINK_ARGUMENTS) {
        let url = if is_url(argument) {
            argument.to_string()
        } else if !argument.starts_with('-') {
            let path = PathBuf::from(argument.as_ref());
            let path = if path.is_absolute() {
                path
            } else {
                cwd.join(path)
            };
            if !path.exists() {
                continue;
            }
            file_url(&path)
        } else {
            continue;
        };

        if url.is_empty() || url.len() > MAX_PLATFORM_URL_BYTES || url.contains('\0') {
            continue;
        }
        let Some(next_total) = total_bytes.checked_add(url.len()) else {
            break;
        };
        if next_total > MAX_OPEN_URLS_TOTAL_BYTES || urls.len() == MAX_OPEN_URLS {
            break;
        }
        total_bytes = next_total;
        urls.push(Arc::<str>::from(url));
    }

    (!urls.is_empty()).then(|| OpenUrls::from_bounded(urls))
}

fn is_url(value: &str) -> bool {
    let Some(colon) = value.find(':') else {
        return false;
    };
    if colon == 0 || colon > 64 {
        return false;
    }
    #[cfg(target_os = "windows")]
    if colon == 1
        && value
            .as_bytes()
            .get(2)
            .is_some_and(|byte| matches!(byte, b'\\' | b'/'))
    {
        return false;
    }
    value.as_bytes()[0].is_ascii_alphabetic()
        && value.as_bytes()[1..colon]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

fn file_url(path: &Path) -> String {
    let bytes = path.as_os_str().as_encoded_bytes();
    let is_unc = cfg!(target_os = "windows") && bytes.starts_with(b"\\\\");
    let mut url = if is_unc {
        String::from("file:")
    } else {
        String::from("file://")
    };
    for byte in bytes {
        let byte = if cfg!(target_os = "windows") && *byte == b'\\' {
            b'/'
        } else {
            *byte
        };
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/' | b':') {
            url.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            let _ = write!(url, "%{byte:02X}");
        }
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_protocol_and_existing_file_arguments_without_flags() {
        let cwd = Path::new(env!("CARGO_MANIFEST_DIR"));
        let arguments = [
            Arc::from("--inspect"),
            Arc::from("quickgui://open/project?id=7"),
            Arc::from("Cargo.toml"),
            Arc::from("not-a-real-file"),
        ];
        let urls = open_urls_from_arguments(&arguments, cwd).unwrap();
        assert_eq!(urls.len(), 2);
        assert_eq!(urls.iter().next(), Some("quickgui://open/project?id=7"));
        assert!(urls.iter().nth(1).unwrap().starts_with("file:"));
        assert!(urls.iter().nth(1).unwrap().ends_with("/Cargo.toml"));
    }

    #[test]
    fn recognizes_rfc_scheme_shape_without_treating_plain_values_as_urls() {
        assert!(is_url("quickgui+preview:open"));
        assert!(is_url("file:///tmp/readme.md"));
        assert!(!is_url("quick_gui:open"));
        assert!(!is_url("README.md"));
    }
}
