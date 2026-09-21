use quickgui::{
    AnchorPlacement, Application, Color, IntoElement, Point, Tooltip, View, ViewContext,
    WindowOptions, button, div,
};
use std::time::Duration;

struct TooltipFocus;

impl View for TooltipFocus {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div().size_full().bg(Color::BLACK).child(
            button()
                .size(32., 32.)
                .on_click(cx.listener("trigger", |_, _| {}))
                .tooltip(
                    Tooltip::new(div().size(24., 12.).bg(Color::rgb8(255, 0, 0)))
                        .placement(AnchorPlacement::Bottom)
                        .delay(Duration::ZERO),
                ),
        )
    }
}

#[test]
fn pointer_exit_hides_a_clicked_controls_tooltip_and_keyboard_focus_can_show_it() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Tooltip focus").size(120., 100.),
            TooltipFocus,
        )
        .unwrap();
    let window = view.window_handle();
    let is_visible = |snapshot: quickgui::VisualSnapshot| {
        snapshot
            .rgba()
            .chunks_exact(4)
            .any(|pixel| pixel == [255, 0, 0, 255])
    };
    cx.click(window, "trigger").unwrap();
    {
        let mut visual = cx.visual(window).unwrap();
        visual.move_pointer(Point::new(100., 80.)).unwrap();
        visual.advance_time(Duration::from_secs(1)).unwrap();
        assert!(
            !is_visible(visual.capture_screenshot().unwrap()),
            "Pointer exit reopened a clicked control's tooltip"
        );
        visual.move_pointer(Point::new(16., 16.)).unwrap();
        assert!(
            is_visible(visual.capture_screenshot().unwrap()),
            "Hover must still show help"
        );
        visual.move_pointer(Point::new(100., 80.)).unwrap();
        assert!(!is_visible(visual.capture_screenshot().unwrap()));
    }
    cx.simulate_keystrokes(window, "tab").unwrap();
    let mut visual = cx.visual(window).unwrap();
    visual.advance_time(Duration::from_secs(1)).unwrap();
    assert!(
        is_visible(visual.capture_screenshot().unwrap()),
        "Keyboard focus must still show help"
    );
}
