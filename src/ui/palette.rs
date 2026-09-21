use super::*;

#[derive(Clone, Copy)]
pub(super) enum MaskSwatch {
    Foreground,
    Background,
}

impl MaskSwatch {
    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Foreground => "Mask foreground",
            Self::Background => "Mask background",
        }
    }

    fn anchor(self) -> quickgui::ElementId {
        match self {
            Self::Foreground => 320_u64.into(),
            Self::Background => "palette-background".into(),
        }
    }
}

impl Editor {
    pub(super) fn palette_colors(&self, mask: bool) -> [[u8; 4]; 2] {
        if mask {
            let black = [0, 0, 0, 255];
            let white = [255; 4];
            if self.tools.mask_paint_white {
                [white, black]
            } else {
                [black, white]
            }
        } else {
            [self.tools.brush.color, self.tools.background]
        }
    }

    pub(super) fn change_palette(&mut self, swap: bool) -> Result<()> {
        if self.pending || self.gesture.is_some() {
            return Ok(());
        }
        if self.tools.mask_target {
            self.tools.mask_paint_white = swap && !self.tools.mask_paint_white;
        } else if swap {
            std::mem::swap(&mut self.tools.brush.color, &mut self.tools.background);
        } else {
            self.tools.brush.color = [0, 0, 0, 255];
            self.tools.background = [255; 4];
        }
        self.refresh_gradient()
    }

    pub(super) fn sample_palette(&mut self, point: [f64; 2]) -> Result<()> {
        let doc = &self.session().document;
        if point
            .iter()
            .zip([doc.width, doc.height])
            .any(|(v, limit)| !v.is_finite() || *v < 0. || *v >= f64::from(limit))
        {
            return Ok(());
        }
        let pixel = compositor::render::sample(doc, point.map(|v| v.floor() + 0.5))?;
        if pixel[3] > 0. {
            self.tools.brush.color = pixel.map(|v| (v * 255.).round() as u8);
            self.tools.brush.color[3] = 255;
            self.refresh_gradient()?;
        }
        Ok(())
    }

    pub(super) fn mask_color_popover(
        &self,
        cx: &mut ViewContext<'_, Self>,
        target: MaskSwatch,
    ) -> Element {
        let mut controls = div().flex_row().gap(8.);
        for (white, label) in [(false, "Black · Hide"), (true, "White · Reveal")] {
            controls = controls.child(Self::control(label).on_click(cx.listener(
                format!("mask-color-{white}"),
                move |this, cx| {
                    this.tools.mask_paint_white = match target {
                        MaskSwatch::Foreground => white,
                        MaskSwatch::Background => !white,
                    };
                    this.modal = None;
                    let result = this.refresh_gradient();
                    this.operation_result(alerts::Operation::Paint, result, cx);
                },
            )));
        }
        let popup = quickgui::Popover::new(target.anchor(), "mask-color-popup", true)
            .side(quickgui::AnchorSide::Right)
            .align(quickgui::AnchorAlign::End);
        popup.surface_with(
            div()
                .flex_col()
                .gap(12.)
                .p(16.)
                .rounded(7.)
                .shadow(super::surfaces::menu_shadow())
                .bg(Color::rgb8(48, 48, 48))
                .border(1., Color::rgb8(82, 82, 82))
                .child(
                    popup.title_with(
                        text(target.title())
                            .font_bold()
                            .text_size(13.)
                            .line_height(16.),
                    ),
                )
                .child(controls)
                .on_dismiss(cx.dismiss_listener("mask-color-popup", |this, cx| {
                    this.modal = None;
                    cx.invalidate();
                })),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, Keystroke, WindowOptions};

    #[test]
    fn palette_shortcuts_and_mask_controls_preserve_pixel_colors() {
        let mut e = Editor::with_test_document();
        let mut document = Document::new(8, 8).unwrap();
        compositor::edits::add_mask(&mut document, false).unwrap();
        e.tabs = vec![Session::new(document, None).into()];
        e.tools.brush.color = [100, 20, 30, 255];
        e.tools.background = [40, 150, 60, 255];
        let original = e.session().document.clone();
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Palette").size(1280., 900.), e)
            .unwrap();
        let window = view.window_handle();
        cx.simulate_keystroke(
            window,
            Keystroke::new(Key::Character("x".into()), Modifiers::empty()),
        )
        .unwrap();
        let colors = cx.read(view, |e| e.palette_colors(false)).unwrap();
        assert_eq!(colors, [[40, 150, 60, 255], [100, 20, 30, 255]]);
        cx.update(view, |e, _| e.tools.mask_target = true).unwrap();
        cx.simulate_keystroke(
            window,
            Keystroke::new(Key::Character("x".into()), Modifiers::empty()),
        )
        .unwrap();
        assert_eq!(
            cx.read(view, |e| e.palette_colors(true)[0]).unwrap(),
            [255; 4]
        );
        cx.simulate_keystroke(
            window,
            Keystroke::new(Key::Character("d".into()), Modifiers::empty()),
        )
        .unwrap();
        assert_eq!(
            cx.read(view, |e| e.palette_colors(true)[0]).unwrap(),
            [0, 0, 0, 255]
        );
        cx.click(window, 320_u64).unwrap();
        cx.click(window, "mask-color-true").unwrap();
        assert_eq!(
            cx.read(view, |e| e.palette_colors(true)[0]).unwrap(),
            [255; 4]
        );
        cx.click(window, "palette-background").unwrap();
        let anchor = cx.element_bounds(window, "palette-background").unwrap();
        let popup = cx.element_bounds(window, "mask-color-popup").unwrap();
        let black = cx.element_bounds(window, "mask-color-false").unwrap();
        let white = cx.element_bounds(window, "mask-color-true").unwrap();
        assert!(popup.x >= anchor.x + anchor.width);
        assert!(popup.width < 300.);
        assert_eq!(black.y, white.y);
        assert!(white.x > black.x);
        cx.simulate_keystrokes(window, "escape").unwrap();
        assert!(cx.read(view, |e| e.modal.is_none()).unwrap());
        assert!(cx.read(view, |e| e.tools.mask_paint_white).unwrap());
        cx.click(window, "palette-background").unwrap();
        cx.simulate_keystrokes(window, "enter").unwrap();
        assert_eq!(
            cx.read(view, |e| e.palette_colors(true)).unwrap(),
            [[255; 4], [0, 0, 0, 255]]
        );
        cx.click(window, "palette-background").unwrap();
        cx.click(window, "mask-color-true").unwrap();
        assert_eq!(
            cx.read(view, |e| e.palette_colors(true)).unwrap(),
            [[0, 0, 0, 255], [255; 4]]
        );
        assert_eq!(cx.read(view, |e| e.palette_colors(false)).unwrap(), colors);
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            original
        );
        assert!(
            cx.read(view, |e| e.session().undo_label().is_none())
                .unwrap()
        );
        cx.update(view, |e, _| e.tools.mask_target = false).unwrap();
        cx.click(window, "palette-reset").unwrap();
        assert_eq!(
            cx.read(view, |e| e.palette_colors(false)).unwrap(),
            [[0, 0, 0, 255], [255; 4]]
        );
    }
}
