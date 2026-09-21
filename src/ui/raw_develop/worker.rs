use super::*;
use std::{path::Path, time::Duration};

pub(super) enum Output {
    Superseded,
    Loaded(Box<Ready>),
    Preview {
        crop: [f32; 4],
        full: bool,
        preview: DisplayImage,
        before: DisplayImage,
        warnings: DisplayImage,
        histogram: Box<[[u32; 256]; 3]>,
        clipping: [f32; 2],
    },
    Applied {
        settings: DevelopSettings,
        pixels: RgbaImage,
    },
    Exported(PathBuf),
    Preset(DevelopSettings),
    SavedPreset(PathBuf),
}

pub(super) fn image(pixels: RgbaImage) -> Result<DisplayImage> {
    DisplayImage::new(pixels)
}

pub(super) struct Analysis {
    pub preview: DisplayImage,
    pub warnings: DisplayImage,
    pub histogram: [[u32; 256]; 3],
    pub clipping: [f32; 2],
}
pub(super) fn analyze(pixels: RgbaImage) -> Result<Analysis> {
    let mut bins = [[0; 256]; 3];
    let mut counts = [0_u32; 2];
    let mut visible = 0_u32;
    let mut warnings = pixels.clone();
    for (p, w) in pixels.pixels().zip(warnings.pixels_mut()) {
        if p[3] == 0 {
            continue;
        }
        visible += 1;
        for c in 0..3 {
            bins[c][p[c] as usize] += 1;
        }
        if p.0[..3].contains(&255) {
            *w = image::Rgba([255, 35, 65, 255]);
            counts[1] += 1;
        } else if p.0[..3].iter().all(|v| *v <= 1) {
            *w = image::Rgba([40, 100, 255, 255]);
            counts[0] += 1;
        }
    }
    Ok(Analysis {
        preview: image(pixels)?,
        warnings: image(warnings)?,
        histogram: bins,
        clipping: counts.map(|v| 100. * v as f32 / visible.max(1) as f32),
    })
}

fn before(
    full: &DecodedRaw,
    settings: &DevelopSettings,
    cancel: &AtomicBool,
) -> Result<DisplayImage> {
    // Match the developed crop while retaining default tone and lens settings.
    let defaults = DevelopSettings {
        crop: settings.crop,
        ..Default::default()
    };
    image(raw::render(full, &defaults, cancel)?)
}

impl Editor {
    pub(in crate::ui) fn start_raw_work(&mut self, cx: &ViewContext<'_, Self>) {
        if self.develop.is_none()
            && !self.pending
            && self.modal.is_none()
            && self.psd_conversion.is_none()
            && self.errors.is_empty()
            && let Some((source, target)) = self.raw_queue.pop_front()
        {
            self.develop = Some(Develop::new(source, target));
        }
        let Some(d) = &mut self.develop else {
            return;
        };
        if d.running {
            return;
        }
        if d.request.is_none()
            && d.ready.is_some()
            && d.rendered != Some(d.revision)
            && d.error.is_none()
        {
            d.request = Some(Request::Preview);
        }
        let Some(request) = d.request.take() else {
            return;
        };
        let (id, revision) = (d.id, d.revision);
        let settings = d.settings.clone();
        let cancel = d.cancel.clone();
        let desired_revision = d.desired_revision.clone();
        let delay = Duration::from_millis(100).saturating_sub(d.last_change.elapsed());
        let full = d.ready.as_ref().map(|r| r.full.clone());
        let input = d.ready.as_ref().map(|r| {
            if d.full_preview {
                r.full.clone()
            } else {
                r.proxy.clone()
            }
        });
        let preview_request = matches!(request, Request::Preview);
        let cached_before = d
            .ready
            .as_ref()
            .filter(|r| r.before_crop == settings.crop && r.before_full == d.full_preview)
            .map(|r| r.before.clone());
        let full_preview = d.full_preview;
        d.running = true;
        let operation = move || -> Result<Output> {
            if cancel.load(Ordering::Relaxed) {
                return Err(invalid("RAW operation cancelled."));
            }
            match request {
                Request::Load(source) => {
                    let (asset, decoded) = match source {
                        Source::Path(path) => raw::open(&path)?,
                        Source::Asset(asset) => ((*asset).clone(), raw::decode(&asset.bytes)?),
                    };
                    let full = Arc::new(decoded);
                    let proxy = Arc::new(full.preview(1600));
                    let before = before(&proxy, &asset.settings, &cancel)?;
                    let Analysis {
                        preview,
                        warnings,
                        histogram,
                        clipping,
                    } = analyze(raw::render(&proxy, &asset.settings, &cancel)?)?;
                    Ok(Output::Loaded(Box::new(Ready {
                        before_crop: asset.settings.crop,
                        before_full: false,
                        asset,
                        full,
                        proxy,
                        before,
                        preview,
                        warnings,
                        histogram,
                        clipping,
                    })))
                }
                Request::Preview => {
                    std::thread::sleep(delay);
                    if desired_revision.load(Ordering::Relaxed) != revision {
                        return Ok(Output::Superseded);
                    }
                    let input = input.ok_or_else(|| invalid("RAW decoding has not finished."))?;
                    let Analysis {
                        preview,
                        warnings,
                        histogram,
                        clipping,
                    } = analyze(raw::render(&input, &settings, &cancel)?)?;
                    Ok(Output::Preview {
                        crop: settings.crop,
                        full: full_preview,
                        before: match cached_before {
                            Some(image) => image,
                            None => before(&input, &settings, &cancel)?,
                        },
                        preview,
                        warnings,
                        histogram: Box::new(histogram),
                        clipping,
                    })
                }
                Request::Apply => {
                    let full = full.ok_or_else(|| invalid("RAW decoding has not finished."))?;
                    Ok(Output::Applied {
                        pixels: raw::render(&full, &settings, &cancel)?,
                        settings,
                    })
                }
                Request::Export(path) => {
                    let full = full.ok_or_else(|| invalid("RAW decoding has not finished."))?;
                    export_tiff(&path, &full, &settings, &cancel)?;
                    Ok(Output::Exported(path))
                }
                Request::LoadPreset(path) => {
                    use std::io::Read;
                    let metadata = std::fs::metadata(&path)?;
                    if !metadata.is_file() || metadata.len() > 2 * 1024 * 1024 {
                        return Err(invalid(
                            "Choose a regular RAW preset file no larger than 2 MiB.",
                        ));
                    }
                    let mut bytes = Vec::new();
                    std::fs::File::open(&path)?
                        .take(2 * 1024 * 1024 + 1)
                        .read_to_end(&mut bytes)?;
                    if bytes.len() > 2 * 1024 * 1024 {
                        return Err(invalid(
                            "RAW preset exceeds the 2 MiB limit. Choose a smaller settings file.",
                        ));
                    }
                    let settings: DevelopSettings = serde_json::from_slice(&bytes)?;
                    settings.validate()?;
                    Ok(Output::Preset(settings))
                }
                Request::SavePreset(path) => {
                    settings.validate()?;
                    image_io::export_encoded(&path, &serde_json::to_vec_pretty(&settings)?)?;
                    Ok(Output::SavedPreset(path))
                }
            }
        };
        let launched = cx.spawn_background(operation, move |this,result,cx| {
            let result=result.map_err(|e|invalid(format!("RAW worker failed: {e}. Your existing layers are unchanged. Retry or cancel development."))).and_then(|r|r);
            this.receive_raw(id,revision,preview_request,result);
            this.changed(cx);
        });
        if let Err(error) = launched {
            self.receive_raw(
                id,
                revision,
                preview_request,
                Err(invalid(format!(
                    "Could not start RAW processing: {error}. Existing layers are unchanged."
                ))),
            );
        }
    }
    pub(super) fn receive_raw(
        &mut self,
        id: Uuid,
        revision: u64,
        preview_request: bool,
        result: Result<Output>,
    ) {
        let Some(mut d) = self.develop.take() else {
            return;
        };
        if d.id != id {
            self.develop = Some(d);
            return;
        }
        d.running = false;
        if preview_request && revision != d.revision {
            self.develop = Some(d);
            return;
        }
        match result {
            Ok(Output::Superseded) => {}
            Ok(Output::Loaded(ready)) => {
                d.settings = ready.asset.settings.clone();
                d.ready = Some(*ready);
                d.rendered = Some(revision);
            }
            Ok(Output::Preview {
                crop,
                full,
                preview,
                before,
                warnings,
                histogram,
                clipping,
            }) => {
                if let Some(r) = &mut d.ready {
                    r.before_crop = crop;
                    r.before_full = full;
                    r.preview = preview;
                    r.before = before;
                    r.warnings = warnings;
                    r.histogram = *histogram;
                    r.clipping = clipping;
                }
                d.rendered = Some(revision);
            }
            Ok(Output::Applied { settings, pixels }) => {
                if let Some(r) = &d.ready {
                    let mut asset = r.asset.clone();
                    asset.settings = settings;
                    match self.apply_developed(&d.target, asset, pixels) {
                        Ok(()) => return,
                        Err(e) => d.error = Some(e.to_string()),
                    }
                }
                d.committing = false;
            }
            Ok(Output::Exported(path)) => {
                d.committing = false;
                d.notice = format!("Saved 16-bit TIFF: {}", path.display());
            }
            Ok(Output::SavedPreset(path)) => {
                d.committing = false;
                d.notice = format!("Saved preset: {}", path.display());
            }
            Ok(Output::Preset(settings)) => {
                d.committing = false;
                d.edit(|s| *s = settings);
                d.selected_mask = None;
            }
            Err(error) => {
                if preview_request {
                    // A commit can queue behind this preview. Cancel that request
                    // before unlocking edits so it cannot later apply stale settings.
                    d.request = None;
                }
                d.committing = false;
                d.error = Some(error.to_string());
            }
        }
        self.develop = Some(d);
    }
}

pub(super) fn export_tiff(
    path: &Path,
    full: &DecodedRaw,
    settings: &DevelopSettings,
    cancel: &AtomicBool,
) -> Result<()> {
    use image::ImageEncoder;
    if !path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("tif") || s.eq_ignore_ascii_case("tiff"))
    {
        return Err(invalid(
            "Use a .tif or .tiff filename for 16-bit export. No file was written.",
        ));
    }
    let pixels = raw::render_16(full, settings, cancel)?;
    let mut file = tempfile::NamedTempFile::new_in(
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?;
    let mut encoder = image::codecs::tiff::TiffEncoder::new(file.as_file_mut());
    encoder
        .set_icc_profile(include_bytes!("../../../assets/color/sRGB.icc").to_vec())
        .map_err(image::ImageError::Unsupported)?;
    encoder.write_image(
        bytemuck::cast_slice(pixels.as_raw()),
        pixels.width(),
        pixels.height(),
        image::ExtendedColorType::Rgba16,
    )?;
    if cancel.load(Ordering::Relaxed) {
        return Err(invalid(
            "RAW TIFF export cancelled. The previous destination file is preserved.",
        ));
    }
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| {
        invalid(format!(
            "Could not save {}: {}. Choose another destination and retry.",
            path.display(),
            e.error
        ))
    })?;
    Ok(())
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
