//! Isolated, cancellable RAW editing. A document changes only after Develop.
mod canvas;
mod controls;
mod dialogs;
mod display;
mod fields;
mod panels;
mod presets;
mod worker;
use display::DisplayImage;
#[cfg(test)]
mod tests;

use super::*;
use compositor::{
    document::Layer,
    invalid,
    raw::{self, DecodedRaw, DevelopSettings, RawAsset},
};
use image::RgbaImage;
use std::{
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::Instant,
};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub(super) enum Target {
    New,
    Insert { tab: Uuid, center: Option<[f64; 2]> },
    Existing { tab: Uuid, layer: Uuid },
}

pub(super) enum Source {
    Path(PathBuf),
    Asset(Arc<RawAsset>),
}

#[derive(Clone, Copy, PartialEq)]
enum Compare {
    Edited,
    Original,
    Split,
    SideBySide,
}

struct Ready {
    asset: RawAsset,
    full: Arc<DecodedRaw>,
    proxy: Arc<DecodedRaw>,
    before: DisplayImage,
    before_crop: [f32; 4],
    before_full: bool,
    preview: DisplayImage,
    warnings: DisplayImage,
    histogram: [[u32; 256]; 3],
    clipping: [f32; 2],
}

enum Request {
    Load(Source),
    Preview,
    Apply,
    Export(PathBuf),
    SavePreset(PathBuf),
    LoadPreset(PathBuf),
}

pub(super) struct Develop {
    id: Uuid,
    target: Target,
    title: String,
    settings: DevelopSettings,
    ready: Option<Ready>,
    request: Option<Request>,
    running: bool,
    committing: bool,
    cancel: Arc<AtomicBool>,
    revision: u64,
    desired_revision: Arc<AtomicU64>,
    rendered: Option<u64>,
    last_change: Instant,
    error: Option<String>,
    notice: String,
    panel: usize,
    compare: Compare,
    split: f32,
    full_preview: bool,
    clipping: bool,
    fit: bool,
    zoom: f32,
    pan: [f32; 2],
    curve_channel: usize,
    curve_knot: Option<usize>,
    hsl_band: usize,
    selected_mask: Option<usize>,
    draw_mask: bool,
    show_mask: bool,
    picker: bool,
    last_brush: Option<raw::Point>,
    undo: Vec<DevelopSettings>,
    redo: Vec<DevelopSettings>,
    gesture_start: Option<DevelopSettings>,
    numeric_draft: Option<fields::NumericDraft>,
}

impl Develop {
    fn new(source: Source, target: Target) -> Self {
        let (title, settings) = match &source {
            Source::Path(path) => (
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                DevelopSettings::default(),
            ),
            Source::Asset(asset) => (asset.filename.clone(), asset.settings.clone()),
        };
        Self {
            id: Uuid::new_v4(),
            target,
            title,
            settings,
            ready: None,
            request: Some(Request::Load(source)),
            running: false,
            committing: false,
            cancel: Arc::new(AtomicBool::new(false)),
            revision: 0,
            desired_revision: Arc::new(AtomicU64::new(0)),
            rendered: None,
            last_change: Instant::now(),
            error: None,
            notice: String::new(),
            panel: 0,
            compare: Compare::Edited,
            split: 0.5,
            full_preview: false,
            clipping: false,
            fit: true,
            zoom: 1.,
            pan: [0.; 2],
            curve_channel: 0,
            curve_knot: None,
            hsl_band: 0,
            selected_mask: None,
            draw_mask: false,
            show_mask: true,
            picker: false,
            last_brush: None,
            undo: Vec::new(),
            redo: Vec::new(),
            gesture_start: None,
            numeric_draft: None,
        }
    }
    fn changed(&mut self) {
        self.revision += 1;
        self.desired_revision
            .store(self.revision, Ordering::Relaxed);
        self.last_change = Instant::now();
        self.error = None;
        self.notice.clear();
    }
    fn begin_gesture(&mut self) {
        if self.gesture_start.is_none() {
            self.gesture_start = Some(self.settings.clone());
        }
    }
    fn finish_gesture(&mut self, cancel: bool) {
        if let Some(previous) = self.gesture_start.take() {
            if cancel {
                self.settings = previous;
                self.changed();
            } else if previous != self.settings {
                self.push_undo(previous);
            }
        }
    }
    fn push_undo(&mut self, previous: DevelopSettings) {
        self.undo.push(previous);
        if self.undo.len() > 64 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }
    fn edit(&mut self, change: impl FnOnce(&mut DevelopSettings)) {
        if self.committing || self.ready.is_none() {
            return;
        }
        let previous = self.settings.clone();
        change(&mut self.settings);
        if let Err(error) = self.settings.validate() {
            self.settings = previous;
            self.error = Some(error.to_string());
            return;
        }
        if previous != self.settings {
            if self.gesture_start.is_none() {
                self.push_undo(previous);
            }
            self.changed();
        }
    }
    fn history(&mut self, redo: bool) {
        if self.committing {
            return;
        }
        self.finish_gesture(false);
        let state = if redo {
            self.redo.pop()
        } else {
            self.undo.pop()
        };
        if let Some(settings) = state {
            let previous = std::mem::replace(&mut self.settings, settings);
            if redo {
                self.undo.push(previous);
            } else {
                self.redo.push(previous);
            }
            self.selected_mask = None;
            self.changed();
        }
    }
}

impl Drop for Develop {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Editor {
    pub(super) fn queue_raw(&mut self, path: PathBuf, target: Target) {
        self.raw_queue.push_back((Source::Path(path), target));
    }
    pub(super) fn start_develop_layer(&mut self, id: Uuid) -> Result<()> {
        self.finish_pending_edits()?;
        let layer = self
            .session()
            .document
            .layer(id)
            .ok_or_else(|| invalid("Select a RAW layer to develop."))?;
        let asset = layer
            .raw
            .clone()
            .ok_or_else(|| invalid("This layer has no embedded RAW source."))?;
        self.develop = Some(Develop::new(
            Source::Asset(asset),
            Target::Existing {
                tab: self.tabs[self.current].id,
                layer: id,
            },
        ));
        self.gesture = None;
        self.tools.mask_target = false;
        Ok(())
    }
    fn cancel_develop(&mut self) {
        self.develop = None;
        self.status = "RAW development cancelled. Existing layers are unchanged.".into();
    }
    fn apply_developed(
        &mut self,
        target: &Target,
        asset: RawAsset,
        pixels: RgbaImage,
    ) -> Result<()> {
        let layer = || {
            let mut layer = Layer::blank(
                std::path::Path::new(&asset.filename)
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                pixels.width(),
                pixels.height(),
            );
            layer.content =
                compositor::document::LayerContent::Raster(Some(Arc::new(pixels.clone())));
            layer.raw = Some(Arc::new(asset.clone()));
            layer
        };
        match target {
            Target::New => {
                let mut document = Document::new(pixels.width(), pixels.height())?;
                document.layers.clear();
                document.active = None;
                document.selected.clear();
                document.add(layer())?;
                document.validate()?;
                if self.has_document() {
                    self.add_empty_tab();
                }
                self.tabs[self.current].create_document(document)?;
            }
            Target::Insert { tab, .. } | Target::Existing { tab, .. } => {
                let index = self.tabs.iter().position(|t| t.id == *tab).ok_or_else(|| {
                    invalid("The destination project has closed. Cancel and reopen this RAW file.")
                })?;
                let size = pixels.dimensions();
                let edit = |document: &mut Document| {
                    match target {
                        Target::Insert { center, .. } => {
                            let mut layer = layer();
                            let center = center.unwrap_or([
                                document.width as f64 / 2.,
                                document.height as f64 / 2.,
                            ]);
                            layer.transform.origin = [
                                center[0] - layer.transform.size[0] / 2.,
                                center[1] - layer.transform.size[1] / 2.,
                            ];
                            layer.parent = document
                                .active_layer()
                                .and_then(|l| if l.is_group() { Some(l.id) } else { l.parent });
                            document.add(layer)?;
                        }
                        Target::Existing { layer, .. } => {
                            let target = document.layers.iter_mut().find(|l|l.id==*layer).ok_or_else(|| invalid("The RAW layer is no longer available. Existing edits are unchanged."))?;
                            raw::update_layer(target, asset.clone(), pixels.clone())?;
                        }
                        Target::New => unreachable!(),
                    }
                    document.validate()
                };
                self.tabs[index].edit_or_create(
                    "Develop RAW",
                    || {
                        let mut d = Document::new(size.0, size.1)?;
                        d.layers.clear();
                        d.active = None;
                        d.selected.clear();
                        Ok(d)
                    },
                    edit,
                )?;
                self.activate_tab(index);
            }
        }
        self.tools.tool = Tool::Move;
        self.tools.mask_target = false;
        self.status = "RAW developed. Double-click the layer to edit its settings.".into();
        Ok(())
    }
}
