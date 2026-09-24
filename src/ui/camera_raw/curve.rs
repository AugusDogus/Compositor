//! Direct manipulation of Camera Raw's parametric dividers and point curves.
use super::*;
use compositor::adjustment::{Channel, CurvePoint};
use quickgui::{MouseButton, PointerEvent, PointerPhase};

#[derive(Clone, Copy, Default, PartialEq)]
pub(super) enum Page {
    #[default]
    Parametric,
    Point,
}
#[derive(Default)]
pub(super) enum Interaction {
    #[default]
    Idle,
    Point {
        index: usize,
        channel: usize,
        original: Vec<CurvePoint>,
    },
    Divider {
        index: usize,
        original: [f64; 3],
    },
}
fn curves_for_readout(edit: &Edit, index: usize) -> Option<CurvePoint> {
    edit.settings.curves.channels[edit.channel]
        .get(index)
        .copied()
}
fn splits(edit: &Edit) -> [f64; 3] {
    let c = &edit.settings.curve;
    [c.shadow_split, c.dark_split, c.light_split]
}
fn parametric(edit: &Edit, tone: f64) -> f64 {
    let [s, d, l] = splits(edit).map(|v| v / 100.);
    let c = &edit.settings.curve;
    let (amount, lo, hi) = if tone < s {
        (c.shadows, 0., s)
    } else if tone < d {
        (c.darks, s, d)
    } else if tone < l {
        (c.lights, d, l)
    } else {
        (c.highlights, l, 1.)
    };
    let weight = (1. - (tone - (lo + hi) / 2.).abs() / ((hi - lo).max(0.02) / 2.)).max(0.);
    (tone + amount / 100. * weight * 0.22).clamp(0., 1.)
}
impl Editor {
    pub(in crate::ui) fn camera_graph_field_visible(&self, index: usize) -> bool {
        match self.camera_raw.group {
            Group::Curve => {
                if self.camera_raw.curve_page == Page::Parametric {
                    index < 4
                } else {
                    index == 7 && self.camera_raw.channel == 0
                }
            }
            Group::Grading if self.camera_raw.grading_page == super::grading::Page::ThreeWay => {
                index >= 3
            }
            _ => true,
        }
    }
    pub(super) fn camera_curve_editor(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let page = self.camera_raw.curve_page;
        let curves = self.camera_raw.settings.curves.clone();
        let channel = self.camera_raw.channel;
        let samples: Vec<_> = (0..=255)
            .map(|i| parametric(&self.camera_raw, f64::from(i) / 255.))
            .collect();
        let dividers = splits(&self.camera_raw);
        let graph = quickgui::canvas(move |bounds, painter| {
            painter.fill_rect(bounds, Color::BLACK.with_alpha(0.35));
            let position = |x: f64, y: f64| {
                quickgui::Point::new(x as f32 * bounds.width, (1. - y) as f32 * bounds.height)
            };
            for i in 1..4 {
                let t = i as f64 / 4.;
                let mut path = quickgui::PathBuilder::stroke(1.);
                path.move_to(position(t, 0.));
                path.line_to(position(t, 1.));
                path.move_to(position(0., t));
                path.line_to(position(1., t));
                if let Ok(path) = path.build() {
                    painter.paint_path(path, Color::WHITE.with_alpha(0.12));
                }
            }
            let mut path = quickgui::PathBuilder::stroke(1.5);
            for (i, sample) in samples.iter().enumerate() {
                let y = if page == Page::Parametric {
                    *sample
                } else {
                    curves.sample(
                        i as f64,
                        [Channel::RGB, Channel::Red, Channel::Green, Channel::Blue][channel],
                    ) / 255.
                };
                let p = position(i as f64 / 255., y);
                if i == 0 {
                    path.move_to(p);
                } else {
                    path.line_to(p);
                }
            }
            if let Ok(path) = path.build() {
                painter.paint_path(path, Color::WHITE);
            }
            if page == Page::Point {
                for p in &curves.channels[channel] {
                    let p = position(p.x / 255., p.y / 255.);
                    painter.fill_rounded_rect(
                        quickgui::Rect::new(p.x - 4., p.y - 4., 8., 8.),
                        4.,
                        Color::WHITE,
                    );
                }
            } else {
                for divider in dividers {
                    let x = divider as f32 / 100. * bounds.width;
                    painter.fill_rect(
                        quickgui::Rect::new(x - 1.5, bounds.height - 10., 3., 10.),
                        Color::WHITE,
                    );
                }
            }
        })
        .id("camera-curve-graph")
        .w_full()
        .h(150.)
        .flex_shrink_0()
        .on_mouse_down(
            MouseButton::Left,
            cx.mouse_down_listener("camera-curve-graph", |this, event, cx| {
                if event.click_count == 2 && this.camera_raw.curve_page == Page::Point {
                    this.camera_change(|e| {
                        if let Some(i) = e.curve_selected.take() {
                            let points = &mut e.settings.curves.channels[e.channel];
                            if i > 0 && i + 1 < points.len() {
                                points.remove(i);
                            }
                        }
                        e.curve_interaction = Interaction::Idle;
                    });
                    cx.prevent_default();
                    this.changed(cx);
                }
            }),
        )
        .on_pointer(
            cx.pointer_listener("camera-curve-graph", |this, event, cx| {
                this.camera_curve_pointer(event);
                this.changed(cx);
            }),
        );
        let mut pages = div().flex_row().gap(6.);
        for (label, value) in [("Parametric", Page::Parametric), ("Point", Page::Point)] {
            pages = pages.child(
                Self::check_control(label, page == value).on_click(cx.listener(
                    format!("camera-curve-page-{label}"),
                    move |this, cx| {
                        this.camera_change(|e| {
                            e.curve_page = value;
                            e.curve_interaction = Interaction::Idle;
                        });
                        this.changed(cx);
                    },
                )),
            )
        }
        let mut controls = div().flex_col().gap(8.).child(pages).child(graph);
        if page == Page::Point {
            if let Some(point) = self
                .camera_raw
                .curve_selected
                .and_then(|i| curves_for_readout(&self.camera_raw, i))
            {
                controls = controls.child(
                    text(format!("Input {:.0} · Output {:.0}", point.x, point.y)).text_size(12.),
                );
            }
            let mut presets = div().flex_row().flex_wrap().gap(6.);
            for (label, strength) in [
                ("Linear", 0.),
                ("Medium contrast", 0.07),
                ("Strong contrast", 0.15),
            ] {
                presets = presets.child(Self::control(label).on_click(cx.listener(
                    format!("camera-curve-preset-{label}"),
                    move |this, cx| {
                        this.camera_change(|e| {
                            let mut points = vec![CurvePoint { x: 0., y: 0. }];
                            if strength > 0. {
                                points.extend([
                                    CurvePoint {
                                        x: 63.75,
                                        y: (0.25 - strength) * 255.,
                                    },
                                    CurvePoint {
                                        x: 191.25,
                                        y: (0.75 + strength) * 255.,
                                    },
                                ]);
                            }
                            points.push(CurvePoint { x: 255., y: 255. });
                            e.settings.curves.channels[e.channel] = points;
                            e.curve_selected = None;
                        });
                        this.changed(cx);
                    },
                )));
            }
            controls = controls.child(presets).child(
                Self::control("Remove selected point")
                    .disabled(self.camera_raw.curve_selected.is_none_or(|i| {
                        i == 0 || i + 1 >= self.camera_raw.settings.curves.channels[channel].len()
                    }))
                    .on_click(cx.listener("camera-curve-remove", |this, cx| {
                        this.camera_change(|e| {
                            if let Some(i) = e.curve_selected.take() {
                                let points = &mut e.settings.curves.channels[e.channel];
                                if i > 0 && i + 1 < points.len() {
                                    points.remove(i);
                                }
                            }
                        });
                        this.changed(cx);
                    })),
            );
        }
        controls.child(
            text(if page == Page::Point {
                "Click to add a point. Drag to adjust."
            } else {
                "Drag a divider to change the tonal regions."
            })
            .text_size(11.)
            .wrap(),
        )
    }
    pub(super) fn camera_curve_pointer(&mut self, event: &PointerEvent) {
        if event.button != MouseButton::Left || self.filter_applying() {
            return;
        }
        let x = (f64::from(event.local_position.x) / f64::from(event.size.width.max(1.)) * 255.)
            .clamp(0., 255.);
        let y = ((1. - f64::from(event.local_position.y) / f64::from(event.size.height.max(1.)))
            * 255.)
            .clamp(0., 255.);
        self.camera_change(|edit| {
            if event.phase == PointerPhase::Down {
                if edit.curve_page == Page::Parametric {
                    let original = splits(edit);
                    let at = x / 255. * 100.;
                    let index = (0..3)
                        .min_by(|a, b| {
                            (original[*a] - at)
                                .abs()
                                .total_cmp(&(original[*b] - at).abs())
                        })
                        .unwrap_or(0);
                    edit.curve_interaction = Interaction::Divider { index, original };
                } else {
                    let points = &mut edit.settings.curves.channels[edit.channel];
                    let original = points.clone();
                    let hit = points.iter().position(|p| (p.x - x).hypot(p.y - y) < 14.);
                    let index = match hit {
                        Some(i) => i,
                        None if points.len() < 32
                            && x > 1.
                            && x < 254.
                            && points.iter().all(|p| (p.x - x).abs() > 1.) =>
                        {
                            let i = points.partition_point(|p| p.x < x);
                            points.insert(i, CurvePoint { x, y });
                            i
                        }
                        None => return,
                    };
                    edit.curve_selected = Some(index);
                    edit.curve_interaction = Interaction::Point {
                        index,
                        channel: edit.channel,
                        original,
                    };
                }
            }
            match &edit.curve_interaction {
                Interaction::Point { index, channel, .. }
                    if matches!(
                        event.phase,
                        PointerPhase::Down | PointerPhase::Move | PointerPhase::Up
                    ) =>
                {
                    let points = &mut edit.settings.curves.channels[*channel];
                    let i = *index;
                    if i < points.len() {
                        if i > 0 && i + 1 < points.len() {
                            let low = points[i - 1].x + 1.;
                            let high = points[i + 1].x - 1.;
                            if low <= high {
                                points[i].x = x.clamp(low, high);
                            }
                        }
                        points[i].y = y;
                    }
                }
                Interaction::Divider { index, .. }
                    if matches!(
                        event.phase,
                        PointerPhase::Down | PointerPhase::Move | PointerPhase::Up
                    ) =>
                {
                    let at = x / 255. * 100.;
                    let c = &mut edit.settings.curve;
                    match index {
                        0 => c.shadow_split = at.clamp(5., (c.dark_split - 2.).min(90.)),
                        1 => {
                            c.dark_split = at
                                .clamp((c.shadow_split + 2.).max(7.), (c.light_split - 2.).min(95.))
                        }
                        _ => c.light_split = at.clamp((c.dark_split + 2.).max(9.), 98.),
                    }
                }
                _ => {}
            }
            if matches!(event.phase, PointerPhase::Up | PointerPhase::Cancel) {
                let interaction = std::mem::take(&mut edit.curve_interaction);
                if event.phase == PointerPhase::Cancel {
                    match interaction {
                        Interaction::Point {
                            channel, original, ..
                        } => {
                            edit.settings.curves.channels[channel] = original;
                            edit.curve_selected = None;
                        }
                        Interaction::Divider { original, .. } => {
                            let c = &mut edit.settings.curve;
                            [c.shadow_split, c.dark_split, c.light_split] = original;
                        }
                        Interaction::Idle => {}
                    }
                }
            }
        });
    }
}
