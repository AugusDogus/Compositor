//! Hue/saturation wheels with independent luminance controls for the tonal ranges.
use super::*;
use compositor::camera_raw::Wheel;
use quickgui::{MouseButton, PointerEvent, PointerPhase};
#[derive(Clone, Copy, Default, PartialEq)]
pub(super) enum Page {
    #[default]
    ThreeWay,
    Single,
}
#[derive(Default)]
pub(super) enum Interaction {
    #[default]
    Idle,
    Dragging {
        wheel: usize,
        original: Wheel,
    },
    Luminance {
        wheel: usize,
        original: Wheel,
    },
}
fn value(event: &PointerEvent) -> [f64; 2] {
    let side = f64::from(event.size.width.min(event.size.height));
    let radius = (side / 2. - 6.).max(1.);
    let dx = f64::from(event.local_position.x) - side / 2.;
    let dy = side / 2. - f64::from(event.local_position.y);
    [
        dy.atan2(dx).to_degrees().rem_euclid(360.),
        (dx.hypot(dy) / radius * 100.).min(100.),
    ]
}
impl Editor {
    pub(super) fn camera_grading_editor(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let three = self.camera_raw.grading_page == Page::ThreeWay;
        let mut row = div().flex_row().justify_center().gap(8.);
        let indices = if three {
            vec![0, 1, 2]
        } else {
            vec![self.camera_raw.wheel]
        };
        for i in indices {
            row = row.child(self.camera_grade_wheel(cx, i, if three { 96. } else { 164. }));
        }
        div()
            .flex_col()
            .gap(8.)
            .child(
                Self::check_control("Three-way", three).on_click(cx.listener(
                    "camera-grading-three",
                    |this, cx| {
                        this.camera_change(|e| e.grading_page = Page::ThreeWay);
                        this.changed(cx);
                    },
                )),
            )
            .child(row)
            .child(
                text("Drag to set hue and saturation. Reset restores a neutral tint.")
                    .text_size(11.)
                    .wrap(),
            )
    }
    fn camera_grade_wheel(
        &self,
        cx: &mut ViewContext<'_, Self>,
        index: usize,
        side: f32,
    ) -> Element {
        let wheel = self.camera_raw.settings.grading.wheels[index].clone();
        let readout = format!("{:.0}° · {:.0}%", wheel.hue, wheel.saturation);
        let hue = wheel.hue;
        let saturation = wheel.saturation;
        let shader = self.color_picker_shader.clone();
        let graph = quickgui::canvas(move |bounds, painter| {
            let center = quickgui::Point::new(bounds.width / 2., bounds.height / 2.);
            let radius = bounds.width.min(bounds.height) / 2. - 6.;
            painter.paint_shader(
                quickgui::Rect::new(6., 6., radius * 2., radius * 2.),
                &shader,
                quickgui::ShaderParameters::new().vector(0, [0., 2., 0., 0.]),
            );
            let angle = hue.to_radians();
            let distance = radius * saturation as f32 / 100.;
            let x = center.x + distance * angle.cos() as f32;
            let y = center.y - distance * angle.sin() as f32;
            painter.fill_rounded_rect(
                quickgui::Rect::new(x - 5., y - 5., 10., 10.),
                5.,
                Color::BLACK,
            );
            painter.fill_rounded_rect(
                quickgui::Rect::new(x - 3.5, y - 3.5, 7., 7.),
                3.5,
                Color::WHITE,
            );
        })
        .id(format!("camera-grading-wheel-{index}"))
        .w(side)
        .h(side)
        .flex_shrink_0()
        .on_mouse_down(
            MouseButton::Left,
            cx.mouse_down_listener(
                format!("camera-grading-wheel-{index}"),
                move |this, event, cx| {
                    if event.click_count == 2 {
                        this.camera_change(|e| {
                            let w = &mut e.settings.grading.wheels[index];
                            w.hue = 0.;
                            w.saturation = 0.;
                            e.grading_interaction = Interaction::Idle;
                        });
                        cx.prevent_default();
                        this.changed(cx);
                    }
                },
            ),
        )
        .on_pointer(cx.pointer_listener(
            format!("camera-grading-wheel-{index}"),
            move |this, event, cx| {
                this.camera_grading_pointer(index, event);
                this.changed(cx);
            },
        ));
        let luminance = wheel.luminance;
        let track = quickgui::canvas(move |bounds, painter| {
            painter.fill_rounded_rect(
                quickgui::Rect::new(0., bounds.height / 2. - 2., bounds.width, 4.),
                2.,
                Color::rgb8(100, 100, 100),
            );
            let x = ((luminance + 100.) / 200.) as f32 * (bounds.width - 8.) + 4.;
            painter.fill_rounded_rect(
                quickgui::Rect::new(x - 4., bounds.height / 2. - 4., 8., 8.),
                4.,
                Color::WHITE,
            );
        })
        .id(format!("camera-grading-luminance-{index}"))
        .w(side)
        .h(22.)
        .on_pointer(cx.pointer_listener(
            format!("camera-grading-luminance-{index}"),
            move |this, event, cx| {
                this.camera_luminance_pointer(index, event);
                this.changed(cx);
            },
        ));
        div()
            .flex_col()
            .items_center()
            .gap(3.)
            .w(side)
            .child(text(["Shadows", "Midtones", "Highlights", "Global"][index]).text_size(12.))
            .child(graph)
            .child(text(readout).text_size(11.))
            .child(track)
            .child(Self::control("Reset").on_click(cx.listener(
                format!("camera-grading-reset-{index}"),
                move |this, cx| {
                    this.camera_change(|e| {
                        let w = &mut e.settings.grading.wheels[index];
                        w.hue = 0.;
                        w.saturation = 0.;
                    });
                    this.changed(cx);
                },
            )))
    }
    pub(super) fn camera_luminance_pointer(&mut self, index: usize, event: &PointerEvent) {
        if event.button != MouseButton::Left || self.filter_applying() {
            return;
        }
        self.camera_change(|edit| {
            if event.phase == PointerPhase::Down {
                edit.grading_interaction = Interaction::Luminance {
                    wheel: index,
                    original: edit.settings.grading.wheels[index].clone(),
                };
                edit.wheel = index;
            }
            if let Interaction::Luminance { wheel, .. } = &edit.grading_interaction
                && matches!(
                    event.phase,
                    PointerPhase::Down | PointerPhase::Move | PointerPhase::Up
                )
            {
                edit.settings.grading.wheels[*wheel].luminance =
                    ((f64::from(event.local_position.x) - 4.)
                        / f64::from((event.size.width - 8.).max(1.))
                        * 200.
                        - 100.)
                        .clamp(-100., 100.);
            }
            if matches!(event.phase, PointerPhase::Up | PointerPhase::Cancel) {
                let interaction = std::mem::take(&mut edit.grading_interaction);
                if event.phase == PointerPhase::Cancel
                    && let Interaction::Luminance { wheel, original } = interaction
                {
                    edit.settings.grading.wheels[wheel] = original;
                }
            }
        });
    }
    pub(super) fn camera_grading_pointer(&mut self, index: usize, event: &PointerEvent) {
        if event.button != MouseButton::Left || self.filter_applying() {
            return;
        }
        self.camera_change(|edit| {
            if event.phase == PointerPhase::Down {
                edit.grading_interaction = Interaction::Dragging {
                    wheel: index,
                    original: edit.settings.grading.wheels[index].clone(),
                };
                edit.wheel = index;
            }
            if let Interaction::Dragging { wheel, .. } = &edit.grading_interaction
                && matches!(
                    event.phase,
                    PointerPhase::Down | PointerPhase::Move | PointerPhase::Up
                )
            {
                let [hue, saturation] = value(event);
                let w = &mut edit.settings.grading.wheels[*wheel];
                w.hue = hue;
                w.saturation = saturation;
            }
            if matches!(event.phase, PointerPhase::Up | PointerPhase::Cancel) {
                let interaction = std::mem::take(&mut edit.grading_interaction);
                if event.phase == PointerPhase::Cancel
                    && let Interaction::Dragging { wheel, original } = interaction
                {
                    edit.settings.grading.wheels[wheel] = original;
                }
            }
        });
    }
}
