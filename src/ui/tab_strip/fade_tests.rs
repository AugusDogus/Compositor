use super::*;
use quickgui::{Application, WindowOptions};
use std::time::Duration;

#[derive(Default)]
struct FadeSample {
    scrolled: bool,
    clicks: usize,
}

impl View for FadeSample {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(Color::WHITE)
            .child(
                div()
                    .id("under-fade")
                    .w(20.)
                    .h(34.)
                    .on_click(cx.listener("under-fade", |this, _| this.clicks += 1)),
            )
            .child(leading_edge_fade(self.scrolled))
    }
}

#[test]
fn leading_fade_animates_width_reverses_continuously_and_keeps_tabs_clickable() {
    for (width, height) in [(100., 34.), (1920., 1080.)] {
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Tab edge animation").size(width, height),
                FadeSample::default(),
            )
            .unwrap();
        let window = view.window_handle();
        let row = |shot: quickgui::VisualSnapshot| {
            let scale = shot.width() as f32 / width;
            (0..32)
                .map(|x| {
                    shot.pixel((x as f32 * scale) as u32, (17. * scale) as u32)
                        .unwrap()
                })
                .collect::<Vec<_>>()
        };
        let initial = row(cx.capture_screenshot(window).unwrap());
        assert!(initial.iter().all(|p| *p == [255; 4]));
        cx.update(view, |this, cx| {
            this.scrolled = true;
            cx.invalidate();
        })
        .unwrap();
        assert_eq!(
            row(cx.capture_screenshot(window).unwrap()),
            initial,
            "Fade appeared without animating"
        );
        cx.advance_time(Duration::from_millis(75)).unwrap();
        let middle = row(cx.capture_screenshot(window).unwrap());
        assert_ne!(middle, initial);
        assert_eq!(
            middle[25], [255; 4],
            "Halfway fade already fills all 28 points"
        );
        cx.update(view, |this, cx| {
            this.scrolled = false;
            cx.invalidate();
        })
        .unwrap();
        assert_eq!(
            row(cx.capture_screenshot(window).unwrap()),
            middle,
            "Reversal jumped"
        );
        cx.advance_time(Duration::from_millis(150)).unwrap();
        assert_eq!(row(cx.capture_screenshot(window).unwrap()), initial);
        cx.update(view, |this, cx| {
            this.scrolled = true;
            cx.invalidate();
        })
        .unwrap();
        cx.capture_screenshot(window).unwrap();
        cx.advance_time(Duration::from_millis(150)).unwrap();
        let complete = row(cx.capture_screenshot(window).unwrap());
        assert!(complete[2][0] < 160);
        assert!(complete[25][0] < 255);
        assert_eq!(complete[29], [255; 4]);
        cx.click(window, "under-fade").unwrap();
        assert_eq!(cx.read(view, |this| this.clicks).unwrap(), 1);
    }
}

#[test]
fn reduced_motion_resolves_the_tab_fade_immediately() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Static tab edge")
                .size(100., 34.)
                .reduce_motion(true),
            FadeSample::default(),
        )
        .unwrap();
    let window = view.window_handle();
    let initial = cx.capture_screenshot(window).unwrap();
    cx.update(view, |this, cx| {
        this.scrolled = true;
        cx.invalidate();
    })
    .unwrap();
    let active = cx.capture_screenshot(window).unwrap();
    assert!(
        active.rgba() != initial.rgba(),
        "Reduced-motion fade did not appear"
    );
    cx.advance_time(Duration::from_millis(150)).unwrap();
    assert!(cx.capture_screenshot(window).unwrap().rgba() == active.rgba());
}
