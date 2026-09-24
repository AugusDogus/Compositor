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
                | Self::ExportPng
                | Self::ExportTiff
                | Self::ExportWebp
                | Self::ExportJpeg
                | Self::ExportJpegFile
                | Self::Import
        )
    }
}

impl Editor {
    pub(super) fn can_start_project_operation(&self) -> bool {
        self.develop.is_none()
            && self.layout_drag.is_none()
            && self.psd_conversion.is_none()
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
            || self.psd_conversion.is_some()
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
        match action {
            action if action.is_project_operation() => self.can_start_project_operation(),
            Action::New | Action::Open | Action::OpenPsd | Action::OpenRaw | Action::CloseTab => {
                self.can_switch_projects()
            }
            Action::OpenClipboard => self.can_switch_projects(),
            Action::Undo => self.can_undo(),
            Action::Redo => self.can_redo(),
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
            Action::AdjustPixels(_) | Action::RemoveBackground => self.can_adjust_colors(),
            Action::Filter(compositor::filters::Filter::ContentFill) => {
                self.can_content_aware_fill()
            }
            Action::CameraRaw | Action::Filter(_) => self.can_adjust_colors(),
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
