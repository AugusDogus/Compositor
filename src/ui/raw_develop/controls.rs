use super::fields::Field;
use super::*;

impl Editor {
    pub(super) fn raw_button(
        &self,
        cx: &mut ViewContext<'_, Self>,
        id: &str,
        label: &str,
        change: impl Fn(&mut Develop) + Send + Sync + 'static,
    ) -> Element {
        Self::control(label.to_owned())
            .text_size(12.)
            .on_click(cx.listener(id.to_owned(), move |this, cx| {
                if let Some(d) = &mut this.develop
                    && d.finish_numeric_input()
                {
                    change(d);
                }
                cx.invalidate();
            }))
    }
    pub(super) fn raw_fields(
        &self,
        cx: &mut ViewContext<'_, Self>,
        fields: &[(&str, Field, (f32, f32))],
    ) -> Element {
        let mut panel = div().flex_col().gap(6.);
        for (label, field, range) in fields {
            panel = panel.child(self.raw_field(cx, label, *field, *range));
        }
        panel
    }
    fn raw_histogram(&self) -> Element {
        let Some(r) = self.develop.as_ref().and_then(|d| d.ready.as_ref()) else {
            return div();
        };
        let bins = r.histogram;
        let clip = r.clipping;
        let graph = quickgui::canvas(move |bounds, painter| {
            let max = bins.iter().flatten().copied().max().unwrap_or(1).max(1) as f32;
            for (channel, color) in [
                Color::rgb8(240, 85, 85),
                Color::rgb8(95, 210, 115),
                Color::rgb8(80, 145, 250),
            ]
            .into_iter()
            .enumerate()
            {
                let mut path = quickgui::PathBuilder::stroke(1.);
                for (x, count) in bins[channel].iter().enumerate() {
                    let p = quickgui::Point::new(
                        x as f32 * bounds.width / 255.,
                        bounds.height - *count as f32 / max * bounds.height,
                    );
                    if x == 0 {
                        path.move_to(p);
                    } else {
                        path.line_to(p);
                    }
                }
                if let Ok(path) = path.build() {
                    painter.paint_path(&path, color);
                }
            }
        })
        .w(260.)
        .h(76.)
        .bg(Color::rgb8(25, 27, 31));
        div().flex_col().gap(4.).child(graph).child(
            text(format!(
                "Clipped shadows {:.1}% · highlights {:.1}%",
                clip[0], clip[1]
            ))
            .text_size(10.),
        )
    }
    pub(in crate::ui) fn raw_workspace(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let Some(d) = &self.develop else {
            return div();
        };
        let busy = d.committing || d.ready.is_none();
        let panel = d.panel;
        let title = format!("Develop RAW · {}", d.title);
        let mut toolbar = div()
            .flex_row()
            .items_center()
            .gap(6.)
            .p(8.)
            .h(44.)
            .flex_shrink_0();
        for (label, compare) in [
            ("Edited", Compare::Edited),
            ("Original", Compare::Original),
            ("Split", Compare::Split),
            ("Side by side", Compare::SideBySide),
        ] {
            toolbar = toolbar.child(
                self.raw_button(cx, &format!("raw-compare-{label}"), label, move |d| {
                    d.compare = compare;
                    d.fit = true;
                })
                .bg(if d.compare == compare {
                    Color::rgb8(40, 85, 130)
                } else {
                    Color::rgb8(48, 48, 48)
                }),
            );
        }
        toolbar = toolbar
            .child(div().flex_1())
            .child(self.raw_button(cx, "raw-fit", "Fit", |d| {
                d.fit = true;
            }))
            .child(self.raw_button(cx, "raw-actual", "100%", |d| {
                d.full_preview = true;
                d.fit = false;
                d.zoom = 1.;
                d.pan = [0.; 2];
                d.changed();
            }))
            .child(
                Self::check_control("Full preview", d.full_preview).on_click(cx.listener(
                    "raw-full",
                    |this, cx| {
                        if let Some(d) = &mut this.develop {
                            d.full_preview = !d.full_preview;
                            d.changed();
                        }
                        cx.invalidate();
                    },
                )),
            )
            .child(
                Self::check_control("Clipping", d.clipping).on_click(cx.listener(
                    "raw-clipping",
                    |this, cx| {
                        if let Some(d) = &mut this.develop {
                            d.clipping = !d.clipping;
                        }
                        cx.invalidate();
                    },
                )),
            );
        let mut tabs = div().flex_row().gap(4.).flex_wrap();
        for (i, name) in ["Basic", "Tone", "Detail", "Lens", "Masks", "Info"]
            .into_iter()
            .enumerate()
        {
            tabs = tabs.child(
                self.raw_button(cx, &format!("raw-panel-{i}"), name, move |d| d.panel = i)
                    .bg(if panel == i {
                        Color::rgb8(40, 85, 130)
                    } else {
                        Color::rgb8(48, 48, 48)
                    }),
            );
        }
        let mut controls = match panel {
            0 => self.raw_basic(cx),
            1 => self.raw_tone(cx),
            2 => self.raw_detail(cx),
            3 => self.raw_lens(cx),
            4 => self.raw_masks(cx),
            _ => self.raw_info(),
        };
        if busy {
            controls.disable_subtree();
        }
        let sidebar = div()
            .w(292.)
            .flex_shrink_0()
            .flex_col()
            .gap(10.)
            .p(12.)
            .bg(Color::rgb8(35, 37, 42))
            .child(self.raw_histogram())
            .child(self.raw_builtin_presets(cx))
            .child(tabs)
            .child(controls.flex_1().min_h(0.).overflow_y_scroll());
        let mut footer = div()
            .flex_row()
            .gap(6.)
            .items_center()
            .p(8.)
            .h(48.)
            .flex_shrink_0()
            .child(
                self.raw_button(cx, "raw-undo", "Undo", |d| d.history(false))
                    .disabled(busy || d.undo.is_empty()),
            )
            .child(
                self.raw_button(cx, "raw-redo", "Redo", |d| d.history(true))
                    .disabled(busy || d.redo.is_empty()),
            )
            .child(
                self.raw_button(cx, "raw-reset", "Reset", |d| {
                    d.edit(|s| *s = DevelopSettings::default());
                    d.selected_mask = None;
                })
                .disabled(busy),
            );
        for (id, label, kind) in [
            (
                "raw-load-preset",
                "Load preset",
                dialogs::Dialog::LoadPreset,
            ),
            (
                "raw-save-preset",
                "Save preset",
                dialogs::Dialog::SavePreset,
            ),
            ("raw-export", "16-bit TIFF…", dialogs::Dialog::Export),
        ] {
            footer = footer.child(
                Self::control(label)
                    .text_size(12.)
                    .disabled(busy)
                    .on_click(cx.listener(id, move |this, cx| this.raw_dialog(kind, cx))),
            );
        }
        footer = footer
            .child(div().flex_1())
            .child(
                Self::control("Cancel").on_click(cx.listener("raw-cancel", |this, cx| {
                    this.cancel_develop();
                    cx.invalidate();
                })),
            )
            .child(
                self.raw_button(cx, "raw-apply", "Develop", |d| {
                    d.finish_gesture(false);
                    d.request = Some(Request::Apply);
                    d.committing = true;
                })
                .disabled(busy)
                .bg(Color::rgb8(0, 122, 255)),
            );
        let status = if let Some(error) = &d.error {
            error.clone()
        } else if d.committing {
            "Developing at full resolution…".into()
        } else if d.ready.is_none() {
            "Decoding camera sensor data…".into()
        } else if d.picker {
            "Click a neutral gray or white area.".into()
        } else if d.draw_mask {
            "Drag to draw the selected local mask.".into()
        } else if d.running {
            "Updating preview…".into()
        } else {
            d.notice.clone()
        };
        let status = text(status)
            .h(48.)
            .flex_shrink_0()
            .overflow_hidden()
            .text_size(12.)
            .px(12.)
            .py(5.)
            .text_color(if d.error.is_some() {
                Color::rgb8(255, 150, 140)
            } else {
                Color::rgb8(174, 180, 190)
            });
        div()
            .id("workspace")
            .focusable()
            .size_full()
            .flex_col()
            .bg(Color::rgb8(24, 26, 30))
            .text_color(Color::rgb8(224, 228, 234))
            .on_key_down(cx.key_down_listener("workspace", |this, event, cx| {
                if let Some(d) = &mut this.develop {
                    if event.modifiers.contains(Modifiers::CONTROL)
                        && matches!(&event.key,Key::Character(c)if c.eq_ignore_ascii_case("z"))
                    {
                        d.history(event.modifiers.contains(Modifiers::SHIFT));
                        cx.prevent_default();
                    } else if event.key == Key::Escape {
                        this.cancel_develop();
                        cx.prevent_default();
                    }
                }
                cx.invalidate();
            }))
            .child(self.window_titlebar(cx, text(title).text_size(13.).px(12.)))
            .child(toolbar)
            .child(
                div()
                    .flex_row()
                    .h((cx.size().height - 168.).max(1.))
                    .flex_shrink_0()
                    .min_h(0.)
                    .child(self.raw_canvas(cx))
                    .child(sidebar),
            )
            .child(status)
            .child(footer)
    }
}
