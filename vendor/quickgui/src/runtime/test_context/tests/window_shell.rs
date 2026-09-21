use super::*;

use crate::{
    ActivationPolicy, Display, DisplayId, DisplayUuid, Displays, DockAttention,
    MAX_WINDOW_ASPECT_RATIO, ResolvedWindowRestoreState, WindowRestoreState,
};

#[derive(Default)]
struct LifecycleView {
    log: Vec<String>,
    constrain_width: Option<f32>,
    constrain_position: Option<Point>,
}

impl View for LifecycleView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div().child(text(self.log.len().to_string()))
    }

    fn event(&mut self, event: &Event, cx: &mut EventContext) {
        match event {
            Event::Minimized(value) => self.log.push(format!("minimized:{value}")),
            Event::Maximized(value) => self.log.push(format!("maximized:{value}")),
            Event::FullscreenChanged(value) => self.log.push(format!("fullscreen:{value}")),
            Event::OcclusionChanged(value) => self.log.push(format!("occluded:{value}")),
            Event::FirstPresented => self.log.push("first-presented".to_owned()),
            Event::WindowLevelChanged(level) => self.log.push(format!("level:{level:?}")),
            Event::WillResize { proposed_size } => {
                self.log
                    .push(format!("will-resize:{}", proposed_size.width));
                if let Some(width) = self.constrain_width {
                    cx.constrain_resize(Size::new(width, proposed_size.height))
                        .expect("a finite constrained size is valid");
                }
            }
            Event::WillMove { proposed_position } => {
                self.log.push(format!("will-move:{}", proposed_position.x));
                if let Some(position) = self.constrain_position {
                    cx.constrain_move(position)
                        .expect("a finite constrained position is valid");
                }
            }
            Event::Resized { logical_size, .. } => {
                self.log.push(format!("resized:{}", logical_size.width));
            }
            Event::Moved {
                logical_position, ..
            } => self.log.push(format!("moved:{}", logical_position.x)),
            _ => {}
        }
    }
}

fn log(cx: &mut TestAppContext, window: TestWindowHandle<LifecycleView>) -> Vec<String> {
    cx.read(window, |view| view.log.clone()).unwrap()
}

#[test]
fn window_lifecycle_events_are_delivered_once_per_effective_change() {
    let (mut cx, window) = TestAppContext::new(LifecycleView::default()).unwrap();
    let handle = window.window_handle();

    cx.simulate_minimize(handle, true).unwrap();
    cx.simulate_minimize(handle, true).unwrap();
    cx.simulate_minimize(handle, false).unwrap();
    cx.simulate_maximize(handle, true).unwrap();
    cx.simulate_maximize(handle, true).unwrap();
    cx.simulate_fullscreen_change(handle, true).unwrap();
    cx.simulate_occlusion_change(handle, true).unwrap();
    cx.simulate_occlusion_change(handle, true).unwrap();
    cx.simulate_occlusion_change(handle, false).unwrap();

    assert_eq!(
        log(&mut cx, window),
        vec![
            "minimized:true",
            "minimized:false",
            "maximized:true",
            "fullscreen:true",
            "occluded:true",
            "occluded:false",
        ]
    );
    assert!(cx.window_state(handle).unwrap().fullscreen);
    assert!(!cx.window_state(handle).unwrap().minimized);
}

#[test]
fn ready_to_show_is_delivered_exactly_once() {
    let (mut cx, window) = TestAppContext::new(LifecycleView::default()).unwrap();
    let handle = window.window_handle();
    cx.simulate_first_presented(handle).unwrap();
    cx.simulate_first_presented(handle).unwrap();
    cx.simulate_first_presented(handle).unwrap();
    assert_eq!(log(&mut cx, window), vec!["first-presented"]);
}

#[test]
fn a_hidden_window_can_be_shown_from_the_first_presented_event() {
    #[derive(Default)]
    struct DeferredView {
        shown: bool,
    }

    impl View for DeferredView {
        fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            div()
        }

        fn event(&mut self, event: &Event, cx: &mut EventContext) {
            if matches!(event, Event::FirstPresented) {
                cx.show_window().expect("the current window is available");
                self.shown = true;
            }
        }
    }

    let (mut cx, window) = TestAppContext::from_application(
        Application::new(),
        WindowOptions::new("Deferred").show(false),
        DeferredView::default(),
    )
    .unwrap();
    let handle = window.window_handle();
    assert!(!cx.window_state(handle).unwrap().visible);
    cx.simulate_first_presented(handle).unwrap();
    assert!(cx.window_state(handle).unwrap().visible);
}

#[test]
fn constrain_resize_narrows_the_applied_inner_size() {
    let (mut cx, window) = TestAppContext::new(LifecycleView::default()).unwrap();
    let handle = window.window_handle();

    let applied = cx
        .simulate_window_resize(handle, Size::new(1_200.0, 700.0))
        .unwrap();
    assert_eq!(applied, Size::new(1_200.0, 700.0));

    cx.update(window, |view, _cx| view.constrain_width = Some(800.0))
        .unwrap();
    let applied = cx
        .simulate_window_resize(handle, Size::new(1_200.0, 700.0))
        .unwrap();
    assert_eq!(applied, Size::new(800.0, 700.0));
    assert_eq!(
        cx.window_state(handle).unwrap().bounds.bounds().width,
        800.0
    );
    assert_eq!(
        log(&mut cx, window),
        vec![
            "will-resize:1200",
            "resized:1200",
            "will-resize:1200",
            "resized:800",
        ]
    );
}

#[test]
fn constrain_move_replaces_the_applied_position() {
    let (mut cx, window) = TestAppContext::new(LifecycleView::default()).unwrap();
    let handle = window.window_handle();
    cx.update(window, |view, _cx| {
        view.constrain_position = Some(Point::new(64.0, 48.0));
    })
    .unwrap();
    let applied = cx
        .simulate_window_move(handle, Point::new(-4_000.0, 12.0))
        .unwrap();
    assert_eq!(applied, Point::new(64.0, 48.0));
    let bounds = cx.window_state(handle).unwrap().bounds.bounds();
    assert_eq!((bounds.x, bounds.y), (64.0, 48.0));
}

#[test]
fn a_constrain_hook_rejects_values_outside_the_supported_desktop_range() {
    let mut cx = EventContext::default();
    assert_eq!(
        cx.constrain_resize(Size::new(f32::NAN, 100.0)).unwrap_err(),
        WindowCommandError::InvalidBounds
    );
    assert_eq!(
        cx.constrain_move(Point::new(f32::INFINITY, 0.0))
            .unwrap_err(),
        WindowCommandError::InvalidBounds
    );
    assert!(cx.constrain_resize(Size::new(320.0, 240.0)).is_ok());
    assert!(cx.constrain_move(Point::new(10.0, 10.0)).is_ok());
}

#[test]
fn an_aspect_ratio_clamps_a_window_manager_resize_after_the_view_narrowing() {
    let (mut cx, window) = TestAppContext::new(LifecycleView::default()).unwrap();
    let handle = window.window_handle();
    cx.update(window, |_view, cx| {
        cx.set_aspect_ratio(Some(Size::new(2.0, 1.0)))
            .expect("a positive ratio is valid");
    })
    .unwrap();
    assert_eq!(
        cx.window_state(handle).unwrap().aspect_ratio,
        Some(Size::new(2.0, 1.0))
    );

    let applied = cx
        .simulate_window_resize(handle, Size::new(1_000.0, 900.0))
        .unwrap();
    assert_eq!(applied, Size::new(1_000.0, 500.0));

    cx.update(window, |_view, cx| {
        cx.clear_aspect_ratio().expect("clearing is valid");
    })
    .unwrap();
    assert_eq!(cx.window_state(handle).unwrap().aspect_ratio, None);
    let applied = cx
        .simulate_window_resize(handle, Size::new(1_000.0, 900.0))
        .unwrap();
    assert_eq!(applied, Size::new(1_000.0, 900.0));
}

#[test]
fn an_invalid_aspect_ratio_is_rejected_before_it_is_retained() {
    let (mut cx, window) = TestAppContext::new(LifecycleView::default()).unwrap();
    cx.update(window, |_view, cx| {
        for invalid in [
            Size::new(0.0, 1.0),
            Size::new(1.0, 0.0),
            Size::new(f32::NAN, 1.0),
            Size::new(MAX_WINDOW_ASPECT_RATIO * 2.0, 1.0),
        ] {
            assert_eq!(
                cx.set_aspect_ratio(Some(invalid)).unwrap_err(),
                WindowCommandError::InvalidAspectRatio
            );
        }
    })
    .unwrap();
    assert_eq!(
        cx.window_state(window.window_handle())
            .unwrap()
            .aspect_ratio,
        None
    );
}

#[test]
fn stacking_and_input_policy_commands_update_the_retained_snapshot() {
    let (mut cx, window) = TestAppContext::new(LifecycleView::default()).unwrap();
    let handle = window.window_handle();

    cx.update(window, |_view, cx| {
        cx.set_window_level(WindowLevel::Status).unwrap();
        cx.set_ignore_mouse_events(true, true).unwrap();
        cx.set_window_enabled(false).unwrap();
        cx.set_window_button_visibility(false).unwrap();
        cx.set_window_shadow(false).unwrap();
        cx.move_window_top().unwrap();
    })
    .unwrap();

    let state = cx.window_state(handle).unwrap();
    assert_eq!(state.window_level, WindowLevel::Status);
    assert!(state.ignore_mouse_events);
    assert!(state.forward_mouse_events);
    assert!(!state.window_enabled);
    assert!(!state.window_buttons_visible);
    assert!(!state.shadow);
    assert_eq!(log(&mut cx, window), vec!["level:Status"]);

    // `forward` is meaningless without pass-through and is normalized away.
    cx.update(window, |_view, cx| {
        cx.set_ignore_mouse_events(false, true).unwrap();
    })
    .unwrap();
    let state = cx.window_state(handle).unwrap();
    assert!(!state.ignore_mouse_events);
    assert!(!state.forward_mouse_events);
}

#[test]
fn window_levels_map_to_their_documented_appkit_constants() {
    assert_eq!(WindowLevel::AlwaysOnBottom.macos_level(), -1);
    assert_eq!(WindowLevel::Normal.macos_level(), 0);
    assert_eq!(WindowLevel::AlwaysOnTop.macos_level(), 3);
    assert_eq!(WindowLevel::Floating.macos_level(), 3);
    assert_eq!(WindowLevel::ModalPanel.macos_level(), 8);
    assert_eq!(WindowLevel::MainMenu.macos_level(), 24);
    assert_eq!(WindowLevel::Status.macos_level(), 25);
    assert_eq!(WindowLevel::PopUpMenu.macos_level(), 101);
    assert_eq!(WindowLevel::ScreenSaver.macos_level(), 1_000);
    assert!(!WindowLevel::Normal.is_above_normal());
    assert!(!WindowLevel::AlwaysOnBottom.is_above_normal());
    assert!(WindowLevel::ScreenSaver.is_above_normal());
}

#[test]
fn a_window_cannot_be_ordered_above_itself() {
    let (mut cx, window) = TestAppContext::new(LifecycleView::default()).unwrap();
    let handle = window.window_handle();
    cx.update(window, |_view, cx| {
        assert_eq!(
            cx.move_window_above(handle).unwrap_err(),
            WindowCommandError::InvalidWindowOrder
        );
        assert!(
            cx.move_window_above_handle(handle, WindowHandle::next())
                .is_ok()
        );
    })
    .unwrap();
}

fn two_display_snapshot() -> Displays {
    let left = Display::new(
        DisplayId::new(1),
        "Left",
        Rect::new(0.0, 0.0, 1_440.0, 900.0),
        Rect::new(0.0, 24.0, 1_440.0, 876.0),
        2.0,
    )
    .unwrap()
    .with_uuid(DisplayUuid::from_bytes([1; 16]));
    let right = Display::new(
        DisplayId::new(2),
        "Right",
        Rect::new(1_440.0, 0.0, 1_920.0, 1_080.0),
        Rect::new(1_440.0, 0.0, 1_920.0, 1_080.0),
        1.0,
    )
    .unwrap()
    .with_uuid(DisplayUuid::from_bytes([2; 16]));
    Displays::new(vec![left, right], Some(DisplayId::new(1))).unwrap()
}

#[test]
fn restore_state_round_trips_through_serde() {
    let (mut cx, window) = TestAppContext::new(LifecycleView::default()).unwrap();
    let handle = window.window_handle();
    cx.update(window, |_view, cx| {
        cx.set_window_bounds(WindowBounds::windowed(120.0, 80.0, 640.0, 480.0))
            .unwrap();
    })
    .unwrap();

    let state = cx.window_restore_state(handle).unwrap();
    assert_eq!(state.bounds(), Rect::new(120.0, 80.0, 640.0, 480.0));
    assert!(!state.maximized);
    assert!(!state.fullscreen);
    assert!(state.is_valid());

    let encoded = serde_json::to_string(&state).unwrap();
    let decoded: WindowRestoreState = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, state);
}

#[test]
fn restore_state_keeps_bounds_that_still_intersect_a_connected_display() {
    let displays = two_display_snapshot();
    let mut state = WindowRestoreState::new(Rect::new(1_500.0, 40.0, 800.0, 600.0));
    state.display_id = Some(2);
    state.display_uuid = Some([2; 16]);

    let ResolvedWindowRestoreState {
        bounds,
        display_id,
        adjusted,
    } = state.resolve(&displays);
    assert_eq!(bounds, WindowBounds::windowed(1_500.0, 40.0, 800.0, 600.0));
    assert_eq!(display_id, Some(DisplayId::new(2)));
    assert!(!adjusted);
}

#[test]
fn restore_state_clamps_bounds_that_left_their_remembered_display() {
    let displays = two_display_snapshot();
    let mut state = WindowRestoreState::new(Rect::new(9_000.0, 9_000.0, 800.0, 600.0));
    state.display_uuid = Some([2; 16]);

    let resolved = state.resolve(&displays);
    assert!(resolved.adjusted);
    assert_eq!(resolved.display_id, Some(DisplayId::new(2)));
    let rect = resolved.bounds.bounds();
    let work_area = displays.find(DisplayId::new(2)).unwrap().visible_bounds();
    assert_eq!(work_area.intersection(rect), Some(rect));
}

#[test]
fn restore_state_centers_when_no_connected_display_contains_it() {
    let displays = two_display_snapshot();
    // A display that has since been disconnected, with bounds far outside every work area.
    let mut state = WindowRestoreState::new(Rect::new(-9_000.0, -9_000.0, 800.0, 600.0));
    state.display_id = Some(77);
    state.display_uuid = Some([77; 16]);

    let resolved = state.resolve(&displays);
    assert!(resolved.adjusted);
    assert_eq!(resolved.display_id, Some(DisplayId::new(1)));
    let primary = displays.primary().unwrap();
    assert_eq!(
        resolved.bounds,
        WindowBounds::Windowed(primary.centered_bounds(Size::new(800.0, 600.0)))
    );
}

#[test]
fn an_invalid_restore_state_falls_back_to_a_centered_default() {
    let displays = two_display_snapshot();
    let state = WindowRestoreState::new(Rect::new(f32::NAN, 0.0, 0.0, -1.0));
    assert!(!state.is_valid());
    let resolved = state.resolve(&displays);
    assert!(resolved.adjusted);
    assert_eq!(resolved.display_id, Some(DisplayId::new(1)));
    assert!(matches!(resolved.bounds, WindowBounds::Windowed(_)));
}

#[test]
fn restore_state_preserves_maximized_and_fullscreen_modes() {
    let displays = two_display_snapshot();
    let mut state = WindowRestoreState::new(Rect::new(10.0, 40.0, 800.0, 600.0));
    state.maximized = true;
    assert!(matches!(
        state.resolve(&displays).bounds,
        WindowBounds::Maximized(_)
    ));
    state.fullscreen = true;
    assert!(matches!(
        state.resolve(&displays).bounds,
        WindowBounds::Fullscreen(_)
    ));
}

#[test]
fn window_options_restore_applies_validated_geometry() {
    let displays = two_display_snapshot();
    let mut state = WindowRestoreState::new(Rect::new(1_600.0, 60.0, 900.0, 700.0));
    state.display_uuid = Some([2; 16]);
    let options = WindowOptions::new("Restored").restore(&state, &displays);
    assert_eq!(
        options.window_bounds,
        Some(WindowBounds::windowed(1_600.0, 60.0, 900.0, 700.0))
    );
    assert_eq!(options.display_id, Some(DisplayId::new(2)));
    assert_eq!(options.size, Size::new(900.0, 700.0));
}

#[derive(Default)]
struct ShellView;

impl View for ShellView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div()
    }
}

#[test]
fn application_shell_services_are_recorded_deterministically() {
    let (mut cx, window) = TestAppContext::new(ShellView).unwrap();
    cx.update(window, |_view, cx| {
        drop(
            cx.set_activation_policy(ActivationPolicy::Accessory)
                .unwrap(),
        );
        cx.activate_application(true).unwrap();
        cx.hide_application().unwrap();
        cx.set_secure_keyboard_entry(true).unwrap();
        cx.beep().unwrap();
        cx.beep().unwrap();
    })
    .unwrap();

    let shell = cx.application_shell();
    assert_eq!(shell.activation_policy, ActivationPolicy::Accessory);
    assert!(!shell.dock_visible);
    // Activation happens before hiding in queue order, so the window stays hidden.
    assert!(shell.hidden);
    assert_eq!(shell.activations, 1);
    assert!(shell.last_activation_forced);
    assert!(shell.secure_keyboard_entry);
    assert_eq!(shell.beeps, 2);

    cx.update(window, |_view, cx| {
        cx.unhide_application().unwrap();
        drop(cx.set_dock_visible(true).unwrap());
        cx.set_secure_keyboard_entry(false).unwrap();
    })
    .unwrap();
    let shell = cx.application_shell();
    assert!(!shell.hidden);
    assert!(shell.dock_visible);
    assert_eq!(shell.activation_policy, ActivationPolicy::Regular);
    assert!(!shell.secure_keyboard_entry);
}

#[test]
fn dock_attention_requests_can_be_cancelled() {
    let (mut cx, window) = TestAppContext::new(ShellView).unwrap();
    cx.update(window, |_view, cx| {
        drop(cx.request_dock_attention(DockAttention::Critical).unwrap());
    })
    .unwrap();
    assert_eq!(
        cx.application_shell().dock_attention,
        Some(DockAttention::Critical)
    );

    cx.update(window, |_view, cx| {
        cx.cancel_dock_attention(crate::DockAttentionRequest::new(1))
            .unwrap();
    })
    .unwrap();
    assert_eq!(cx.application_shell().dock_attention, None);
}

#[test]
fn exit_with_code_records_the_process_status_after_ordinary_teardown() {
    let (mut cx, window) = TestAppContext::new(ShellView).unwrap();
    assert_eq!(cx.exit_code(), None);
    cx.update(window, |_view, cx| cx.exit_with_code(3)).unwrap();
    assert!(cx.is_exited());
    assert_eq!(cx.exit_code(), Some(3));
}

#[test]
fn application_packaging_is_false_inside_a_cargo_build_directory() {
    // The test binary always runs from the shared Cargo target directory.
    assert!(!crate::is_application_packaged());
}

#[test]
fn a_view_can_capture_restore_state_during_rendering() {
    struct RestoringView {
        captured: Option<WindowRestoreState>,
    }

    impl View for RestoringView {
        fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
            self.captured = Some(cx.window_restore_state());
            div()
        }
    }

    let (mut cx, window) = TestAppContext::from_application(
        Application::new(),
        WindowOptions::new("Restoring")
            .window_bounds(WindowBounds::windowed(40.0, 60.0, 720.0, 540.0)),
        RestoringView { captured: None },
    )
    .unwrap();
    cx.run_until_idle().unwrap();
    let captured = cx.read(window, |view| view.captured).unwrap().unwrap();
    assert_eq!(captured.bounds(), Rect::new(40.0, 60.0, 720.0, 540.0));
    assert_eq!(
        captured,
        cx.window_restore_state(window.window_handle()).unwrap()
    );
}
