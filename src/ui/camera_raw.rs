//! Camera Raw's grouped controls and draft settings. All edits use the filter transaction.
mod controls;
mod fields;
mod parameters;
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
    pub color: usize,
    pub wheel: usize,
    pub channel: usize,
    pub mixer_page: MixerPage,
    pub point: usize,
}
impl Default for Edit {
    fn default() -> Self {
        Self {
            settings: Default::default(),
            group: Group::Light,
            color: 0,
            wheel: 0,
            channel: 0,
            mixer_page: Default::default(),
            point: 0,
        }
    }
}
impl Editor {
    fn sync_camera_fields(&mut self) {
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
