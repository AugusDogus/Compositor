//! Dispatch checks eligibility before it resolves any pending edits.
use super::*;

impl Action {
    pub(super) fn is_project_operation(self) -> bool {
        matches!(
            self,
            Self::Trim
                | Self::CanvasSize
                | Self::ImageSize
                | Self::Save
                | Self::SaveAs
                | Self::ExportPsd
                | Self::ExportArtboards
                | Self::ExportLayers
                | Self::ExportSizes
                | Self::ExportPng
                | Self::ExportTiff
                | Self::ExportWebp
                | Self::ExportAvif
                | Self::ExportGif
                | Self::ExportJpeg
                | Self::ExportJpegFile
                | Self::Import
        )
    }
}

impl Editor {
    pub(super) fn has_conversion_prompt(&self) -> bool {
        self.psd_conversion.is_some() || !self.authoring_copies.is_empty()
    }

    pub(super) fn can_start_project_operation(&self) -> bool {
        self.develop.is_none()
            && self.layout_drag.is_none()
            && !self.has_conversion_prompt()
            && self.errors.is_empty()
            && !self.pending
            && self.retained_panel.is_none()
            && self.gesture.is_none()
            && self.rename.is_none()
            && !self.editing_adjustment_layer()
            && !self
                .adjustment_edit
                .as_ref()
                .is_some_and(|edit| edit.settings.kind == Kind::Levels)
    }

    pub(super) fn action_available(&self, action: Action) -> bool {
        if self.develop.is_some()
            || self.layout_drag.is_some()
            || self.has_conversion_prompt()
            || !self.errors.is_empty()
            || self.pending
            || self.gesture.is_some()
            || self.retained_panel.is_some()
        {
            return false;
        }
        if !self.has_document()
            && !matches!(
                action,
                Action::Undo
                    | Action::Redo
                    | Action::New
                    | Action::Open
                    | Action::OpenPsd
                    | Action::OpenRaw
                    | Action::OpenClipboard
                    | Action::Import
                    | Action::Paste
                    | Action::CloseTab
                    | Action::Color
            )
        {
            return false;
        }
        if matches!(action, Action::ExportLayers)
            && !compositor::layer_export::has_exportable_layers(self.session().committed_document())
        {
            return false;
        }
        if matches!(action, Action::ExportArtboards)
            && !self.session().committed_document().layers.iter().any(|l| {
                l.visible && matches!(l.content, compositor::document::LayerContent::Artboard(_))
            })
        {
            return false;
        }
        match action {
            Action::PathShapeSettings | Action::RasterizePathShape => {
                self.can_edit_layers()
                    && self
                        .session()
                        .document
                        .active_layer()
                        .is_some_and(|l| l.is_path_shape())
            }
            Action::EditPathShape(id) => {
                self.can_edit_layers()
                    && self
                        .session()
                        .document
                        .layer(id)
                        .is_some_and(|l| l.is_path_shape())
            }
            Action::ArtboardSettings | Action::EditArtboard(_) => {
                self.can_edit_layers() && self.active_artboard().is_some()
            }
            Action::ArtboardFromLayers => {
                self.can_edit_layers() && !self.session().document.selected.is_empty()
            }
            action if action.is_project_operation() => self.can_start_project_operation(),
            Action::New | Action::Open | Action::OpenPsd | Action::OpenRaw | Action::CloseTab => {
                self.can_switch_projects()
            }
            Action::OpenClipboard => self.can_switch_projects(),
            Action::Undo => self.can_undo(),
            Action::Redo => self.can_redo(),
            Action::Fade => self.can_fade(),
            Action::History => self.modal.is_none() && self.can_browse_history(),
            Action::Duplicate => self.can_duplicate_layer(),
            Action::DevelopRaw | Action::RasterizeRaw => {
                self.can_edit_layers()
                    && self
                        .session()
                        .document
                        .active_layer()
                        .is_some_and(|layer| layer.raw.is_some())
            }
            Action::ClearGuides => {
                !self.tools.layout.locked
                    && !self.session().document.guides.is_empty()
                    && self.can_edit_layers()
            }
            Action::FeatherSelection => self.can_modify_selection(),
            Action::AdjustPixels(_) | Action::BrightnessContrast | Action::RemoveBackground => {
                self.can_adjust_colors()
            }
            Action::Filter(compositor::filters::Filter::Vignette(_)) => {
                !self.tools.mask_target && self.can_edit_pixels()
            }
            Action::Filter(compositor::filters::Filter::ContentFill) => {
                self.can_content_aware_fill()
            }
            Action::CameraRaw | Action::Filter(_) | Action::Dither => self.can_adjust_colors(),
            Action::InvertPixels => self.can_invert(),
            Action::Fill | Action::FillBackground => self.can_edit_pixels(),
            Action::Clear => self.can_edit_pixels() && self.session().document.selection.is_some(),
            Action::Copy | Action::CopyMerged | Action::Cut | Action::Paste => {
                self.clipboard_available(action)
            }
            action if action.requires_layer_edit() || action.edits_selection() => {
                self.can_edit_layers()
            }
            _ => true,
        }
    }
}
