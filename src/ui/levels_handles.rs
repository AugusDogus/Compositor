//! LevelsSheet gives each triangle its own 22×20 drag target, including the overhang.
use super::adjustment_fields::channel_index;
use super::*;
use compositor::adjustment::LevelRange;
use quickgui::{LayoutBoundsHandle, MouseButton, PathBuilder, Point, PointerPhase};

#[derive(Default)]
pub(super) struct Handles {
    active: Option<Handle>,
    input_bounds: LayoutBoundsHandle,
    output_bounds: LayoutBoundsHandle,
}

#[derive(Clone, Copy, PartialEq)]
enum Handle {
    InputBlack,
    Gamma,
    InputWhite,
    OutputBlack,
    OutputWhite,
}

impl Handle {
    fn label(self) -> &'static str {
        match self {
            Self::InputBlack => "Input black",
            Self::Gamma => "Gamma",
            Self::InputWhite => "Input white",
            Self::OutputBlack => "Output black",
            Self::OutputWhite => "Output white",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::InputBlack => "levels-input-black-handle",
            Self::Gamma => "levels-gamma-handle",
            Self::InputWhite => "levels-input-white-handle",
            Self::OutputBlack => "levels-output-black-handle",
            Self::OutputWhite => "levels-output-white-handle",
        }
    }

    fn position(self, range: LevelRange) -> f64 {
        match self {
            Self::InputBlack => range.black,
            Self::Gamma => range.black + (range.white - range.black) * 0.5_f64.powf(range.gamma),
            Self::InputWhite => range.white,
            Self::OutputBlack => range.output_black,
            Self::OutputWhite => range.output_white,
        }
    }

    fn update(self, range: &mut LevelRange, tone: f64) {
        match self {
            Self::InputBlack => range.black = tone.round().min(range.white - 1.),
            Self::InputWhite => range.white = tone.round().max(range.black + 1.),
            Self::Gamma => {
                range.gamma = (((tone - range.black) / (range.white - range.black))
                    .clamp(0.001, 0.999)
                    .ln()
                    / 0.5_f64.ln())
                .clamp(0.1, 9.99);
            }
            Self::OutputBlack => range.output_black = tone.round(),
            Self::OutputWhite => range.output_white = tone.round(),
        }
    }

    fn glyph(self) -> Element {
        static SHADOW: std::sync::OnceLock<quickgui::Svg> = std::sync::OnceLock::new();
        let shadow = SHADOW.get_or_init(|| {
            quickgui::Svg::from_bytes(include_bytes!(
                "../../assets/icons/compositor-levels-handle-shadow.svg"
            ))
            .expect("Embedded Levels shadow must be valid SVG")
        });
        let color = match self {
            Self::InputBlack | Self::OutputBlack => Color::BLACK,
            Self::InputWhite | Self::OutputWhite => Color::WHITE,
            Self::Gamma => Color::rgb8(142, 142, 147),
        };
        let triangle = quickgui::canvas(move |_, painter| {
            let mut path = PathBuilder::fill();
            path.move_to(Point::new(11., 5.));
            path.line_to(Point::new(6., 15.));
            path.line_to(Point::new(16., 15.));
            path.close();
            painter.paint_path(path.build().expect("Finite Levels triangle"), color);
        })
        .w(22.)
        .h(20.);
        // A cached 22×20 SVG mask avoids a full-window compositing group per shadow.
        div()
            .relative()
            .w(22.)
            .h(20.)
            .accessibility_hidden(true)
            .child(
                quickgui::svg(shadow.clone())
                    .absolute()
                    .inset_0()
                    .w(22.)
                    .h(20.)
                    .text_color(Color::rgb8(142, 142, 147)),
            )
            .child(triangle)
    }
}

impl Editor {
    pub(super) fn levels_handles(&self, cx: &mut ViewContext<'_, Self>, output: bool) -> Element {
        let Some(edit) = &self.adjustment_edit else {
            return div();
        };
        let (id, bounds, handles): (_, _, &[Handle]) = if output {
            (
                "levels-output-handles",
                &edit.levels_handles.output_bounds,
                &[Handle::OutputBlack, Handle::OutputWhite],
            )
        } else {
            (
                "levels-input-handles",
                &edit.levels_handles.input_bounds,
                &[Handle::InputBlack, Handle::Gamma, Handle::InputWhite],
            )
        };
        let range = edit.settings.levels.ranges[channel_index(edit.settings.levels.channel)];
        let mut track = div()
            .id(id)
            .relative()
            .w_full()
            .h(20.)
            .flex_shrink_0()
            .report_bounds(bounds.clone());
        for &handle in handles {
            let bounds = bounds.clone();
            let pointer = cx.pointer_listener(handle.id(), move |this, event, cx| {
                if event.phase == PointerPhase::Down && event.button != MouseButton::Left {
                    return;
                }
                let Some(bounds) = bounds.bounds().filter(|b| b.width > 0.) else {
                    return;
                };
                let Some(edit) = &mut this.adjustment_edit else {
                    return;
                };
                if event.phase == PointerPhase::Down {
                    edit.levels_handles.active = Some(handle);
                }
                if edit.levels_handles.active != Some(handle) {
                    return;
                }
                if event.phase != PointerPhase::Cancel {
                    let tone =
                        ((event.position.x - bounds.x) / bounds.width).clamp(0., 1.) as f64 * 255.;
                    handle.update(
                        &mut edit.settings.levels.ranges
                            [channel_index(edit.settings.levels.channel)],
                        tone,
                    );
                }
                if matches!(event.phase, PointerPhase::Up | PointerPhase::Cancel) {
                    edit.levels_handles.active = None;
                }
                this.show_adjustment_fields();
                this.refresh_adjustment();
                this.changed(cx);
            });
            // The wrapper's right edge follows the track fraction without a measuring frame.
            // Its child is centered there, with the source's y=9 and 22×20 content shape.
            track = track.child(
                div()
                    .absolute()
                    .left(0.)
                    .top(-1.)
                    .w_fraction((handle.position(range) / 255.) as f32)
                    .h(20.)
                    .child(
                        div()
                            .absolute()
                            .right(-11.)
                            .w(22.)
                            .h(20.)
                            .id(handle.id())
                            .accessibility_label(handle.label())
                            .on_pointer(pointer)
                            .child(handle.glyph()),
                    ),
            );
        }
        track
    }
}

#[cfg(test)]
mod tests;
