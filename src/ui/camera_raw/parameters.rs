use super::*;
use crate::ui::{parameter_controls::Scale, scalar_controls::Scalar};
impl Editor {
    pub(super) fn reset_camera_parameter(&mut self, index: usize) {
        if self.filter_applying() {
            return;
        }
        if let Some(binding) = self.camera_raw.bindings().get(index) {
            let (_, _, _, default) = binding.metadata();
            self.update_form_field(index, &default.to_string());
        }
    }

    pub(in crate::ui) fn camera_parameter_control(
        &self,
        cx: &mut ViewContext<'_, Self>,
        index: usize,
        value: &str,
    ) -> Option<Element> {
        let (label, min, max, _) = self.camera_raw.bindings().get(index)?.metadata();
        let decimals = if max <= 5. { 2 } else { 0 };
        let ids = [
            "camera-param-0",
            "camera-param-1",
            "camera-param-2",
            "camera-param-3",
            "camera-param-4",
            "camera-param-5",
            "camera-param-6",
            "camera-param-7",
            "camera-param-8",
            "camera-param-9",
            "camera-param-10",
            "camera-param-11",
            "camera-param-12",
            "camera-param-13",
            "camera-param-14",
            "camera-param-15",
            "camera-param-16",
            "camera-param-17",
            "camera-param-18",
            "camera-param-19",
            "camera-param-20",
            "camera-param-21",
            "camera-param-22",
        ];
        let reset =
            move |this: &mut Self, event: &quickgui::MouseDownEvent, cx: &mut EventContext| {
                if event.click_count == 2 {
                    this.reset_camera_parameter(index);
                    cx.prevent_default();
                    this.changed(cx);
                }
            };
        let label_reset = cx.mouse_down_listener(format!("camera-reset-{index}"), reset);
        let slider_reset = cx.mouse_down_listener(*ids.get(index)?, reset);
        Some(
            div()
                .flex_row()
                .items_center()
                .gap(8.)
                .child(
                    text(label)
                        .text_size(12.)
                        .line_height(15.)
                        .w(128.)
                        .flex_shrink_0()
                        .wrap()
                        .on_mouse_down(quickgui::MouseButton::Left, label_reset),
                )
                .child(
                    self.scalar_slider(
                        cx,
                        ids.get(index)?,
                        label,
                        Scalar::Parameter(index, Scale::Linear(decimals)),
                        (min, max),
                        120.,
                    )
                    .on_mouse_down(quickgui::MouseButton::Left, slider_reset),
                )
                .child(
                    self.form_number_input(cx, index, value, usize::from(decimals))
                        .w(56.),
                ),
        )
    }
}
