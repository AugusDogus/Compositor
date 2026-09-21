use super::*;

struct RetainedUpdateView {
    label: Arc<str>,
    clicks: usize,
}

impl View for RetainedUpdateView {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let click = cx.listener("button", |view, _cx| view.clicks += 1);
        button()
            .id("button")
            .on_click(click)
            .child(text(self.label.clone()).id("label"))
    }
}

#[test]
fn retained_updates_preserve_listeners_and_accessibility_without_rendering_view() {
    let (mut cx, view) = TestAppContext::new(RetainedUpdateView {
        label: Arc::from("Before"),
        clicks: 0,
    })
    .unwrap();
    let window = view.window_handle();
    cx.focus(window, "button").unwrap();
    let renders = cx.render_count(window).unwrap();
    cx.update(view, |view, _cx| view.label = Arc::from("After 🌍"))
        .unwrap();
    assert!(
        cx.update_elements(
            window,
            &[
                crate::ElementUpdate::Text {
                    id: "label".into(),
                    content: Arc::from("After 🌍")
                },
                crate::ElementUpdate::BackgroundColor {
                    id: "button".into(),
                    color: Color::WHITE
                },
            ]
        )
        .unwrap()
    );
    assert_eq!(cx.render_count(window).unwrap(), renders);
    let accessibility = cx.accessibility_update(window).unwrap();
    let button = accessibility
        .nodes
        .iter()
        .find(|(id, _)| id.0 == ElementId::from("button").value())
        .unwrap();
    assert_eq!(button.1.label(), Some("After 🌍"));
    cx.click(window, "button").unwrap();
    assert_eq!(cx.read(view, |view| view.clicks).unwrap(), 1);
    assert_eq!(cx.render_count(window).unwrap(), renders);
    cx.update(view, |_view, cx| cx.invalidate()).unwrap();
    assert_eq!(cx.render_count(window).unwrap(), renders + 1);
}

#[cfg(feature = "inspector")]
struct InspectorTestView;

#[cfg(feature = "inspector")]
impl View for InspectorTestView {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let focus = cx.focus_handle("inspector-button");
        div().id("inspector-root").size_full().child(
            button()
                .id("inspector-button")
                .track_focus(focus)
                .auto_focus()
                .accessibility_label("Inspectable button")
                .size(120.0, 36.0)
                .child(text("Inspect me")),
        )
    }
}

#[cfg(feature = "inspector")]
#[test]
fn inspector_snapshot_reuses_production_tree_and_toggle_does_not_rebuild_clean_view() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(480.0, 320.0).inspector(true),
            InspectorTestView,
        )
        .unwrap();
    let window = view.window_handle();
    assert!(cx.window_state(window).unwrap().inspector_active);

    let snapshot = cx.inspector_snapshot(window).unwrap().unwrap();
    assert_eq!(snapshot.viewport, Size::new(480.0, 320.0));
    assert!(!snapshot.nodes_truncated);
    let root = snapshot
        .nodes
        .iter()
        .find(|node| node.id == ElementId::named("inspector-root"))
        .unwrap();
    let control = snapshot
        .nodes
        .iter()
        .find(|node| node.id == ElementId::named("inspector-button"))
        .unwrap();
    assert_eq!(control.parent, Some(root.id));
    assert!(control.focused && control.on_focus_path);
    assert_eq!(control.accessibility.role, AccessibilityRole::Button);
    assert_eq!(
        control.accessibility.label.as_deref(),
        Some("Inspectable button")
    );
    assert!(control.hit_region.is_some_and(|hit| hit.focusable));

    let frame = cx.capture_screenshot(window).unwrap();
    let y = frame.height() / 2;
    assert_ne!(frame.pixel(8, y), frame.pixel(frame.width() - 8, y));

    let renders = cx.render_count(window).unwrap();
    cx.update(view, |_view, cx| cx.toggle_inspector().unwrap())
        .unwrap();
    assert!(!cx.window_state(window).unwrap().inspector_active);
    assert!(cx.inspector_snapshot(window).unwrap().is_none());
    assert_eq!(cx.render_count(window).unwrap(), renders);

    cx.update(view, |_view, cx| cx.set_inspector(true).unwrap())
        .unwrap();
    assert!(cx.window_state(window).unwrap().inspector_active);
    assert_eq!(cx.render_count(window).unwrap(), renders);
}

#[derive(Default)]
struct AssetAccessView {
    rendered: Arc<str>,
    clicked: Arc<str>,
}

impl View for AssetAccessView {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let message = cx.assets().load_required("copy/message.txt").unwrap();
        self.rendered = Arc::from(std::str::from_utf8(message.as_ref()).unwrap());
        let load_from_event = cx.listener("load-asset", |this, cx| {
            let message = cx.asset_source().load_required("copy/message.txt").unwrap();
            this.clicked = Arc::from(std::str::from_utf8(message.as_ref()).unwrap());
            cx.invalidate();
        });
        button().on_click(load_from_event).child("Load")
    }
}

#[test]
fn application_assets_are_shared_with_view_and_event_contexts() {
    let bundle = BundledAssets::new()
        .with("copy/message.txt", &b"bundled message"[..])
        .unwrap();
    let (mut cx, view) = Application::new()
        .with_assets(bundle)
        .into_test_context(WindowOptions::default(), AssetAccessView::default())
        .unwrap();

    assert_eq!(
        cx.read(view, |view| view.rendered.clone())
            .unwrap()
            .as_ref(),
        "bundled message"
    );
    cx.click(view.window_handle(), "load-asset").unwrap();
    assert_eq!(
        cx.read(view, |view| view.clicked.clone()).unwrap().as_ref(),
        "bundled message"
    );
}

#[derive(Default)]
struct EnvironmentAccessView {
    rendered: String,
    clicked: String,
}

impl View for EnvironmentAccessView {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let info = cx.app_info().unwrap();
        let paths = cx.app_paths().unwrap();
        self.rendered = format!(
            "{}:{}:{}",
            info.name(),
            paths.config_dir().unwrap().display(),
            cx.system_info().architecture()
        );
        let read_environment = cx.listener("read-environment", |this, cx| {
            this.clicked = format!(
                "{}:{}:{}",
                cx.app_info().unwrap().version(),
                cx.app_paths().unwrap().resource_dir().display(),
                cx.system_info().operating_system().as_str()
            );
            cx.invalidate();
        });
        button().on_click(read_environment).child("Read")
    }
}

#[test]
fn application_environment_is_shared_with_view_and_event_contexts() {
    let info = AppInfo::new("Example", "1.2.3", "dev.quickgui.example").unwrap();
    let paths = info
        .paths()
        .unwrap()
        .with_config_dir(Some("/tmp/quickgui-config"))
        .with_resource_dir("/tmp/quickgui-resources");
    let (mut cx, view) = Application::new()
        .app_info(info.clone())
        .app_paths(paths.clone())
        .into_test_context(WindowOptions::default(), EnvironmentAccessView::default())
        .unwrap();

    assert_eq!(cx.app_info(), Some(&info));
    assert_eq!(cx.app_paths(), Some(&paths));
    assert!(
        cx.read(view, |view| view.rendered.clone())
            .unwrap()
            .starts_with("Example:/tmp/quickgui-config:")
    );
    cx.click(view.window_handle(), "read-environment").unwrap();
    assert!(
        cx.read(view, |view| view.clicked.clone())
            .unwrap()
            .starts_with("1.2.3:/tmp/quickgui-resources:")
    );
}

#[derive(Default)]
struct SystemPreferencesView {
    observing: bool,
    rendered: SystemPreferences,
    clicked: SystemPreferences,
}

impl View for SystemPreferencesView {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        if self.observing {
            self.rendered = cx.system_preferences();
        }
        let read_preferences = cx.listener("read-system-preferences", |this, cx| {
            this.clicked = cx.system_preferences();
        });
        button().on_click(read_preferences).child("Read")
    }
}

#[test]
fn system_preferences_invalidate_only_current_declarative_subscribers() {
    let (mut cx, view) = TestAppContext::new(SystemPreferencesView {
        observing: true,
        ..SystemPreferencesView::default()
    })
    .unwrap();
    let window = view.window_handle();
    let initial_renders = cx.render_count(window).unwrap();
    let reduced = SystemPreferences::default()
        .with_color_scheme(ColorScheme::Dark)
        .with_reduce_motion(Some(true))
        .with_increase_contrast(Some(true));

    cx.simulate_system_preferences_change(reduced).unwrap();
    assert_eq!(cx.system_preferences(), reduced);
    assert_eq!(cx.read(view, |view| view.rendered).unwrap(), reduced);
    assert_eq!(cx.render_count(window).unwrap(), initial_renders + 1);

    cx.update(view, |view, cx| {
        view.observing = false;
        cx.invalidate();
    })
    .unwrap();
    let unsubscribed_renders = cx.render_count(window).unwrap();
    let restored = SystemPreferences::default()
        .with_color_scheme(ColorScheme::Light)
        .with_reduce_motion(Some(false));
    cx.simulate_system_preferences_change(restored).unwrap();
    assert_eq!(cx.render_count(window).unwrap(), unsubscribed_renders);
    assert_eq!(cx.read(view, |view| view.rendered).unwrap(), reduced);

    cx.click(window, "read-system-preferences").unwrap();
    assert_eq!(cx.read(view, |view| view.clicked).unwrap(), restored);
}

#[test]
fn invalid_or_missing_custom_fonts_fail_before_creating_a_test_window() {
    assert!(matches!(
        Application::new()
            .font(&b"not-a-font"[..])
            .into_test_context(WindowOptions::default(), AssetAccessView::default()),
        Err(TestAppError::Asset(AssetError::InvalidFont { index: 0 }))
    ));
    assert!(matches!(
        Application::new()
            .font("fonts/missing.ttf")
            .into_test_context(WindowOptions::default(), AssetAccessView::default()),
        Err(TestAppError::Asset(AssetError::NotFound(path)))
            if path.as_ref() == "fonts/missing.ttf"
    ));
    assert!(matches!(
        Application::new()
            .fonts((0..=MAX_CUSTOM_FONTS).map(|_| vec![0_u8]))
            .into_test_context(WindowOptions::default(), AssetAccessView::default()),
        Err(TestAppError::Asset(AssetError::TooManyFonts))
    ));
}

#[derive(Default)]
struct ContainerQuerySemanticView {
    clicks: usize,
}

impl View for ContainerQuerySemanticView {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let activate = cx.listener("responsive-query-action", |this, cx| {
            this.clicks += 1;
            cx.invalidate();
        });
        container_query(move |size| {
            div()
                .id(if size.width < 480.0 {
                    "compact-query-branch"
                } else {
                    "wide-query-branch"
                })
                .child(button().on_click(activate).child("Activate"))
        })
    }
}

#[test]
fn semantic_context_materializes_and_reconciles_container_queries_without_a_gpu() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(360.0, 240.0),
            ContainerQuerySemanticView::default(),
        )
        .unwrap();
    let window = view.window_handle();

    assert!(cx.contains_element(window, "compact-query-branch").unwrap());
    assert!(!cx.contains_element(window, "wide-query-branch").unwrap());
    cx.click(window, "responsive-query-action").unwrap();
    assert_eq!(cx.read(view, |view| view.clicks).unwrap(), 1);
    assert!(cx.visual_renderer.is_none());

    cx.update(view, |_view, cx| {
        cx.resize_window(Size::new(720.0, 240.0)).unwrap();
    })
    .unwrap();
    assert!(!cx.contains_element(window, "compact-query-branch").unwrap());
    assert!(cx.contains_element(window, "wide-query-branch").unwrap());
    assert!(cx.visual_renderer.is_none());
}

#[derive(Default)]
struct InteractionView {
    value: Arc<str>,
    submitted: Arc<str>,
    submissions: usize,
    clicks: usize,
    click_events: usize,
    key_events: usize,
    focus_events: usize,
    saves: usize,
    child_bubbles: usize,
    parent_bubbles: usize,
}

impl View for InteractionView {
    fn event(&mut self, event: &Event, _cx: &mut EventContext) {
        match event {
            Event::Click(_) => self.click_events += 1,
            Event::KeyDown { .. } => self.key_events += 1,
            Event::FocusChanged(_) => self.focus_events += 1,
            _ => {}
        }
    }

    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let root = cx.focus_handle("interaction-root");
        let input_focus = cx.focus_handle("name");
        let edit = cx.input_listener("name", |this, value, cx| {
            this.value = Arc::from(value);
            cx.invalidate();
        });
        let submit = cx.form_submit_listener("profile", |this, event, cx| {
            this.submitted = Arc::from(event.value("name").unwrap_or_default());
            this.submissions += 1;
            cx.invalidate();
        });
        let increment = cx.listener("increment", |this, cx| {
            this.clicks += 1;
            cx.invalidate();
        });
        let save = cx.action_listener("name", |this, _: &SaveForTest, cx| {
            this.saves += 1;
            cx.invalidate();
        });
        let child_bubble = cx.action_listener("name", |this, _: &BubbleForTest, cx| {
            this.child_bubbles += 1;
            cx.propagate();
        });
        let parent_bubble =
            cx.action_listener("interaction-root", |this, _: &BubbleForTest, cx| {
                this.parent_bubbles += 1;
                cx.invalidate();
            });

        div()
            .focus_scope(root)
            .key_context("Workspace")
            .on_action(parent_bubble)
            .child(
                form()
                    .on_form_submit(submit)
                    .child(
                        text_input(self.value.clone())
                            .track_focus(input_focus)
                            .auto_focus()
                            .key_context("Editor")
                            .on_input(edit)
                            .on_action(save)
                            .on_action(child_bubble),
                    )
                    .child(submit_button().id("submit").child(text("Submit"))),
            )
            .child(button().on_click(increment).child(text("Increment")))
    }
}

#[test]
fn interaction_focus_actions_keymap_input_and_forms_share_production_state() {
    let (mut cx, view) = Application::new()
        .bind_keys([
            KeyBinding::new("ctrl-s", SaveForTest, Some("Editor")),
            KeyBinding::new("ctrl-k left", BubbleForTest, Some("Workspace > Editor")),
        ])
        .into_test_context(WindowOptions::default(), InteractionView::default())
        .unwrap();
    let window = view.window_handle();

    assert!(cx.contains_element(window, "name").unwrap());
    assert!(cx.contains_element(window, "increment").unwrap());
    assert_eq!(cx.focused(window).unwrap(), Some(ElementId::named("name")));

    cx.simulate_input(window, "Ada").unwrap();
    assert_eq!(
        cx.focused_input_value(window).unwrap().as_deref(),
        Some("Ada")
    );
    cx.simulate_keystrokes(window, "ctrl-s").unwrap();
    assert!(cx.dispatch_action(window, BubbleForTest).unwrap());
    cx.simulate_keystrokes(window, "x backspace").unwrap();
    cx.simulate_keystrokes(window, "enter").unwrap();

    cx.click(window, "submit").unwrap();
    cx.click(window, "increment").unwrap();
    assert_eq!(
        cx.focused(window).unwrap(),
        Some(ElementId::named("increment"))
    );
    cx.read(view, |view| {
        assert_eq!(&*view.value, "Ada");
        assert_eq!(&*view.submitted, "Ada");
        assert_eq!(view.submissions, 2);
        assert_eq!(view.saves, 1);
        assert_eq!(view.child_bubbles, 1);
        assert_eq!(view.parent_bubbles, 1);
        assert_eq!(view.clicks, 1);
        assert_eq!(view.click_events, 2);
        assert_eq!(view.key_events, 3);
        assert!(view.focus_events >= 3);
    })
    .unwrap();
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PhaseAction {
    stop_at_parent: bool,
    move_focus: bool,
    fall_through: bool,
}

#[derive(Default)]
struct FocusedDispatchView {
    trace: Vec<&'static str>,
    value: Arc<str>,
    root_key_events: usize,
    disable_parent: bool,
}

impl View for FocusedDispatchView {
    fn event(&mut self, event: &Event, _cx: &mut EventContext) {
        if matches!(event, Event::KeyDown { .. } | Event::KeyUp { .. }) {
            self.root_key_events += 1;
        }
    }

    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let root = cx.focus_handle("phase-root");
        let target = cx.focus_handle("phase-target");
        let alternate = cx.focus_handle("phase-alternate");

        let root_action_capture =
            cx.action_listener("phase-root", move |this, action: &PhaseAction, cx| {
                this.trace.push("action-root-capture");
                if action.move_focus {
                    cx.focus(alternate);
                }
            });
        let parent_action_capture =
            cx.action_listener("phase-parent", |this, action: &PhaseAction, cx| {
                this.trace.push("action-parent-capture");
                if action.stop_at_parent {
                    cx.stop_propagation();
                }
            });
        let target_action_capture =
            cx.action_listener("phase-target", |this, _: &PhaseAction, _cx| {
                this.trace.push("action-target-capture");
            });
        let target_action_bubble =
            cx.action_listener("phase-target", |this, _: &PhaseAction, cx| {
                this.trace.push("action-target-bubble");
                cx.propagate();
            });
        let parent_action_bubble =
            cx.action_listener("phase-parent", |this, _: &PhaseAction, cx| {
                this.trace.push("action-parent-bubble");
                cx.propagate();
            });
        let root_action_bubble =
            cx.action_listener("phase-root", |this, action: &PhaseAction, cx| {
                this.trace.push("action-root-bubble");
                if action.fall_through {
                    cx.propagate();
                }
            });

        let root_key_down = cx.key_down_listener("phase-root", |this, _event, _cx| {
            this.trace.push("key-root-capture-or-bubble");
        });
        let root_key_down_bubble = cx.key_down_listener("phase-root", |this, _event, _cx| {
            this.trace.push("key-root-bubble");
        });
        let parent_key_down = cx.key_down_listener("phase-parent", |this, event, cx| {
            this.trace.push("key-parent-capture");
            if matches!(
                &event.key,
                Key::Character(value) if value == "s" || value == "p"
            ) {
                if matches!(&event.key, Key::Character(value) if value == "p") {
                    cx.prevent_default();
                }
                cx.stop_propagation();
            }
        });
        let parent_key_down_bubble = cx.key_down_listener("phase-parent", |this, _event, _cx| {
            this.trace.push("key-parent-bubble");
        });
        let target_key_down = cx.key_down_listener("phase-target", |this, event, cx| {
            this.trace.push("key-target-capture");
            if matches!(&event.key, Key::Character(value) if value == "x") {
                cx.prevent_default();
            }
        });
        let target_key_down_bubble = cx.key_down_listener("phase-target", |this, _event, _cx| {
            this.trace.push("key-target-bubble");
        });

        let root_key_up = cx.key_up_listener("phase-root", |this, _event, _cx| {
            this.trace.push("up-root-capture");
        });
        let root_key_up_bubble = cx.key_up_listener("phase-root", |this, _event, _cx| {
            this.trace.push("up-root-bubble");
        });
        let target_key_up = cx.key_up_listener("phase-target", |this, _event, _cx| {
            this.trace.push("up-target-capture");
        });
        let target_key_up_bubble = cx.key_up_listener("phase-target", |this, _event, _cx| {
            this.trace.push("up-target-bubble");
        });
        let edit = cx.input_listener("phase-target", |this, value, cx| {
            this.value = Arc::from(value);
            cx.invalidate();
        });

        div()
            .focus_scope(root)
            .capture_action(root_action_capture)
            .on_action(root_action_bubble)
            .capture_key_down(root_key_down)
            .on_key_down(root_key_down_bubble)
            .capture_key_up(root_key_up)
            .on_key_up(root_key_up_bubble)
            .child(
                div()
                    .id("phase-parent")
                    .disabled(self.disable_parent)
                    .capture_action(parent_action_capture)
                    .on_action(parent_action_bubble)
                    .capture_key_down(parent_key_down)
                    .on_key_down(parent_key_down_bubble)
                    .child(
                        text_input(self.value.clone())
                            .track_focus(target)
                            .auto_focus()
                            .on_input(edit)
                            .capture_action(target_action_capture)
                            .on_action(target_action_bubble)
                            .capture_key_down(target_key_down)
                            .on_key_down(target_key_down_bubble)
                            .capture_key_up(target_key_up)
                            .on_key_up(target_key_up_bubble),
                    ),
            )
            .child(button().track_focus(alternate).child("Alternate"))
    }
}

#[test]
fn focused_actions_use_capture_then_default_consuming_bubble_without_idle_rebuilds() {
    let (mut cx, view) = TestAppContext::new(FocusedDispatchView::default()).unwrap();
    let window = view.window_handle();
    let renders = cx.render_count(window).unwrap();

    assert!(
        cx.dispatch_action(
            window,
            PhaseAction {
                stop_at_parent: false,
                move_focus: false,
                fall_through: false,
            },
        )
        .unwrap()
    );
    cx.read(view, |view| {
        assert_eq!(
            view.trace,
            [
                "action-root-capture",
                "action-parent-capture",
                "action-target-capture",
                "action-target-bubble",
                "action-parent-bubble",
                "action-root-bubble",
            ]
        );
    })
    .unwrap();
    assert_eq!(cx.render_count(window).unwrap(), renders);
}

#[test]
fn keymap_actions_precede_raw_key_listeners_and_can_explicitly_fall_through() {
    let consuming = PhaseAction {
        stop_at_parent: false,
        move_focus: false,
        fall_through: false,
    };
    let (mut cx, view) = Application::new()
        .bind_keys([KeyBinding::new("x", consuming, None)])
        .into_test_context(WindowOptions::default(), FocusedDispatchView::default())
        .unwrap();
    let window = view.window_handle();
    cx.simulate_keystrokes(window, "x").unwrap();
    cx.read(view, |view| {
        assert_eq!(
            view.trace,
            [
                "action-root-capture",
                "action-parent-capture",
                "action-target-capture",
                "action-target-bubble",
                "action-parent-bubble",
                "action-root-bubble",
            ]
        );
        assert!(view.value.is_empty());
        assert_eq!(view.root_key_events, 0);
    })
    .unwrap();

    let falling_through = PhaseAction {
        stop_at_parent: false,
        move_focus: false,
        fall_through: true,
    };
    let (mut cx, view) = Application::new()
        .bind_keys([KeyBinding::new("a", falling_through, None)])
        .into_test_context(WindowOptions::default(), FocusedDispatchView::default())
        .unwrap();
    let window = view.window_handle();
    cx.simulate_keystrokes(window, "a").unwrap();
    cx.read(view, |view| {
        assert_eq!(
            view.trace,
            [
                "action-root-capture",
                "action-parent-capture",
                "action-target-capture",
                "action-target-bubble",
                "action-parent-bubble",
                "action-root-bubble",
                "key-root-capture-or-bubble",
                "key-parent-capture",
                "key-target-capture",
                "key-target-bubble",
                "key-parent-bubble",
                "key-root-bubble",
            ]
        );
        assert_eq!(&*view.value, "a");
        assert_eq!(view.root_key_events, 1);
    })
    .unwrap();
}

#[test]
fn capture_actions_can_stop_or_retarget_focus_without_mutating_the_frozen_path() {
    let (mut cx, view) = TestAppContext::new(FocusedDispatchView::default()).unwrap();
    let window = view.window_handle();
    assert!(
        cx.dispatch_action(
            window,
            PhaseAction {
                stop_at_parent: true,
                move_focus: false,
                fall_through: false,
            },
        )
        .unwrap()
    );
    cx.read(view, |view| {
        assert_eq!(view.trace, ["action-root-capture", "action-parent-capture"]);
    })
    .unwrap();

    let (mut cx, view) = TestAppContext::new(FocusedDispatchView::default()).unwrap();
    let window = view.window_handle();
    assert!(
        cx.dispatch_action(
            window,
            PhaseAction {
                stop_at_parent: false,
                move_focus: true,
                fall_through: false,
            },
        )
        .unwrap()
    );
    cx.read(view, |view| {
        assert_eq!(
            view.trace,
            [
                "action-root-capture",
                "action-parent-capture",
                "action-target-capture",
                "action-target-bubble",
                "action-parent-bubble",
                "action-root-bubble",
            ]
        );
    })
    .unwrap();
    assert_eq!(
        cx.focused(window).unwrap(),
        Some(ElementId::named("phase-alternate"))
    );
}

#[test]
fn focused_key_dispatch_orders_phases_and_separates_stop_from_prevent_default() {
    let (mut cx, view) = TestAppContext::new(FocusedDispatchView::default()).unwrap();
    let window = view.window_handle();
    cx.simulate_keystrokes(window, "x").unwrap();
    cx.read(view, |view| {
        assert_eq!(
            view.trace,
            [
                "key-root-capture-or-bubble",
                "key-parent-capture",
                "key-target-capture",
                "key-target-bubble",
                "key-parent-bubble",
                "key-root-bubble",
            ]
        );
        assert!(view.value.is_empty());
        assert_eq!(view.root_key_events, 1);
    })
    .unwrap();

    let (mut cx, view) = TestAppContext::new(FocusedDispatchView::default()).unwrap();
    let window = view.window_handle();
    cx.simulate_keystrokes(window, "s").unwrap();
    cx.read(view, |view| {
        assert_eq!(
            view.trace,
            ["key-root-capture-or-bubble", "key-parent-capture"]
        );
        assert_eq!(&*view.value, "s");
    })
    .unwrap();

    let (mut cx, view) = TestAppContext::new(FocusedDispatchView::default()).unwrap();
    let window = view.window_handle();
    cx.simulate_keystrokes(window, "p").unwrap();
    cx.read(view, |view| {
        assert_eq!(
            view.trace,
            ["key-root-capture-or-bubble", "key-parent-capture"]
        );
        assert!(view.value.is_empty());
    })
    .unwrap();
}

#[test]
fn focused_key_up_uses_capture_and_bubble_without_scheduling_a_rebuild() {
    let (mut cx, view) = TestAppContext::new(FocusedDispatchView::default()).unwrap();
    let window = view.window_handle();
    let renders = cx.render_count(window).unwrap();
    cx.simulate_key_up(window, Keystroke::parse("k").unwrap())
        .unwrap();
    cx.read(view, |view| {
        assert_eq!(
            view.trace,
            [
                "up-root-capture",
                "up-target-capture",
                "up-target-bubble",
                "up-root-bubble",
            ]
        );
        assert_eq!(view.root_key_events, 1);
    })
    .unwrap();
    assert_eq!(cx.render_count(window).unwrap(), renders);
}

#[test]
fn disabled_ancestors_do_not_contribute_action_or_key_listeners_to_the_focus_path() {
    let view = FocusedDispatchView {
        disable_parent: true,
        ..Default::default()
    };
    let (mut cx, view) = TestAppContext::new(view).unwrap();
    let window = view.window_handle();
    assert!(
        cx.dispatch_action(
            window,
            PhaseAction {
                stop_at_parent: false,
                move_focus: false,
                fall_through: false,
            },
        )
        .unwrap()
    );
    cx.read(view, |view| {
        assert_eq!(
            view.trace,
            [
                "action-root-capture",
                "action-target-capture",
                "action-target-bubble",
                "action-root-bubble",
            ]
        );
    })
    .unwrap();

    let view = FocusedDispatchView {
        disable_parent: true,
        ..Default::default()
    };
    let (mut cx, view) = TestAppContext::new(view).unwrap();
    let window = view.window_handle();
    cx.simulate_keystrokes(window, "x").unwrap();
    cx.read(view, |view| {
        assert_eq!(
            view.trace,
            [
                "key-root-capture-or-bubble",
                "key-target-capture",
                "key-target-bubble",
                "key-root-bubble",
            ]
        );
        assert!(view.value.is_empty());
    })
    .unwrap();
}

struct ExcessKeyListeners;

impl View for ExcessKeyListeners {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let id = ElementId::named("excess-key-listeners");
        let mut element = div().id(id);
        for _ in 0..=crate::MAX_KEY_LISTENERS_PER_ELEMENT {
            let listener = cx.key_down_listener(id, |_this, _event, _cx| {});
            element = element.on_key_down(listener);
        }
        element
    }
}

#[test]
#[should_panic(expected = "focused key listeners")]
fn focused_key_listener_count_is_bounded_per_element() {
    let _ = TestAppContext::new(ExcessKeyListeners);
}

struct ExcessActionListeners;

impl View for ExcessActionListeners {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let id = ElementId::named("excess-action-listeners");
        let mut element = div().id(id);
        for _ in 0..=crate::MAX_ACTION_LISTENERS_PER_ELEMENT {
            let listener = cx.action_listener(id, |_this, _: &PhaseAction, _cx| {});
            element = element.on_action(listener);
        }
        element
    }
}

#[test]
#[should_panic(expected = "typed action listeners")]
fn focused_action_listener_count_is_bounded_per_element() {
    let _ = TestAppContext::new(ExcessActionListeners);
}

#[test]
fn focused_listener_registries_have_hard_per_window_bounds() {
    let mut listeners = ListenerRegistry::default();
    let key_callback: KeyListenerCallback = Arc::new(|_view, _event, _cx| {});
    for _ in 0..MAX_KEY_LISTENERS_PER_WINDOW {
        listeners.push_key_listener(key_callback.clone());
    }
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            listeners.push_key_listener(key_callback.clone());
        }))
        .is_err()
    );

    let mut listeners = ListenerRegistry::default();
    let action_callback: ActionCallback = Arc::new(|_view, _action, _cx| {});
    for _ in 0..MAX_ACTION_LISTENERS_PER_WINDOW {
        listeners.push_action_listener(action_callback.clone());
    }
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            listeners.push_action_listener(action_callback.clone());
        }))
        .is_err()
    );
}

#[cfg(target_os = "macos")]
#[test]
fn localized_key_equivalents_follow_the_simulated_layout_without_reloading_bindings() {
    let (mut cx, view) = Application::new()
        .bind_keys([KeyBinding::new("cmd-[", SaveForTest, Some("Editor")).use_key_equivalents()])
        .into_test_context(WindowOptions::default(), InteractionView::default())
        .unwrap();
    cx.simulate_keyboard_layout_change(
        KeyboardLayout::new("com.apple.keylayout.German", "German").unwrap(),
    )
    .unwrap();
    cx.simulate_keystrokes(view.window_handle(), "cmd-ö")
        .unwrap();
    assert_eq!(cx.read(view, |view| view.saves).unwrap(), 1);
}

#[test]
fn printable_key_char_binding_consumes_without_synthesizing_text() {
    let stroke = Keystroke::new(Key::Character("a".to_owned()), Modifiers::ALT)
        .with_key_char(Key::Character("å".to_owned()));
    let (mut cx, view) = Application::new()
        .bind_keys([KeyBinding::new("å", SaveForTest, Some("Editor"))])
        .into_test_context(WindowOptions::default(), InteractionView::default())
        .unwrap();
    let window = view.window_handle();
    cx.simulate_keystroke(window, stroke.clone()).unwrap();
    cx.read(view, |view| {
        assert_eq!(view.saves, 1);
        assert!(view.value.is_empty());
    })
    .unwrap();

    let (mut cx, view) = TestAppContext::new(InteractionView::default()).unwrap();
    cx.simulate_keystroke(view.window_handle(), stroke).unwrap();
    assert_eq!(&*cx.read(view, |view| view.value.clone()).unwrap(), "å");
}

#[test]
fn editor_shortcuts_and_event_context_share_the_bounded_clipboard_service() {
    let (mut cx, view) = TestAppContext::new(InteractionView::default()).unwrap();
    let window = view.window_handle();
    let primary = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };

    cx.simulate_input(window, "copied from editor").unwrap();
    cx.simulate_keystrokes(window, &format!("{primary}-a {primary}-c"))
        .unwrap();
    assert_eq!(
        cx.read_from_clipboard()
            .unwrap()
            .and_then(|item| item.text())
            .as_deref(),
        Some("copied from editor")
    );

    cx.update(view, |_view, cx| {
        cx.write_to_clipboard(
            ClipboardItem::new_string_with_metadata("pasted through context", "metadata").unwrap(),
        )
        .unwrap();
    })
    .unwrap();
    cx.simulate_keystrokes(window, &format!("{primary}-a {primary}-v"))
        .unwrap();
    assert_eq!(
        cx.focused_input_value(window).unwrap().as_deref(),
        Some("pasted through context")
    );
    assert_eq!(
        cx.read_from_clipboard().unwrap().unwrap().metadata(),
        Some("metadata")
    );
}

struct WatchedGlobal(u32);

impl Global for WatchedGlobal {}

struct GlobalView {
    observing: bool,
    observed: u32,
}

impl View for GlobalView {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        if self.observing {
            self.observed = cx.watch_global::<WatchedGlobal, _>(|global| global.0);
        }
        div()
    }
}

#[test]
fn global_observation_invalidates_only_current_declarative_subscribers() {
    let (mut cx, view) = Application::new()
        .global(WatchedGlobal(1))
        .into_test_context(
            WindowOptions::default(),
            GlobalView {
                observing: true,
                observed: 0,
            },
        )
        .unwrap();
    let window = view.window_handle();
    let initial_renders = cx.render_count(window).unwrap();

    cx.update_global::<WatchedGlobal, _>(|global, _| global.0 = 2)
        .unwrap();
    assert_eq!(cx.read(view, |view| view.observed).unwrap(), 2);
    assert_eq!(cx.render_count(window).unwrap(), initial_renders + 1);

    cx.update(view, |view, cx| {
        view.observing = false;
        cx.invalidate();
    })
    .unwrap();
    let unsubscribed_renders = cx.render_count(window).unwrap();
    cx.update_global::<WatchedGlobal, _>(|global, _| global.0 = 3)
        .unwrap();
    assert_eq!(cx.render_count(window).unwrap(), unsubscribed_renders);
    assert_eq!(cx.read_global::<WatchedGlobal, _>(|global| global.0), 3);
}

#[derive(Default)]
struct AsyncView {
    phase: u8,
    animate: bool,
}

impl View for AsyncView {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        if self.animate {
            cx.request_animation_frame();
        }
        div()
    }
}

#[test]
fn foreground_updates_timers_and_frames_advance_only_when_requested() {
    let (mut cx, view) = TestAppContext::new(AsyncView::default()).unwrap();
    let window = view.window_handle();
    cx.update(view, |_view, cx| {
        cx.spawn(|task_cx: AsyncViewContext<AsyncView>| async move {
            task_cx
                .update(|view, cx| {
                    view.phase = 1;
                    cx.invalidate();
                })
                .await
                .unwrap();
            task_cx.sleep(Duration::from_millis(10)).await.unwrap();
            task_cx
                .update(|view, cx| {
                    view.phase = 2;
                    cx.invalidate();
                })
                .await
                .unwrap();
        })
        .unwrap()
        .detach();
    })
    .unwrap();

    assert_eq!(cx.read(view, |view| view.phase).unwrap(), 1);
    cx.advance_time(Duration::from_millis(9)).unwrap();
    assert_eq!(cx.read(view, |view| view.phase).unwrap(), 1);
    cx.advance_time(Duration::from_millis(1)).unwrap();
    assert_eq!(cx.read(view, |view| view.phase).unwrap(), 2);

    cx.update(view, |view, cx| {
        view.animate = true;
        cx.invalidate();
    })
    .unwrap();
    let before_frame = cx.render_count(window).unwrap();
    assert_eq!(cx.advance_frame().unwrap(), 1);
    assert_eq!(cx.render_count(window).unwrap(), before_frame + 1);
    assert_eq!(cx.advance_frame().unwrap(), 1);
    assert_eq!(cx.render_count(window).unwrap(), before_frame + 2);
}

struct DeclarativeAnimationView {
    values: Rc<RefCell<Vec<f32>>>,
    max_fps: Option<f32>,
    repeating: bool,
    synced: bool,
}

impl View for DeclarativeAnimationView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let mut animation = Animation::new(Duration::from_millis(100));
        if self.repeating {
            animation = if self.synced {
                animation.repeat_synced()
            } else {
                animation.repeat()
            };
        }
        if let Some(max_fps) = self.max_fps {
            animation = animation.with_max_fps(max_fps);
        }
        let values = self.values.clone();
        div().with_animation("declarative-progress", animation, move |element, value| {
            values.borrow_mut().push(value);
            element.id("animated-progress").h(8.0).w(value * 100.0)
        })
    }
}

#[test]
fn declarative_animation_advances_only_on_explicit_frames_and_sleeps_when_done() {
    let values = Rc::new(RefCell::new(Vec::new()));
    let (mut cx, view) = TestAppContext::new(DeclarativeAnimationView {
        values: values.clone(),
        max_fps: None,
        repeating: false,
        synced: false,
    })
    .unwrap();
    let window = view.window_handle();
    let initial_renders = cx.render_count(window).unwrap();
    assert_eq!(values.borrow().as_slice(), &[0.0]);

    cx.advance_time(Duration::from_millis(50)).unwrap();
    assert_eq!(cx.render_count(window).unwrap(), initial_renders);
    assert_eq!(cx.advance_frame().unwrap(), 1);
    assert!((values.borrow().last().copied().unwrap() - 0.5).abs() < 0.001);

    cx.advance_time(Duration::from_millis(50)).unwrap();
    assert_eq!(cx.advance_frame().unwrap(), 1);
    assert_eq!(values.borrow().last().copied(), Some(1.0));
    assert_eq!(cx.advance_frame().unwrap(), 0);
    assert_eq!(cx.render_count(window).unwrap(), initial_renders + 2);
}

#[test]
fn declarative_animation_max_fps_uses_exact_deadlines_without_frame_requests() {
    let values = Rc::new(RefCell::new(Vec::new()));
    let (mut cx, view) = TestAppContext::new(DeclarativeAnimationView {
        values: values.clone(),
        max_fps: Some(10.0),
        repeating: true,
        synced: false,
    })
    .unwrap();
    let window = view.window_handle();
    let initial_renders = cx.render_count(window).unwrap();
    assert_eq!(cx.advance_frame().unwrap(), 0);

    cx.advance_time(Duration::from_millis(99)).unwrap();
    assert_eq!(cx.render_count(window).unwrap(), initial_renders);
    cx.advance_time(Duration::from_millis(1)).unwrap();
    assert_eq!(cx.render_count(window).unwrap(), initial_renders + 1);
    assert!((values.borrow().last().copied().unwrap() - 0.0).abs() < 0.001);
}

#[test]
fn reduced_motion_resolves_static_terminal_and_repeating_phases() {
    let oneshot_values = Rc::new(RefCell::new(Vec::new()));
    let (mut cx, oneshot) = Application::new()
        .into_test_context(
            WindowOptions::default().reduce_motion(true),
            DeclarativeAnimationView {
                values: oneshot_values.clone(),
                max_fps: None,
                repeating: false,
                synced: false,
            },
        )
        .unwrap();
    assert_eq!(oneshot_values.borrow().as_slice(), &[1.0]);
    assert_eq!(cx.advance_frame().unwrap(), 0);

    let repeating_values = Rc::new(RefCell::new(Vec::new()));
    let repeating_window = cx
        .update(oneshot, |_view, cx| {
            cx.open_window(
                WindowOptions::default().reduce_motion(true),
                DeclarativeAnimationView {
                    values: repeating_values.clone(),
                    max_fps: None,
                    repeating: true,
                    synced: false,
                },
            )
        })
        .unwrap();
    assert_eq!(repeating_values.borrow().as_slice(), &[0.0]);
    assert_eq!(cx.advance_frame().unwrap(), 0);
    assert!(cx.is_window_open(oneshot.window_handle()));
    assert!(cx.is_window_open(repeating_window));
}

#[test]
fn repeat_synced_animations_share_one_application_epoch_across_windows() {
    let first_values = Rc::new(RefCell::new(Vec::new()));
    let (mut cx, first) = TestAppContext::new(DeclarativeAnimationView {
        values: first_values.clone(),
        max_fps: None,
        repeating: true,
        synced: true,
    })
    .unwrap();
    cx.advance_time(Duration::from_millis(25)).unwrap();

    let second_values = Rc::new(RefCell::new(Vec::new()));
    cx.update(first, |_view, cx| {
        cx.open_window(
            WindowOptions::default(),
            DeclarativeAnimationView {
                values: second_values.clone(),
                max_fps: None,
                repeating: true,
                synced: true,
            },
        );
    })
    .unwrap();
    assert!((second_values.borrow().last().copied().unwrap() - 0.25).abs() < 0.001);

    assert_eq!(cx.advance_frame().unwrap(), 2);
    assert!((first_values.borrow().last().copied().unwrap() - 0.25).abs() < 0.001);
    assert!((second_values.borrow().last().copied().unwrap() - 0.25).abs() < 0.001);
}

struct SpringAnimationView {
    target: f32,
    playback: SpringPlayback,
    initial: Option<f32>,
    values: Rc<RefCell<Vec<f32>>>,
}

impl View for SpringAnimationView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let mut spring = SpringAnimation::new(SpringConfig::new(100.0, 2.0, 1.0))
            .to(self.target)
            .with_epsilon(0.01)
            .playback(self.playback);
        if let Some(initial) = self.initial {
            spring = spring.from(initial);
        }
        let values = self.values.clone();
        div().with_spring("retargetable-spring", spring, move |element, value| {
            values.borrow_mut().push(value);
            element.id("spring-position").relative().left(value)
        })
    }
}

#[test]
fn springs_preserve_velocity_when_retargeted_and_pause_without_catching_up() {
    let values = Rc::new(RefCell::new(Vec::new()));
    let (mut cx, view) = TestAppContext::new(SpringAnimationView {
        target: 0.0,
        playback: SpringPlayback::Running,
        initial: None,
        values: values.clone(),
    })
    .unwrap();
    let window = view.window_handle();
    assert_eq!(values.borrow().as_slice(), &[0.0]);
    assert_eq!(cx.advance_frame().unwrap(), 0);

    cx.update(view, |view, cx| {
        view.target = 100.0;
        cx.invalidate();
    })
    .unwrap();
    cx.advance_time(Duration::from_millis(50)).unwrap();
    assert_eq!(cx.advance_frame().unwrap(), 1);
    let before_retarget = values.borrow().last().copied().unwrap();
    assert!(before_retarget > 0.0 && before_retarget < 100.0);

    cx.update(view, |view, cx| {
        view.target = 0.0;
        cx.invalidate();
    })
    .unwrap();
    cx.advance_time(Duration::from_millis(5)).unwrap();
    assert_eq!(cx.advance_frame().unwrap(), 1);
    let after_retarget = values.borrow().last().copied().unwrap();
    assert!(after_retarget > before_retarget);

    cx.update(view, |view, cx| {
        view.playback = SpringPlayback::Paused;
        cx.invalidate();
    })
    .unwrap();
    let paused = values.borrow().last().copied().unwrap();
    cx.advance_time(Duration::from_secs(1)).unwrap();
    assert_eq!(cx.advance_frame().unwrap(), 0);
    assert_eq!(values.borrow().last().copied(), Some(paused));

    cx.update(view, |view, cx| {
        view.playback = SpringPlayback::Running;
        cx.invalidate();
    })
    .unwrap();
    assert_eq!(values.borrow().last().copied(), Some(paused));
    cx.advance_time(Duration::from_millis(5)).unwrap();
    assert_eq!(cx.advance_frame().unwrap(), 1);
    assert_ne!(values.borrow().last().copied(), Some(paused));
    assert!(cx.render_count(window).unwrap() >= 7);
}

#[test]
fn reduced_motion_snaps_springs_to_their_target_without_scheduling() {
    let values = Rc::new(RefCell::new(Vec::new()));
    let (mut cx, _view) = Application::new()
        .into_test_context(
            WindowOptions::default().reduce_motion(true),
            SpringAnimationView {
                target: 100.0,
                playback: SpringPlayback::Running,
                initial: Some(0.0),
                values: values.clone(),
            },
        )
        .unwrap();
    assert_eq!(values.borrow().as_slice(), &[100.0]);
    assert_eq!(cx.advance_frame().unwrap(), 0);
}
