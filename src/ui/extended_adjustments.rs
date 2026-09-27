//! Small layer transactions for adjustments outside the native macOS inventory.
use super::*;
use compositor::{adjustment::ExtendedAdjustment, document::LayerContent, invalid};
use uuid::Uuid;

pub(super) struct Draft {
    session: Uuid,
    layer: Uuid,
    original: ExtendedAdjustment,
    preview: bool,
}
impl Editor {
    pub(super) fn open_extended_adjustment(
        &mut self,
        create: Option<ExtendedAdjustment>,
    ) -> Result<()> {
        if let Some(settings) = create {
            settings.validate()?;
            self.add_adjustment_content(
                settings.label(),
                LayerContent::ExtendedAdjustment(Box::new(settings)),
            )?;
        }
        let layer = self
            .session()
            .document
            .active_layer()
            .ok_or_else(|| invalid("Select an adjustment layer to edit."))?;
        let LayerContent::ExtendedAdjustment(settings) = &layer.content else {
            return Err(invalid("Select a Linux adjustment layer."));
        };
        let (layer, parent, settings) = (layer.id, layer.parent, **settings);
        self.session_mut()
            .begin(format!("Edit {} Adjustment", settings.label()))?;
        if let Some(parent) = parent {
            self.session_mut().collapsed.remove(&parent);
        }
        self.tools.mask_target = false;
        self.extended_edit = Some(Draft {
            session: self.session().id,
            layer,
            original: settings,
            preview: true,
        });
        let fields = match settings {
            ExtendedAdjustment::ShadowsHighlights(s) => {
                super::filter_controls::shadows_highlights::fields(s)
            }
            ExtendedAdjustment::Posterize(s) => super::filter_controls::posterize::fields(s),
            ExtendedAdjustment::Vibrance(s) => super::filter_controls::vibrance::fields(s),
            ExtendedAdjustment::Threshold(s) => super::filter_controls::threshold::fields(s),
            ExtendedAdjustment::PhotoFilter(s) => super::photo_filter_controls::fields(s),
            ExtendedAdjustment::ChannelMixer(s) => super::channel_mixer_controls::fields(s),
            ExtendedAdjustment::SelectiveColor(s) => super::selective_color_controls::fields(s),
        };
        self.modal = Some(Form::Edit {
            title: settings.label(),
            action: Action::EditExtendedAdjustment,
            fields,
            error: String::new(),
        });
        Ok(())
    }
    pub(super) fn extended_control_action(&self, action: Action) -> Action {
        if !matches!(action, Action::EditExtendedAdjustment) {
            return action;
        }
        match self.extended_edit.as_ref().map(|draft| draft.original) {
            Some(ExtendedAdjustment::ShadowsHighlights(settings)) => {
                Action::Filter(compositor::filters::Filter::ShadowsHighlights(settings))
            }
            Some(ExtendedAdjustment::Vibrance(settings)) => {
                Action::Filter(compositor::filters::Filter::Vibrance(settings))
            }
            Some(ExtendedAdjustment::Posterize(settings)) => {
                Action::Filter(compositor::filters::Filter::Posterize(settings))
            }
            Some(ExtendedAdjustment::Threshold(settings)) => {
                Action::Filter(compositor::filters::Filter::Threshold(settings))
            }
            Some(ExtendedAdjustment::PhotoFilter(settings)) => {
                Action::Filter(compositor::filters::Filter::PhotoFilter(settings))
            }
            Some(ExtendedAdjustment::ChannelMixer(settings)) => {
                Action::Filter(compositor::filters::Filter::ChannelMixer(settings))
            }
            Some(ExtendedAdjustment::SelectiveColor(settings)) => {
                Action::Filter(compositor::filters::Filter::SelectiveColor(settings))
            }
            None => action,
        }
    }
    fn extended_settings(&self) -> Result<ExtendedAdjustment> {
        let draft = self
            .extended_edit
            .as_ref()
            .ok_or_else(|| invalid("The adjustment editor has closed."))?;
        let Some(Form::Edit {
            action: Action::EditExtendedAdjustment,
            fields,
            ..
        }) = &self.modal
        else {
            return Err(invalid(
                "Close the color picker before applying the adjustment.",
            ));
        };
        let values: Vec<_> = fields.iter().map(|(_, value)| value.clone()).collect();
        match draft.original {
            ExtendedAdjustment::ShadowsHighlights(_) => {
                super::filter_controls::shadows_highlights::parse(&values)
                    .map(ExtendedAdjustment::ShadowsHighlights)
            }
            ExtendedAdjustment::Vibrance(_) => {
                super::filter_controls::vibrance::parse(&values).map(ExtendedAdjustment::Vibrance)
            }
            ExtendedAdjustment::Posterize(_) => {
                super::filter_controls::posterize::parse(&values).map(ExtendedAdjustment::Posterize)
            }
            ExtendedAdjustment::Threshold(_) => {
                super::filter_controls::threshold::parse(&values).map(ExtendedAdjustment::Threshold)
            }
            ExtendedAdjustment::SelectiveColor(_) => {
                super::selective_color_controls::parse(&values)
                    .map(ExtendedAdjustment::SelectiveColor)
            }
            ExtendedAdjustment::ChannelMixer(_) => {
                super::channel_mixer_controls::parse(&values).map(ExtendedAdjustment::ChannelMixer)
            }
            ExtendedAdjustment::PhotoFilter(original) => {
                super::photo_filter_controls::parse(&values, original)
                    .map(ExtendedAdjustment::PhotoFilter)
            }
        }
    }
    fn set_extended_preview(&mut self, settings: ExtendedAdjustment, show: bool) -> Result<()> {
        let draft = self
            .extended_edit
            .as_ref()
            .ok_or_else(|| invalid("The adjustment editor has closed."))?;
        if self.session().id != draft.session {
            return Err(invalid(
                "The adjustment belongs to another project. Cancel and reopen its settings.",
            ));
        }
        let (id, settings) = (draft.layer, if show { settings } else { draft.original });
        let layer = self
            .session_mut()
            .document
            .layers
            .iter_mut()
            .find(|l| l.id == id)
            .ok_or_else(|| {
                invalid("The adjustment layer is missing. Cancel this edit to restore it.")
            })?;
        layer.content = LayerContent::ExtendedAdjustment(Box::new(settings));
        Ok(())
    }
    pub(super) fn refresh_extended_adjustment(&mut self) {
        if !matches!(
            self.modal,
            Some(Form::Edit {
                action: Action::EditExtendedAdjustment,
                ..
            })
        ) {
            return;
        }
        let result = self.extended_settings().and_then(|settings| {
            let show = self.extended_edit.as_ref().is_some_and(|e| e.preview);
            self.set_extended_preview(settings, show)
        });
        if let Some(Form::Edit { error, .. }) = &mut self.modal {
            *error = result.err().map_or_else(String::new, |e| e.to_string());
        }
    }
    pub(super) fn finish_extended_adjustment(&mut self) -> Result<()> {
        let settings = self.extended_settings()?;
        self.set_extended_preview(settings, true)?;
        self.session_mut().commit()?;
        self.extended_edit = None;
        Ok(())
    }
    pub(super) fn cancel_extended_adjustment(&mut self) {
        if let Some(draft) = self.extended_edit.take()
            && let Some(session) = self
                .tabs
                .iter_mut()
                .find(|tab| tab.id == draft.session)
                .and_then(ProjectTab::history_session_mut)
        {
            session.cancel();
        }
    }
    pub(super) fn extended_preview_control(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        self.check_control(
            "Preview",
            self.extended_edit
                .as_ref()
                .is_some_and(|draft| draft.preview),
        )
        .on_click(cx.listener("extended-adjustment-preview", |this, cx| {
            if let Some(draft) = &mut this.extended_edit {
                draft.preview = !draft.preview;
            }
            this.refresh_extended_adjustment();
            this.changed(cx);
        }))
    }
}

#[cfg(test)]
mod tests;
