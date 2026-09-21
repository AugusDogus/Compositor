use super::{adjustment_fields::channel_index, *};
use compositor::adjustment::CurvePoint;
use quickgui::{MouseButton, PointerEvent, PointerPhase};

#[derive(Clone, Copy)]
pub(super) enum CurveState {
    None,
    Selected(usize),
    Dragging(usize),
}

impl CurveState {
    fn selected(self) -> Option<usize> {
        match self {
            Self::None => None,
            Self::Selected(i) | Self::Dragging(i) => Some(i),
        }
    }
}

impl Editor {
    pub(super) fn curve_editor(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let Some(edit) = &self.adjustment_edit else {
            return div();
        };
        let curves = edit.settings.curves.clone();
        let channel = curves.channel;
        let selected = edit.curve.selected();
        let pointer = cx.pointer_listener("curve-graph", |this, event, cx| {
            if let Err(error) = this.curve_pointer(event)
                && let Some(Form::Edit { error: message, .. }) = &mut this.modal
            {
                *message = error.to_string();
            }
            this.changed(cx);
        });
        let grid = self.guide_grid_shader.clone();
        let scale = cx.scale_factor();
        let graph = quickgui::canvas(move |bounds, painter| {
            painter.fill_rect(bounds, Color::BLACK.with_alpha(0.35));
            painter.paint_shader(
                bounds,
                &grid,
                quickgui::ShaderParameters::new()
                    .vector(0, [0., 0., bounds.width, bounds.height])
                    .vector(1, [scale, 4., 0., 0.12]),
            );
            let position = |x: f64, y: f64| {
                quickgui::Point::new(
                    (x / 255.) as f32 * bounds.width,
                    (1. - y / 255.) as f32 * bounds.height,
                )
            };
            let mut path = quickgui::PathBuilder::stroke(2.);
            path.move_to(position(0., curves.sample(0., channel)));
            for x in 1..=255 {
                path.line_to(position(x as f64, curves.sample(x as f64, channel)));
            }
            let path = path
                .build()
                .expect("Validated curves produce a finite path");
            painter.paint_path(&path, Color::WHITE);
            for (i, point) in curves.channels[channel_index(channel)].iter().enumerate() {
                let p = position(point.x, point.y);
                painter.fill_rounded_rect(
                    quickgui::Rect::new(p.x - 4., p.y - 4., 8., 8.),
                    4.,
                    if selected == Some(i) {
                        Color::rgb8(0, 122, 255)
                    } else {
                        Color::WHITE
                    },
                );
            }
        })
        .id("curve-graph")
        .w_full()
        .h(260.)
        .flex_shrink(0.)
        .on_pointer(pointer);
        div()
            .flex_col()
            .gap(12.)
            .child(graph)
            .child(
                text("Click to add a point. Drag to adjust.")
                    .text_size(10.)
                    .line_height(13.)
                    .text_color(Color::rgb8(181, 181, 181))
                    .wrap(),
            )
            .child(
                div()
                    .flex_row()
                    .h(26.)
                    .items_center()
                    .gap(6.)
                    .child(
                        text(
                            selected
                                .and_then(|index| {
                                    edit.settings.curves.channels[channel_index(channel)].get(index)
                                })
                                .map_or_else(String::new, |point| {
                                    format!("Input {} · Output {}", point.x as u16, point.y as u16)
                                }),
                        )
                        .text_size(13.)
                        .line_height(16.)
                        .font_features(
                            quickgui::FontFeatures::new()
                                .enable(quickgui::FontFeatureTag::TABULAR_NUMBERS),
                        )
                        .flex_1()
                        .min_w(0.)
                        .truncate(),
                    )
                    .child(
                        Self::control("Remove point")
                            .disabled_style(|style| style.opacity(0.4))
                            .disabled(selected.is_none_or(|index| {
                                index == 0
                                    || index + 1
                                        >= edit.settings.curves.channels[channel_index(channel)]
                                            .len()
                            }))
                            .on_click(cx.listener("curve-remove", |this, cx| {
                                this.remove_curve_point();
                                this.changed(cx);
                            })),
                    ),
            )
            .child(
                Self::control("Reset curve")
                    .self_start()
                    .on_click(cx.listener("curve-reset", |this, cx| {
                        if let Some(edit) = &mut this.adjustment_edit {
                            edit.settings.curves.channels
                                [channel_index(edit.settings.curves.channel)] =
                                vec![CurvePoint { x: 0., y: 0. }, CurvePoint { x: 255., y: 255. }];
                            edit.curve = CurveState::None;
                        }
                        this.show_adjustment_fields();
                        this.refresh_adjustment();
                        this.changed(cx);
                    })),
            )
    }

    fn remove_curve_point(&mut self) {
        if let Some(edit) = &mut self.adjustment_edit
            && let Some(index) = edit.curve.selected()
        {
            let points =
                &mut edit.settings.curves.channels[channel_index(edit.settings.curves.channel)];
            if index > 0 && index + 1 < points.len() {
                points.remove(index);
            }
            edit.curve = CurveState::None;
        }
        self.show_adjustment_fields();
        self.refresh_adjustment();
    }

    pub(super) fn curve_pointer(&mut self, event: &PointerEvent) -> Result<()> {
        if event.phase == PointerPhase::Down {
            self.preview_adjustment()?;
        }
        let Some(edit) = &mut self.adjustment_edit else {
            return Ok(());
        };
        let points =
            &mut edit.settings.curves.channels[channel_index(edit.settings.curves.channel)];
        let width = event.size.width.max(1.) as f64;
        let height = event.size.height.max(1.) as f64;
        let x = (event.local_position.x as f64 / width * 255.).clamp(0., 255.);
        let y = ((1. - event.local_position.y as f64 / height) * 255.).clamp(0., 255.);
        if event.phase == PointerPhase::Down && event.button == MouseButton::Left {
            // Swift measures distance in the 0..255 curve coordinates, independent of panel width.
            let distance = |p: &CurvePoint| (p.x - x).hypot(p.y - y);
            let hit = points
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| distance(a).total_cmp(&distance(b)))
                .filter(|(_, p)| distance(p) < 14.)
                .map(|(index, _)| index);
            let index = match hit {
                Some(index) => index,
                None if points.len() < 32
                    && x > 1.
                    && x < 254.
                    && points.iter().all(|p| (p.x - x).abs() > 1.) =>
                {
                    let index = points.partition_point(|p| p.x < x);
                    points.insert(index, CurvePoint { x, y });
                    index
                }
                None => return Ok(()),
            };
            edit.curve = CurveState::Dragging(index);
        }
        if matches!(event.phase, PointerPhase::Down | PointerPhase::Move)
            && let CurveState::Dragging(index) = edit.curve
        {
            if index >= points.len() {
                edit.curve = CurveState::None;
                return Ok(());
            }
            if index > 0 && index + 1 < points.len() {
                let low = points[index - 1].x + 1.;
                let high = points[index + 1].x - 1.;
                if low <= high {
                    points[index].x = x.clamp(low, high);
                }
            }
            points[index].y = y;
        } else if matches!(event.phase, PointerPhase::Up | PointerPhase::Cancel) {
            edit.curve = edit
                .curve
                .selected()
                .map_or(CurveState::None, CurveState::Selected);
        }
        self.show_adjustment_fields();
        self.preview_adjustment()
    }
}

#[cfg(test)]
mod tests;
