//! Native dialog titles, filename suggestions, and accepted destinations.
use compositor::{Result, invalid};
use quickgui::{FileDialogFilter, SavePathOptions};
use std::path::{Path, PathBuf};

// QuickGUI's Linux portal backend prefixes each extension with "*.". XDG
// filters use case-sensitive globs, unlike the source app's content types.
pub(super) fn file_filter(name: &str, extensions: &[&str]) -> FileDialogFilter {
    FileDialogFilter::new(
        name,
        extensions.iter().map(|extension| {
            extension
                .chars()
                .map(|letter| {
                    format!(
                        "[{}{}]",
                        letter.to_ascii_lowercase(),
                        letter.to_ascii_uppercase()
                    )
                })
                .collect::<String>()
        }),
    )
}

#[derive(Clone, Copy)]
pub(super) enum SaveDialog {
    Project,
    ProjectAs,
    Png,
    Psd,
    Jpeg,
}

impl SaveDialog {
    fn title(self) -> &'static str {
        match self {
            Self::Project => "Save Project",
            Self::ProjectAs => "Save Project As",
            Self::Png => "Export PNG",
            Self::Psd => "Export PSD",
            Self::Jpeg => "Export JPEG",
        }
    }

    fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Project | Self::ProjectAs => &["comp"],
            Self::Png => &["png"],
            Self::Psd => &["psd"],
            Self::Jpeg => &["jpg", "jpeg"],
        }
    }

    pub(super) fn options(self, project: Option<&Path>) -> SavePathOptions {
        let directory = project
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let name = project
            .and_then(Path::file_stem)
            .unwrap_or_else(|| std::ffi::OsStr::new("Untitled"));
        let filename = match (self, project.and_then(Path::file_name)) {
            (Self::Project | Self::ProjectAs, Some(filename)) => {
                filename.to_string_lossy().into_owned()
            }
            _ => format!("{}.{}", name.to_string_lossy(), self.extensions()[0]),
        };
        let filter = match self {
            Self::Project | Self::ProjectAs => "Compositor project",
            Self::Png => "PNG image",
            Self::Psd => "Photoshop document",
            Self::Jpeg => "JPEG image",
        };
        SavePathOptions::new(directory)
            .title(self.title())
            .suggested_name(filename)
            .filters([file_filter(filter, self.extensions())])
    }

    pub(super) fn destination(self, path: PathBuf) -> Result<PathBuf> {
        // Validate the exact path confirmed by the native chooser. Appending an
        // extension here could overwrite another file without its confirmation.
        if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| {
                self.extensions()
                    .iter()
                    .any(|accepted| ext.eq_ignore_ascii_case(accepted))
            })
        {
            Ok(path)
        } else {
            Err(invalid(format!(
                "{} needs a .{} filename. Your files and open edits are unchanged. Choose {} again and use that extension.",
                self.title(),
                self.extensions()[0],
                self.title(),
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_dialogs_use_the_project_name_directory_title_and_format() {
        for (dialog, title, extension) in [
            (SaveDialog::Project, "Save Project", "comp"),
            (SaveDialog::ProjectAs, "Save Project As", "comp"),
            (SaveDialog::Png, "Export PNG", "png"),
            (SaveDialog::Jpeg, "Export JPEG", "jpg"),
        ] {
            for stem in ["Portrait", "Version.2", "Étude 花"] {
                let project = PathBuf::from("/tmp/projects").join(format!("{stem}.comp"));
                let options = dialog.options(Some(&project));
                assert_eq!(options.directory, Path::new("/tmp/projects"));
                assert_eq!(options.title.as_deref(), Some(title));
                assert_eq!(
                    options.suggested_name.as_deref(),
                    Some(format!("{stem}.{extension}").as_str())
                );
                assert_eq!(options.filters.len(), 1);
                assert_eq!(
                    options.filters[0].extensions.len(),
                    dialog.extensions().len()
                );
            }
            assert_eq!(
                dialog.options(None).suggested_name.as_deref(),
                Some(format!("Untitled.{extension}").as_str())
            );
        }
    }

    #[test]
    fn destinations_preserve_confirmed_paths_and_reject_missing_or_conflicting_extensions() {
        for (dialog, extension) in [
            (SaveDialog::Project, "comp"),
            (SaveDialog::ProjectAs, "comp"),
            (SaveDialog::Png, "png"),
            (SaveDialog::Jpeg, "jpg"),
        ] {
            assert!(dialog.destination("/tmp/Portrait".into()).is_err());
            let uppercase = PathBuf::from(format!("/tmp/Portrait.{}", extension.to_uppercase()));
            assert_eq!(dialog.destination(uppercase.clone()).unwrap(), uppercase);
            assert!(dialog.destination("/tmp/Portrait.webp".into()).is_err());
        }
        assert_eq!(
            SaveDialog::Jpeg
                .destination("/tmp/Portrait.jpeg".into())
                .unwrap(),
            PathBuf::from("/tmp/Portrait.jpeg")
        );
        assert!(
            SaveDialog::Png
                .destination("/tmp/Portrait.jpg".into())
                .is_err()
        );
        assert!(
            SaveDialog::Jpeg
                .destination("/tmp/Portrait.png".into())
                .is_err()
        );
    }
}
