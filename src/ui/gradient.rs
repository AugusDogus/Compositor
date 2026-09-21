use super::*;
use compositor::geometry::Point;

#[derive(Clone, Copy)]
pub(super) enum Endpoint {
    Start,
    End,
}

pub(super) struct PendingGradient {
    original: Document,
    pub start: Point,
    pub end: Point,
    mask: bool,
}

impl Editor {
    pub(super) fn gradient_preview_colors(&self) -> [[u8; 4]; 2] {
        let mut colors = self.palette_colors(self.tools.mask_target);
        if self.tools.gradient.style == compositor::gradient::Style::ForegroundToTransparent {
            colors[1] = colors[0];
            colors[1][3] = 0;
        }
        if self.tools.gradient.reversed {
            colors.swap(0, 1);
        }
        colors
    }

    pub(super) fn begin_gradient(&mut self, point: Point) -> Result<()> {
        if let Some(edit) = &mut self.pending_gradient {
            edit.start = point;
            edit.end = point;
        } else {
            let label = if self.tools.mask_target {
                "Gradient Mask"
            } else {
                "Gradient"
            };
            self.session_mut().begin(label)?;
            self.pending_gradient = Some(PendingGradient {
                original: self.session().document.clone(),
                start: point,
                end: point,
                mask: self.tools.mask_target,
            });
        }
        self.refresh_gradient()
    }

    pub(super) fn move_gradient(
        &mut self,
        point: Point,
        endpoint: Endpoint,
        constrained: bool,
    ) -> Result<()> {
        if let Some(edit) = &mut self.pending_gradient {
            let (anchor, target) = match endpoint {
                Endpoint::Start => (edit.end, &mut edit.start),
                Endpoint::End => (edit.start, &mut edit.end),
            };
            *target = point;
            if constrained {
                let dx = point[0] - anchor[0];
                let dy = point[1] - anchor[1];
                let angle = (dy.atan2(dx) / std::f64::consts::FRAC_PI_4).round()
                    * std::f64::consts::FRAC_PI_4;
                *target = [
                    anchor[0] + dx.hypot(dy) * angle.cos(),
                    anchor[1] + dx.hypot(dy) * angle.sin(),
                ];
            }
        }
        self.refresh_gradient()
    }

    pub(super) fn refresh_gradient(&mut self) -> Result<()> {
        let Some(edit) = &self.pending_gradient else {
            return Ok(());
        };
        let mut doc = edit.original.clone();
        if (edit.end[0] - edit.start[0]).hypot(edit.end[1] - edit.start[1]) >= 0.5 {
            let result = self.tools.gradient.apply(
                &mut doc,
                edit.start,
                edit.end,
                self.palette_colors(edit.mask)[0],
                self.palette_colors(edit.mask)[1],
                edit.mask,
            );
            if let Err(error) = result {
                self.pending_gradient = None;
                self.session_mut().cancel();
                return Err(error);
            }
        }
        self.session_mut().document = doc;
        Ok(())
    }

    pub(super) fn commit_gradient(&mut self) -> Result<()> {
        if self.pending_gradient.take().is_some() {
            self.session_mut().commit()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, WindowOptions};

    #[test]
    fn gradient_overlay_uses_color_handles_and_a_contrasting_line() {
        let mut editor = Editor::with_test_document();
        let mut original = Document::new(512, 512).unwrap();
        compositor::edits::fill(&mut original, [30, 30, 30, 255], false, false).unwrap();
        editor.tabs = vec![Session::new(original.clone(), None).into()];
        editor.tools.tool = Tool::Gradient;
        editor.tools.brush.color = [220, 40, 20, 255];
        editor.session_mut().zoom_at(1., [0., 0.]);
        editor.begin_gradient([128.5, 256.5]).unwrap();
        // Keep a uniform image behind the controls so their colors can be measured.
        editor.pending_gradient.as_mut().unwrap().end = [256.5, 256.5];
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Gradient overlay").size(1000., 900.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        let canvas = cx.element_bounds(window, "canvas").unwrap();
        let (zoom, offset) = cx
            .update(view, |e, _| e.viewport(canvas.width, canvas.height))
            .unwrap();
        let frame = cx.capture_screenshot(window).unwrap();
        let scale = frame.width() as f64 / 1000.;
        let pixel = |x: f64, y: f64| {
            frame
                .pixel(
                    ((f64::from(canvas.x) + offset[0] + x * zoom) * scale) as u32,
                    ((f64::from(canvas.y) + offset[1] + y * zoom) * scale) as u32,
                )
                .unwrap()
        };
        assert_eq!(pixel(192.5, 256.5), [255; 4], "Gradient line must be white");
        assert_eq!(
            pixel(128.5, 256.5),
            [220, 40, 20, 255],
            "Start handle must show the foreground"
        );
        assert_eq!(
            pixel(256.5, 256.5),
            [191, 191, 191, 255],
            "Transparent endpoint must show gray"
        );
        assert_eq!(
            pixel(128.5 + 6. / zoom, 256.5 + 6. / zoom),
            [30, 30, 30, 255],
            "Round handles must not fill their square corners"
        );
        cx.update(view, |e, cx| {
            e.tools.gradient.shape = compositor::gradient::Shape::Radial;
            cx.invalidate();
        })
        .unwrap();
        let radial = cx.capture_screenshot(window).unwrap();
        let mut dashes = 0;
        let mut gaps = 0;
        let mut rim_peak = 0;
        for i in 10..190 {
            let angle = f64::from(i) * std::f64::consts::TAU / 200.;
            let point = [128.5 + 128. * angle.cos(), 256.5 + 128. * angle.sin()];
            let x = ((f64::from(canvas.x) + offset[0] + point[0] * zoom) * scale) as u32;
            let y = ((f64::from(canvas.y) + offset[1] + point[1] * zoom) * scale) as u32;
            assert_eq!(frame.pixel(x, y), Some([30, 30, 30, 255]));
            let pixel = radial.pixel(x, y).unwrap();
            rim_peak = rim_peak.max(pixel[0]);
            if pixel[0] > 100 {
                dashes += 1;
            }
            if pixel == [30, 30, 30, 255] {
                gaps += 1;
            }
        }
        assert!(
            dashes > 40 && gaps > 40,
            "Radial rim must have visible dashes and gaps: {dashes}/{gaps}"
        );
        // Black at 50%, then white at 80%, over a 30/255 background.
        assert!(
            (195..=208).contains(&rim_peak),
            "Radial guide opacity must match the source: {rim_peak}"
        );
        cx.click(window, "gradient-cancel").unwrap();
        cx.read(view, |e| {
            assert_eq!(e.session().document, original);
            assert!(e.session().undo_label().is_none());
        })
        .unwrap();
    }

    #[test]
    fn mask_gradient_swatch_uses_mask_colors_and_keeps_its_size() {
        for width in [1500., 900.] {
            let mut editor = Editor::with_test_document();
            compositor::edits::add_mask(&mut editor.session_mut().document, false).unwrap();
            editor.tools.tool = Tool::Gradient;
            editor.tools.mask_target = true;
            editor.tools.brush.color = [240, 20, 10, 255];
            editor.tools.background = [20, 40, 240, 255];
            editor.tools.gradient.style = compositor::gradient::Style::ForegroundToBackground;
            let original = editor.session().document.clone();
            let (mut cx, view) = Application::new()
                .font(crate::UI_FONT)
                .into_test_context(
                    WindowOptions::new("Mask gradient colors").size(width, 900.),
                    editor,
                )
                .unwrap();
            let window = view.window_handle();
            let swatch = cx.element_bounds(window, "gradient-swatch").unwrap();
            assert_eq!([swatch.width, swatch.height], [56., 18.]);
            for white in [false, true] {
                cx.update(view, |e, cx| {
                    e.tools.mask_paint_white = white;
                    cx.invalidate();
                })
                .unwrap();
                let frame = cx.capture_screenshot(window).unwrap();
                let scale = frame.width() as f32 / width;
                let pixel = frame
                    .pixel(
                        ((swatch.x + 4.) * scale) as u32,
                        ((swatch.y + 9.) * scale) as u32,
                    )
                    .unwrap();
                assert_eq!(
                    pixel[0], pixel[1],
                    "Mask preview must be grayscale: {pixel:?}"
                );
                assert_eq!(pixel[1], pixel[2]);
                assert_eq!(pixel[0] > 128, white);
            }
            cx.read(view, |e| {
                assert_eq!(e.session().document, original);
                assert!(e.session().undo_label().is_none());
            })
            .unwrap();
        }
    }

    #[test]
    fn failed_redraw_discards_the_previous_preview_without_committing_history() {
        let mut editor = Editor::with_test_document();
        let mut original = Document::new(8, 8).unwrap();
        compositor::edits::fill(&mut original, [80, 120, 160, 255], false, false).unwrap();
        editor.tabs = vec![Session::new(original.clone(), None).into()];
        editor.begin_gradient([0., 0.]).unwrap();
        editor
            .move_gradient([8., 8.], Endpoint::End, false)
            .unwrap();
        assert_ne!(editor.session().document, original);
        editor.tools.gradient.opacity = 2.;
        assert!(editor.refresh_gradient().is_err());
        assert_eq!(editor.session().document, original);
        assert!(editor.pending_gradient.is_none());
        assert!(!editor.session().has_pending_edit());
        assert!(editor.session().undo_label().is_none());
    }
}
