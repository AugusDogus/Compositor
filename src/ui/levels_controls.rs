//! Original-pixel Levels histogram and its source display scale.
use super::adjustment_fields::channel_index;
use super::adjustment_histogram::AdjustmentHistogram;
use super::*;
use compositor::adjustment::Channel;

impl Editor {
    pub(super) fn levels_histogram(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let Some(edit) = &self.adjustment_edit else {
            return div();
        };
        let mut controls = div().flex_col().gap(0.);
        if let Some(job) = &edit.histogram
            && job.ready().is_none()
        {
            controls = controls.child(
                div()
                    .id("levels-histogram")
                    .h(150.)
                    .flex_shrink_0()
                    .bg(Color::BLACK.with_alpha(0.25))
                    .p(8.)
                    .child(
                        text(job.error().unwrap_or("Loading histogram…"))
                            .text_size(10.)
                            .line_height(13.)
                            .wrap(),
                    ),
            );
        }
        if let Some(histogram) = edit.histogram.as_ref().and_then(AdjustmentHistogram::ready) {
            let channel = channel_index(edit.settings.levels.channel);
            let color = match edit.settings.levels.channel {
                Channel::RGB => Color::rgb8(142, 142, 147),
                Channel::Red => Color::rgb8(255, 69, 58),
                Channel::Green => Color::rgb8(48, 209, 88),
                Channel::Blue => Color::rgb8(10, 132, 255),
            };
            let bins = histogram.0[channel];
            let mut interior: Vec<_> = bins[1..255].iter().copied().filter(|v| *v > 0.).collect();
            interior.sort_by(f64::total_cmp);
            let peak = bins.iter().copied().fold(0., f64::max);
            let ceiling = if interior.is_empty() {
                peak
            } else {
                peak.min(interior[((interior.len() - 1) as f64 * 0.95) as usize] * 4.)
            };
            controls = controls.child(
                quickgui::canvas(move |bounds, painter| {
                    painter.fill_rect(bounds, Color::BLACK.with_alpha(0.25));
                    if ceiling <= 0. {
                        return;
                    }
                    for (i, count) in bins.iter().enumerate() {
                        let h = (*count / ceiling).min(1.) as f32 * bounds.height;
                        painter.fill_rect(
                            quickgui::Rect::new(
                                i as f32 / 256. * bounds.width,
                                bounds.height - h,
                                bounds.width / 256. + 0.1,
                                h,
                            ),
                            color,
                        );
                    }
                })
                .id("levels-histogram")
                .w_full()
                .h(150.)
                .tooltip("Linear histogram with automatic vertical scaling. Tall spikes may extend beyond the graph; all tones from 0 to 255 remain included.")
                .accessibility_label(format!("Original {:?} histogram", edit.settings.levels.channel))
                .flex_shrink_0(),
            );
        }
        controls = controls.child(self.levels_handles(cx, false));
        if let Some(job) = &edit.histogram
            && job.error().is_some()
        {
            controls = controls.child(
                Self::control("Retry histogram")
                    .id("histogram-retry")
                    .on_click(cx.listener("histogram-retry", |this, cx| {
                        if let Some(job) = this
                            .adjustment_edit
                            .as_mut()
                            .and_then(|edit| edit.histogram.as_mut())
                        {
                            job.retry();
                        }
                        cx.invalidate();
                    })),
            );
        }
        controls
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, Point, WindowOptions};
    #[test]
    fn empty_handle_track_does_not_adjust_levels() {
        let mut editor = Editor::with_test_document();
        editor.tabs[0].set_document(Document::new(8, 8).unwrap(), None);
        compositor::edits::fill(
            &mut editor.session_mut().document,
            [100, 80, 60, 255],
            false,
            false,
        )
        .unwrap();
        editor.open_pixel_adjustment(Kind::Levels).unwrap();
        let original = editor.session().document.clone();
        let settings = editor.adjustment_edit.as_ref().unwrap().settings.clone();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Levels hit areas").size(1500., 900.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        for id in ["levels-input-handles", "levels-output-handles"] {
            let track = cx.element_bounds(window, id).unwrap();
            let point = Point::new(track.x + track.width * 0.25, track.y + 9.);
            assert!(
                matches!(
                    cx.simulate_pointer_drag(window, id, point, point),
                    Err(quickgui::TestAppError::NotListening {
                        kind: "pointer",
                        ..
                    })
                ),
                "Empty track must not capture a drag"
            );
            cx.read(view, |e| {
                assert_eq!(e.adjustment_edit.as_ref().unwrap().settings, settings);
                assert_eq!(e.session().document, original);
                assert!(e.session().undo_label().is_none());
            })
            .unwrap();
        }
    }

    #[test]
    fn histogram_handles_preview_and_cancel_preserves_pixels() {
        let mut editor = Editor::with_test_document();
        editor.tabs = vec![Session::new(Document::new(8, 8).unwrap(), None).into()];
        compositor::edits::fill(
            &mut editor.session_mut().document,
            [100, 80, 60, 255],
            false,
            false,
        )
        .unwrap();
        let original = editor.session().document.clone();
        editor.open_pixel_adjustment(Kind::Levels).unwrap();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Levels handles").size(1280., 900.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        let bounds = cx.element_bounds(window, "levels-input-handles").unwrap();
        let histogram = cx.element_bounds(window, "levels-histogram").unwrap();
        let output = cx.element_bounds(window, "levels-output-handles").unwrap();
        let ramp = cx.element_bounds(window, "levels-output-ramp").unwrap();
        assert_eq!(histogram.bottom(), bounds.y);
        assert_eq!(ramp.bottom(), output.y);
        assert_eq!(histogram.width, bounds.width);
        assert_eq!(ramp.width, output.width);
        let frame = cx.capture_screenshot(window).unwrap();
        let scale = frame.width() as f32 / 1280.;
        let background = frame
            .pixel(
                ((histogram.x + 20.) * scale) as u32,
                ((histogram.y + 40.) * scale) as u32,
            )
            .unwrap()[0];
        assert!(
            (i16::from(background) - 34).abs() <= 1,
            "25% black over the panel should produce 34, got {background}"
        );
        for handles in [bounds, output] {
            let white_tip = frame
                .pixel(
                    ((handles.right() + 2.) * scale) as u32,
                    ((handles.y + 12.) * scale) as u32,
                )
                .unwrap();
            assert!(
                white_tip[0] > 200,
                "End handle must extend beyond the chart: {white_tip:?}"
            );
        }
        cx.simulate_pointer_drag(
            window,
            "levels-input-black-handle",
            Point::new(bounds.x, bounds.y + 5.),
            Point::new(bounds.x + bounds.width * 0.2, bounds.y + 5.),
        )
        .unwrap();
        assert_eq!(
            cx.read(view, |e| e
                .adjustment_edit
                .as_ref()
                .unwrap()
                .settings
                .levels
                .ranges[0]
                .black)
                .unwrap(),
            51.
        );
        assert_ne!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            original
        );
        // Dragging gamma past the white point must stay within Swift's 0.1 minimum.
        cx.simulate_pointer_drag(
            window,
            "levels-gamma-handle",
            Point::new(bounds.x + bounds.width * 0.6, bounds.y + 5.),
            Point::new(bounds.x + bounds.width, bounds.y + 5.),
        )
        .unwrap();
        cx.read(view, |e| {
            assert_eq!(
                e.adjustment_edit.as_ref().unwrap().settings.levels.ranges[0].gamma,
                0.1
            );
            assert!(matches!(&e.modal, Some(Form::Edit { error, .. }) if error.is_empty()));
        })
        .unwrap();
        cx.click(window, "form-cancel").unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            original
        );
    }
}
