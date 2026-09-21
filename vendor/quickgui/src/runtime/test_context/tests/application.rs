use super::*;

struct ChildView(u32);

impl View for ChildView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div().child(text(self.0.to_string()))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ChildCommand(u32);

#[test]
fn application_actions_can_open_a_window_after_the_last_window_closes() {
    let opened = Rc::new(Cell::new(None));
    let calls = Rc::new(Cell::new(0));
    let application = Application::new()
        .quit_mode(QuitMode::Explicit)
        .menu(Menu::new("File").item(crate::MenuItem::action("Open", ChildCommand(7))))
        .on_action({
            let opened = Rc::clone(&opened);
            let calls = Rc::clone(&calls);
            move |action: &ChildCommand, cx| {
                assert!(cx.window_handle().is_none());
                calls.set(calls.get() + 1);
                opened.set(Some(cx.open_window(
                    WindowOptions::new("Opened from menu"),
                    ChildView(action.0),
                )));
            }
        });
    let (mut cx, original) = application
        .into_test_context(WindowOptions::default(), ChildView(0))
        .unwrap();
    for window in [Some(original.window_handle()), None] {
        let window = window.unwrap_or_else(|| opened.get().unwrap());
        assert!(cx.simulate_close_requested(window).unwrap());
        assert!(cx.windows().is_empty());
        assert!(!cx.is_exited());
        assert!(
            cx.application_callbacks
                .actions
                .contains_key(&TypeId::of::<ChildCommand>())
        );
        let menu = collect_menu_actions(cx.menus());
        assert!(
            cx.invoke_application_action(None, menu[0].action.as_ref().unwrap())
                .unwrap()
        );
        cx.run_until_idle().unwrap();
        let replacement = opened.get().unwrap();
        assert!(cx.is_window_open(replacement));
        assert_eq!(cx.window(replacement).unwrap().parent, None);
    }
    assert_eq!(calls.get(), 2);
}

#[test]
fn focused_action_handlers_take_precedence_over_application_handlers() {
    let total = Rc::new(Cell::new(0));
    let application = Application::new().quit_mode(QuitMode::Explicit).on_action({
        let total = Rc::clone(&total);
        move |action: &ChildCommand, _cx| total.set(total.get() + action.0)
    });
    let (mut cx, root) = application
        .into_test_context(WindowOptions::default(), ActionParentView::default())
        .unwrap();
    assert!(cx.dispatch_application_action(ChildCommand(3)).unwrap());
    assert_eq!(cx.read(root, |view| view.command_total).unwrap(), 3);
    assert_eq!(total.get(), 0);
    assert!(cx.simulate_close_requested(root.window_handle()).unwrap());
    assert!(cx.dispatch_application_action(ChildCommand(5)).unwrap());
    assert_eq!(total.get(), 5);
    assert!(!cx.dispatch_application_action(123_u64).unwrap());
}

struct ActionChildView;

impl View for ActionChildView {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let choose = cx.listener("child-command", |_view, cx| {
            assert!(cx.dispatch_action_to_parent(ChildCommand(7)));
            cx.close_window();
        });
        button().on_click(choose).auto_focus().child(text("Choose"))
    }
}

#[derive(Default)]
struct ActionParentView {
    child: Option<WindowHandle>,
    command_total: u32,
}

impl View for ActionParentView {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let command =
            cx.action_listener("parent-action-root", |view, action: &ChildCommand, cx| {
                view.command_total = view.command_total.saturating_add(action.0);
                cx.invalidate();
            });
        div()
            .focus_scope(cx.focus_handle("parent-action-root"))
            .on_action(command)
            .child(button().id("parent-focus").auto_focus().child("Parent"))
    }
}

#[test]
fn child_actions_dispatch_to_the_parent_before_the_runtime_returns_idle() {
    let (mut cx, parent) = TestAppContext::new(ActionParentView::default()).unwrap();
    let child = cx
        .update(parent, |view, cx| {
            let child = cx.open_window(WindowOptions::new("Child action"), ActionChildView);
            view.child = Some(child);
            child
        })
        .unwrap();
    assert_eq!(
        cx.window(child).unwrap().parent,
        Some(parent.window_handle())
    );

    cx.click(child, "child-command").unwrap();
    assert!(!cx.is_window_open(child));
    assert_eq!(cx.read(parent, |view| view.command_total).unwrap(), 7);
    let renders = cx.render_count(parent.window_handle()).unwrap();
    cx.run_until_idle().unwrap();
    assert_eq!(cx.render_count(parent.window_handle()).unwrap(), renders);
}

#[derive(Default)]
struct ChildLifecycleParentView {
    child: Option<WindowHandle>,
    closed_count: usize,
}

impl View for ChildLifecycleParentView {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        cx.on_any_child_window_closed(|view, closed, cx| {
            if view.child == Some(closed) {
                view.child = None;
                view.closed_count += 1;
                cx.invalidate();
            }
        });
        div()
    }
}

#[test]
fn declarative_child_close_listener_synchronizes_owner_state_and_returns_to_sleep() {
    let (mut cx, parent) = TestAppContext::new(ChildLifecycleParentView::default()).unwrap();
    let child = cx
        .update(parent, |view, cx| {
            let child = cx.open_window(WindowOptions::new("Observed child"), ChildView(9));
            view.child = Some(child);
            cx.invalidate();
            child
        })
        .unwrap();

    cx.update(parent, |_view, cx| cx.close_window_handle(child))
        .unwrap();

    assert!(!cx.is_window_open(child));
    assert_eq!(
        cx.read(parent, |view| (view.child, view.closed_count))
            .unwrap(),
        (None, 1)
    );
    let renders = cx.render_count(parent.window_handle()).unwrap();
    cx.run_until_idle().unwrap();
    assert_eq!(cx.render_count(parent.window_handle()).unwrap(), renders);
}

#[test]
fn any_child_close_listener_covers_open_and_close_in_one_effect_turn() {
    let (mut cx, parent) = TestAppContext::new(ChildLifecycleParentView::default()).unwrap();
    let child = cx
        .update(parent, |view, cx| {
            let child = cx.open_window(WindowOptions::new("Ephemeral child"), ChildView(11));
            view.child = Some(child);
            cx.close_window_handle(child);
            child
        })
        .unwrap();

    assert!(!cx.is_window_open(child));
    assert_eq!(
        cx.read(parent, |view| (view.child, view.closed_count))
            .unwrap(),
        (None, 1)
    );
}

#[derive(Default)]
struct ParentView {
    child: Option<WindowHandle>,
}

impl View for ParentView {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let _state = cx.window_state();
        div()
    }
}

#[test]
fn multiple_windows_keep_typed_views_commands_focus_and_tree_ownership() {
    let (mut cx, parent) = Application::new()
        .into_test_context(
            WindowOptions::default().title("Parent"),
            ParentView::default(),
        )
        .unwrap();
    let parent_window = parent.window_handle();
    cx.update(parent, |view, cx| {
        assert_eq!(cx.windows(), [parent_window]);
        assert_eq!(cx.active_window(), Some(parent_window));
        assert!(cx.window_registry().contains(parent_window));
        let child = cx.open_window(
            WindowOptions::default().title("Child").size(320.0, 180.0),
            ChildView(7),
        );
        view.child = Some(child);
        cx.set_window_title("Renamed parent").unwrap();
        cx.resize_window(Size::new(640.0, 360.0)).unwrap();
    })
    .unwrap();

    let child_window = cx.read(parent, |view| view.child.unwrap()).unwrap();
    let child = cx.typed_window::<ChildView>(child_window).unwrap();
    assert_eq!(cx.read(child, |view| view.0).unwrap(), 7);
    assert_eq!(cx.windows().len(), 2);
    assert_eq!(cx.window_registry().windows(), cx.windows());
    assert_eq!(cx.window_registry().active_window(), Some(child_window));
    assert!(!cx.window_registry().is_truncated());
    assert_eq!(cx.window_title(parent_window).unwrap(), "Renamed parent");
    assert_eq!(
        cx.window_state(parent_window).unwrap().viewport_size,
        Size::new(640.0, 360.0)
    );
    assert_eq!(cx.active_window(), Some(child_window));

    cx.update(parent, |_view, cx| cx.close_window_handle(child_window))
        .unwrap();
    assert_eq!(cx.active_window(), Some(parent_window));
    assert_eq!(cx.windows(), [parent_window]);
    assert!(!cx.window_registry().contains(child_window));

    let replacement = cx
        .update(parent, |view, cx| {
            let child = cx.open_window(WindowOptions::new("Replacement"), ChildView(8));
            view.child = Some(child);
            child
        })
        .unwrap();
    cx.update(parent, |_view, cx| cx.close_window()).unwrap();
    assert!(cx.windows().is_empty());
    assert!(!cx.is_window_open(parent_window));
    assert!(!cx.is_window_open(child_window));
    assert!(!cx.is_window_open(replacement));
}

#[test]
fn minimum_window_size_is_observable_and_mutable_without_polling() {
    let initial = Size::new(480.0, 300.0);
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().minimum_size(initial.width, initial.height),
            ParentView::default(),
        )
        .unwrap();
    let window = view.window_handle();
    assert_eq!(cx.window_state(window).unwrap().minimum_size, Some(initial));
    let renders = cx.render_count(window).unwrap();

    let runtime_minimum = Size::new(640.0, 420.0);
    cx.update(view, |_view, cx| {
        cx.set_window_minimum_size(runtime_minimum).unwrap();
    })
    .unwrap();
    assert_eq!(
        cx.window_state(window).unwrap().minimum_size,
        Some(runtime_minimum)
    );
    assert_eq!(cx.render_count(window).unwrap(), renders + 1);

    // Repeating the effective constraint does not rebuild an observing declaration.
    cx.update(view, |_view, cx| {
        cx.set_window_minimum_size(runtime_minimum).unwrap();
    })
    .unwrap();
    assert_eq!(cx.render_count(window).unwrap(), renders + 1);

    let invalid = cx
        .update(view, |_view, cx| {
            cx.set_window_minimum_size(Size::new(f32::NAN, 100.0))
        })
        .unwrap();
    assert_eq!(invalid, Err(WindowCommandError::InvalidBounds));
    assert_eq!(
        cx.window_state(window).unwrap().minimum_size,
        Some(runtime_minimum)
    );

    cx.update(view, |_view, cx| {
        cx.clear_window_minimum_size().unwrap();
    })
    .unwrap();
    assert_eq!(cx.window_state(window).unwrap().minimum_size, None);
    assert_eq!(cx.render_count(window).unwrap(), renders + 2);
}

#[test]
fn window_policies_and_size_constraints_are_independently_mutable() {
    let minimum = Size::new(200.0, 120.0);
    let maximum = Size::new(800.0, 600.0);
    let icon = Image::from_rgba(1, 1, [12, 34, 56, 255].as_slice()).unwrap();
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default()
                .minimum_size(minimum.width, minimum.height)
                .maximum_size(maximum.width, maximum.height)
                .minimizable(false)
                .maximizable(false)
                .closable(false)
                .decorations(false)
                .shadow(false)
                .content_protected(true)
                .window_level(WindowLevel::AlwaysOnBottom)
                .focusable(false)
                .skip_taskbar(true)
                .visible_on_all_workspaces(true)
                .opacity(0.75)
                .icon(icon)
                .taskbar_progress(TaskbarProgressState::Paused, 0.25)
                .taskbar_overlay_icon(
                    Image::from_rgba(1, 1, [56, 34, 12, 255].as_slice()).unwrap(),
                    "Needs attention",
                )
                .cursor_visible(false)
                .cursor_grab(CursorGrabMode::Confined)
                .cursor_hit_test(false)
                .cursor_position(Point::new(32.0, 48.0)),
            ParentView::default(),
        )
        .unwrap();
    let window = view.window_handle();
    let state = cx.window_state(window).unwrap();
    assert_eq!(state.minimum_size, Some(minimum));
    assert_eq!(state.maximum_size, Some(maximum));
    assert!(!state.minimizable);
    assert!(!state.maximizable);
    assert!(!state.closable);
    assert!(!state.decorated);
    assert!(!state.shadow);
    assert!(state.content_protected);
    assert_eq!(state.window_level, WindowLevel::AlwaysOnBottom);
    assert!(!state.focusable);
    assert!(state.skip_taskbar);
    assert!(state.visible_on_all_workspaces);
    assert_eq!(state.opacity, 0.75);
    assert!(state.has_icon);
    assert_eq!(state.taskbar_progress_state, TaskbarProgressState::Paused);
    assert_eq!(state.taskbar_progress, 0.25);
    assert!(state.has_taskbar_overlay_icon);
    assert!(!state.cursor_visible);
    assert_eq!(state.cursor_grab, CursorGrabMode::Confined);
    assert!(!state.cursor_hit_test);
    assert_eq!(state.cursor_position, Some(Point::new(32.0, 48.0)));

    let runtime_maximum = Size::new(500.0, 400.0);
    cx.update(view, |_view, cx| {
        cx.set_window_maximum_size(runtime_maximum).unwrap();
        cx.resize_window(Size::new(900.0, 700.0)).unwrap();
        cx.set_window_minimizable(true).unwrap();
        cx.set_window_maximizable(true).unwrap();
        cx.set_window_closable(true).unwrap();
        cx.set_window_decorated(true).unwrap();
        cx.set_window_shadow(true).unwrap();
        cx.set_window_content_protected(false).unwrap();
        cx.set_window_always_on_top(true).unwrap();
        cx.set_window_focusable(true).unwrap();
        cx.set_window_skip_taskbar(false).unwrap();
        cx.set_window_visible_on_all_workspaces(false).unwrap();
        cx.set_window_opacity(0.5).unwrap();
        cx.clear_window_icon().unwrap();
        cx.set_taskbar_progress(TaskbarProgressState::Normal, 0.75)
            .unwrap();
        cx.clear_taskbar_overlay_icon().unwrap();
        cx.set_cursor_visible(true).unwrap();
        cx.set_cursor_grab(CursorGrabMode::Locked).unwrap();
        cx.set_cursor_hit_test(true).unwrap();
        cx.set_cursor_position(Point::new(64.0, 96.0)).unwrap();
    })
    .unwrap();
    let state = cx.window_state(window).unwrap();
    assert_eq!(state.maximum_size, Some(runtime_maximum));
    assert_eq!(state.viewport_size, Size::new(900.0, 700.0));
    assert!(state.minimizable);
    assert!(state.maximizable);
    assert!(state.closable);
    assert!(state.decorated);
    assert!(state.shadow);
    assert!(!state.content_protected);
    assert_eq!(state.window_level, WindowLevel::AlwaysOnTop);
    assert!(state.focusable);
    assert!(!state.skip_taskbar);
    assert!(!state.visible_on_all_workspaces);
    assert_eq!(state.opacity, 0.5);
    assert!(!state.has_icon);
    assert_eq!(state.taskbar_progress_state, TaskbarProgressState::Normal);
    assert_eq!(state.taskbar_progress, 0.75);
    assert!(!state.has_taskbar_overlay_icon);
    assert!(state.cursor_visible);
    assert_eq!(state.cursor_grab, CursorGrabMode::Locked);
    assert!(state.cursor_hit_test);
    assert_eq!(state.cursor_position, Some(Point::new(64.0, 96.0)));

    cx.update(view, |_view, cx| {
        cx.clear_window_maximum_size().unwrap();
        cx.use_automatic_window_level().unwrap();
    })
    .unwrap();
    let state = cx.window_state(window).unwrap();
    assert_eq!(state.maximum_size, None);
    assert_eq!(state.window_level, WindowLevel::Normal);

    assert!(matches!(
        Application::new().into_test_context(
            WindowOptions::default()
                .minimum_size(500.0, 400.0)
                .maximum_size(300.0, 200.0),
            ParentView::default(),
        ),
        Err(TestAppError::InvalidWindow(
            WindowCommandError::InvalidSizeConstraints
        ))
    ));
    assert!(matches!(
        Application::new().into_test_context(
            WindowOptions::default().opacity(f32::NAN),
            ParentView::default(),
        ),
        Err(TestAppError::InvalidWindow(
            WindowCommandError::InvalidOpacity
        ))
    ));
}

#[test]
fn document_window_state_and_bounded_native_tabs_are_deterministic() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default()
                .document_path("Cargo.toml")
                .document_edited(true)
                .tabbing_identifier("dev.quickgui.workspace"),
            ParentView::default(),
        )
        .unwrap();
    let window = view.window_handle();
    let initial = cx.window_state(window).unwrap();
    assert!(initial.represented_file);
    assert!(initial.document_edited);
    assert!(initial.native_tabbing);
    assert_eq!(initial.native_tabs, WindowTabState::default());

    cx.update(view, |_view, cx| {
        cx.clear_represented_file().unwrap();
        cx.set_window_edited(false).unwrap();
    })
    .unwrap();
    let state = cx.window_state(window).unwrap();
    assert!(!state.represented_file);
    assert!(!state.document_edited);

    let renders = cx.render_count(window).unwrap();
    let tabs = WindowTabState {
        count: 4,
        selected_index: Some(2),
        tab_bar_visible: true,
        overview_visible: false,
        truncated: false,
    };
    cx.simulate_window_tab_state(window, tabs).unwrap();
    assert_eq!(cx.window_state(window).unwrap().native_tabs, tabs);
    assert_eq!(cx.render_count(window).unwrap(), renders + 1);

    cx.update(view, |_view, cx| cx.select_next_tab().unwrap())
        .unwrap();
    assert_eq!(
        cx.window_state(window).unwrap().native_tabs.selected_index,
        Some(3)
    );
    assert!(matches!(
        cx.simulate_window_tab_state(
            window,
            WindowTabState {
                count: 0,
                selected_index: None,
                tab_bar_visible: false,
                overview_visible: false,
                truncated: false,
            },
        ),
        Err(TestAppError::InvalidWindowTabState)
    ));
}

struct LifecycleView {
    prevent_close: bool,
    close_requests: usize,
}

impl View for LifecycleView {
    fn event(&mut self, event: &Event, cx: &mut EventContext) {
        if matches!(event, Event::CloseRequested) {
            self.close_requests += 1;
            if self.prevent_close {
                cx.prevent_close();
            }
            cx.invalidate();
        }
    }

    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div()
    }
}

#[test]
fn lifecycle_callbacks_and_interceptable_close_requests_are_synchronous() {
    let opened = Rc::new(RefCell::new(Vec::new()));
    let reopened = Rc::new(Cell::new(false));
    let wakes = Rc::new(Cell::new(0_usize));
    let power_events = Rc::new(RefCell::new(Vec::new()));
    let keyboard_layouts = Rc::new(RefCell::new(Vec::new()));
    let notification = Rc::new(RefCell::new(None));
    let application = Application::new()
        .menu(Menu::new("File"))
        .on_open_urls({
            let opened = opened.clone();
            move |urls, _cx| opened.borrow_mut().extend(urls.iter().map(str::to_owned))
        })
        .on_reopen({
            let reopened = reopened.clone();
            move |visible, _cx| reopened.set(visible)
        })
        .on_system_wake({
            let wakes = wakes.clone();
            move |_cx| wakes.set(wakes.get() + 1)
        })
        .on_power_event({
            let power_events = power_events.clone();
            move |event, _cx| power_events.borrow_mut().push(event)
        })
        .on_keyboard_layout_change({
            let keyboard_layouts = keyboard_layouts.clone();
            move |layout, cx| {
                keyboard_layouts
                    .borrow_mut()
                    .push((layout.id().to_owned(), cx.keyboard_layout().id().to_owned()));
            }
        })
        .on_system_notification_response({
            let notification = notification.clone();
            move |response, _cx| {
                *notification.borrow_mut() = Some((response.tag, response.action_id));
            }
        });
    let (mut cx, view) = application
        .into_test_context(
            WindowOptions::default(),
            LifecycleView {
                prevent_close: true,
                close_requests: 0,
            },
        )
        .unwrap();
    let window = view.window_handle();

    assert_eq!(cx.menus().len(), 1);
    cx.simulate_open_urls(["quickgui://one", "quickgui://two"])
        .unwrap();
    cx.simulate_reopen(true).unwrap();
    cx.simulate_system_wake().unwrap();
    cx.simulate_power_event(PowerEvent::ThermalStateChanged(ThermalState::Serious))
        .unwrap();
    cx.simulate_power_event(PowerEvent::ShutdownRequested)
        .unwrap();
    cx.simulate_keyboard_layout_change(
        KeyboardLayout::new("com.apple.keylayout.German", "German").unwrap(),
    )
    .unwrap();
    cx.simulate_system_notification_response(SystemNotificationResponse {
        tag: Arc::from("build"),
        action_id: Some(Arc::from("open")),
        reply: None,
    })
    .unwrap();
    assert_eq!(&*opened.borrow(), &["quickgui://one", "quickgui://two"]);
    assert!(reopened.get());
    assert_eq!(wakes.get(), 1);
    assert_eq!(
        &*power_events.borrow(),
        &[
            PowerEvent::ThermalStateChanged(ThermalState::Serious),
            PowerEvent::ShutdownRequested,
        ]
    );
    assert_eq!(
        &*keyboard_layouts.borrow(),
        &[(
            "com.apple.keylayout.German".to_owned(),
            "com.apple.keylayout.German".to_owned(),
        )]
    );
    assert_eq!(
        &*notification.borrow(),
        &Some((Arc::from("build"), Some(Arc::from("open"))))
    );

    assert!(!cx.simulate_close_requested(window).unwrap());
    assert_eq!(cx.read(view, |view| view.close_requests).unwrap(), 1);
    cx.update(view, |view, _cx| view.prevent_close = false)
        .unwrap();
    assert!(cx.simulate_close_requested(window).unwrap());
    assert!(cx.is_exited() || cx.windows().is_empty());
}

#[test]
fn quit_modes_and_post_close_callbacks_preserve_native_lifecycle() {
    let closed = Rc::new(RefCell::new(Vec::new()));
    let reopened = Rc::new(Cell::new(None));
    let application = Application::new()
        .quit_mode(QuitMode::Explicit)
        .on_window_closed({
            let closed = closed.clone();
            move |window, _cx| closed.borrow_mut().push(window)
        })
        .on_reopen({
            let reopened = reopened.clone();
            move |_visible, cx| {
                reopened.set(Some(
                    cx.open_window(WindowOptions::new("Reopened"), ChildView(99)),
                ));
            }
        });
    let (mut cx, parent) = application
        .into_test_context(WindowOptions::default(), ParentView::default())
        .unwrap();
    let parent_window = parent.window_handle();
    let child_window = cx
        .update(parent, |_view, cx| {
            cx.open_window(WindowOptions::new("Child"), ChildView(7))
        })
        .unwrap();

    cx.update(parent, |_view, cx| cx.close_window()).unwrap();
    assert_eq!(&*closed.borrow(), &[child_window, parent_window]);
    assert!(cx.windows().is_empty());
    assert!(!cx.is_exited());

    cx.simulate_reopen(false).unwrap();
    let reopened_window = reopened
        .get()
        .expect("reopen callback should create a window");
    assert!(cx.is_window_open(reopened_window));
    assert_eq!(cx.windows(), [reopened_window]);

    let (mut cx, root) = Application::new()
        .with_quit_mode(QuitMode::LastWindowClosed)
        .into_test_context(WindowOptions::default(), ChildView(1))
        .unwrap();
    cx.update(root, |_view, cx| cx.close_window()).unwrap();
    assert!(cx.windows().is_empty());
    assert!(cx.is_exited());
}

#[test]
fn explicit_exit_closes_owned_windows_and_runs_each_close_callback() {
    let closed = Rc::new(RefCell::new(Vec::new()));
    let (mut cx, parent) = Application::new()
        .quit_mode(QuitMode::Explicit)
        .on_window_closed({
            let closed = closed.clone();
            move |window, _cx| closed.borrow_mut().push(window)
        })
        .into_test_context(WindowOptions::default(), ParentView::default())
        .unwrap();
    let parent_window = parent.window_handle();
    let child_window = cx
        .update(parent, |_view, cx| {
            cx.open_window(WindowOptions::new("Child"), ChildView(3))
        })
        .unwrap();

    cx.update(parent, |_view, cx| cx.exit()).unwrap();

    assert!(cx.is_exited());
    assert!(cx.windows().is_empty());
    assert_eq!(&*closed.borrow(), &[child_window, parent_window]);
}

#[test]
fn before_and_will_quit_are_ordered_and_independently_preventable() {
    let phases = Rc::new(RefCell::new(Vec::new()));
    let prevent_before_once = Rc::new(Cell::new(true));
    let (mut cx, root) = Application::new()
        .quit_mode(QuitMode::Explicit)
        .on_before_quit({
            let phases = phases.clone();
            let prevent_before_once = prevent_before_once.clone();
            move |request, cx| {
                phases.borrow_mut().push(("before", request.reason));
                if prevent_before_once.replace(false) {
                    cx.prevent_quit();
                }
            }
        })
        .on_will_quit({
            let phases = phases.clone();
            move |request, _cx| phases.borrow_mut().push(("will", request.reason))
        })
        .into_test_context(WindowOptions::default(), ParentView::default())
        .unwrap();

    cx.update(root, |_view, cx| cx.exit()).unwrap();
    assert!(!cx.is_exited());
    assert!(cx.is_window_open(root.window_handle()));
    assert_eq!(&*phases.borrow(), &[("before", QuitReason::Explicit)]);

    cx.update(root, |_view, cx| cx.exit()).unwrap();
    assert!(cx.is_exited());
    assert!(cx.windows().is_empty());
    assert_eq!(
        &*phases.borrow(),
        &[
            ("before", QuitReason::Explicit),
            ("before", QuitReason::Explicit),
            ("will", QuitReason::Explicit),
        ]
    );
}

#[test]
fn orderly_relaunch_is_retained_only_after_child_first_teardown() {
    let closed = Rc::new(RefCell::new(Vec::new()));
    let (mut cx, parent) = Application::new()
        .quit_mode(QuitMode::Explicit)
        .on_window_closed({
            let closed = closed.clone();
            move |window, _cx| closed.borrow_mut().push(window)
        })
        .into_test_context(WindowOptions::default(), ParentView::default())
        .unwrap();
    let parent_window = parent.window_handle();
    let child_window = cx
        .update(parent, |_view, cx| {
            cx.open_window(WindowOptions::new("Child"), ChildView(5))
        })
        .unwrap();
    let executable = std::env::current_exe().unwrap();
    cx.update(parent, |_view, cx| {
        cx.relaunch_with(
            RelaunchOptions::new()
                .executable(&executable)
                .arguments(["--restored"])
                .working_directory(std::env::current_dir().unwrap()),
        )
        .unwrap();
    })
    .unwrap();

    assert!(cx.is_exited());
    assert!(cx.windows().is_empty());
    assert_eq!(&*closed.borrow(), &[child_window, parent_window]);
    let request = cx.relaunch_request().unwrap();
    assert_eq!(request.executable(), executable);
    assert_eq!(
        request.arguments(),
        [std::ffi::OsString::from("--restored")]
    );
}

struct AppearanceView {
    observe: bool,
    rendered: WindowAppearance,
    native_events: Vec<WindowAppearance>,
}

impl View for AppearanceView {
    fn event(&mut self, event: &Event, _cx: &mut EventContext) {
        if let Event::AppearanceChanged(appearance) = event {
            self.native_events.push(*appearance);
        }
    }

    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        if self.observe {
            self.rendered = cx.appearance();
        }
        div()
    }
}

#[test]
fn appearance_simulation_respects_forced_and_system_following_modes() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().window_appearance(WindowAppearance::Dark),
            AppearanceView {
                observe: true,
                rendered: WindowAppearance::Light,
                native_events: Vec::new(),
            },
        )
        .unwrap();
    let window = view.window_handle();
    let initial_renders = cx.render_count(window).unwrap();

    assert_eq!(
        cx.window_state(window).unwrap().appearance,
        WindowAppearance::Dark
    );
    assert_eq!(
        cx.read(view, |view| view.rendered).unwrap(),
        WindowAppearance::Dark
    );

    // A forced window remembers, but does not adopt or dispatch, the system change.
    cx.simulate_appearance_change(window, WindowAppearance::Light)
        .unwrap();
    assert_eq!(
        cx.window_state(window).unwrap().appearance,
        WindowAppearance::Dark
    );
    assert_eq!(cx.render_count(window).unwrap(), initial_renders);
    assert!(cx.read(view, |view| view.native_events.is_empty()).unwrap());

    // Returning to system mode adopts the remembered value and rebuilds the observer.
    cx.update(view, |_view, cx| {
        cx.follow_system_window_appearance().unwrap();
    })
    .unwrap();
    assert_eq!(
        cx.window_state(window).unwrap().appearance,
        WindowAppearance::Light
    );
    assert_eq!(
        cx.read(view, |view| view.rendered).unwrap(),
        WindowAppearance::Light
    );
    assert_eq!(cx.render_count(window).unwrap(), initial_renders + 1);

    cx.simulate_appearance_change(window, WindowAppearance::Dark)
        .unwrap();
    assert_eq!(
        cx.window_state(window).unwrap().appearance,
        WindowAppearance::Dark
    );
    assert_eq!(
        cx.read(view, |view| view.native_events.clone()).unwrap(),
        [WindowAppearance::Dark]
    );
    assert_eq!(cx.render_count(window).unwrap(), initial_renders + 2);

    // Explicit commands update retained state immediately without fabricating an OS event.
    cx.update(view, |_view, cx| {
        cx.set_window_appearance(WindowAppearance::Light).unwrap();
    })
    .unwrap();
    assert_eq!(
        cx.window_state(window).unwrap().appearance,
        WindowAppearance::Light
    );
    assert_eq!(
        cx.read(view, |view| view.native_events.clone()).unwrap(),
        [WindowAppearance::Dark]
    );
}

#[test]
fn unobserved_appearance_event_does_not_rebuild_the_view() {
    let (mut cx, view) = TestAppContext::new(AppearanceView {
        observe: false,
        rendered: WindowAppearance::Light,
        native_events: Vec::new(),
    })
    .unwrap();
    let window = view.window_handle();
    let initial_renders = cx.render_count(window).unwrap();

    cx.simulate_appearance_change(window, WindowAppearance::Dark)
        .unwrap();

    assert_eq!(
        cx.window_state(window).unwrap().appearance,
        WindowAppearance::Dark
    );
    assert_eq!(
        cx.read(view, |view| view.native_events.clone()).unwrap(),
        [WindowAppearance::Dark]
    );
    assert_eq!(cx.render_count(window).unwrap(), initial_renders);
}

struct BackgroundAppearanceView {
    rendered: WindowBackgroundAppearance,
}

impl View for BackgroundAppearanceView {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        self.rendered = cx.window_state().background_appearance;
        div()
    }
}

#[test]
fn background_appearance_commands_update_retained_state_once() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().window_background(WindowBackgroundAppearance::Blurred),
            BackgroundAppearanceView {
                rendered: WindowBackgroundAppearance::Opaque,
            },
        )
        .unwrap();
    let window = view.window_handle();

    assert_eq!(
        cx.window_state(window).unwrap().background_appearance,
        WindowBackgroundAppearance::Blurred
    );
    assert_eq!(
        cx.read(view, |view| view.rendered).unwrap(),
        WindowBackgroundAppearance::Blurred
    );
    let initial_renders = cx.render_count(window).unwrap();

    cx.update(view, |_view, cx| {
        cx.set_window_background_appearance(WindowBackgroundAppearance::Transparent)
            .unwrap();
    })
    .unwrap();
    assert_eq!(
        cx.window_state(window).unwrap().background_appearance,
        WindowBackgroundAppearance::Transparent
    );
    assert_eq!(cx.render_count(window).unwrap(), initial_renders + 1);

    // Repeating an identical compositor command is a no-op and schedules no frame.
    cx.update(view, |_view, cx| {
        cx.set_window_background_appearance_handle(window, WindowBackgroundAppearance::Transparent)
            .unwrap();
    })
    .unwrap();
    assert_eq!(cx.render_count(window).unwrap(), initial_renders + 1);

    cx.update(view, |_view, cx| {
        cx.set_window_background_appearance(WindowBackgroundAppearance::Opaque)
            .unwrap();
    })
    .unwrap();
    assert_eq!(
        cx.read(view, |view| view.rendered).unwrap(),
        WindowBackgroundAppearance::Opaque
    );
    assert_eq!(cx.render_count(window).unwrap(), initial_renders + 2);
}

struct VibrancyView {
    rendered: Option<MacOsVibrancy>,
    effect_state: MacOsVisualEffectState,
}

impl View for VibrancyView {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let state = cx.window_state();
        self.rendered = state.macos_vibrancy;
        self.effect_state = state.macos_visual_effect_state;
        div()
    }
}

#[test]
fn macos_vibrancy_commands_update_retained_state_once() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().macos_vibrancy(MacOsVibrancy::Sidebar),
            VibrancyView {
                rendered: None,
                effect_state: MacOsVisualEffectState::Inactive,
            },
        )
        .unwrap();
    let window = view.window_handle();

    assert_eq!(
        cx.window_state(window).unwrap().macos_vibrancy,
        Some(MacOsVibrancy::Sidebar)
    );
    let initial_renders = cx.render_count(window).unwrap();

    cx.update(view, |_view, cx| {
        cx.set_macos_window_vibrancy(Some(MacOsVibrancy::UnderWindow))
            .unwrap();
        cx.set_macos_visual_effect_state(MacOsVisualEffectState::Active)
            .unwrap();
    })
    .unwrap();
    assert_eq!(
        cx.read(view, |view| (view.rendered, view.effect_state))
            .unwrap(),
        (
            Some(MacOsVibrancy::UnderWindow),
            MacOsVisualEffectState::Active
        )
    );
    assert_eq!(cx.render_count(window).unwrap(), initial_renders + 1);

    cx.update(view, |_view, cx| {
        cx.set_macos_window_vibrancy(Some(MacOsVibrancy::UnderWindow))
            .unwrap();
        cx.set_macos_visual_effect_state(MacOsVisualEffectState::Active)
            .unwrap();
    })
    .unwrap();
    assert_eq!(cx.render_count(window).unwrap(), initial_renders + 1);

    cx.update(view, |_view, cx| {
        cx.set_macos_window_vibrancy(None).unwrap();
    })
    .unwrap();
    assert_eq!(cx.read(view, |view| view.rendered).unwrap(), None);
    assert_eq!(cx.render_count(window).unwrap(), initial_renders + 2);
}
