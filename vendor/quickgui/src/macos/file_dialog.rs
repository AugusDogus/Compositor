use super::*;

pub(crate) fn present_native_open_panel(
    window: Option<&Arc<Window>>,
    context: MacPlatformDialogContext,
    options: &PathPromptOptions,
    responder: PlatformResponder<Option<Vec<PathBuf>>>,
) -> Result<MacPlatformDialog, String> {
    let parent = window.map(deepest_appkit_sheet).transpose()?;
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| "native file panels must start on the AppKit main thread".to_owned())?;
    let panel = unsafe { NSOpenPanel::openPanel(mtm) };
    unsafe {
        panel.setCanChooseFiles(options.files);
        panel.setCanChooseDirectories(options.directories);
        panel.setAllowsMultipleSelection(options.multiple);
        panel.setCanCreateDirectories(options.can_create_directories);
        panel.setResolvesAliases(options.resolves_aliases);
        panel.setTreatsFilePackagesAsDirectories(options.treats_file_packages_as_directories);
        panel.setShowsHiddenFiles(options.shows_hidden_files);
        // `message` is the panel's explanatory header. `title` predates it and keeps mapping to
        // the same slot so existing callers are unaffected when no message is supplied.
        if let Some(message) = options.message.as_deref().or(options.title.as_deref()) {
            panel.setMessage(Some(&NSString::from_str(message)));
        }
        if let Some(prompt) = &options.prompt {
            panel.setPrompt(Some(&NSString::from_str(prompt)));
        }
        if let Some(directory) = &options.directory {
            let directory = native_file_url(directory, true)?;
            panel.setDirectoryURL(Some(&directory));
        }
        if let Some(name) = &options.suggested_name {
            panel.setNameFieldStringValue(&NSString::from_str(name));
        }
        if options.files
            && let Some(types) = native_file_dialog_types(&options.filters)
        {
            #[allow(deprecated)]
            panel.setAllowedFileTypes(Some(&types));
        }
    }

    let completion_panel = panel.clone();
    let completion = RcBlock::new(move |response: NSModalResponse| {
        let result = if response == NSModalResponseOK {
            let urls = unsafe { completion_panel.URLs() };
            if urls.len() > MAX_SELECTED_PATHS {
                Err(PlatformError::SelectionTooLarge)
            } else {
                let mut total_bytes = 0_usize;
                let mut paths = Vec::with_capacity(urls.len());
                let mut error = None;
                for url in &urls {
                    match native_file_path(url) {
                        Ok(path) => {
                            total_bytes =
                                total_bytes.saturating_add(path.as_os_str().as_bytes().len());
                            if total_bytes > MAX_SELECTED_PATHS_TOTAL_BYTES {
                                error = Some(PlatformError::SelectionTooLarge);
                                break;
                            }
                            paths.push(path);
                        }
                        Err(path_error) => {
                            error = Some(path_error);
                            break;
                        }
                    }
                }
                error.map_or_else(|| Ok(Some(paths)), Err)
            }
        } else {
            Ok(None)
        };
        finish_native_dialog(&context, &responder, result);
    });
    unsafe {
        if let Some(parent) = parent {
            panel.beginSheetModalForWindow_completionHandler(&parent, &completion);
        } else {
            panel.beginWithCompletionHandler(&completion);
        }
    }
    Ok(MacPlatformDialog::Open(panel))
}

pub(crate) fn present_native_save_panel(
    window: Option<&Arc<Window>>,
    context: MacPlatformDialogContext,
    options: &SavePathOptions,
    responder: PlatformResponder<Option<PathBuf>>,
) -> Result<MacPlatformDialog, String> {
    let parent = window.map(deepest_appkit_sheet).transpose()?;
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| "native file panels must start on the AppKit main thread".to_owned())?;
    let panel = unsafe { NSSavePanel::savePanel(mtm) };
    let directory = native_file_url(&options.directory, true)?;
    unsafe {
        panel.setCanCreateDirectories(true);
        panel.setShowsHiddenFiles(options.shows_hidden_files);
        panel.setShowsTagField(options.shows_tag_field);
        panel.setDirectoryURL(Some(&directory));
        if let Some(title) = &options.title {
            panel.setMessage(Some(&NSString::from_str(title)));
        }
        if let Some(label) = &options.name_field_label {
            panel.setNameFieldLabel(Some(&NSString::from_str(label)));
        }
        if let Some(name) = &options.suggested_name {
            panel.setNameFieldStringValue(&NSString::from_str(name));
        }
        if let Some(prompt) = &options.prompt {
            panel.setPrompt(Some(&NSString::from_str(prompt)));
        }
        if let Some(types) = native_file_dialog_types(&options.filters) {
            #[allow(deprecated)]
            panel.setAllowedFileTypes(Some(&types));
        }
    }

    let completion_panel = panel.clone();
    let completion = RcBlock::new(move |response: NSModalResponse| {
        let result = if response == NSModalResponseOK {
            unsafe { completion_panel.URL() }
                .ok_or_else(|| {
                    PlatformError::Platform("the native save panel returned no URL".into())
                })
                .and_then(|url| native_file_path(&url))
                .map(Some)
        } else {
            Ok(None)
        };
        finish_native_dialog(&context, &responder, result);
    });
    unsafe {
        if let Some(parent) = parent {
            panel.beginSheetModalForWindow_completionHandler(&parent, &completion);
        } else {
            panel.beginWithCompletionHandler(&completion);
        }
    }
    Ok(MacPlatformDialog::Save(panel))
}

fn native_file_dialog_types(filters: &[FileDialogFilter]) -> Option<Retained<NSArray<NSString>>> {
    if filters
        .iter()
        .flat_map(|filter| &filter.extensions)
        .any(|extension| extension.as_ref() == "*")
    {
        return None;
    }
    let extensions = filters
        .iter()
        .flat_map(|filter| &filter.extensions)
        .map(|extension| NSString::from_str(extension))
        .collect::<Vec<_>>();
    (!extensions.is_empty()).then(|| NSArray::from_id_slice(&extensions))
}
