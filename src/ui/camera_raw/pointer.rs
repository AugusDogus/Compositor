use super::*;
use compositor::camera_raw::{Guide, PointColor, sampling};
use quickgui::{MouseButton, PointerEvent, PointerPhase};
#[derive(Clone, Copy, Default, PartialEq)]
pub(super) enum Tool {
    #[default]
    None,
    WhiteBalance,
    PointColor,
    Guide,
    Curve,
    Hue,
    Saturation,
    Luminance,
}
pub(super) enum Drag {
    Guide {
        start: [f64; 2],
        end: [f64; 2],
    },
    Target {
        start_y: f32,
        tone: f64,
        hue: f64,
        original: Box<Settings>,
    },
}
impl Editor {
    pub(in crate::ui) fn camera_tool_active(&self) -> bool {
        self.camera_raw.tool != Tool::None
    }
    pub(in crate::ui) fn camera_sampling(&self) -> bool {
        matches!(
            self.modal,
            Some(Form::Edit {
                action: Action::CameraRaw,
                ..
            })
        ) && !self.filter_applying()
    }
    pub(in crate::ui) fn camera_sample_pointer(&mut self, event: &PointerEvent) -> Result<()> {
        if event.phase == PointerPhase::Cancel {
            if let Some(Drag::Target { original, .. }) = self.camera_raw.drag.take() {
                self.camera_raw.settings = *original;
                self.sync_camera_fields();
            }
            return Ok(());
        }
        let (zoom, offset) = self.viewport(event.size.width, event.size.height);
        let point = [
            (f64::from(event.local_position.x) - offset[0]) / zoom,
            (f64::from(event.local_position.y) - offset[1]) / zoom,
        ];
        let (document, _) = self.filter_source()?;
        let layer = document
            .active_layer()
            .ok_or_else(|| compositor::invalid("The Camera Raw layer was removed."))?;
        let source = layer
            .raster()
            .ok_or_else(|| compositor::invalid("The Camera Raw layer has no pixels."))?;
        let unit = layer.transform.unit(point);
        self.camera_raw.readout = self
            .camera_scope_pixels()
            .and_then(|pixels| sampling::sample(pixels, unit));
        if event.phase == PointerPhase::Down && event.button == MouseButton::Left {
            let rgb = match sampling::sample(source, unit) {
                Some(rgb) => rgb,
                None if self.camera_raw.tool == Tool::Guide => [0.; 3],
                None => return Ok(()),
            };
            match self.camera_raw.tool {
                Tool::WhiteBalance => {
                    let [temperature, tint] = sampling::white_balance(rgb)?;
                    self.camera_change(|e| {
                        e.settings.color.temperature = temperature;
                        e.settings.color.tint = tint;
                    });
                }
                Tool::PointColor => {
                    let [hue, saturation, luminance] = sampling::hsl(rgb);
                    self.camera_change(|e| {
                        if e.settings.mixer.points.len() < 8 {
                            e.point = e.settings.mixer.points.len();
                            e.settings.mixer.points.push(PointColor {
                                hue,
                                saturation,
                                luminance,
                                ..Default::default()
                            });
                        } else if let Some(p) = e.settings.mixer.points.get_mut(e.point) {
                            p.hue = hue;
                            p.saturation = saturation;
                            p.luminance = luminance;
                        }
                    });
                }
                Tool::Guide => {
                    if self.camera_raw.settings.guides.len() < 4 {
                        let start = [unit[0].clamp(0., 1.), (1. - unit[1]).clamp(0., 1.)];
                        self.camera_raw.drag = Some(Drag::Guide { start, end: start });
                    }
                }
                Tool::Curve | Tool::Hue | Tool::Saturation | Tool::Luminance => {
                    let tone = rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
                    self.camera_raw.drag = Some(Drag::Target {
                        start_y: event.local_position.y,
                        tone,
                        hue: sampling::hsl(rgb)[0],
                        original: Box::new(self.camera_raw.settings.clone()),
                    });
                }
                Tool::None => {}
            }
        } else if matches!(event.phase, PointerPhase::Move | PointerPhase::Up) {
            let mut changed = false;
            if let Some(drag) = &mut self.camera_raw.drag {
                match drag {
                    Drag::Guide { end, .. } => {
                        *end = [unit[0].clamp(0., 1.), (1. - unit[1]).clamp(0., 1.)];
                    }
                    Drag::Target {
                        start_y,
                        tone,
                        hue,
                        original,
                    } => {
                        let delta = f64::from(*start_y - event.local_position.y) * 0.5;
                        let mut settings = original.as_ref().clone();
                        match self.camera_raw.tool {
                            Tool::Curve => {
                                let c = &mut settings.curve;
                                let o = &original.curve;
                                let (value, base) = if *tone < c.shadow_split / 100. {
                                    (&mut c.shadows, o.shadows)
                                } else if *tone < c.dark_split / 100. {
                                    (&mut c.darks, o.darks)
                                } else if *tone < c.light_split / 100. {
                                    (&mut c.lights, o.lights)
                                } else {
                                    (&mut c.highlights, o.highlights)
                                };
                                *value = (base + delta).clamp(-100., 100.);
                            }
                            tool => {
                                let target = match tool {
                                    Tool::Hue => &mut settings.mixer.hue,
                                    Tool::Saturation => &mut settings.mixer.saturation,
                                    _ => &mut settings.mixer.luminance,
                                };
                                for (i, center) in [0., 30., 60., 120., 180., 240., 270., 300.]
                                    .into_iter()
                                    .enumerate()
                                {
                                    let distance = (center - *hue).abs();
                                    let weight = (1. - distance.min(360. - distance) / 40.).max(0.);
                                    target[i] = (target[i] + delta * weight).clamp(-100., 100.);
                                }
                            }
                        }
                        self.camera_raw.settings = settings;
                        changed = true;
                    }
                }
            }
            if event.phase == PointerPhase::Up
                && let Some(Drag::Guide { start, end }) = self.camera_raw.drag.take()
                && (end[0] - start[0]).hypot(end[1] - start[1]) > 0.01
            {
                self.camera_raw.settings.guided = true;
                self.camera_raw.settings.guides.push(Guide { start, end });
                changed = true;
            }
            if changed {
                self.sync_camera_fields();
            }
        }
        Ok(())
    }
    pub(super) fn camera_tool_controls(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let tools: &[(&str, Tool)] = match self.camera_raw.group {
            Group::Color => &[("White Balance Eyedropper", Tool::WhiteBalance)],
            Group::Curve => &[("Target tone", Tool::Curve)],
            Group::Mixer => {
                if self.camera_raw.mixer_page == MixerPage::Points {
                    &[("Pick color", Tool::PointColor)]
                } else {
                    &[
                        ("Target hue", Tool::Hue),
                        ("Target saturation", Tool::Saturation),
                        ("Target luminance", Tool::Luminance),
                    ]
                }
            }
            Group::Geometry => &[("Draw guide", Tool::Guide)],
            _ => &[],
        };
        let mut row = div().flex_row().flex_wrap().gap(4.);
        for &(label, tool) in tools {
            row = row.child(
                Self::check_control(label, self.camera_raw.tool == tool).on_click(cx.listener(
                    format!("camera-tool-{label}"),
                    move |this, cx| {
                        this.camera_change(|e| {
                            e.tool = if e.tool == tool { Tool::None } else { tool };
                            e.drag = None;
                        });
                        this.changed(cx);
                    },
                )),
            );
        }
        row
    }
    pub(in crate::ui) fn camera_guides_overlay(&self, zoom: f64, offset: [f64; 2]) -> Element {
        if !self.camera_sampling() || self.camera_raw.group != Group::Geometry {
            return div();
        }
        let Some(layer) = self.session().document.active_layer() else {
            return div();
        };
        let mut guides = self.camera_raw.settings.guides.clone();
        if let Some(Drag::Guide { start, end }) = &self.camera_raw.drag {
            guides.push(Guide {
                start: *start,
                end: *end,
            });
        }
        let lines: Vec<_> = guides
            .iter()
            .map(|g| {
                [g.start, g.end].map(|p| {
                    let doc = layer.transform.point([p[0], 1. - p[1]]);
                    quickgui::Point::new(
                        (offset[0] + doc[0] * zoom) as f32,
                        (offset[1] + doc[1] * zoom) as f32,
                    )
                })
            })
            .collect();
        quickgui::canvas(move |_, painter| {
            let mut path = quickgui::PathBuilder::stroke(1.5);
            for [start, end] in &lines {
                path.move_to(*start);
                path.line_to(*end);
            }
            if let Ok(path) = path.build() {
                painter.paint_path(path, Color::rgb8(80, 180, 255));
            }
        })
        .absolute()
        .size_full()
        .accessibility_hidden(true)
        .into_element()
    }
}
