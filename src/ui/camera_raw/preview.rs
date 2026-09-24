use super::*;
use compositor::camera_raw::Clipping;
use quickgui::{PointerEvent, PointerPhase};
impl Editor {
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
            if self.camera_raw.group == Group::Light {
                options.clipping = match index {
                    0 | 2 | 4 => Some(Clipping::Highlights),
                    3 | 5 => Some(Clipping::Shadows),
                    _ => None,
                };
            }
            if self.camera_raw.group == Group::Detail && index == 3 {
                options.sharpen_mask = true;
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
