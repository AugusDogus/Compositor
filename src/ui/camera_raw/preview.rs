use super::bindings::Binding;
use super::*;
use compositor::camera_raw::{
    Clipping,
    scalars::{DetailParameter, LightParameter},
};
use quickgui::{PointerEvent, PointerPhase};
impl Editor {
    pub(in crate::ui) fn camera_preview_modifiers(&mut self, modifiers: Modifiers) {
        if !modifiers.contains(Modifiers::ALT)
            && (self.camera_raw.preview.clipping.is_some() || self.camera_raw.preview.sharpen_mask)
        {
            self.camera_raw.preview.clipping = None;
            self.camera_raw.preview.sharpen_mask = false;
            self.refresh_filter();
        }
    }

    pub(in crate::ui) fn camera_slider_preview(
        &mut self,
        scalar: super::super::scalar_controls::Scalar,
        event: &PointerEvent,
    ) {
        if !matches!(
            self.modal,
            Some(Form::Edit {
                action: Action::CameraRaw,
                ..
            })
        ) {
            return;
        }
        let mut options = self.camera_raw.preview;
        options.clipping = None;
        options.sharpen_mask = false;
        if matches!(event.phase, PointerPhase::Down | PointerPhase::Move)
            && event.modifiers.contains(Modifiers::ALT)
            && let super::super::scalar_controls::Scalar::Parameter(index, _) = scalar
        {
            match self.camera_raw.bindings().get(index) {
                Some(Binding::Light(parameter)) => {
                    options.clipping = match parameter.id {
                        LightParameter::Exposure
                        | LightParameter::Highlights
                        | LightParameter::Whites => Some(Clipping::Highlights),
                        LightParameter::Shadows | LightParameter::Blacks => Some(Clipping::Shadows),
                        LightParameter::Contrast => None,
                    };
                }
                Some(Binding::Detail(parameter)) => {
                    options.sharpen_mask = parameter.id == DetailParameter::SharpenMasking;
                }
                _ => {}
            }
        }
        if options != self.camera_raw.preview {
            self.camera_raw.preview = options;
            self.refresh_filter();
        }
    }
    pub(in crate::ui) fn camera_preview_controls(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let options = self.camera_raw.preview;
        let mut controls = div()
            .flex_row()
            .flex_wrap()
            .gap(8.)
            .child(
                Self::check_control("Shadow clipping", options.shadow_overlay).on_click(
                    cx.listener("camera-shadow-overlay", |this, cx| {
                        this.camera_raw.preview.shadow_overlay =
                            !this.camera_raw.preview.shadow_overlay;
                        this.refresh_filter();
                        this.changed(cx);
                    }),
                ),
            )
            .child(
                Self::check_control("Highlight clipping", options.highlight_overlay).on_click(
                    cx.listener("camera-highlight-overlay", |this, cx| {
                        this.camera_raw.preview.highlight_overlay =
                            !this.camera_raw.preview.highlight_overlay;
                        this.refresh_filter();
                        this.changed(cx);
                    }),
                ),
            );
        if self.camera_raw.group == Group::Mixer
            && self.camera_raw.mixer_page == MixerPage::Points
            && !self.camera_raw.settings.mixer.points.is_empty()
        {
            controls = controls.child(
                Self::check_control("Visualize range", options.point_color.is_some()).on_click(
                    cx.listener("camera-point-visualize", |this, cx| {
                        this.camera_raw.preview.point_color =
                            if this.camera_raw.preview.point_color.is_some() {
                                None
                            } else {
                                Some(this.camera_raw.point)
                            };
                        this.refresh_filter();
                        this.changed(cx);
                    }),
                ),
            );
        }
        controls
    }
}
