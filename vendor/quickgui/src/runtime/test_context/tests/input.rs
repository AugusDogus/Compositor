use super::*;

#[derive(Default)]
struct GestureView {
    pressure: Option<MousePressureEvent>,
    pinch: Option<PinchEvent>,
    rotation: Option<RotationEvent>,
    smart_magnify: Option<SmartMagnifyEvent>,
    window_events: usize,
}

impl View for GestureView {
    fn event(&mut self, event: &Event, _cx: &mut EventContext) {
        if matches!(
            event,
            Event::MousePressure(_) | Event::Pinch(_) | Event::Rotation(_) | Event::SmartMagnify(_)
        ) {
            self.window_events += 1;
        }
    }

    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let pressure = cx.mouse_pressure_listener("pressure", |view, event, cx| {
            view.pressure = Some(*event);
            cx.invalidate();
        });
        let pinch = cx.pinch_listener("pinch", |view, event, cx| {
            view.pinch = Some(*event);
            cx.invalidate();
        });
        let rotation = cx.rotation_listener("rotation", |view, event, cx| {
            view.rotation = Some(*event);
            cx.invalidate();
        });
        let smart_magnify = cx.smart_magnify_listener("smart-magnify", |view, event, cx| {
            view.smart_magnify = Some(*event);
            cx.invalidate();
        });
        div()
            .child(div().on_mouse_pressure(pressure))
            .child(div().on_pinch(pinch))
            .child(div().on_rotation(rotation))
            .child(div().on_smart_magnify(smart_magnify))
    }
}

#[test]
fn gesture_simulation_runs_element_then_window_callbacks_without_native_services() {
    let (mut cx, view) = TestAppContext::new(GestureView::default()).unwrap();
    let window = view.window_handle();
    let pressure = MousePressureEvent {
        position: Point::new(10.0, 20.0),
        pressure: 0.75,
        stage: PressureStage::Force,
        modifiers: Modifiers::SUPER,
    };
    let pinch = PinchEvent {
        position: Point::new(30.0, 40.0),
        delta: 0.125,
        phase: GesturePhase::Moved,
        modifiers: Modifiers::empty(),
    };
    let rotation = RotationEvent {
        position: Point::new(50.0, 60.0),
        delta: -12.0,
        phase: GesturePhase::Ended,
        modifiers: Modifiers::SHIFT,
    };
    let smart_magnify = SmartMagnifyEvent {
        position: Point::new(70.0, 80.0),
        modifiers: Modifiers::ALT,
    };

    cx.simulate_mouse_pressure(window, ElementId::named("pressure"), pressure)
        .unwrap();
    cx.simulate_pinch(window, ElementId::named("pinch"), pinch)
        .unwrap();
    cx.simulate_rotation(window, ElementId::named("rotation"), rotation)
        .unwrap();
    cx.simulate_smart_magnify(window, ElementId::named("smart-magnify"), smart_magnify)
        .unwrap();

    cx.read(view, |view| {
        assert_eq!(view.pressure, Some(pressure));
        assert_eq!(view.pinch, Some(pinch));
        assert_eq!(view.rotation, Some(rotation));
        assert_eq!(view.smart_magnify, Some(smart_magnify));
        assert_eq!(view.window_events, 4);
    })
    .unwrap();
    assert!(matches!(
        cx.simulate_pinch(window, ElementId::named("pressure"), pinch),
        Err(TestAppError::NotListening { kind: "pinch", .. })
    ));
}

#[derive(Default)]
struct ScrollWheelView {
    stop_at_child: bool,
    order: Vec<&'static str>,
    events: Vec<ScrollWheelEvent>,
}

impl View for ScrollWheelView {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let parent = cx.scroll_wheel_listener("wheel-parent", |view, event, _cx| {
            view.order.push("parent");
            view.events.push(*event);
        });
        let child = cx.scroll_wheel_listener("wheel-child", |view, event, cx| {
            view.order.push("child");
            view.events.push(*event);
            cx.prevent_default();
            if view.stop_at_child {
                cx.stop_propagation();
            }
        });

        div()
            .on_scroll_wheel(parent)
            .child(div().on_scroll_wheel(child).child(div().id("wheel-target")))
    }
}

#[test]
fn scroll_wheel_simulation_bubbles_and_controls_default_independently() {
    let (mut cx, view) = TestAppContext::new(ScrollWheelView::default()).unwrap();
    let window = view.window_handle();
    let event = ScrollWheelEvent {
        position: Point::new(24.0, 36.0),
        delta: ScrollDelta::Pixels(Vector::new(2.0, -18.0)),
        phase: GesturePhase::Moved,
        modifiers: Modifiers::SUPER,
    };

    assert!(
        cx.simulate_scroll_wheel(window, "wheel-target", event)
            .unwrap()
    );
    cx.read(view, |view| {
        assert_eq!(view.order, ["child", "parent"]);
        assert_eq!(view.events, [event, event]);
    })
    .unwrap();

    cx.update(view, |view, cx| {
        view.stop_at_child = true;
        view.order.clear();
        view.events.clear();
        cx.invalidate();
    })
    .unwrap();
    assert!(
        cx.simulate_scroll_wheel(window, "wheel-target", event)
            .unwrap()
    );
    cx.read(view, |view| {
        assert_eq!(view.order, ["child"]);
        assert_eq!(view.events, [event]);
    })
    .unwrap();

    assert!(matches!(
        cx.simulate_scroll_wheel(window, "missing-wheel-listener", event),
        Err(TestAppError::UnknownElement { .. })
    ));
}

#[derive(Default)]
struct DesktopMouseView {
    stop_at_child: bool,
    order: Vec<&'static str>,
    downs: Vec<MouseDownEvent>,
    ups: Vec<MouseUpEvent>,
    moves: Vec<MouseMoveEvent>,
    exits: Vec<MouseExitEvent>,
    hover: Vec<bool>,
}

impl View for DesktopMouseView {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let outside = cx.mouse_down_listener("mouse-outside", |view, event, _cx| {
            view.order.push("outside-capture");
            view.downs.push(*event);
        });
        let parent_capture = cx.mouse_down_listener("mouse-parent", |view, event, _cx| {
            view.order.push("parent-capture");
            view.downs.push(*event);
        });
        let parent_bubble = cx.mouse_down_listener("mouse-parent", |view, event, _cx| {
            view.order.push("parent-bubble");
            view.downs.push(*event);
        });
        let child_down = cx.mouse_down_listener("mouse-child", |view, event, cx| {
            view.order.push("child-bubble");
            view.downs.push(*event);
            cx.prevent_default();
            if view.stop_at_child {
                cx.stop_propagation();
            }
        });
        let child_up = cx.mouse_up_listener("mouse-child", |view, event, _cx| {
            view.ups.push(*event);
        });
        let child_move = cx.mouse_move_listener("mouse-child", |view, event, cx| {
            assert_eq!(cx.pointer_position(), Some(event.position));
            view.moves.push(*event);
        });
        let child_exit = cx.mouse_exit_listener("mouse-child", |view, event, cx| {
            assert_eq!(cx.pointer_position(), Some(event.position));
            view.exits.push(*event);
        });
        let child_hover = cx.hover_listener("mouse-child", |view, hovered, _cx| {
            view.hover.push(*hovered);
        });

        div().children([
            div().id("mouse-outside").on_mouse_down_out(outside),
            div()
                .id("mouse-parent")
                .capture_any_mouse_down(parent_capture)
                .on_mouse_down(MouseButton::Left, parent_bubble)
                .child(
                    div()
                        .id("mouse-child")
                        .size(100.0, 100.0)
                        .on_any_mouse_down(child_down)
                        .on_mouse_up(MouseButton::Left, child_up)
                        .on_mouse_move(child_move)
                        .on_mouse_exit(child_exit)
                        .on_hover(child_hover),
                ),
        ])
    }
}

#[test]
fn desktop_mouse_simulation_preserves_capture_bubble_and_default_control() {
    let (mut cx, view) = TestAppContext::new(DesktopMouseView::default()).unwrap();
    let window = view.window_handle();
    let down = MouseDownEvent {
        button: MouseButton::Left,
        position: Point::new(20.0, 30.0),
        modifiers: Modifiers::SUPER,
        click_count: 2,
        first_mouse: true,
    };
    assert!(cx.simulate_mouse_down(window, "mouse-child", down).unwrap());
    cx.read(view, |view| {
        assert_eq!(
            view.order,
            [
                "outside-capture",
                "parent-capture",
                "child-bubble",
                "parent-bubble",
            ]
        );
        assert_eq!(view.downs, [down, down, down, down]);
    })
    .unwrap();

    cx.update(view, |view, cx| {
        view.stop_at_child = true;
        view.order.clear();
        view.downs.clear();
        cx.invalidate();
    })
    .unwrap();
    assert!(cx.simulate_mouse_down(window, "mouse-child", down).unwrap());
    cx.read(view, |view| {
        assert_eq!(
            view.order,
            ["outside-capture", "parent-capture", "child-bubble"]
        );
        assert_eq!(view.downs, [down, down, down]);
    })
    .unwrap();

    let up = MouseUpEvent {
        button: MouseButton::Left,
        position: down.position,
        modifiers: Modifiers::SHIFT,
        click_count: 2,
    };
    let moved = MouseMoveEvent {
        position: Point::new(40.0, 50.0),
        pressed_button: Some(MouseButton::Left),
        modifiers: Modifiers::ALT,
    };
    let exited = MouseExitEvent {
        position: Point::new(101.0, 50.0),
        pressed_button: None,
        modifiers: Modifiers::empty(),
    };
    assert!(!cx.simulate_mouse_up(window, "mouse-child", up).unwrap());
    cx.simulate_mouse_move(window, "mouse-child", moved)
        .unwrap();
    cx.simulate_mouse_exit(window, "mouse-child", exited)
        .unwrap();
    cx.read(view, |view| {
        assert_eq!(view.ups, [up]);
        assert_eq!(view.moves, [moved]);
        assert_eq!(view.exits, [exited]);
    })
    .unwrap();
}

#[test]
fn visual_pointer_delivers_hover_entry_and_exit_without_idle_frames() {
    let (mut cx, view) = TestAppContext::new(DesktopMouseView::default()).unwrap();
    let window = view.window_handle();
    {
        let mut visual = cx.visual(window).unwrap();
        assert!(visual.move_pointer(Point::new(10.0, 10.0)).unwrap());
        assert!(visual.move_pointer(Point::new(300.0, 300.0)).unwrap());
    }
    assert_eq!(
        cx.read(view, |view| view.hover.clone()).unwrap(),
        [true, false]
    );
    let renders = cx.render_count(window).unwrap();
    cx.run_until_idle().unwrap();
    assert_eq!(cx.render_count(window).unwrap(), renders);
}

#[derive(Default)]
struct MouseMutationView {
    remove_parent: bool,
    child_disabled: bool,
    order: Vec<&'static str>,
}

impl View for MouseMutationView {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let parent_down = cx.mouse_down_listener("mutation-parent", |view, _event, _cx| {
            view.order.push("parent");
        });
        let child = cx.mouse_down_listener("mutation-child", |view, _event, cx| {
            view.order.push("child");
            view.remove_parent = true;
            cx.invalidate();
        });
        let child = div()
            .id("mutation-child")
            .disabled(self.child_disabled)
            .on_any_mouse_down(child);
        let parent = div().id("mutation-parent").child(child);
        if self.remove_parent {
            parent
        } else {
            parent.on_any_mouse_down(parent_down)
        }
    }
}

#[test]
fn mouse_path_survives_callback_invalidation_and_disabled_nodes_do_not_listen() {
    let (mut cx, view) = TestAppContext::new(MouseMutationView::default()).unwrap();
    let window = view.window_handle();
    let event = MouseDownEvent {
        button: MouseButton::Left,
        position: Point::new(4.0, 4.0),
        modifiers: Modifiers::empty(),
        click_count: 1,
        first_mouse: false,
    };

    assert!(
        !cx.simulate_mouse_down(window, "mutation-child", event)
            .unwrap()
    );
    assert_eq!(
        cx.read(view, |view| view.order.clone()).unwrap(),
        ["child", "parent"]
    );

    cx.update(view, |view, cx| {
        view.order.clear();
        view.remove_parent = false;
        view.child_disabled = true;
        cx.invalidate();
    })
    .unwrap();
    assert!(
        !cx.simulate_mouse_down(window, "mutation-child", event)
            .unwrap()
    );
    assert_eq!(
        cx.read(view, |view| view.order.clone()).unwrap(),
        ["parent"]
    );
}

struct RetainedOverflowStressView;

impl View for RetainedOverflowStressView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div().size_full().child(
            div()
                .id("retained-overflow")
                .size(320.0, 120.0)
                .overflow_y_scroll()
                .child(div().w_full().h(100_000.0).flex_none()),
        )
    }
}

#[test]
fn retained_overflow_scrolls_back_and_forth_without_rebuilding_the_view() {
    let (mut cx, view) = TestAppContext::new(RetainedOverflowStressView).unwrap();
    let window = view.window_handle();
    let initial_renders = cx.render_count(window).unwrap();
    assert_eq!(
        cx.retained_scroll_offset(window, "retained-overflow")
            .unwrap(),
        Vector::ZERO
    );

    for _ in 0..4 {
        let mut moved_down = false;
        for _ in 0..256 {
            moved_down |= cx
                .simulate_retained_scroll(window, "retained-overflow", Vector::new(0.0, -400.0))
                .unwrap();
        }
        assert!(moved_down);
        assert!(
            cx.retained_scroll_offset(window, "retained-overflow")
                .unwrap()
                .y
                > 0.0
        );

        for _ in 0..256 {
            cx.simulate_retained_scroll(window, "retained-overflow", Vector::new(0.0, 400.0))
                .unwrap();
        }
        assert_eq!(
            cx.retained_scroll_offset(window, "retained-overflow")
                .unwrap(),
            Vector::ZERO
        );
    }

    assert_eq!(cx.render_count(window).unwrap(), initial_renders);
    cx.run_until_idle().unwrap();
    assert_eq!(cx.render_count(window).unwrap(), initial_renders);
}

#[derive(Default)]
struct RawTouchView {
    stop_at_child: bool,
    deliveries: Vec<(&'static str, TouchEvent)>,
    window_events: usize,
}

impl View for RawTouchView {
    fn event(&mut self, event: &Event, _cx: &mut EventContext) {
        if matches!(event, Event::Touch(_)) {
            self.window_events += 1;
        }
    }

    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let parent = cx.touch_listener("touch-parent", |view, event, _cx| {
            view.deliveries.push(("parent", *event));
        });
        let child = cx.touch_listener("touch-child", |view, event, cx| {
            view.deliveries.push(("child", *event));
            if view.stop_at_child {
                cx.stop_propagation();
            }
        });
        div()
            .on_touch(parent)
            .child(div().on_touch(child).child(div().id("touch-start-target")))
            .child(div().id("touch-outside-target"))
    }
}

#[test]
fn raw_touch_is_hit_once_captured_bounded_and_cancel_safe() {
    let (mut cx, view) = TestAppContext::new(RawTouchView::default()).unwrap();
    let window = view.window_handle();
    let sample = |id, phase, x| TouchEvent {
        id: TouchId(id),
        phase,
        position: Point::new(x, 30.0),
        force: Some(0.625),
    };

    let started = sample(7, TouchPhase::Started, 20.0);
    let moved = sample(7, TouchPhase::Moved, 800.0);
    let ended = sample(7, TouchPhase::Ended, 900.0);
    cx.simulate_touch(window, "touch-start-target", started)
        .unwrap();
    // The supplied element changes, but ID 7 stays on its original listener path.
    cx.simulate_touch(window, "touch-outside-target", moved)
        .unwrap();
    cx.simulate_touch(window, "touch-outside-target", ended)
        .unwrap();
    // A post-terminal sample has no capture and reaches only the window-level raw event.
    cx.simulate_touch(
        window,
        "touch-outside-target",
        sample(7, TouchPhase::Moved, 950.0),
    )
    .unwrap();

    cx.read(view, |view| {
        assert_eq!(
            view.deliveries,
            [
                ("child", started),
                ("parent", started),
                ("child", moved),
                ("parent", moved),
                ("child", ended),
                ("parent", ended),
            ]
        );
        assert_eq!(view.window_events, 4);
    })
    .unwrap();
    assert!(cx.window(window).unwrap().touch_captures.is_empty());

    cx.update(view, |view, cx| {
        view.stop_at_child = true;
        view.deliveries.clear();
        cx.invalidate();
    })
    .unwrap();
    let cancelled = sample(8, TouchPhase::Cancelled, 40.0);
    cx.simulate_touch(
        window,
        "touch-start-target",
        sample(8, TouchPhase::Started, 30.0),
    )
    .unwrap();
    cx.simulate_touch(window, "touch-outside-target", cancelled)
        .unwrap();
    cx.read(view, |view| {
        assert_eq!(view.deliveries.len(), 2);
        assert!(view.deliveries.iter().all(|(owner, _)| *owner == "child"));
    })
    .unwrap();
    assert!(cx.window(window).unwrap().touch_captures.is_empty());

    cx.update(view, |view, _cx| {
        view.stop_at_child = false;
        view.deliveries.clear();
    })
    .unwrap();
    for id in 0..MAX_ACTIVE_TOUCHES_PER_WINDOW as u64 {
        cx.simulate_touch(
            window,
            "touch-start-target",
            sample(100 + id, TouchPhase::Started, id as f32),
        )
        .unwrap();
    }
    assert_eq!(
        cx.window(window).unwrap().touch_captures.len(),
        MAX_ACTIVE_TOUCHES_PER_WINDOW
    );
    let before_overflow = cx.read(view, |view| view.deliveries.len()).unwrap();
    cx.simulate_touch(
        window,
        "touch-start-target",
        sample(999, TouchPhase::Started, 0.0),
    )
    .unwrap();
    assert_eq!(
        cx.read(view, |view| view.deliveries.len()).unwrap(),
        before_overflow
    );
    assert_eq!(
        cx.window(window).unwrap().touch_captures.len(),
        MAX_ACTIVE_TOUCHES_PER_WINDOW
    );
}

struct DisplayObserverView {
    observe: bool,
    display_count: usize,
    primary: Option<DisplayId>,
}

impl View for DisplayObserverView {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        if self.observe {
            self.display_count = cx.displays().len();
            self.primary = cx.primary_display().map(Display::id);
        }
        div()
    }
}

fn two_test_displays() -> Displays {
    let primary = DisplayId::new(10);
    let secondary = DisplayId::new(20);
    Displays::new(
        vec![
            Display::new(
                primary,
                "Primary",
                Rect::new(0.0, 0.0, 1_200.0, 800.0),
                Rect::new(0.0, 24.0, 1_200.0, 776.0),
                2.0,
            )
            .unwrap(),
            Display::new(
                secondary,
                "Secondary",
                Rect::new(1_200.0, 0.0, 1_000.0, 700.0),
                Rect::new(1_200.0, 0.0, 1_000.0, 700.0),
                1.0,
            )
            .unwrap(),
        ],
        Some(primary),
    )
    .unwrap()
}

#[test]
fn display_changes_only_rebuild_declarative_observers() {
    let (mut cx, view) = TestAppContext::new(DisplayObserverView {
        observe: true,
        display_count: 0,
        primary: None,
    })
    .unwrap();
    assert_eq!(cx.render_count(view.window_handle()).unwrap(), 1);

    cx.simulate_displays_change(two_test_displays()).unwrap();
    assert_eq!(cx.render_count(view.window_handle()).unwrap(), 2);
    cx.read(view, |view| {
        assert_eq!(view.display_count, 2);
        assert_eq!(view.primary, Some(DisplayId::new(10)));
    })
    .unwrap();

    cx.update(view, |view, cx| {
        view.observe = false;
        cx.invalidate();
    })
    .unwrap();
    let renders = cx.render_count(view.window_handle()).unwrap();
    cx.simulate_displays_change(Displays::test_default())
        .unwrap();
    assert_eq!(cx.render_count(view.window_handle()).unwrap(), renders);
}

struct KeyboardLayoutObserverView {
    observe: bool,
    layout_id: String,
}

impl View for KeyboardLayoutObserverView {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        if self.observe {
            self.layout_id = cx.keyboard_layout().id().to_owned();
        }
        div()
    }
}

#[test]
fn keyboard_layout_changes_only_rebuild_declarative_observers() {
    let (mut cx, view) = TestAppContext::new(KeyboardLayoutObserverView {
        observe: true,
        layout_id: String::new(),
    })
    .unwrap();
    assert_eq!(cx.render_count(view.window_handle()).unwrap(), 1);

    let german = KeyboardLayout::new("com.apple.keylayout.German", "German").unwrap();
    cx.simulate_keyboard_layout_change(german).unwrap();
    assert_eq!(cx.render_count(view.window_handle()).unwrap(), 2);
    assert_eq!(
        cx.read(view, |view| view.layout_id.clone()).unwrap(),
        "com.apple.keylayout.German"
    );

    cx.update(view, |view, cx| {
        view.observe = false;
        cx.invalidate();
    })
    .unwrap();
    let renders = cx.render_count(view.window_handle()).unwrap();
    cx.simulate_keyboard_layout_change(
        KeyboardLayout::new("com.apple.keylayout.US", "U.S.").unwrap(),
    )
    .unwrap();
    assert_eq!(cx.render_count(view.window_handle()).unwrap(), renders);
}

#[test]
fn explicit_display_centers_new_windows_and_falls_back_safely() {
    let (mut cx, root) = TestAppContext::new(DisplayObserverView {
        observe: false,
        display_count: 0,
        primary: None,
    })
    .unwrap();
    let displays = two_test_displays();
    cx.simulate_displays_change(displays).unwrap();

    let secondary = cx
        .update(root, |_view, cx| {
            cx.open_window(
                WindowOptions::new("Secondary")
                    .size(600.0, 400.0)
                    .display(DisplayId::new(20)),
                DisplayObserverView {
                    observe: false,
                    display_count: 0,
                    primary: None,
                },
            )
        })
        .unwrap();
    let state = cx.window_state(secondary).unwrap();
    assert_eq!(state.display_id, Some(DisplayId::new(20)));
    assert_eq!(
        state.bounds,
        WindowBounds::Windowed(Rect::new(1_400.0, 150.0, 600.0, 400.0))
    );

    let fallback = cx
        .update(root, |_view, cx| {
            cx.open_window(
                WindowOptions::new("Fallback")
                    .size(600.0, 400.0)
                    .display(DisplayId::new(999)),
                DisplayObserverView {
                    observe: false,
                    display_count: 0,
                    primary: None,
                },
            )
        })
        .unwrap();
    assert_eq!(
        cx.window_state(fallback).unwrap().display_id,
        Some(DisplayId::new(10))
    );
}

#[derive(Default)]
struct SpellCheckedView {
    value: Arc<str>,
}

impl View for SpellCheckedView {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let edit = cx.input_listener("notes", |this, value, cx| {
            this.value = Arc::from(value);
            cx.invalidate();
        });
        div().child(
            text_input(self.value.clone())
                .auto_focus()
                .spellcheck(true)
                .on_input(edit),
        )
    }
}

#[test]
fn settled_spell_checks_run_on_the_event_loop_deadline() {
    let provider = std::rc::Rc::new(crate::TestSpellCheckProvider::new().misspelling("helo"));
    crate::set_shared_spell_check_provider(provider);
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::default(), SpellCheckedView::default())
        .unwrap();
    let window = view.window_handle();
    assert_eq!(cx.focused(window).unwrap(), Some(ElementId::named("notes")));

    // An accepted edit arms exactly one settle deadline; nothing is flagged before it elapses.
    cx.simulate_input(window, "helo world").unwrap();
    assert!(cx.focused_input_misspellings(window).unwrap().is_empty());
    cx.advance_time(crate::SPELL_CHECK_SETTLE_DELAY - Duration::from_millis(1))
        .unwrap();
    assert!(cx.focused_input_misspellings(window).unwrap().is_empty());

    // The deadline pump flags the settled word without any further interaction.
    cx.advance_time(Duration::from_millis(1)).unwrap();
    assert_eq!(cx.focused_input_misspellings(window).unwrap(), vec![0..4]);

    // A settled, checked input holds no deadline: more time changes nothing.
    cx.advance_time(crate::SPELL_CHECK_SETTLE_DELAY).unwrap();
    assert_eq!(cx.focused_input_misspellings(window).unwrap(), vec![0..4]);
    crate::clear_spell_check_provider();
}

/// Three ways a listener can move focus — from a click, from a mouse press, and from a keyboard
/// action — plus plain controls for the press default, Tab, arrows, and Enter to land on.
struct FocusVisibilityView;

impl View for FocusVisibilityView {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let root = cx.focus_handle("visibility-root");
        let target = cx.focus_handle("visibility-target");
        let focus_from_click = cx.listener("visibility-click", move |_this, cx| cx.focus(target));
        let focus_from_press =
            cx.mouse_down_listener("visibility-press", move |_this, _event, cx| {
                cx.focus(target);
            });
        let focus_from_action =
            cx.action_listener("visibility-root", move |_this, _: &SaveForTest, cx| {
                cx.focus(target);
            });
        div()
            .focus_scope(root)
            .on_action(focus_from_action)
            .children([
                button()
                    .id("visibility-click")
                    .on_click(focus_from_click)
                    .child("Focus by click"),
                div()
                    .id("visibility-press")
                    .size(40.0, 40.0)
                    .on_mouse_down(MouseButton::Left, focus_from_press),
                button().track_focus(target).child("Target"),
                button().id("visibility-plain").clickable().child("Plain"),
            ])
    }
}

#[test]
fn focus_visibility_follows_the_input_that_moved_focus() {
    let (mut cx, view) = Application::new()
        .bind_keys([KeyBinding::new("ctrl-s", SaveForTest, None)])
        .into_test_context(WindowOptions::default(), FocusVisibilityView)
        .unwrap();
    let window = view.window_handle();
    let click = ElementId::named("visibility-click");
    let target = ElementId::named("visibility-target");
    let plain = ElementId::named("visibility-plain");
    let press = |button| MouseDownEvent {
        button,
        position: Point::new(4.0, 4.0),
        modifiers: Modifiers::empty(),
        click_count: 1,
        first_mouse: false,
    };

    // A fresh window paints the focus styles of a programmatic focus.
    cx.focus(window, plain).unwrap();
    assert!(cx.focus_visible(window).unwrap());

    // A click whose listener focuses lands pointer focus: no focus styles.
    cx.click(window, click).unwrap();
    assert_eq!(cx.focused(window).unwrap(), Some(target));
    assert!(!cx.focus_visible(window).unwrap());

    // A programmatic focus keeps the hidden answer.
    cx.focus(window, plain).unwrap();
    assert!(!cx.focus_visible(window).unwrap());

    // An action listener that focuses in response to a key lands visible focus.
    cx.simulate_keystrokes(window, "ctrl-s").unwrap();
    assert_eq!(cx.focused(window).unwrap(), Some(target));
    assert!(cx.focus_visible(window).unwrap());

    // A mouse press whose listener focuses lands pointer focus again.
    cx.simulate_mouse_down(window, "visibility-press", press(MouseButton::Left))
        .unwrap();
    assert_eq!(cx.focused(window).unwrap(), Some(target));
    assert!(!cx.focus_visible(window).unwrap());

    // Tab moves focus with the keyboard and shows it.
    cx.simulate_keystrokes(window, "tab").unwrap();
    assert_eq!(cx.focused(window).unwrap(), Some(plain));
    assert!(cx.focus_visible(window).unwrap());

    // A press with no listener of its own applies the production press default: the pressed
    // button takes pointer focus and paints no focus styles.
    cx.simulate_mouse_down(window, click, press(MouseButton::Left))
        .unwrap();
    assert_eq!(cx.focused(window).unwrap(), Some(click));
    assert!(!cx.focus_visible(window).unwrap());
    cx.simulate_mouse_up(
        window,
        click,
        MouseUpEvent {
            button: MouseButton::Left,
            position: Point::new(4.0, 4.0),
            modifiers: Modifiers::empty(),
            click_count: 1,
        },
    )
    .unwrap();
    assert_eq!(cx.focused(window).unwrap(), Some(click));

    // An arrow key that moves nothing reveals the focus already there.
    cx.simulate_keystrokes(window, "down").unwrap();
    assert_eq!(cx.focused(window).unwrap(), Some(click));
    assert!(cx.focus_visible(window).unwrap());

    // A right press never applies the press default, so focus and its styles stay put.
    cx.simulate_mouse_down(window, plain, press(MouseButton::Right))
        .unwrap();
    assert_eq!(cx.focused(window).unwrap(), Some(click));
    assert!(cx.focus_visible(window).unwrap());

    // Enter activates the focused button inside the key dispatch, so the focus its click listener
    // lands is still keyboard-driven and stays visible.
    cx.simulate_keystrokes(window, "enter").unwrap();
    assert_eq!(cx.focused(window).unwrap(), Some(target));
    assert!(cx.focus_visible(window).unwrap());
}
