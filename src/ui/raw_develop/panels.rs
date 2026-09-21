use super::fields::{Field, MaskField};
use super::*;

impl Editor {
    pub(super) fn raw_basic(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let mut wb = div()
            .flex_col()
            .gap(6.)
            .child(text("White balance").text_size(13.));
        let mut presets = div().flex_row().gap(4.).flex_wrap();
        for (label, temp) in [
            ("As shot", 0.),
            ("Daylight", 5500.),
            ("Cloudy", 6500.),
            ("Shade", 7500.),
            ("Tungsten", 2850.),
            ("Fluorescent", 4000.),
            ("Flash", 6000.),
        ] {
            presets =
                presets.child(
                    self.raw_button(cx, &format!("raw-wb-{label}"), label, move |d| {
                        d.edit(|s| {
                            s.white_balance = if temp == 0. {
                                raw::WhiteBalance::AsShot
                            } else {
                                raw::WhiteBalance::Temperature
                            };
                            if temp > 0. {
                                s.temperature = temp;
                            }
                            s.tint = 0.;
                        })
                    }),
                );
        }
        wb = wb.child(presets).child(
            div()
                .flex_row()
                .gap(6.)
                .child(self.raw_button(cx, "raw-neutral", "Pick neutral", |d| {
                    d.picker = !d.picker;
                    d.draw_mask = false;
                }))
                .child(self.raw_button(cx, "raw-auto", "Auto exposure", |d| {
                    if let Some(r) = &d.ready {
                        let exposure = raw::auto_exposure(&r.proxy);
                        d.edit(|s| s.exposure = exposure);
                    }
                })),
        );
        wb.child(self.raw_fields(
            cx,
            &[
                ("Temperature", Field::Temperature, (2000., 25000.)),
                ("Tint", Field::Tint, (-150., 150.)),
                ("Exposure", Field::Exposure, (-10., 10.)),
                ("Brightness", Field::Brightness, (-100., 100.)),
                ("Contrast", Field::Contrast, (-100., 100.)),
                ("Highlights", Field::Highlights, (-100., 100.)),
                ("Shadows", Field::Shadows, (-100., 100.)),
                ("Whites", Field::Whites, (-100., 100.)),
                ("Blacks", Field::Blacks, (-100., 100.)),
                ("Clarity", Field::Clarity, (-100., 100.)),
                ("Texture", Field::Texture, (-100., 100.)),
                ("Dehaze", Field::Dehaze, (-100., 100.)),
                ("Vibrance", Field::Vibrance, (-100., 100.)),
                ("Saturation", Field::Saturation, (-100., 100.)),
            ],
        ))
    }
    pub(super) fn raw_tone(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let Some(d) = &self.develop else {
            return div();
        };
        let (channel, band, mono) = (d.curve_channel, d.hsl_band, d.settings.monochrome);
        let mut channels = div().flex_row().gap(4.);
        for (i, name) in ["Master", "R", "G", "B"].into_iter().enumerate() {
            channels = channels.child(
                self.raw_button(cx, &format!("raw-curve-{i}"), name, move |d| {
                    d.curve_channel = i
                })
                .bg(if channel == i {
                    Color::rgb8(40, 85, 130)
                } else {
                    Color::rgb8(48, 48, 48)
                }),
            );
        }
        let mut panel = div()
            .flex_col()
            .gap(8.)
            .child(channels)
            .child(self.raw_curve(cx))
            .child(self.raw_curve_presets(cx));
        let mut bands = div().flex_row().gap(3.).flex_wrap();
        for (i, name) in [
            "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Magenta",
        ]
        .into_iter()
        .enumerate()
        {
            bands = bands.child(
                self.raw_button(cx, &format!("raw-band-{i}"), name, move |d| d.hsl_band = i)
                    .bg(if band == i {
                        Color::rgb8(40, 85, 130)
                    } else {
                        Color::rgb8(48, 48, 48)
                    }),
            );
        }
        panel = panel
            .child(text("HSL bands").text_size(13.))
            .child(bands)
            .child(self.raw_fields(
                cx,
                &[
                    ("Hue", Field::Hsl(band, 0), (-100., 100.)),
                    ("Band saturation", Field::Hsl(band, 1), (-100., 100.)),
                    ("Lightness", Field::Hsl(band, 2), (-100., 100.)),
                ],
            ));
        let mut mix = self.raw_fields(
            cx,
            &[
                ("Red mix", Field::Mix(0), (-1., 2.)),
                ("Green mix", Field::Mix(1), (-1., 2.)),
                ("Blue mix", Field::Mix(2), (-1., 2.)),
            ],
        );
        if !mono {
            mix.disable_subtree();
            mix = mix.opacity(0.45);
        }
        panel = panel
            .child(
                Self::check_control("Monochrome", mono).on_click(cx.listener(
                    "raw-monochrome",
                    |this, cx| {
                        if let Some(d) = &mut this.develop {
                            d.edit(|s| s.monochrome = !s.monochrome);
                        }
                        cx.invalidate();
                    },
                )),
            )
            .child(mix)
            .child(self.raw_fields(
                cx,
                &[
                    ("Shadow hue", Field::Shadow(0), (0., 360.)),
                    ("Shadow saturation", Field::Shadow(1), (0., 100.)),
                    ("Highlight hue", Field::Highlight(0), (0., 360.)),
                    ("Highlight saturation", Field::Highlight(1), (0., 100.)),
                    ("Tone balance", Field::ToneBalance, (-100., 100.)),
                ],
            ));
        panel
    }
    pub(super) fn raw_detail(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        div()
            .flex_col()
            .gap(8.)
            .child(text("Use 100% or Full preview to evaluate sensor-scale detail.").text_size(12.))
            .child(self.raw_fields(
                cx,
                &[
                    ("Luminance noise", Field::LuminanceNoise, (0., 100.)),
                    ("Chroma noise", Field::ColorNoise, (0., 100.)),
                    ("Sharpening", Field::Sharpen, (0., 200.)),
                    ("Sharpen radius", Field::SharpenRadius, (0.3, 5.)),
                    ("Sharpen threshold", Field::SharpenThreshold, (0., 1.)),
                ],
            ))
    }
    pub(super) fn raw_lens(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        self.raw_fields(
            cx,
            &[
                ("Distortion", Field::Distortion, (-100., 100.)),
                ("Red/cyan CA", Field::ChromaticRed, (-100., 100.)),
                ("Blue/yellow CA", Field::ChromaticBlue, (-100., 100.)),
                ("Purple defringe", Field::Defringe, (0., 100.)),
                ("Vignette", Field::Vignette, (-100., 100.)),
                ("Rotation", Field::Rotation, (-45., 45.)),
                (
                    "Horizontal perspective",
                    Field::Perspective(0),
                    (-100., 100.),
                ),
                ("Vertical perspective", Field::Perspective(1), (-100., 100.)),
                ("Crop left", Field::Crop(0), (0., 0.99)),
                ("Crop top", Field::Crop(1), (0., 0.99)),
                ("Crop right", Field::Crop(2), (0.01, 1.)),
                ("Crop bottom", Field::Crop(3), (0.01, 1.)),
            ],
        )
        .child(self.raw_crop_presets(cx))
    }
    pub(super) fn raw_masks(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let Some(d) = &self.develop else {
            return div();
        };
        let mut panel = div().flex_col().gap(8.);
        let mut add = div().flex_row().gap(4.);
        for (name, kind) in [
            ("Linear", raw::OverlayKind::Linear),
            ("Radial", raw::OverlayKind::Radial),
            ("Brush", raw::OverlayKind::Brush),
        ] {
            add=add.child(self.raw_button(cx,&format!("raw-add-{name}"),name,move |d| {
                if d.settings.overlays.len()>=32 {d.error=Some("A RAW image supports at most 32 local masks. Remove a mask before adding another.".into());return;}
                let index=d.settings.overlays.len();d.edit(|s|s.overlays.push(raw::Overlay {name:format!("{name} {}",index+1),kind,..Default::default()}));d.selected_mask=Some(index);d.draw_mask=true;d.show_mask=true;d.picker=false;
            }).disabled(d.settings.overlays.len()>=32));
        }
        panel = panel.child(add);
        for (i, mask) in d.settings.overlays.iter().enumerate() {
            panel = panel.child(
                self.raw_button(cx, &format!("raw-mask-{i}"), &mask.name, move |d| {
                    d.selected_mask = Some(i)
                })
                .bg(if d.selected_mask == Some(i) {
                    Color::rgb8(40, 85, 130)
                } else {
                    Color::rgb8(48, 48, 48)
                }),
            );
        }
        if let Some((i, mask)) = d
            .selected_mask
            .and_then(|i| d.settings.overlays.get(i).map(|m| (i, m)))
        {
            panel = panel
                .child(
                    Self::text_field(mask.name.clone())
                        .id("raw-mask-name")
                        .on_input(cx.input_listener("raw-mask-name", move |this, value, cx| {
                            if let Some(d) = &mut this.develop {
                                d.edit(|s| {
                                    if let Some(m) = s.overlays.get_mut(i) {
                                        m.name = value.to_string();
                                    }
                                });
                            }
                            cx.invalidate();
                        })),
                )
                .child(
                    div()
                        .flex_row()
                        .gap(6.)
                        .child(self.raw_button(
                            cx,
                            "raw-draw-mask",
                            if d.draw_mask {
                                "Stop drawing"
                            } else {
                                "Draw mask"
                            },
                            |d| {
                                d.draw_mask = !d.draw_mask;
                                d.picker = false;
                            },
                        ))
                        .child(self.raw_button(cx, "raw-delete-mask", "Delete", move |d| {
                            d.edit(|s| {
                                s.overlays.remove(i);
                            });
                            d.selected_mask = None;
                            d.draw_mask = false;
                        })),
                );
            for (id, label, checked) in [
                ("raw-mask-enabled", "Enabled", mask.enabled),
                ("raw-mask-invert", "Invert", mask.invert),
                ("raw-mask-show", "Show mask", d.show_mask),
            ] {
                panel = panel.child(Self::check_control(label, checked).on_click(cx.listener(
                    id,
                    move |this, cx| {
                        if let Some(d) = &mut this.develop {
                            if id == "raw-mask-show" {
                                d.show_mask = !d.show_mask;
                            } else {
                                d.edit(|s| {
                                    if let Some(m) = s.overlays.get_mut(i) {
                                        if id == "raw-mask-enabled" {
                                            m.enabled = !m.enabled;
                                        } else {
                                            m.invert = !m.invert;
                                        }
                                    }
                                });
                            }
                        }
                        cx.invalidate();
                    },
                )));
            }
            panel = panel.child(self.raw_fields(
                cx,
                &[
                    (
                        "Local exposure",
                        Field::Mask(i, MaskField::Exposure),
                        (-10., 10.),
                    ),
                    ("Warmth", Field::Mask(i, MaskField::Warmth), (-100., 100.)),
                    (
                        "Local saturation",
                        Field::Mask(i, MaskField::Saturation),
                        (-100., 100.),
                    ),
                ],
            ));
            if mask.kind == raw::OverlayKind::Brush {
                panel = panel.child(self.raw_field(
                    cx,
                    "Brush radius",
                    Field::Mask(i, MaskField::Radius),
                    (0.001, 1.),
                ));
            }
            if mask.kind != raw::OverlayKind::Linear {
                panel = panel.child(self.raw_field(
                    cx,
                    "Feather",
                    Field::Mask(i, MaskField::Feather),
                    (0.01, 1.),
                ));
            }
            if mask.kind == raw::OverlayKind::Brush {
                panel = panel.child(self.raw_button(
                    cx,
                    "raw-clear-brush",
                    "Clear brush strokes",
                    move |d| d.edit(|s| s.overlays[i].points.clear()),
                ));
            }
        }
        panel.child(text("Drag on the image to draw the selected mask. Brush radius and feather affect all strokes in that mask.").text_size(12.))
    }
    pub(super) fn raw_info(&self) -> Element {
        let mut panel = div().flex_col().gap(10.);
        if let Some(r) = self.develop.as_ref().and_then(|d| d.ready.as_ref()) {
            let m = &r.asset.metadata;
            for (label, value) in [
                ("Camera", m.camera.clone()),
                ("Lens", m.lens.clone()),
                ("Dimensions", format!("{} × {}", m.width, m.height)),
                ("Decoded depth", format!("{} bits", m.bits)),
                ("ISO", m.iso.map(|v| v.to_string()).unwrap_or_default()),
                (
                    "Aperture",
                    m.aperture.map(|v| format!("f/{v:.1}")).unwrap_or_default(),
                ),
                (
                    "Shutter",
                    m.shutter.map(|v| format!("{v:.5} s")).unwrap_or_default(),
                ),
                (
                    "Focal length",
                    m.focal_length
                        .map(|v| format!("{v:.1} mm"))
                        .unwrap_or_default(),
                ),
                (
                    "Embedded source",
                    format!("{:.1} MiB", r.asset.bytes.len() as f64 / 1048576.),
                ),
            ] {
                panel = panel.child(
                    div()
                        .flex_col()
                        .gap(2.)
                        .child(
                            text(label)
                                .text_size(11.)
                                .text_color(Color::rgb8(150, 158, 170)),
                        )
                        .child(
                            text(if value.is_empty() {
                                "Unknown".into()
                            } else {
                                value
                            })
                            .text_size(13.),
                        ),
                );
            }
        }
        panel
    }
}
