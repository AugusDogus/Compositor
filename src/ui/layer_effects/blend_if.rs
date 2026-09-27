use super::*;
use compositor::blend_if::{Range, Settings};

#[derive(Clone, Copy)]
pub(in crate::ui) enum Handle {
    Source(usize),
    Underlying(usize),
}
impl Handle {
    pub(in crate::ui) fn value(self, edit: &EffectsEditor) -> f64 {
        let settings = edit.blend_if.unwrap_or_default();
        let (range, index) = match self {
            Self::Source(i) => (settings.source, i),
            Self::Underlying(i) => (settings.underlying, i),
        };
        f64::from(range.endpoints()[index])
    }
    pub(in crate::ui) fn set(self, edit: &mut EffectsEditor, value: f64) {
        let Some(settings) = &mut edit.blend_if else {
            return;
        };
        let (range, index) = match self {
            Self::Source(i) => (&mut settings.source, i),
            Self::Underlying(i) => (&mut settings.underlying, i),
        };
        let mut points = range.endpoints();
        let value = value.round().clamp(0., 255.) as u8;
        // Keep the other split handle fixed while dragging this one.
        points[index] = if index % 2 == 0 {
            value.min(points[index + 1])
        } else {
            value.max(points[index - 1])
        };
        if let Ok(updated) = Range::from_endpoints(points) {
            *range = updated;
        }
    }
}

impl Editor {
    pub(super) fn blend_if_controls(
        &self,
        cx: &mut ViewContext<'_, Self>,
        edit: &EffectsEditor,
    ) -> Element {
        let mut controls = div()
            .flex_col()
            .gap(6.)
            .child(text("Blend If: Gray").text_size(13.))
            .child(
                text("Split each pair to fade dark or light tones.")
                    .wrap()
                    .text_size(12.),
            );
        for (source, label) in [(true, "This layer"), (false, "Underlying layers")] {
            controls = controls.child(text(label).text_size(12.));
            for (index, label) in ["Black start", "Black end", "White start", "White end"]
                .into_iter()
                .enumerate()
            {
                let handle = if source {
                    Handle::Source(index)
                } else {
                    Handle::Underlying(index)
                };
                let id = format!(
                    "blend-if-{}-{index}",
                    if source { "source" } else { "underlying" }
                );
                let scalar = super::super::scalar_controls::Scalar::BlendIf(handle);
                controls = controls.child(
                    div()
                        .flex_row()
                        .items_center()
                        .gap(8.)
                        .child(text(label).w(100.).text_size(12.))
                        .child(self.scalar_slider(cx, id.clone(), label, scalar, (0., 255.), 165.))
                        .child(
                            self.text_field(format!("{:.0}", handle.value(edit)))
                                .id(format!("{id}-value"))
                                .w(65.)
                                .h(26.)
                                .accessibility_label(format!(
                                    "{} {label}",
                                    if source {
                                        "This layer"
                                    } else {
                                        "Underlying layers"
                                    }
                                ))
                                .on_input(cx.input_listener(
                                    format!("{id}-value"),
                                    move |this, value, cx| {
                                        if let Ok(number) = value.parse::<u8>() {
                                            this.change_effect(|edit| {
                                                handle.set(edit, f64::from(number))
                                            });
                                            this.changed(cx);
                                        }
                                    },
                                )),
                        ),
                );
            }
        }
        controls.child(
            self.control("Reset ranges")
                .id("blend-if-reset")
                .on_click(cx.listener("blend-if-reset", |this, cx| {
                    this.change_effect(|edit| {
                        let enabled = edit.blend_if.is_none_or(|s| s.enabled);
                        edit.blend_if = Some(Settings {
                            enabled,
                            ..Default::default()
                        });
                    });
                    this.changed(cx);
                })),
        )
    }
}

#[cfg(test)]
mod tests;
