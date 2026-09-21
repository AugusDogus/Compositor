use super::*;
use compositor::{
    adjustment::{ColorRange, HueSaturation},
    hue_sample::{HueSample, HueTarget, sampled_hue},
};
use quickgui::{MouseButton, PointerEvent, PointerPhase};

#[derive(Clone, Copy, Default)]
pub(super) enum HueSampling {
    #[default]
    Off,
    Sample(HueSample),
    Target,
    Dragging {
        start: f64,
        target: HueTarget,
    },
}

impl HueSampling {
    fn active(self, button: Self) -> bool {
        match (self, button) {
            (Self::Sample(current), Self::Sample(mode)) => current == mode,
            (Self::Target | Self::Dragging { .. }, Self::Target) => true,
            _ => false,
        }
    }
}

impl Editor {
    pub(super) fn adjustment_sampling(&self) -> bool {
        self.levels_sample_mode().is_some()
            || self
                .adjustment_edit
                .as_ref()
                .is_some_and(|edit| !matches!(edit.hue_sampling, HueSampling::Off))
    }

    pub(super) fn stop_adjustment_sampling(&mut self) {
        self.stop_levels_sampling();
        if let Some(edit) = &mut self.adjustment_edit {
            edit.hue_sampling = HueSampling::Off;
        }
        self.status = "Adjust the color settings, then Apply or Cancel.".into();
    }

    fn arm_hue_sampling(&mut self, sampling: HueSampling) -> Result<()> {
        self.preview_adjustment()?;
        if let Some(edit) = &mut self.adjustment_edit {
            let hsv = edit
                .settings
                .hsv_settings
                .get_or_insert_with(HueSaturation::default);
            if hsv.colorize
                || (matches!(sampling, HueSampling::Sample(_)) && hsv.range == ColorRange::Master)
            {
                return Ok(());
            }
            if edit.hue_sampling.active(sampling) {
                self.stop_adjustment_sampling();
                return Ok(());
            }
            edit.hue_sampling = sampling;
        }
        self.status = match sampling {
            HueSampling::Sample(_) => "Click a colored pixel to change the selected hue range. Click the eyedropper again to stop.",
            _ => "Drag horizontally to adjust that color's saturation. Hold Ctrl to adjust hue. Click the targeted adjustment button again to stop.",
        }.into();
        Ok(())
    }

    pub(super) fn adjustment_sample_pointer(&mut self, event: &PointerEvent) -> Result<()> {
        let (zoom, offset) = self.viewport(event.size.width, event.size.height);
        let point = [
            (event.local_position.x as f64 - offset[0]) / zoom,
            (event.local_position.y as f64 - offset[1]) / zoom,
        ];
        if self.levels_sample_mode().is_some() {
            if event.phase == PointerPhase::Down && event.button == MouseButton::Left {
                self.sample_levels(point)?;
            }
            return Ok(());
        }
        let sampling = self
            .adjustment_edit
            .as_ref()
            .map_or(HueSampling::Off, |edit| edit.hue_sampling);
        if event.phase == PointerPhase::Cancel || event.phase == PointerPhase::Up {
            if matches!(sampling, HueSampling::Dragging { .. })
                && let Some(edit) = &mut self.adjustment_edit
            {
                edit.hue_sampling = HueSampling::Target;
            }
            return Ok(());
        }
        if event.phase == PointerPhase::Down && event.button == MouseButton::Left {
            let doc = &self.session().document;
            if point
                .iter()
                .zip([doc.width, doc.height])
                .any(|(v, size)| !v.is_finite() || *v < 0. || *v >= size as f64)
            {
                return Ok(());
            }
            let Some(hue) = sampled_hue(compositor::render::sample(doc, point)) else {
                self.status =
                    "Choose a colored, nontransparent pixel. Neutral colors have no hue to sample."
                        .into();
                return Ok(());
            };
            if let Some(edit) = &mut self.adjustment_edit {
                let settings = edit
                    .settings
                    .hsv_settings
                    .get_or_insert_with(HueSaturation::default);
                match sampling {
                    HueSampling::Sample(mode) => mode.apply(settings, hue)?,
                    HueSampling::Target => {
                        if let Some(target) = HueTarget::begin(settings, hue) {
                            edit.hue_sampling = HueSampling::Dragging {
                                start: event.local_position.x as f64,
                                target,
                            };
                        }
                    }
                    _ => return Ok(()),
                }
            }
        } else if event.phase == PointerPhase::Move
            && let HueSampling::Dragging { start, target } = sampling
        {
            if let Some(edit) = &mut self.adjustment_edit {
                let settings = edit
                    .settings
                    .hsv_settings
                    .get_or_insert_with(HueSaturation::default);
                target.update(
                    settings,
                    event.local_position.x as f64 - start,
                    event.modifiers.contains(Modifiers::CONTROL),
                )?;
            }
        } else {
            return Ok(());
        }
        self.show_adjustment_fields();
        self.preview_adjustment()
    }

    pub(super) fn hue_sample_controls(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let settings = self
            .adjustment_edit
            .as_ref()
            .and_then(|edit| edit.settings.hsv_settings.as_ref());
        let colorize = settings.is_some_and(|settings| settings.colorize);
        let range = settings.map_or(ColorRange::Master, |settings| settings.range);
        let workspace = cx.focus_handle("workspace");
        let sampling = self
            .adjustment_edit
            .as_ref()
            .map_or(HueSampling::Off, |e| e.hue_sampling);
        let mut row = div().flex_row().items_center().gap(6.);
        for (label, mode) in [
            ("Sample range", HueSampling::Sample(HueSample::Center)),
            ("Add color", HueSampling::Sample(HueSample::Include)),
            ("Remove color", HueSampling::Sample(HueSample::Exclude)),
            ("Target color", HueSampling::Target),
        ] {
            if colorize || (matches!(mode, HueSampling::Sample(_)) && range == ColorRange::Master) {
                continue;
            }
            let id = format!("hue-{label}");
            if matches!(mode, HueSampling::Target) && range != ColorRange::Master {
                row = row.child(div().w(1.).h(16.).bg(Color::rgb8(65, 65, 65)));
            }
            row = row.child(
                (if matches!(mode, HueSampling::Target) {
                    Icon::Pointer
                } else {
                    Icon::Pipette
                })
                .button(label)
                .accessibility_label(if matches!(mode, HueSampling::Target) { "Targeted adjustment" } else if matches!(mode, HueSampling::Sample(HueSample::Center)) { "Sample color" } else { label })
                .tooltip(match mode {
                    HueSampling::Sample(HueSample::Center) => "Click the image to center this range on that color",
                    HueSampling::Sample(HueSample::Include) => "Click the image to widen this range to include that color",
                    HueSampling::Sample(HueSample::Exclude) => "Click the image to narrow this range to exclude that color",
                    _ => "Targeted adjustment: drag on the image to change that color's saturation, or its hue with Ctrl held",
                })
                .w(24.)
                .h(20.)
                .relative()
                .rounded(4.)
                .selected(sampling.active(mode))
                .selected_style(|s| s.bg(Color::rgba8(65, 107, 158, 64)))
                .child(if label == "Add color" {
                    Icon::Plus.element(8.).absolute().right(0.).bottom(0.)
                } else if label == "Remove color" {
                    text("−").text_size(11.).absolute().right(0.).bottom(0.)
                } else {
                    div()
                })
                .id(id.clone())
                .on_click(cx.listener(id, move |this, cx| {
                    if let Err(error) = this.arm_hue_sampling(mode)
                        && let Some(Form::Edit { error: message, .. }) = &mut this.modal
                    {
                        *message = error.to_string();
                    }
                    cx.focus(workspace);
                    this.changed(cx);
                })),
            );
        }
        row
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, WindowOptions};

    #[test]
    fn sampling_buttons_toggle_and_switch_without_changing_the_preview_or_history() {
        let mut editor = Editor::with_test_document();
        compositor::edits::fill(
            &mut editor.session_mut().document,
            [80, 120, 160, 255],
            false,
            false,
        )
        .unwrap();
        let original = editor.session().document.clone();
        editor.open_pixel_adjustment(Kind::HueSaturation).unwrap();
        editor.choose_adjustment_channel(1).unwrap();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Hue sampling toggles").size(1280., 900.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        let canvas = cx.element_bounds(window, "canvas").unwrap();
        let canvas_point = quickgui::Point::new(canvas.x + 20., canvas.y + canvas.height - 20.);
        let settings = cx
            .read(view, |e| {
                e.adjustment_edit.as_ref().unwrap().settings.clone()
            })
            .unwrap();
        for (label, mode) in [
            ("Sample range", HueSampling::Sample(HueSample::Center)),
            ("Add color", HueSampling::Sample(HueSample::Include)),
            ("Remove color", HueSampling::Sample(HueSample::Exclude)),
            ("Target color", HueSampling::Target),
        ] {
            let id = format!("hue-{label}");
            let before = cx.element_bounds(window, id.clone()).unwrap();
            cx.click(window, id.clone()).unwrap();
            assert!(
                cx.read(view, |e| e
                    .adjustment_edit
                    .as_ref()
                    .unwrap()
                    .hue_sampling
                    .active(mode))
                    .unwrap()
            );
            assert_eq!(cx.element_bounds(window, id.clone()).unwrap(), before);
            cx.update(view, |_, cx| cx.focus_window(window)).unwrap();
            cx.simulate_mouse_move(
                window,
                "canvas",
                quickgui::MouseMoveEvent {
                    position: canvas_point,
                    pressed_button: None,
                    modifiers: Modifiers::empty(),
                },
            )
            .unwrap();
            if matches!(mode, HueSampling::Target) {
                assert_eq!(
                    cx.visual(window)
                        .unwrap()
                        .cursor_style_at(canvas_point)
                        .unwrap(),
                    Some(quickgui::CursorStyle::ResizeLeftRight)
                );
                assert_eq!(
                    cx.window_state(window).unwrap().cursor_override,
                    Some(quickgui::CursorOverrideId::System(
                        quickgui::CursorStyle::ResizeLeftRight
                    ))
                );
            } else {
                let scale = cx.window_state(window).unwrap().scale_factor;
                let eyedropper = cx
                    .update(view, |e, _| {
                        e.cursor_art
                            .image(cursor_art::Glyph::Eyedropper, scale)
                            .unwrap()
                            .id()
                    })
                    .unwrap();
                assert_eq!(
                    cx.window_state(window).unwrap().cursor_override,
                    Some(quickgui::CursorOverrideId::Image(eyedropper)),
                    "{label}"
                );
            }
            cx.click(window, id.clone()).unwrap();
            assert!(!cx.read(view, |e| e.adjustment_sampling()).unwrap());
            cx.click(window, id).unwrap();
        }
        cx.click(window, "hue-Target color").unwrap();
        cx.read(view, |e| {
            assert!(!e.adjustment_sampling());
            assert_eq!(e.adjustment_edit.as_ref().unwrap().settings, settings);
            assert_eq!(e.session().document, original);
            assert!(e.session().undo_label().is_none());
        })
        .unwrap();
        cx.click(window, "hue-Target color").unwrap();
        cx.simulate_keystrokes(window, "escape").unwrap();
        cx.read(view, |e| {
            assert!(e.adjustment_edit.is_none());
            assert_eq!(e.status, e.tool_hint());
            assert_eq!(e.session().document, original);
        })
        .unwrap();
    }
}
