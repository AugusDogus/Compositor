//! Camera Raw's grouped controls and draft settings. All edits use the filter transaction.
mod balance;
mod bindings;
mod controls;
mod curve;
mod fields;
mod grading;
#[cfg(test)]
mod graph_tests;
mod groups;
mod histogram;
mod parameters;
mod pointer;
mod preview;
#[cfg(test)]
mod tests;
use super::*;
use compositor::camera_raw::{Group, Settings};
#[derive(Clone, Copy, Default, PartialEq)]
pub(super) enum MixerPage {
    #[default]
    Families,
    Points,
}
pub(super) struct Edit {
    pub settings: Settings,
    pub group: Group,
    groups: crate::ui::dropdown::Dropdown<Group>,
    pub color: usize,
    pub wheel: usize,
    pub channel: usize,
    pub mixer_page: MixerPage,
    pub point: usize,
    curve_page: curve::Page,
    curve_interaction: curve::Interaction,
    curve_selected: Option<usize>,
    grading_page: grading::Page,
    grading_interaction: grading::Interaction,
    tool: pointer::Tool,
    drag: Option<pointer::Drag>,
    pub preview: compositor::camera_raw::Preview,
    pub committing: Option<Settings>,
    readout: Option<[f64; 3]>,
    scope: histogram::Scope,
    balance: balance::Work,
    balance_id: uuid::Uuid,
}
impl Default for Edit {
    fn default() -> Self {
        Self {
            settings: Default::default(),
            group: Group::Light,
            groups: groups::menu(),
            color: 0,
            wheel: 0,
            channel: 0,
            mixer_page: Default::default(),
            point: 0,
            curve_page: Default::default(),
            curve_interaction: Default::default(),
            curve_selected: None,
            grading_page: Default::default(),
            grading_interaction: Default::default(),
            tool: Default::default(),
            drag: None,
            preview: Default::default(),
            committing: None,
            readout: None,
            scope: Default::default(),
            balance: Default::default(),
            balance_id: uuid::Uuid::new_v4(),
        }
    }
}
impl Edit {
    pub(super) fn with_settings(settings: Settings) -> Self {
        Self {
            settings,
            ..Default::default()
        }
    }
}
impl Editor {
    pub(in crate::ui) fn camera_panel_header(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        div()
            .flex_col()
            .gap(10.)
            .child(self.camera_histogram(cx))
            .child(self.camera_raw_controls(cx))
            .child(self.camera_preview_controls(cx))
    }
    fn sync_camera_fields(&mut self) {
        self.camera_raw
            .groups
            .select_id(self.camera_raw.group.name());
        let fields = self.camera_raw.fields();
        if let Some(Form::Edit {
            action: Action::CameraRaw,
            fields: existing,
            ..
        }) = &mut self.modal
        {
            *existing = fields;
        }
        self.refresh_filter();
    }
    fn camera_change(&mut self, change: impl FnOnce(&mut Edit)) {
        if self.filter_applying() {
            return;
        }
        if !matches!(
            self.modal,
            Some(Form::Edit {
                action: Action::CameraRaw,
                ..
            })
        ) {
            return;
        }
        // Numeric drafts are parsed when their input changes. Commands operate on the
        // last valid typed settings and replace drafts, so Reset can recover from invalid input.
        change(&mut self.camera_raw);
        self.sync_camera_fields();
    }
}
