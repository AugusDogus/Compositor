//! Camera Raw's grouped controls and draft settings. All edits use the filter transaction.
mod balance;
mod controls;
mod fields;
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
        // Preserve the current group's draft before switching panels or editing switches.
        let values = match &self.modal {
            Some(Form::Edit {
                action: Action::CameraRaw,
                fields,
                ..
            }) => fields.iter().map(|(_, v)| v.clone()).collect::<Vec<_>>(),
            _ => return,
        };
        if let Err(error) = self.camera_raw.parse_fields(&values) {
            if let Some(Form::Edit { error: target, .. }) = &mut self.modal {
                *target = error.to_string();
            }
            return;
        }
        change(&mut self.camera_raw);
        self.sync_camera_fields();
    }
}
