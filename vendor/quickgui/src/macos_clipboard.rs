use std::{collections::HashSet, path::PathBuf};

use objc2::rc::Retained;
use objc2_app_kit::{
    NSFilenamesPboardType, NSPasteboard, NSPasteboardNameFind, NSPasteboardType,
    NSPasteboardTypeHTML, NSPasteboardTypeRTF, NSPasteboardTypeString, NSPasteboardTypeURL,
};
use objc2_foundation::{NSArray, NSData, NSString};

use crate::{
    ClipboardBookmark, ClipboardData, ClipboardEntry, ClipboardError, ClipboardImage,
    ClipboardImageFormat, ClipboardItem, ClipboardString, ExternalPaths,
    MAX_CLIPBOARD_BOOKMARK_TITLE_BYTES, MAX_CLIPBOARD_BOOKMARK_URL_BYTES, MAX_CLIPBOARD_DATA_BYTES,
    MAX_CLIPBOARD_ENTRIES, MAX_CLIPBOARD_IMAGE_BYTES, MAX_CLIPBOARD_METADATA_BYTES,
    MAX_CLIPBOARD_PATH_BYTES, MAX_CLIPBOARD_PATHS, MAX_CLIPBOARD_TEXT_BYTES,
    MAX_CLIPBOARD_TOTAL_PATH_BYTES,
};

const TEXT_TYPE_NAME: &str = "public.utf8-plain-text";
const FILENAMES_TYPE_NAME: &str = "NSFilenamesPboardType";
const TEXT_HASH_TYPE_NAME: &str = "dev.quickgui.clipboard-text-hash";
const METADATA_TYPE_NAME: &str = "dev.quickgui.clipboard-metadata";
const HTML_TYPE_NAME: &str = "public.html";
const RTF_TYPE_NAME: &str = "public.rtf";
const URL_TYPE_NAME: &str = "public.url";
const URL_NAME_TYPE_NAME: &str = "public.url-name";

/// Lazily retained AppKit pasteboard and QuickGUI's metadata type identifiers.
pub(crate) struct MacPasteboard {
    inner: Retained<NSPasteboard>,
    text_hash_type: Retained<NSString>,
    metadata_type: Retained<NSString>,
}

impl MacPasteboard {
    pub(crate) fn general() -> Self {
        // SAFETY: AppKit returns the process-wide retained general pasteboard wrapper. QuickGUI
        // calls this only from its serialized application event thread.
        let inner = unsafe { NSPasteboard::generalPasteboard() };
        Self::new(inner)
    }

    pub(crate) fn find() -> Self {
        // SAFETY: `NSPasteboardNameFind` is an AppKit-owned immutable string constant.
        let inner = unsafe { NSPasteboard::pasteboardWithName(NSPasteboardNameFind) };
        Self::new(inner)
    }

    #[cfg(test)]
    fn unique() -> Self {
        // SAFETY: AppKit creates an isolated pasteboard with retained ownership.
        let inner = unsafe { NSPasteboard::pasteboardWithUniqueName() };
        Self::new(inner)
    }

    fn new(inner: Retained<NSPasteboard>) -> Self {
        Self {
            inner,
            text_hash_type: NSString::from_str(TEXT_HASH_TYPE_NAME),
            metadata_type: NSString::from_str(METADATA_TYPE_NAME),
        }
    }

    pub(crate) fn read(&self) -> Result<Option<ClipboardItem>, ClipboardError> {
        let native_types = unsafe { self.inner.types() };
        let declares = |native_type: &NSString| {
            native_types
                .as_ref()
                // SAFETY: `types` is AppKit's NSArray of NSPasteboardType strings and the queried
                // value is another live NSString-compatible pasteboard type.
                .is_some_and(|types| unsafe { types.containsObject(native_type) })
        };
        let mut entries = Vec::new();
        if let Some(paths) = self.read_paths()? {
            entries.push(ClipboardEntry::ExternalPaths(paths));
        }

        // A native file copy often carries a convenience string. The file representation remains
        // useful if that optional companion text is malformed or over the text limit.
        match self.read_string() {
            Ok(Some(text)) => entries.push(ClipboardEntry::String(text)),
            Ok(None) => {}
            Err(error) if !entries.is_empty() => {
                tracing::debug!(%error, "ignored invalid companion clipboard text")
            }
            Err(error) => return Err(error),
        }

        if declares(unsafe { NSPasteboardTypeURL })
            && let Some(bookmark) = self.read_bookmark()?
        {
            entries.push(ClipboardEntry::Bookmark(bookmark));
        }

        for (mime_type, native_type) in [
            ("text/html", unsafe { NSPasteboardTypeHTML }),
            ("text/rtf", unsafe { NSPasteboardTypeRTF }),
        ] {
            if !declares(native_type) {
                continue;
            }
            if let Some(bytes) = self.read_data(native_type, MAX_CLIPBOARD_DATA_BYTES, |bytes| {
                ClipboardError::DataTooLarge {
                    bytes,
                    maximum: MAX_CLIPBOARD_DATA_BYTES,
                }
            })? {
                entries.push(ClipboardEntry::Data(ClipboardData::new(mime_type, bytes)?));
            }
        }

        for format in ClipboardImageFormat::ALL {
            let data_type = NSString::from_str(format.uniform_type());
            if !declares(&data_type) {
                continue;
            }
            if let Some(bytes) = self.read_data(&data_type, MAX_CLIPBOARD_IMAGE_BYTES, |bytes| {
                ClipboardError::ImageTooLarge {
                    bytes,
                    maximum: MAX_CLIPBOARD_IMAGE_BYTES,
                }
            })? && !bytes.is_empty()
            {
                let image = ClipboardImage::new(format, bytes)?;
                entries.push(ClipboardEntry::Image(image));
            }
        }

        if let Some(types) = native_types {
            for native_type in types.iter() {
                if entries.len() == MAX_CLIPBOARD_ENTRIES {
                    break;
                }
                let mime_type = native_type.to_string();
                if is_known_native_type(&mime_type) {
                    continue;
                }
                let Some(bytes) =
                    self.read_data(native_type, MAX_CLIPBOARD_DATA_BYTES, |bytes| {
                        ClipboardError::DataTooLarge {
                            bytes,
                            maximum: MAX_CLIPBOARD_DATA_BYTES,
                        }
                    })?
                else {
                    continue;
                };
                if let Ok(data) = ClipboardData::new(mime_type, bytes) {
                    entries.push(ClipboardEntry::Data(data));
                }
            }
        }

        if entries.is_empty() {
            Ok(None)
        } else {
            ClipboardItem::new(entries).map(Some)
        }
    }

    fn read_bookmark(&self) -> Result<Option<ClipboardBookmark>, ClipboardError> {
        let Some(url) = self.read_data(
            unsafe { NSPasteboardTypeURL },
            MAX_CLIPBOARD_BOOKMARK_URL_BYTES,
            |_| ClipboardError::InvalidBookmarkUrl {
                maximum: MAX_CLIPBOARD_BOOKMARK_URL_BYTES,
            },
        )?
        else {
            return Ok(None);
        };
        let url = String::from_utf8(url).map_err(|_| ClipboardError::InvalidText)?;
        let title_type = NSString::from_str(URL_NAME_TYPE_NAME);
        let title = self
            .read_data(&title_type, MAX_CLIPBOARD_BOOKMARK_TITLE_BYTES, |_| {
                ClipboardError::InvalidBookmarkTitle {
                    maximum: MAX_CLIPBOARD_BOOKMARK_TITLE_BYTES,
                }
            })?
            .map(String::from_utf8)
            .transpose()
            .map_err(|_| ClipboardError::InvalidText)?
            .unwrap_or_default();
        ClipboardBookmark::new(title, url).map(Some)
    }

    fn read_paths(&self) -> Result<Option<ExternalPaths>, ClipboardError> {
        // SAFETY: AppKit defines this property list as an NSArray of NSString file paths.
        let Some(value) = self
            .inner
            .propertyListForType(unsafe { NSFilenamesPboardType })
        else {
            return Ok(None);
        };
        // SAFETY: The `NSFilenamesPboardType` contract guarantees an NSArray<NSString> value.
        let filenames: Retained<NSArray<NSString>> = unsafe { Retained::cast(value) };
        if filenames.is_empty() {
            return Ok(None);
        }
        if filenames.len() > MAX_CLIPBOARD_PATHS {
            return Err(ClipboardError::TooManyPaths {
                actual: filenames.len(),
                maximum: MAX_CLIPBOARD_PATHS,
            });
        }

        let mut paths = Vec::with_capacity(filenames.len());
        let mut total = 0usize;
        for filename in filenames.iter() {
            let bytes = filename.len();
            if bytes == 0 {
                return Err(ClipboardError::InvalidPath);
            }
            if bytes > MAX_CLIPBOARD_PATH_BYTES {
                return Err(ClipboardError::PathTooLong {
                    bytes,
                    maximum: MAX_CLIPBOARD_PATH_BYTES,
                });
            }
            total = total.saturating_add(bytes);
            if total > MAX_CLIPBOARD_TOTAL_PATH_BYTES {
                return Err(ClipboardError::PathsTooLarge {
                    bytes: total,
                    maximum: MAX_CLIPBOARD_TOTAL_PATH_BYTES,
                });
            }
            paths.push(PathBuf::from(filename.to_string()));
        }
        ExternalPaths::new(paths).map(Some)
    }

    fn read_string(&self) -> Result<Option<ClipboardString>, ClipboardError> {
        // Reading NSData lets us reject an oversized native value before allocating a Rust String.
        let Some(bytes) = self.read_data(
            unsafe { NSPasteboardTypeString },
            MAX_CLIPBOARD_TEXT_BYTES,
            |bytes| ClipboardError::TextTooLarge {
                bytes,
                maximum: MAX_CLIPBOARD_TEXT_BYTES,
            },
        )?
        else {
            return Ok(None);
        };
        let text = String::from_utf8(bytes).map_err(|_| ClipboardError::InvalidText)?;
        let mut value = ClipboardString::new(text)?;

        let hash = self.read_optional_metadata_data(&self.text_hash_type, size_of::<u64>());
        let metadata =
            self.read_optional_metadata_data(&self.metadata_type, MAX_CLIPBOARD_METADATA_BYTES);
        if let (Some(hash), Some(metadata)) = (hash, metadata)
            && let Ok(hash) = <[u8; 8]>::try_from(hash.as_slice())
            && u64::from_be_bytes(hash) == value.text_hash()
            && let Ok(metadata) = String::from_utf8(metadata)
        {
            value = value.with_metadata(metadata)?;
        }
        Ok(Some(value))
    }

    fn read_optional_metadata_data(
        &self,
        data_type: &NSPasteboardType,
        maximum: usize,
    ) -> Option<Vec<u8>> {
        // Corrupt metadata never makes otherwise valid clipboard text unavailable.
        self.read_data(data_type, maximum, |_| ClipboardError::MetadataTooLarge {
            bytes: maximum.saturating_add(1),
            maximum,
        })
        .ok()
        .flatten()
    }

    fn read_data(
        &self,
        data_type: &NSPasteboardType,
        maximum: usize,
        too_large: impl FnOnce(usize) -> ClipboardError,
    ) -> Result<Option<Vec<u8>>, ClipboardError> {
        // SAFETY: The data type is an immutable NSString and the returned NSData is retained.
        let Some(data) = (unsafe { self.inner.dataForType(data_type) }) else {
            return Ok(None);
        };
        if data.len() > maximum {
            return Err(too_large(data.len()));
        }
        Ok(Some(data.bytes().to_vec()))
    }

    pub(crate) fn write(&self, item: &ClipboardItem) -> Result<(), ClipboardError> {
        if item.is_empty() {
            // SAFETY: The retained pasteboard is valid for the duration of this call.
            unsafe { self.inner.clearContents() };
            return Ok(());
        }

        let mut strings = Vec::new();
        let mut paths = Vec::new();
        let mut images = Vec::new();
        let mut data = Vec::new();
        let mut bookmark = None;
        let mut seen_image_formats = [false; ClipboardImageFormat::ALL.len()];
        let mut seen_data_types = HashSet::new();
        for entry in item.entries() {
            match entry {
                ClipboardEntry::String(value) => strings.push(value),
                ClipboardEntry::ExternalPaths(value) => paths.extend(value.paths()),
                ClipboardEntry::Bookmark(value) => {
                    if bookmark.is_none() {
                        bookmark = Some(value);
                    }
                }
                ClipboardEntry::Data(value) => {
                    let native_type = native_type_for_mime(value.mime_type());
                    if seen_data_types.insert(native_type.clone()) {
                        data.push((native_type, value));
                    }
                }
                ClipboardEntry::Image(value) => {
                    let index = image_format_index(value.format());
                    if !seen_image_formats[index] {
                        seen_image_formats[index] = true;
                        images.push(value);
                    }
                }
            }
        }

        let mut combined_text = String::new();
        for string in &strings {
            combined_text.push_str(string.text());
        }
        if strings.is_empty() {
            if let Some(bookmark) = bookmark {
                combined_text.push_str(bookmark.url());
            } else if !paths.is_empty() {
                for (index, path) in paths.iter().enumerate() {
                    if index != 0 {
                        combined_text.push('\n');
                    }
                    combined_text.push_str(path.to_str().expect("ExternalPaths validates UTF-8"));
                }
            }
        }
        let has_text = !strings.is_empty() || bookmark.is_some() || !paths.is_empty();
        let metadata = match strings.as_slice() {
            [string] => string
                .metadata()
                .map(|metadata| (string.text_hash(), metadata)),
            _ => None,
        };

        let mut types = Vec::with_capacity(
            usize::from(!paths.is_empty())
                + usize::from(has_text)
                + usize::from(bookmark.is_some()) * 2
                + usize::from(metadata.is_some()) * 2
                + images.len()
                + data.len(),
        );
        if !paths.is_empty() {
            types.push(NSString::from_str(FILENAMES_TYPE_NAME));
        }
        if has_text {
            types.push(NSString::from_str(TEXT_TYPE_NAME));
        }
        if bookmark.is_some() {
            types.push(NSString::from_str(URL_TYPE_NAME));
            types.push(NSString::from_str(URL_NAME_TYPE_NAME));
        }
        if metadata.is_some() {
            types.push(self.text_hash_type.clone());
            types.push(self.metadata_type.clone());
        }
        for image in &images {
            types.push(NSString::from_str(image.format().uniform_type()));
        }
        for (native_type, _) in &data {
            if !types
                .iter()
                .any(|existing| existing.to_string() == *native_type)
            {
                types.push(NSString::from_str(native_type));
            }
        }

        let types = NSArray::from_vec(types);
        // `declareTypes` atomically replaces prior representations before individual bounded
        // payloads are installed. QuickGUI never registers a lazy pasteboard owner.
        unsafe { self.inner.declareTypes_owner(&types, None) };

        if !paths.is_empty() {
            let paths = paths
                .iter()
                .map(|path| {
                    NSString::from_str(path.to_str().expect("ExternalPaths validates UTF-8"))
                })
                .collect();
            let paths = NSArray::from_vec(paths);
            // SAFETY: NSArray<NSString> is a valid property list for the legacy filename type.
            let written = unsafe {
                self.inner
                    .setPropertyList_forType(&paths, NSFilenamesPboardType)
            };
            require_written(written, "external paths")?;
        }
        if has_text {
            self.write_data(
                combined_text.as_bytes(),
                unsafe { NSPasteboardTypeString },
                "text",
            )?;
        }
        if let Some(bookmark) = bookmark {
            self.write_data(
                bookmark.url().as_bytes(),
                unsafe { NSPasteboardTypeURL },
                "bookmark URL",
            )?;
            let title_type = NSString::from_str(URL_NAME_TYPE_NAME);
            self.write_data(bookmark.title().as_bytes(), &title_type, "bookmark title")?;
        }
        if let Some((hash, metadata)) = metadata {
            self.write_data(&hash.to_be_bytes(), &self.text_hash_type, "text hash")?;
            self.write_data(metadata.as_bytes(), &self.metadata_type, "metadata")?;
        }
        for image in images {
            let data_type = NSString::from_str(image.format().uniform_type());
            self.write_data(image.bytes(), &data_type, "image")?;
        }
        for (native_type, value) in data {
            if ClipboardImageFormat::from_mime_type(value.mime_type())
                .is_some_and(|format| seen_image_formats[image_format_index(format)])
            {
                continue;
            }
            let data_type = NSString::from_str(&native_type);
            self.write_data(value.bytes(), &data_type, "MIME data")?;
        }
        Ok(())
    }

    fn write_data(
        &self,
        bytes: &[u8],
        data_type: &NSPasteboardType,
        representation: &'static str,
    ) -> Result<(), ClipboardError> {
        let data = NSData::with_bytes(bytes);
        // SAFETY: Both retained objects remain live across the synchronous AppKit call.
        let written = unsafe { self.inner.setData_forType(Some(&data), data_type) };
        require_written(written, representation)
    }
}

fn native_type_for_mime(mime_type: &str) -> String {
    match mime_type {
        "text/html" => HTML_TYPE_NAME.to_owned(),
        "text/rtf" | "application/rtf" => RTF_TYPE_NAME.to_owned(),
        _ => ClipboardImageFormat::from_mime_type(mime_type)
            .map(|format| format.uniform_type().to_owned())
            .unwrap_or_else(|| mime_type.to_owned()),
    }
}

fn is_known_native_type(native_type: &str) -> bool {
    native_type == FILENAMES_TYPE_NAME
        || native_type == TEXT_TYPE_NAME
        || native_type == TEXT_HASH_TYPE_NAME
        || native_type == METADATA_TYPE_NAME
        || native_type == URL_NAME_TYPE_NAME
        || native_type == HTML_TYPE_NAME
        || native_type == RTF_TYPE_NAME
        || native_type == URL_TYPE_NAME
        || ClipboardImageFormat::ALL
            .iter()
            .any(|format| native_type == format.uniform_type())
}

fn image_format_index(format: ClipboardImageFormat) -> usize {
    ClipboardImageFormat::ALL
        .iter()
        .position(|candidate| *candidate == format)
        .expect("all clipboard image formats are indexed")
}

fn require_written(written: bool, representation: &'static str) -> Result<(), ClipboardError> {
    if written {
        Ok(())
    } else {
        Err(ClipboardError::Platform(
            format!("macOS rejected the {representation} representation").into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard};

    use objc2::rc::autoreleasepool;

    use super::*;

    /// Every pasteboard test talks to the one per-user pasteboard server, and AppKit fulfills
    /// promised representations across the process's pasteboards. Running these tests on parallel
    /// test threads occasionally surfaced another test's representation on a freshly created
    /// unique pasteboard, so they are serialized here.
    static PASTEBOARD_SERVER: Mutex<()> = Mutex::new(());

    fn serialize_pasteboard_access() -> MutexGuard<'static, ()> {
        PASTEBOARD_SERVER
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn unique_pasteboard_round_trips_text_metadata_and_clear() {
        let _pasteboard = serialize_pasteboard_access();
        autoreleasepool(|_| {
            let pasteboard = MacPasteboard::unique();
            assert_eq!(pasteboard.read().unwrap(), None);
            let item = ClipboardItem::new_string_with_metadata("hello", "selection:1").unwrap();
            pasteboard.write(&item).unwrap();
            assert_eq!(pasteboard.read().unwrap(), Some(item));
            pasteboard.write(&ClipboardItem::default()).unwrap();
            assert_eq!(pasteboard.read().unwrap(), None);
        });
    }

    #[test]
    fn unique_pasteboard_round_trips_files_with_text_fallback() {
        let _pasteboard = serialize_pasteboard_access();
        autoreleasepool(|_| {
            let pasteboard = MacPasteboard::unique();
            let paths = ExternalPaths::new(["/tmp/one", "/tmp/two"]).unwrap();
            let item = ClipboardItem::new_paths(paths.clone()).unwrap();
            pasteboard.write(&item).unwrap();
            let read = pasteboard.read().unwrap().unwrap();
            assert_eq!(read.entries()[0], ClipboardEntry::ExternalPaths(paths));
            assert_eq!(read.text().as_deref(), Some("/tmp/one\n/tmp/two"));
        });
    }

    #[test]
    fn unique_pasteboard_keeps_image_encoded() {
        let _pasteboard = serialize_pasteboard_access();
        autoreleasepool(|_| {
            let pasteboard = MacPasteboard::unique();
            let image = ClipboardImage::new(ClipboardImageFormat::Png, vec![1, 2, 3, 4]).unwrap();
            let item = ClipboardItem::new_image(image).unwrap();
            pasteboard.write(&item).unwrap();
            assert_eq!(pasteboard.read().unwrap(), Some(item));
        });
    }

    #[test]
    fn unique_pasteboard_round_trips_rich_and_custom_representations() {
        let _pasteboard = serialize_pasteboard_access();
        autoreleasepool(|_| {
            let pasteboard = MacPasteboard::unique();
            let item = ClipboardItem::new([
                ClipboardEntry::Data(ClipboardData::html("<b>Hello</b>").unwrap()),
                ClipboardEntry::Data(ClipboardData::rtf(r"{\rtf1 Hello}").unwrap()),
                ClipboardEntry::Data(
                    ClipboardData::new("application/vnd.quickgui.test", [4_u8, 5, 6]).unwrap(),
                ),
                ClipboardEntry::Bookmark(
                    ClipboardBookmark::new("QuickGUI", "https://quickgui.dev/").unwrap(),
                ),
            ])
            .unwrap();
            pasteboard.write(&item).unwrap();
            let read = pasteboard.read().unwrap().unwrap();
            assert_eq!(read.html().unwrap(), Some("<b>Hello</b>"));
            assert_eq!(read.rtf().unwrap(), Some(r"{\rtf1 Hello}"));
            assert_eq!(
                read.data("application/vnd.quickgui.test")
                    .map(ClipboardData::bytes),
                Some(&[4_u8, 5, 6][..])
            );
            assert!(read.bookmarks().any(|bookmark| {
                bookmark.title() == "QuickGUI" && bookmark.url() == "https://quickgui.dev/"
            }));
        });
    }
}
