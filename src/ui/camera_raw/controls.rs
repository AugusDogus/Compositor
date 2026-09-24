use super::*;
use compositor::camera_raw::{ColorMixer, GlowStyle, Guide, Process, Projection, VignetteStyle};
impl Editor {
    pub(in crate::ui) fn camera_raw_controls(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let groups = self.camera_group_picker(cx);
        let group = self.camera_raw.group;
        let common = div()
            .flex_row()
            .items_center()
            .justify_between()
            .child(
                Self::check_control("Enable group", self.camera_raw.settings.enabled(group))
                    .on_click(cx.listener("camera-group-enabled", move |this, cx| {
                        this.camera_change(|e| {
                            e.settings.enabled[group as usize] = !e.settings.enabled[group as usize]
                        });
                        this.changed(cx);
                    })),
            )
            .child(Self::control("Reset group").on_click(cx.listener(
                "camera-group-reset",
                move |this, cx| {
                    this.camera_change(|e| e.settings.reset(group));
                    this.changed(cx);
                },
            )));
        let mut result = div().flex_col().gap(10.).child(groups).child(common);
        let choices = match group {
            Group::Color => self.camera_group_color(cx),
            Group::Effects => self.camera_group_effects(cx),
            Group::Curve => self.camera_group_curve(cx),
            Group::Mixer => self.camera_group_mixer(cx),
            Group::Grading => self.camera_group_grading(cx),
            Group::Optics => self.camera_group_optics(cx),
            Group::Geometry => self.camera_group_geometry(cx),
            Group::Calibration => self.camera_group_calibration(cx),
            _ => div(),
        };
        result = result.child(choices);
        result.child(self.camera_tool_controls(cx))
    }
}

impl Editor {
    fn camera_group_color(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let result = div().flex_col().gap(8.);
        let mut choices = div().flex_row().flex_wrap().gap(4.);
        choices = choices.child(Self::control("Auto White Balance").on_click(cx.listener(
            "camera-auto-wb",
            |this, cx| {
                this.camera_auto_balance();
                this.changed(cx);
            },
        )));

        result.child(choices)
    }
    fn camera_group_effects(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let result = div().flex_col().gap(8.);
        let mut choices = div().flex_row().flex_wrap().gap(4.);
        for (label, style) in [
            ("Diffusion", GlowStyle::Diffusion),
            ("Bloom", GlowStyle::Bloom),
            ("Halation", GlowStyle::Halation),
        ] {
            choices = choices.child(
                Self::check_control(label, self.camera_raw.settings.glow_style == style).on_click(
                    cx.listener(format!("camera-glow-{label}"), move |this, cx| {
                        this.camera_change(|e| e.settings.glow_style = style);
                        this.changed(cx);
                    }),
                ),
            );
        }
        for (label, style) in [
            ("Highlight priority", VignetteStyle::HighlightPriority),
            ("Color priority", VignetteStyle::ColorPriority),
            ("Paint overlay", VignetteStyle::PaintOverlay),
        ] {
            choices = choices.child(
                Self::check_control(label, self.camera_raw.settings.vignette_style == style)
                    .on_click(
                        cx.listener(format!("camera-vignette-{label}"), move |this, cx| {
                            this.camera_change(|e| e.settings.vignette_style = style);
                            this.changed(cx);
                        }),
                    ),
            );
        }

        result.child(choices)
    }
    fn camera_group_curve(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let result = div().flex_col().gap(8.);
        let mut choices = div().flex_row().flex_wrap().gap(4.);
        for (i, label) in ["RGB", "Red", "Green", "Blue"].into_iter().enumerate() {
            choices = choices.child(
                Self::check_control(label, self.camera_raw.channel == i).on_click(cx.listener(
                    format!("camera-curve-{i}"),
                    move |this, cx| {
                        this.camera_change(|e| {
                            e.channel = i;
                        });
                        this.changed(cx);
                    },
                )),
            );
        }

        result.child(choices)
    }
    fn camera_group_mixer(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let result = div().flex_col().gap(8.);
        let mut choices = div().flex_row().flex_wrap().gap(4.);
        for (label, page) in [
            ("HSL", MixerPage::Families),
            ("Point Color", MixerPage::Points),
        ] {
            choices = choices.child(
                Self::check_control(label, self.camera_raw.mixer_page == page).on_click(
                    cx.listener(format!("camera-mixer-{label}"), move |this, cx| {
                        this.camera_change(|e| e.mixer_page = page);
                        this.changed(cx);
                    }),
                ),
            );
        }
        if self.camera_raw.mixer_page == MixerPage::Families {
            for (i, label) in ColorMixer::NAMES.into_iter().enumerate() {
                choices = choices.child(
                    Self::check_control(label, self.camera_raw.color == i).on_click(cx.listener(
                        format!("camera-color-{i}"),
                        move |this, cx| {
                            this.camera_change(|e| e.color = i);
                            this.changed(cx);
                        },
                    )),
                );
            }
        } else {
            for i in 0..self.camera_raw.settings.mixer.points.len() {
                choices = choices.child(
                    Self::check_control(
                        [
                            "Color 1", "Color 2", "Color 3", "Color 4", "Color 5", "Color 6",
                            "Color 7", "Color 8",
                        ][i],
                        self.camera_raw.point == i,
                    )
                    .on_click(cx.listener(
                        format!("camera-point-{i}"),
                        move |this, cx| {
                            this.camera_change(|e| e.point = i);
                            this.changed(cx);
                        },
                    )),
                );
            }
            if self.camera_raw.settings.mixer.points.len() < 8 {
                choices = choices.child(Self::control("Add color").on_click(cx.listener(
                    "camera-point-add",
                    |this, cx| {
                        this.camera_change(|e| {
                            e.point = e.settings.mixer.points.len();
                            e.settings.mixer.points.push(Default::default());
                        });
                        this.changed(cx);
                    },
                )));
            }
            if !self.camera_raw.settings.mixer.points.is_empty() {
                choices = choices.child(Self::control("Remove color").on_click(cx.listener(
                    "camera-point-remove",
                    |this, cx| {
                        this.camera_change(|e| {
                            e.settings.mixer.points.remove(e.point);
                            e.point = e.point.saturating_sub(1);
                        });
                        this.changed(cx);
                    },
                )));
            }
        }

        result.child(choices)
    }
    fn camera_group_grading(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let result = div().flex_col().gap(8.);
        let mut choices = div().flex_row().flex_wrap().gap(4.);
        for (i, label) in ["Shadows", "Midtones", "Highlights", "Global"]
            .into_iter()
            .enumerate()
        {
            choices = choices.child(
                Self::check_control(label, self.camera_raw.wheel == i).on_click(cx.listener(
                    format!("camera-grade-{i}"),
                    move |this, cx| {
                        this.camera_change(|e| {
                            e.wheel = i;
                        });
                        this.changed(cx);
                    },
                )),
            );
        }

        result.child(choices)
    }
    fn camera_group_optics(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let mut result = div().flex_col().gap(8.);
        let mut choices = div().flex_row().flex_wrap().gap(4.);
        choices = choices
            .child(
                Self::check_control(
                    "Chromatic aberration",
                    self.camera_raw.settings.remove_chromatic,
                )
                .on_click(cx.listener("camera-chromatic", |this, cx| {
                    this.camera_change(|e| {
                        e.settings.remove_chromatic = !e.settings.remove_chromatic
                    });
                    this.changed(cx);
                })),
            )
            .child(
                Self::check_control("Lens profile", self.camera_raw.settings.lens_profile)
                    .on_click(cx.listener("camera-lens-profile", |this, cx| {
                        this.camera_change(|e| e.settings.lens_profile = !e.settings.lens_profile);
                        this.changed(cx);
                    })),
            );
        result = result.child(
            text("Lens profile uses generic correction strengths for rendered images.")
                .text_size(12.)
                .wrap(),
        );

        result.child(choices)
    }
    fn camera_group_geometry(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let result = div().flex_col().gap(8.);
        let mut choices = div().flex_row().flex_wrap().gap(4.);
        for (label, projection) in [
            ("Perspective", Projection::Perspective),
            ("Rectilinear", Projection::Rectilinear),
        ] {
            choices = choices.child(
                Self::check_control(label, self.camera_raw.settings.projection == projection)
                    .on_click(cx.listener(
                        format!("camera-projection-{label}"),
                        move |this, cx| {
                            this.camera_change(|e| e.settings.projection = projection);
                            this.changed(cx);
                        },
                    )),
            );
        }
        choices = choices
            .child(
                Self::check_control("Guided", self.camera_raw.settings.guided).on_click(
                    cx.listener("camera-guided", |this, cx| {
                        this.camera_change(|e| e.settings.guided = !e.settings.guided);
                        this.changed(cx);
                    }),
                ),
            )
            .child(
                Self::check_control("Constrain crop", self.camera_raw.settings.constrain_crop)
                    .on_click(cx.listener("camera-constrain", |this, cx| {
                        this.camera_change(|e| {
                            e.settings.constrain_crop = !e.settings.constrain_crop
                        });
                        this.changed(cx);
                    })),
            );
        if self.camera_raw.settings.guides.len() < 4 {
            choices = choices.child(Self::control("Add guide").on_click(cx.listener(
                "camera-guide-add",
                |this, cx| {
                    this.camera_change(|e| {
                        e.settings.guided = true;
                        e.settings.guides.push(Guide {
                            start: [0.25, 0.25],
                            end: [0.75, 0.25],
                        });
                    });
                    this.changed(cx);
                },
            )));
        }
        if !self.camera_raw.settings.guides.is_empty() {
            choices = choices.child(Self::control("Remove guide").on_click(cx.listener(
                "camera-guide-remove",
                |this, cx| {
                    this.camera_change(|e| {
                        e.settings.guides.pop();
                    });
                    this.changed(cx);
                },
            )));
        }

        result.child(choices)
    }
    fn camera_group_calibration(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let result = div().flex_col().gap(8.);
        let mut choices = div().flex_row().flex_wrap().gap(4.);
        for (i, process) in [
            Process::One,
            Process::Two,
            Process::Three,
            Process::Four,
            Process::Five,
            Process::Six,
        ]
        .into_iter()
        .enumerate()
        {
            choices = choices.child(
                Self::check_control(
                    [
                        "Version 1",
                        "Version 2",
                        "Version 3",
                        "Version 4",
                        "Version 5",
                        "Version 6",
                    ][i],
                    self.camera_raw.settings.process == process,
                )
                .on_click(cx.listener(
                    format!("camera-process-{i}"),
                    move |this, cx| {
                        this.camera_change(|e| e.settings.process = process);
                        this.changed(cx);
                    },
                )),
            );
        }

        result.child(choices)
    }
}
