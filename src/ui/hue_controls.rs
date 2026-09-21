use super::*;
use compositor::{
    adjustment::{ColorRange, HueBand, HueSaturation, hsl_to_rgb},
    hue_band::HueHandle,
};
use quickgui::{MouseButton, PointerEvent, PointerPhase};

#[derive(Clone, Copy)]
enum HueOption {
    Colorize,
    Invert,
    Reset,
}

#[derive(Clone, Copy)]
pub(super) struct HueBandDrag {
    handle: HueHandle,
    original: HueBand,
    range: ColorRange,
}

impl Editor {
    pub(super) fn hue_options(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let settings = self
            .adjustment_edit
            .as_ref()
            .and_then(|e| e.settings.hsv_settings.as_ref())
            .cloned()
            .unwrap_or_default();
        let mut row = div().flex_row().items_center().gap(18.);
        let mut controls = div().flex_col().gap(16.);
        for (id, label, option) in [
            (
                "hue-colorize",
                if settings.colorize {
                    "Colorize: On"
                } else {
                    "Colorize: Off"
                },
                HueOption::Colorize,
            ),
            (
                "hue-invert",
                if settings.invert_range {
                    "Outside range: On"
                } else {
                    "Outside range: Off"
                },
                HueOption::Invert,
            ),
            ("hue-reset", "Reset", HueOption::Reset),
        ] {
            if matches!(option, HueOption::Invert)
                && (settings.range == ColorRange::Master || settings.colorize)
            {
                continue;
            }
            let control = match option {
                HueOption::Colorize => Self::check_control("Colorize", settings.colorize)
                    .text_size(13.)
                    .line_height(16.),
                HueOption::Invert => {
                    Self::check_control("Apply outside this range instead", settings.invert_range)
                        .text_size(13.)
                        .line_height(16.)
                }
                HueOption::Reset => Self::control(label),
            };
            if matches!(option, HueOption::Reset) {
                row = row.child(
                    Self::check_control(
                        "Preview",
                        self.adjustment_edit.as_ref().is_some_and(|e| e.preview),
                    )
                    .text_size(13.)
                    .line_height(16.)
                    .on_click(cx.listener("adjustment-preview", |this, cx| {
                        if let Some(edit) = &mut this.adjustment_edit {
                            edit.preview = !edit.preview;
                        }
                        this.refresh_adjustment();
                        this.changed(cx);
                    })),
                );
            }
            let control = control.id(id).on_click(cx.listener(id, move |this, cx| {
                if let Err(error) = this.change_hue_option(option)
                    && let Some(Form::Edit { error: message, .. }) = &mut this.modal
                {
                    *message = error.to_string();
                }
                this.changed(cx);
            }));
            if matches!(option, HueOption::Invert) {
                controls = controls.child(control);
            } else {
                row = row.child(control);
            }
        }
        controls.child(row)
    }

    fn change_hue_option(&mut self, option: HueOption) -> Result<()> {
        if matches!(option, HueOption::Invert) {
            self.preview_adjustment()?;
        }
        if let Some(edit) = &mut self.adjustment_edit {
            let settings = edit
                .settings
                .hsv_settings
                .get_or_insert_with(HueSaturation::default);
            match option {
                HueOption::Invert => settings.invert_range = !settings.invert_range,
                HueOption::Reset | HueOption::Colorize => {
                    let colorize = if matches!(option, HueOption::Colorize) {
                        !settings.colorize
                    } else {
                        settings.colorize
                    };
                    *settings = HueSaturation {
                        colorize,
                        ..HueSaturation::default()
                    };
                    if colorize {
                        settings.adjustments.push((
                            ColorRange::Master,
                            compositor::adjustment::RangeAdjustment {
                                saturation: 25.,
                                ..Default::default()
                            },
                        ));
                    }
                }
            }
            edit.hue_band_drag = None;
            edit.hue_sampling = super::hue_sampling::HueSampling::Off;
        }
        self.show_adjustment_fields();
        self.preview_adjustment()
    }

    pub(super) fn hue_spectrum(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let Some(settings) = self
            .adjustment_edit
            .as_ref()
            .and_then(|e| e.settings.hsv_settings.as_ref())
            .filter(|s| s.range != ColorRange::Master && !s.colorize)
        else {
            return div();
        };
        let band = settings.band(settings.range);
        let settings = settings.clone();
        let pointer = cx.pointer_listener("hue-band-handles", |this, event, cx| {
            if let Err(error) = this.hue_band_pointer(event)
                && let Some(Form::Edit { error: message, .. }) = &mut this.modal
            {
                *message = error.to_string();
            }
            this.changed(cx);
        });
        let before: Vec<_> = (0..72).map(|i| hue_color(i as f64 * 5.)).collect();
        let after: Vec<_> = (0..72)
            .map(|i| hue_color(settings.shifted_hue(i as f64 * 5.)))
            .collect();
        div()
            .flex_col()
            .gap(5.)
            .child(spectrum_bar(before).id("hue-spectrum-before"))
            .child(
                quickgui::canvas(move |bounds, painter| {
                    for (handle, angle) in band.handles() {
                        let x = (angle / 360.) as f32 * bounds.width;
                        let inner = matches!(handle, HueHandle::RangeStart | HueHandle::RangeEnd);
                        let rect = if inner {
                            quickgui::Rect::new(x - 1., 0., 2., bounds.height)
                        } else {
                            quickgui::Rect::new(x - 3.5, bounds.height / 2. - 2.5, 7., 5.)
                        };
                        painter.fill_rect(rect, Color::rgb8(230, 230, 230));
                    }
                })
                .id("hue-band-handles")
                .w_full()
                .h(12.)
                .flex_shrink(0.)
                .cursor(quickgui::CursorStyle::Crosshair)
                .on_pointer(pointer),
            )
            .child(spectrum_bar(after).id("hue-spectrum-after"))
            .child(
                text(
                    band.handles()
                        .map(|(_, degrees)| format!("{degrees:.0}°"))
                        .join("   "),
                )
                .text_size(10.)
                .line_height(13.)
                .font_features(
                    quickgui::FontFeatures::new().enable(quickgui::FontFeatureTag::TABULAR_NUMBERS),
                )
                .text_color(Color::rgb8(180, 180, 180)),
            )
    }

    fn hue_band_pointer(&mut self, event: &PointerEvent) -> Result<()> {
        if event.size.width <= 0. || !event.local_position.x.is_finite() {
            return Ok(());
        }
        let degrees = (event.local_position.x / event.size.width).clamp(0., 1.) as f64 * 360.;
        if event.phase == PointerPhase::Down {
            if event.button != MouseButton::Left {
                return Ok(());
            }
            self.preview_adjustment()?;
        }
        let Some(edit) = &mut self.adjustment_edit else {
            return Ok(());
        };
        let settings = edit
            .settings
            .hsv_settings
            .get_or_insert_with(HueSaturation::default);
        if settings.colorize || settings.range == ColorRange::Master {
            return Ok(());
        }
        if event.phase == PointerPhase::Down {
            let original = settings.band(settings.range);
            edit.hue_band_drag = Some(HueBandDrag {
                handle: original.nearest_handle(degrees),
                original,
                range: settings.range,
            });
        }
        let Some(drag) = edit.hue_band_drag else {
            return Ok(());
        };
        if settings.range != drag.range {
            edit.hue_band_drag = None;
            return Ok(());
        }
        let mut band = settings.band(settings.range);
        let changed = if event.phase == PointerPhase::Cancel {
            let changed = band != drag.original;
            band = drag.original;
            changed
        } else {
            band.set_handle(drag.handle, degrees)
        };
        if matches!(event.phase, PointerPhase::Cancel | PointerPhase::Up) {
            edit.hue_band_drag = None;
        }
        if changed {
            settings.bands.retain(|(range, _)| *range != settings.range);
            settings.bands.push((settings.range, band));
            self.show_adjustment_fields();
            self.preview_adjustment()?;
        }
        Ok(())
    }
}

fn hue_color(degrees: f64) -> Color {
    let rgb = hsl_to_rgb([degrees, 1., 0.5]).map(|v| (v * 255.).round() as u8);
    Color::rgb8(rgb[0], rgb[1], rgb[2])
}

fn spectrum_bar(colors: Vec<Color>) -> Element {
    quickgui::canvas(move |bounds, painter| {
        let width = bounds.width / colors.len() as f32;
        for (index, color) in colors.iter().enumerate() {
            painter.fill_rect_with_rounded_clip(
                quickgui::Rect::new(index as f32 * width, 0., width + 0.5, bounds.height),
                *color,
                bounds,
                3.,
            );
        }
    })
    .w_full()
    .h(16.)
    .flex_shrink(0.)
    .into_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, Point, WindowOptions};

    #[test]
    fn spectrum_corners_reveal_the_panel_and_retain_the_hue_slices() {
        let mut editor = Editor::with_test_document();
        editor.tabs[0].set_document(Document::new(8, 8).unwrap(), None);
        compositor::edits::fill(
            &mut editor.session_mut().document,
            [130, 90, 70, 255],
            false,
            false,
        )
        .unwrap();
        editor.open_pixel_adjustment(Kind::HueSaturation).unwrap();
        editor
            .adjustment_edit
            .as_mut()
            .unwrap()
            .settings
            .hsv_settings
            .as_mut()
            .unwrap()
            .range = ColorRange::Reds;
        editor.show_adjustment_fields();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Hue spectrum corners").size(1500., 900.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        let screenshot = cx.capture_screenshot(window).unwrap();
        let scale = screenshot.width() as f32 / 1500.;
        let pixel = |x: f32, y: f32| {
            screenshot
                .pixel((x * scale) as u32, (y * scale) as u32)
                .unwrap()
        };
        for id in ["hue-spectrum-before", "hue-spectrum-after"] {
            let bounds = cx.element_bounds(window, id).unwrap();
            assert_eq!(bounds.height, 16.);
            let background = pixel(bounds.x - 1., bounds.y + 8.);
            for x in [bounds.x, bounds.right() - 1. / scale] {
                for y in [bounds.y, bounds.bottom() - 1. / scale] {
                    assert_eq!(pixel(x, y), background, "Square spectrum corner in {id}");
                }
            }
            for (slice, expected) in [
                (0, [255, 0, 0, 255]),
                (12, [255, 255, 0, 255]),
                (24, [0, 255, 0, 255]),
                (36, [0, 255, 255, 255]),
                (48, [0, 0, 255, 255]),
                (60, [255, 0, 255, 255]),
                (71, [255, 0, 21, 255]),
            ] {
                assert_eq!(
                    pixel(
                        bounds.x + (slice as f32 + 0.5) / 72. * bounds.width,
                        bounds.y + 8.
                    ),
                    expected
                );
            }
            // Sample inside the three-point corner, clear of the antialiased top edge.
            assert_eq!(pixel(bounds.x + 3., bounds.y + 1.), [255, 0, 0, 255]);
        }
    }

    #[test]
    fn colorize_and_reset_use_swift_defaults_and_keep_one_undo_step() {
        let mut editor = Editor::with_test_document();
        let mut document = Document::new(2, 2).unwrap();
        compositor::edits::fill(&mut document, [128, 128, 128, 255], false, false).unwrap();
        editor.tabs = vec![Session::new(document.clone(), None).into()];
        editor.open_pixel_adjustment(Kind::HueSaturation).unwrap();
        let (mut cx, editor) = Application::new()
            .into_test_context(WindowOptions::new("Colorize").size(1280., 900.), editor)
            .unwrap();
        let window = editor.window_handle();
        cx.click(window, "adjustment-channel").unwrap();
        let red = cx
            .read(editor, |e| {
                e.adjustment_channel_menu
                    .item_element_id("adjustment-channel-items", 1)
                    .unwrap()
            })
            .unwrap();
        cx.click(window, red).unwrap();
        cx.click(window, "hue-invert").unwrap();
        cx.click(window, "hue-colorize").unwrap();
        cx.read(editor, |e| {
            let settings = e
                .adjustment_edit
                .as_ref()
                .unwrap()
                .settings
                .hsv_settings
                .as_ref()
                .unwrap();
            assert!(settings.colorize);
            assert_eq!(settings.range, ColorRange::Master);
            assert!(!settings.invert_range);
            assert_eq!(settings.adjustments[0].1.saturation, 25.);
            assert!(matches!(&e.modal, Some(Form::Edit { fields, .. }) if fields.len() == 3));
        })
        .unwrap();
        cx.update(editor, |e, _| {
            if let Some(Form::Edit { fields, .. }) = &mut e.modal {
                fields[1].1 = "-1".into();
            }
        })
        .unwrap();
        cx.click(window, "form-apply").unwrap();
        assert!(cx.read(editor, |e| matches!(&e.modal, Some(Form::Edit { error, .. }) if error.contains("saturation from 0"))).unwrap());
        cx.click(window, "hue-reset").unwrap();
        cx.click(window, "form-apply").unwrap();
        cx.read(editor, |e| {
            assert!(e.adjustment_edit.is_none());
            let pixel = e.session().document.layers[0].raster().unwrap()[(0, 0)];
            assert!(pixel[0] > pixel[1]);
            assert_eq!(pixel[1], pixel[2]);
            assert_eq!(e.session().undo_label(), Some("Hue/Saturation"));
        })
        .unwrap();
        cx.focus(window, "workspace").unwrap();
        cx.simulate_keystroke(
            window,
            quickgui::Keystroke::new(Key::Character("z".into()), Modifiers::CONTROL),
        )
        .unwrap();
        assert_eq!(
            cx.read(editor, |e| e.session().document.clone()).unwrap(),
            document
        );
        assert!(
            cx.read(editor, |e| e.session().undo_label().is_none())
                .unwrap()
        );
    }

    #[test]
    fn dragging_a_wrapped_band_updates_fields_and_cancel_restores_the_document() {
        let mut editor = Editor::with_test_document();
        let mut document = Document::new(20, 20).unwrap();
        compositor::edits::fill(&mut document, [130, 90, 70, 255], false, false).unwrap();
        editor.tabs = vec![Session::new(document.clone(), None).into()];
        editor.open_pixel_adjustment(Kind::HueSaturation).unwrap();
        let (mut cx, editor) = Application::new()
            .into_test_context(WindowOptions::new("Spectrum").size(1280., 900.), editor)
            .unwrap();
        let window = editor.window_handle();
        assert!(cx.element_bounds(window, "hue-band-handles").is_err());
        cx.click(window, "adjustment-channel").unwrap();
        let red = cx
            .read(editor, |e| {
                e.adjustment_channel_menu
                    .item_element_id("adjustment-channel-items", 1)
                    .unwrap()
            })
            .unwrap();
        cx.click(window, red).unwrap();
        let bounds = cx.element_bounds(window, "hue-band-handles").unwrap();
        let point = |hue| {
            Point::new(
                bounds.x + bounds.width * hue / 360.,
                bounds.y + bounds.height / 2.,
            )
        };
        cx.simulate_pointer_drag(window, "hue-band-handles", point(345.), point(360.))
            .unwrap();
        cx.read(editor, |e| {
            let edit = e.adjustment_edit.as_ref().unwrap();
            assert!(edit.hue_band_drag.is_none());
            assert_eq!(
                edit.settings
                    .hsv_settings
                    .as_ref()
                    .unwrap()
                    .band(ColorRange::Reds)
                    .range_start,
                0.
            );
            assert!(matches!(&e.modal, Some(Form::Edit { fields, .. }) if fields[4].1 == "0"));
        })
        .unwrap();
        cx.simulate_pointer_drag(window, "hue-band-handles", point(0.), point(30.))
            .unwrap();
        assert_eq!(
            cx.read(editor, |e| e
                .adjustment_edit
                .as_ref()
                .unwrap()
                .settings
                .hsv_settings
                .as_ref()
                .unwrap()
                .band(ColorRange::Reds)
                .range_start)
                .unwrap(),
            0.
        );
        cx.click(window, "form-cancel").unwrap();
        assert_eq!(
            cx.read(editor, |e| e.session().document.clone()).unwrap(),
            document
        );
    }
}
