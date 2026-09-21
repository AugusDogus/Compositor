use super::*;

#[test]
fn cursor_styles_map_to_the_matching_platform_cursor() {
    let cases = [
        (CursorStyle::Arrow, CursorIcon::Default),
        (CursorStyle::IBeam, CursorIcon::Text),
        (CursorStyle::Crosshair, CursorIcon::Crosshair),
        (CursorStyle::ClosedHand, CursorIcon::Grabbing),
        (CursorStyle::OpenHand, CursorIcon::Grab),
        (CursorStyle::PointingHand, CursorIcon::Pointer),
        (CursorStyle::ResizeLeft, CursorIcon::WResize),
        (CursorStyle::ResizeRight, CursorIcon::EResize),
        (CursorStyle::ResizeLeftRight, CursorIcon::EwResize),
        (CursorStyle::ResizeUp, CursorIcon::NResize),
        (CursorStyle::ResizeDown, CursorIcon::SResize),
        (CursorStyle::ResizeUpDown, CursorIcon::NsResize),
        (CursorStyle::ResizeUpLeftDownRight, CursorIcon::NwseResize),
        (CursorStyle::ResizeUpRightDownLeft, CursorIcon::NeswResize),
        (CursorStyle::ResizeColumn, CursorIcon::ColResize),
        (CursorStyle::ResizeRow, CursorIcon::RowResize),
        (
            CursorStyle::IBeamCursorForVerticalLayout,
            CursorIcon::VerticalText,
        ),
        (CursorStyle::OperationNotAllowed, CursorIcon::NotAllowed),
        (CursorStyle::DragLink, CursorIcon::Alias),
        (CursorStyle::DragCopy, CursorIcon::Copy),
        (CursorStyle::ContextualMenu, CursorIcon::ContextMenu),
    ];

    for (style, expected) in cases {
        assert_eq!(platform_cursor(style), expected);
    }
}

struct CounterView(u32);

struct RuntimeEmitter;

struct RuntimeEntityEvent(u32);

impl EventEmitter<RuntimeEntityEvent> for RuntimeEmitter {}

struct RuntimeGlobal(u32);

impl Global for RuntimeGlobal {}

struct AnotherRuntimeGlobal;

impl Global for AnotherRuntimeGlobal {}

impl View for CounterView {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let increment = cx.listener("increment", |this, cx| {
            this.0 += 1;
            cx.invalidate();
        });
        crate::div().on_click(increment)
    }
}

#[test]
fn erased_views_keep_typed_listener_state() {
    let mut view = CounterView(0);
    let listener: ClickCallback = Arc::new(|view, cx| {
        let view = view
            .downcast_mut::<CounterView>()
            .expect("click listener received the wrong view type");
        view.0 += 1;
        cx.invalidate();
    });
    let mut cx = EventContext::default();
    listener(&mut view, &mut cx);

    assert_eq!(view.0, 1);
    assert!(cx.invalidate);
}

#[test]
fn retained_entity_observations_are_exact_and_refresh_per_render() {
    let first = Entity::new(1_u32);
    let second = Entity::new(2_u32);
    let mut listeners = ListenerRegistry::default();

    listeners.observe_entity(first.id());
    listeners.observe_entity(first.id());
    assert_eq!(listeners.observed_entities.len(), 1);
    assert!(listeners.observes_entity_change(&[first.id()], false));
    assert!(!listeners.observes_entity_change(&[second.id()], false));
    assert!(listeners.observes_entity_change(&[], true));

    listeners.clear();
    assert!(!listeners.observes_entity_change(&[first.id()], false));
}

#[test]
fn application_builder_retains_typed_globals_for_the_runtime() {
    let app = Application::new().global(RuntimeGlobal(7));

    assert!(app.globals.has::<RuntimeGlobal>());
    assert_eq!(app.globals.get::<RuntimeGlobal>().0, 7);
}

#[test]
fn application_builder_retains_only_explicit_callbacks() {
    let app = Application::new()
        .on_open_urls(|_, _| {})
        .on_reopen(|_, _| {})
        .on_system_wake(|_| {})
        .on_keyboard_layout_change(|_, _| {})
        .on_system_notification_response(|_, _| {})
        .on_window_closed(|_, _| {});

    assert!(app.application_callbacks.open_urls.is_some());
    assert!(app.application_callbacks.reopen.is_some());
    assert!(app.application_callbacks.system_wake.is_some());
    assert!(app.application_callbacks.keyboard_layout.is_some());
    assert!(
        app.application_callbacks
            .system_notification_response
            .is_some()
    );
    assert!(app.application_callbacks.window_closed.is_some());

    let app = Application::new();
    assert!(app.application_callbacks.open_urls.is_none());
    assert!(app.application_callbacks.reopen.is_none());
    assert!(app.application_callbacks.system_wake.is_none());
    assert!(app.application_callbacks.keyboard_layout.is_none());
    assert!(
        app.application_callbacks
            .system_notification_response
            .is_none()
    );
    assert!(app.application_callbacks.window_closed.is_none());
}

#[test]
fn windowless_application_builder_retains_core_lifecycle_configuration() {
    let application = Application::new()
        .with_quit_mode(QuitMode::Explicit)
        .global(RuntimeGlobal(9))
        .on_open_urls(|_, _| {})
        .on_window_closed(|_, _| {});

    assert_eq!(application.quit_mode, QuitMode::Explicit);
    assert_eq!(application.globals.get::<RuntimeGlobal>().0, 9);
    assert!(application.application_callbacks.open_urls.is_some());
    assert!(application.application_callbacks.window_closed.is_some());
}

#[test]
fn quit_mode_default_matches_native_desktop_convention() {
    assert_eq!(
        QuitMode::Default.quits_when_empty(),
        cfg!(not(target_os = "macos"))
    );
    assert!(QuitMode::LastWindowClosed.quits_when_empty());
    assert!(!QuitMode::Explicit.quits_when_empty());
    assert_eq!(
        Application::new()
            .with_quit_mode(QuitMode::Explicit)
            .quit_mode,
        QuitMode::Explicit
    );
}

#[test]
fn retained_global_observations_are_exact_and_refresh_per_render() {
    let first = TypeId::of::<RuntimeGlobal>();
    let second = TypeId::of::<AnotherRuntimeGlobal>();
    let mut listeners = ListenerRegistry::default();

    listeners.observe_global(first);
    listeners.observe_global(first);
    assert_eq!(listeners.observed_globals.len(), 1);
    assert!(listeners.observes_global_change(&[first], false));
    assert!(!listeners.observes_global_change(&[second], false));
    assert!(listeners.observes_global_change(&[], true));

    listeners.clear();
    assert!(!listeners.observes_global_change(&[first], false));
}

#[test]
fn window_option_builders_preserve_restore_geometry_and_state() {
    let bounds = Rect::new(120.0, 80.0, 640.0, 420.0);
    let options = WindowOptions::new("Inspector")
        .window_bounds(WindowBounds::Fullscreen(bounds))
        .position(200.0, 140.0)
        .size(720.0, 480.0)
        .maximized(true);

    assert_eq!(options.size, Size::new(720.0, 480.0));
    assert_eq!(
        options.window_bounds,
        Some(WindowBounds::Maximized(Rect::new(
            200.0, 140.0, 720.0, 480.0,
        )))
    );
}

#[test]
fn minimum_window_size_bounds_runtime_growth_without_rewriting_explicit_geometry() {
    let minimum = Size::new(640.0, 420.0);
    assert_eq!(
        constrained_window_size(Size::new(320.0, 800.0), Some(minimum), None),
        Size::new(640.0, 800.0)
    );
    assert_eq!(
        WindowOptions::default()
            .minimum_size(500.0, 360.0)
            .minimum_size,
        Some(Size::new(500.0, 360.0))
    );
    assert!(
        WindowOptions::default()
            .without_minimum_size()
            .minimum_size
            .is_none()
    );
}

#[test]
fn window_appearance_builders_and_native_mapping_are_exact() {
    let forced = WindowOptions::new("Inspector").window_appearance(WindowAppearance::Dark);
    assert_eq!(forced.preferred_appearance, Some(WindowAppearance::Dark));
    assert_eq!(forced.follow_system_appearance().preferred_appearance, None);

    for appearance in [WindowAppearance::Light, WindowAppearance::Dark] {
        assert_eq!(
            map_window_appearance(to_winit_theme(appearance)),
            appearance
        );
    }
}

#[test]
fn window_background_builder_retains_the_compositor_policy() {
    let options =
        WindowOptions::new("Palette").window_background(WindowBackgroundAppearance::Blurred);
    assert_eq!(
        options.window_background,
        WindowBackgroundAppearance::Blurred
    );
    assert!(options.window_background.is_transparent());
    assert!(options.window_background.is_blurred());
    assert!(!WindowBackgroundAppearance::Opaque.is_transparent());

    assert_eq!(
        WindowBackgroundAppearance::Transparent.changes_from(WindowBackgroundAppearance::Blurred),
        WindowBackgroundChanges {
            transparency: false,
            blur: true,
        }
    );
    assert_eq!(
        WindowBackgroundAppearance::Opaque.changes_from(WindowBackgroundAppearance::Transparent),
        WindowBackgroundChanges {
            transparency: true,
            blur: false,
        }
    );
    assert_eq!(
        WindowBackgroundAppearance::Blurred.changes_from(WindowBackgroundAppearance::Opaque),
        WindowBackgroundChanges {
            transparency: true,
            blur: true,
        }
    );
}

#[cfg(target_os = "macos")]
#[test]
fn macos_vibrancy_builders_require_alpha_without_enabling_legacy_blur() {
    let options = WindowOptions::new("Materials")
        .window_background(WindowBackgroundAppearance::Blurred)
        .macos_vibrancy(MacOsVibrancy::Sidebar)
        .macos_visual_effect_state(MacOsVisualEffectState::Active);
    assert_eq!(options.macos_vibrancy, Some(MacOsVibrancy::Sidebar));
    assert_eq!(
        options.macos_visual_effect_state,
        MacOsVisualEffectState::Active
    );
    assert!(options.uses_transparent_surface());
    assert!(!options.uses_legacy_background_blur());

    let options = options.without_macos_vibrancy();
    assert_eq!(options.macos_vibrancy, None);
    assert!(options.uses_transparent_surface());
    assert!(options.uses_legacy_background_blur());
}

#[test]
fn focused_top_level_presentation_activates_the_application_but_popovers_do_not() {
    for kind in [WindowKind::Normal, WindowKind::Floating, WindowKind::Dialog] {
        let options = WindowOptions::new("Window").window_kind(kind);
        assert!(window_presentation_activates_application(&options));
    }

    for kind in [WindowKind::Popover, WindowKind::SystemPopover] {
        let options = WindowOptions::new("Popover").window_kind(kind);
        assert!(!window_presentation_activates_application(&options));
    }

    assert!(!window_presentation_activates_application(
        &WindowOptions::new("Inactive").focus(false)
    ));
    assert!(!window_presentation_activates_application(
        &WindowOptions::new("Never key").focusable(false)
    ));
}

#[test]
fn system_popover_builder_selects_bounded_native_menu_defaults() {
    let popover = crate::PopoverOptions::new(Rect::new(24.0, 40.0, 120.0, 32.0));
    let options = WindowOptions::new("Menu")
        .size(240.0, 180.0)
        .system_popover(popover.clone());

    assert_eq!(options.kind, WindowKind::SystemPopover);
    assert_eq!(options.popover, Some(popover));
    assert_eq!(options.title_bar_style, TitleBarStyle::Hidden);
    assert!(options.focus);
    assert!(!options.is_movable);
    assert!(!options.is_resizable);
    assert!(!options.is_minimizable);
    assert!(options.minimum_size.is_none());
    assert_eq!(validate_window_options(&options), Ok(()));

    let never_key_popover = crate::PopoverOptions::new(Rect::ZERO)
        .grab(false)
        .accepts_key_focus(false);
    let never_key = WindowOptions::new("Suggestions").system_popover(never_key_popover);
    assert!(!never_key.focus);
    assert!(window_is_never_key_popover(&never_key));
    assert!(!never_key.popover.as_ref().unwrap().grab);
    assert_eq!(validate_window_options(&never_key), Ok(()));

    let escape_only = WindowOptions::new("Escape only").system_popover(
        crate::PopoverOptions::new(Rect::ZERO)
            .dismiss_on_escape(true)
            .dismiss_on_pointer_outside(false),
    );
    assert!(window_dismisses_system_popover_on_escape(&escape_only));
    assert!(!window_dismisses_system_popover_on_pointer_outside(
        &escape_only
    ));
}

#[test]
fn window_options_reject_unbounded_native_inputs() {
    assert_eq!(
        validate_window_bounds(WindowBounds::windowed(f32::NAN, 0.0, 1.0, 1.0)),
        Err(WindowCommandError::InvalidBounds)
    );
    assert_eq!(
        validate_window_size(Size::new(0.0, 100.0)),
        Err(WindowCommandError::InvalidBounds)
    );
    assert_eq!(
        validate_window_position(Point::new(MAX_WINDOW_LOGICAL_COORDINATE * 2.0, 0.0)),
        Err(WindowCommandError::InvalidBounds)
    );
    assert_eq!(
        validate_window_title(&"x".repeat(MAX_WINDOW_TITLE_BYTES + 1)),
        Err(WindowCommandError::TitleTooLong)
    );
    assert_eq!(
        validate_window_options(
            &WindowOptions::new("Hidden")
                .title_bar_style(TitleBarStyle::Hidden)
                .traffic_light_position(12.0, 12.0),
        ),
        Err(WindowCommandError::HiddenTitleBarTrafficLights)
    );
    assert_eq!(
        validate_window_options(
            &WindowOptions::new("Missing popover").window_kind(WindowKind::SystemPopover),
        ),
        Err(WindowCommandError::InvalidPopoverConfiguration)
    );
    assert_eq!(
        validate_window_options(&WindowOptions::new("Invalid popover").system_popover(
            crate::PopoverOptions::new(Rect::new(f32::INFINITY, 0.0, 0.0, 0.0)),
        ),),
        Err(WindowCommandError::InvalidPopoverConfiguration)
    );
}

#[test]
fn document_window_options_are_bounded_and_keep_gpui_aliases() {
    let options = WindowOptions::new("Document")
        .document_path("Cargo.toml")
        .document_edited(true)
        .tabbing_identifier("dev.quickgui.workspace");
    assert_eq!(options.represented_file, Some(PathBuf::from("Cargo.toml")));
    assert!(options.document_edited);
    assert_eq!(
        options.tabbing_identifier.as_deref(),
        Some("dev.quickgui.workspace")
    );
    assert_eq!(validate_window_options(&options), Ok(()));

    assert_eq!(
        validate_window_options(&WindowOptions::new("Empty path").document_path("")),
        Err(WindowCommandError::InvalidDocumentPath)
    );
    assert_eq!(
        validate_window_options(&WindowOptions::new("NUL path").document_path("a\0b")),
        Err(WindowCommandError::InvalidDocumentPath)
    );
    assert_eq!(
        validate_window_options(&WindowOptions::new("Empty tab").tabbing_identifier("")),
        Err(WindowCommandError::InvalidTabbingIdentifier)
    );
    assert_eq!(
        validate_window_options(
            &WindowOptions::new("Long tab")
                .tabbing_identifier("x".repeat(MAX_WINDOW_TABBING_IDENTIFIER_BYTES + 1)),
        ),
        Err(WindowCommandError::InvalidTabbingIdentifier)
    );

    assert!(WindowTabState::default().is_valid());
    assert!(
        !WindowTabState {
            count: 0,
            selected_index: None,
            ..WindowTabState::default()
        }
        .is_valid()
    );
    assert!(
        !WindowTabState {
            count: 2,
            selected_index: Some(2),
            ..WindowTabState::default()
        }
        .is_valid()
    );

    let options = WindowOptions::default()
        .represented_file("src/lib.rs")
        .document_edited(true)
        .tabbing_identifier("dev.quickgui.source");
    assert_eq!(options.represented_file, Some(PathBuf::from("src/lib.rs")));
    assert!(options.document_edited);
    assert_eq!(
        options.tabbing_identifier.as_deref(),
        Some("dev.quickgui.source")
    );
}

#[test]
fn window_state_observation_is_declarative() {
    let mut listeners = ListenerRegistry {
        observes_window_state: true,
        observes_viewport: true,
        ..ListenerRegistry::default()
    };

    assert!(listeners.requires_window_state_rebuild(true));
    assert!(!listeners.requires_window_state_rebuild(false));

    listeners.clear();

    assert!(!listeners.observes_window_state);
    assert!(!listeners.observes_viewport);
    assert!(!listeners.requires_window_state_rebuild(true));
}

#[test]
fn accessibility_geometry_updates_are_coalesced_without_an_idle_loop() {
    let now = Instant::now();
    let mut updates = AccessibilityUpdateSchedule::default();

    assert_eq!(
        updates.should_update(Some(AccessibilityUpdateKind::ScrollGeometry), now),
        None
    );
    assert_eq!(updates.deadline(), None);
    updates.activate();
    assert_eq!(
        updates.should_update(Some(AccessibilityUpdateKind::ScrollGeometry), now),
        None
    );
    assert_eq!(
        updates.deadline(),
        Some(now + ACCESSIBILITY_GEOMETRY_UPDATE_INTERVAL)
    );
    assert!(
        !updates.advance(now + ACCESSIBILITY_GEOMETRY_UPDATE_INTERVAL - Duration::from_millis(1))
    );
    assert!(updates.advance(now + ACCESSIBILITY_GEOMETRY_UPDATE_INTERVAL));
    assert_eq!(
        updates.should_update(None, now + ACCESSIBILITY_GEOMETRY_UPDATE_INTERVAL),
        Some(AccessibilityUpdateKind::ScrollGeometry),
        "the timer correction must retain the incremental scroll update kind"
    );
    assert_eq!(updates.deadline(), None);

    assert_eq!(
        updates.should_update(
            Some(AccessibilityUpdateKind::LayoutGeometry),
            now + ACCESSIBILITY_GEOMETRY_UPDATE_INTERVAL
        ),
        None
    );
    assert_eq!(
        updates.deadline(),
        Some(now + ACCESSIBILITY_GEOMETRY_UPDATE_INTERVAL * 2)
    );
    let later_resize =
        now + ACCESSIBILITY_GEOMETRY_UPDATE_INTERVAL + ACCESSIBILITY_GEOMETRY_UPDATE_INTERVAL / 2;
    assert_eq!(
        updates.should_update(Some(AccessibilityUpdateKind::LayoutGeometry), later_resize),
        None
    );
    assert_eq!(
        updates.deadline(),
        Some(later_resize + ACCESSIBILITY_GEOMETRY_UPDATE_INTERVAL),
        "continuous resize frames move the trailing correction deadline"
    );
    assert!(!updates.advance(now + ACCESSIBILITY_GEOMETRY_UPDATE_INTERVAL * 2));
    assert!(updates.advance(later_resize + ACCESSIBILITY_GEOMETRY_UPDATE_INTERVAL));
    assert_eq!(
        updates.should_update(None, later_resize + ACCESSIBILITY_GEOMETRY_UPDATE_INTERVAL),
        Some(AccessibilityUpdateKind::LayoutGeometry)
    );

    assert_eq!(
        updates.should_update(None, now),
        Some(AccessibilityUpdateKind::Full)
    );
    assert_eq!(updates.deadline(), None);
    assert!(!updates.advance(now + Duration::from_secs(1)));

    updates.deactivate();
    assert_eq!(updates.should_update(None, now), None);
}

#[test]
fn semantic_update_supersedes_a_pending_scroll_accessibility_correction() {
    let mut updates = AccessibilityUpdateSchedule::default();
    updates.activate();
    let now = Instant::now();
    updates.should_update(Some(AccessibilityUpdateKind::ScrollGeometry), now);
    assert!(updates.advance(now + ACCESSIBILITY_GEOMETRY_UPDATE_INTERVAL));
    updates.semantic_change();
    assert_eq!(
        updates.should_update(None, now),
        Some(AccessibilityUpdateKind::Full)
    );
    assert_eq!(updates.deadline(), None);
}

#[test]
fn platform_effect_queue_completes_overflow_without_retaining_it() {
    let mut pending = (0..crate::MAX_PENDING_PLATFORM_REQUESTS)
        .map(|index| {
            PlatformRequest::open_url(format!("https://example.com/{index}"))
                .expect("bounded test URL")
        })
        .collect::<VecDeque<_>>();
    let (overflow, mut response) = PlatformRequest::prompt(
        WindowHandle::next(),
        crate::PromptLevel::Info,
        "Overflow",
        None,
        &[crate::PromptButton::ok("OK")],
    )
    .expect("valid prompt");
    let mut incoming = vec![overflow];

    enqueue_platform_requests(&mut pending, &mut incoming);

    assert_eq!(pending.len(), crate::MAX_PENDING_PLATFORM_REQUESTS);
    assert!(incoming.is_empty());
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    assert_eq!(
        std::pin::Pin::new(&mut response).poll(&mut context),
        std::task::Poll::Ready(Err(PlatformError::PendingQueueFull))
    );
}

#[test]
fn global_subscriptions_preserve_order_and_cancel_on_drop() {
    let global_type = TypeId::of::<RuntimeGlobal>();
    let deliveries = Rc::new(RefCell::new(Vec::new()));
    let mut listeners = ListenerRegistry::default();

    let callback = |label, deliveries: Rc<RefCell<Vec<&'static str>>>| {
        Rc::new(RefCell::new(
            move |_view: &mut dyn Any, _cx: &mut EventContext| {
                deliveries.borrow_mut().push(label);
            },
        )) as GlobalObserverCallback
    };
    let first = listeners.subscribe_global(global_type, callback("first", deliveries.clone()));
    let second = listeners.subscribe_global(global_type, callback("second", deliveries.clone()));

    assert!(listeners.has_global_subscribers(Some(global_type)));
    for subscription in listeners.global_subscriptions(Some(global_type)) {
        if subscription.is_active() {
            subscription.callback.borrow_mut()(&mut (), &mut EventContext::default());
        }
    }
    assert_eq!(*deliveries.borrow(), ["first", "second"]);

    drop(first);
    listeners.prune_global_subscriptions();
    assert_eq!(listeners.global_observers.len(), 1);
    deliveries.borrow_mut().clear();
    for subscription in listeners.global_subscriptions(Some(global_type)) {
        if subscription.is_active() {
            subscription.callback.borrow_mut()(&mut (), &mut EventContext::default());
        }
    }
    assert_eq!(*deliveries.borrow(), ["second"]);
    drop(second);
    assert!(!listeners.has_global_subscribers(Some(global_type)));
}

#[test]
fn dropping_a_global_subscription_cancels_its_snapshotted_delivery() {
    let global_type = TypeId::of::<RuntimeGlobal>();
    let deliveries = Rc::new(RefCell::new(Vec::new()));
    let second_handle = Rc::new(RefCell::new(None));
    let mut listeners = ListenerRegistry::default();

    let first_deliveries = deliveries.clone();
    let first_second_handle = second_handle.clone();
    let first_callback: GlobalObserverCallback = Rc::new(RefCell::new(
        move |_view: &mut dyn Any, _cx: &mut EventContext| {
            first_deliveries.borrow_mut().push("first");
            first_second_handle.borrow_mut().take();
        },
    ));
    let second_deliveries = deliveries.clone();
    let second_callback: GlobalObserverCallback = Rc::new(RefCell::new(
        move |_view: &mut dyn Any, _cx: &mut EventContext| {
            second_deliveries.borrow_mut().push("second");
        },
    ));
    let _first = listeners.subscribe_global(global_type, first_callback);
    *second_handle.borrow_mut() = Some(listeners.subscribe_global(global_type, second_callback));

    for subscription in listeners.global_subscriptions(Some(global_type)) {
        if subscription.is_active() {
            subscription.callback.borrow_mut()(&mut (), &mut EventContext::default());
        }
    }

    assert_eq!(*deliveries.borrow(), ["first"]);
}

#[test]
fn global_subscription_capacity_reuses_a_dropped_slot_without_growing() {
    let global_type = TypeId::of::<RuntimeGlobal>();
    let callback: GlobalObserverCallback = Rc::new(RefCell::new(
        |_view: &mut dyn Any, _cx: &mut EventContext| {},
    ));
    let mut listeners = ListenerRegistry::default();
    let mut subscriptions = Vec::with_capacity(MAX_GLOBAL_SUBSCRIPTIONS_PER_WINDOW);

    for _ in 0..MAX_GLOBAL_SUBSCRIPTIONS_PER_WINDOW {
        subscriptions.push(listeners.subscribe_global(global_type, callback.clone()));
    }
    assert_eq!(
        listeners.global_observers.len(),
        MAX_GLOBAL_SUBSCRIPTIONS_PER_WINDOW
    );

    drop(subscriptions.pop());
    subscriptions.push(listeners.subscribe_global(global_type, callback));
    assert_eq!(
        listeners.global_observers.len(),
        MAX_GLOBAL_SUBSCRIPTIONS_PER_WINDOW
    );
    assert_eq!(
        listeners.global_subscriptions(Some(global_type)).len(),
        MAX_GLOBAL_SUBSCRIPTIONS_PER_WINDOW
    );
}

#[test]
fn pending_global_notifications_coalesce_and_all_supersedes_exact_types() {
    let first = TypeId::of::<RuntimeGlobal>();
    let second = TypeId::of::<AnotherRuntimeGlobal>();
    let mut pending = VecDeque::new();
    let mut pending_types = HashSet::new();
    let mut pending_all = false;

    assert!(enqueue_global_notifications(
        &mut pending,
        &mut pending_types,
        &mut pending_all,
        &[first, first, second],
        false,
    ));
    assert_eq!(pending.iter().copied().collect::<Vec<_>>(), [first, second]);
    assert_eq!(pending_types.len(), 2);

    assert!(enqueue_global_notifications(
        &mut pending,
        &mut pending_types,
        &mut pending_all,
        &[],
        true,
    ));
    assert!(pending.is_empty());
    assert!(pending_types.is_empty());
    assert!(pending_all);
}

#[test]
fn entity_event_subscriptions_preserve_order_and_cancel_on_drop() {
    let source = Entity::new(());
    let event_type = TypeId::of::<u32>();
    let deliveries = Rc::new(RefCell::new(Vec::new()));
    let mut listeners = ListenerRegistry::default();

    let callback = |label, deliveries: Rc<RefCell<Vec<&'static str>>>| {
        Rc::new(RefCell::new(
            move |_view: &mut dyn Any, event: &dyn Any, _cx: &mut EventContext| {
                assert_eq!(event.downcast_ref::<u32>(), Some(&7));
                deliveries.borrow_mut().push(label);
            },
        )) as EntityEventCallback
    };
    let first = listeners.subscribe_entity_event(
        source.id(),
        event_type,
        callback("first", deliveries.clone()),
    );
    let second = listeners.subscribe_entity_event(
        source.id(),
        event_type,
        callback("second", deliveries.clone()),
    );

    assert!(listeners.has_entity_event_subscribers(source.id(), event_type));
    assert_eq!(listeners.entity_subscription_count, 2);
    for callback in listeners.entity_event_callbacks(source.id(), event_type) {
        callback.borrow_mut()(&mut (), &7_u32, &mut EventContext::default());
    }
    assert_eq!(*deliveries.borrow(), ["first", "second"]);

    drop(first);
    assert_eq!(
        listeners
            .entity_event_callbacks(source.id(), event_type)
            .len(),
        1
    );
    listeners.clear();
    assert_eq!(listeners.entity_subscription_count, 1);

    drop(second);
    assert!(!listeners.has_entity_event_subscribers(source.id(), event_type));
    listeners.clear();
    assert!(listeners.entity_events.is_empty());
    assert_eq!(listeners.entity_subscription_count, 0);
}

#[test]
fn pending_entity_events_and_recursive_delivery_are_hard_bounded() {
    let source = Entity::new(RuntimeEmitter);
    let mut pending = VecDeque::new();
    let mut incoming = (0..MAX_PENDING_ENTITY_EVENTS)
        .map(|value| EntityEvent::new(&source, RuntimeEntityEvent(value as u32)))
        .collect::<Vec<_>>();

    assert!(enqueue_entity_events(&mut pending, &mut incoming));
    assert!(incoming.is_empty());
    assert_eq!(pending.len(), MAX_PENDING_ENTITY_EVENTS);
    assert_eq!(
        pending
            .front()
            .and_then(|event| event.value.downcast_ref::<RuntimeEntityEvent>())
            .map(|event| event.0),
        Some(0)
    );

    let mut overflow = vec![EntityEvent::new(&source, RuntimeEntityEvent(u32::MAX))];
    assert!(!enqueue_entity_events(&mut pending, &mut overflow));
    assert_eq!(pending.len(), MAX_PENDING_ENTITY_EVENTS);
    assert_eq!(overflow.len(), 1);

    let mut deliveries = MAX_ENTITY_EVENT_DELIVERIES_PER_TURN - 1;
    assert!(reserve_entity_event_delivery(&mut deliveries));
    assert_eq!(deliveries, MAX_ENTITY_EVENT_DELIVERIES_PER_TURN);
    assert!(!reserve_entity_event_delivery(&mut deliveries));
    assert_eq!(deliveries, MAX_ENTITY_EVENT_DELIVERIES_PER_TURN);
}

#[test]
fn subscription_capacity_reuses_a_dropped_slot_without_growing() {
    let source = Entity::new(());
    let event_type = TypeId::of::<u8>();
    let callback: EntityEventCallback = Rc::new(RefCell::new(
        |_view: &mut dyn Any, _event: &dyn Any, _cx: &mut EventContext| {},
    ));
    let mut listeners = ListenerRegistry::default();
    let mut subscriptions = Vec::with_capacity(MAX_ENTITY_SUBSCRIPTIONS_PER_WINDOW);

    for _ in 0..MAX_ENTITY_SUBSCRIPTIONS_PER_WINDOW {
        subscriptions.push(listeners.subscribe_entity_event(
            source.id(),
            event_type,
            callback.clone(),
        ));
    }
    assert_eq!(
        listeners.entity_subscription_count,
        MAX_ENTITY_SUBSCRIPTIONS_PER_WINDOW
    );

    drop(subscriptions.pop());
    subscriptions.push(listeners.subscribe_entity_event(source.id(), event_type, callback));
    assert_eq!(
        listeners.entity_subscription_count,
        MAX_ENTITY_SUBSCRIPTIONS_PER_WINDOW
    );
    assert_eq!(
        listeners
            .entity_event_callbacks(source.id(), event_type)
            .len(),
        MAX_ENTITY_SUBSCRIPTIONS_PER_WINDOW
    );
}

#[test]
fn erased_drag_payloads_preserve_their_exact_type_and_preview() {
    let files = FileDragPaths::new([(PathBuf::from("Cargo.toml"), false)]);
    let drag = AnyDrag::new(
        Drag::new(42_u32)
            .preview(crate::div().size(80.0, 40.0))
            .external_files(files.clone()),
    );

    assert_eq!(drag.value_type, TypeId::of::<u32>());
    assert_eq!(drag.value.downcast_ref::<u32>(), Some(&42));
    assert!(drag.preview.is_some());
    assert_eq!(
        drag.external_payload,
        Some(ExternalDragPayload::Files(files))
    );

    let text = AnyDrag::new(Drag::new(7_u8).external_text("QuickGUI native text"));
    assert_eq!(
        text.external_payload,
        Some(ExternalDragPayload::Text(ExternalDragText::new(
            "QuickGUI native text"
        )))
    );

    let url = ExternalDragUrl::new("https://quickgui.dev/").unwrap();
    let drag = AnyDrag::new(Drag::new(9_u8).external_url(url.clone()));
    assert_eq!(drag.external_payload, Some(ExternalDragPayload::Url(url)));
}

#[cfg(target_os = "macos")]
#[test]
fn external_drag_promotion_starts_only_after_leaving_the_viewport() {
    let viewport = Size::new(800.0, 600.0);
    assert!(!point_outside_viewport(Point::new(0.0, 0.0), viewport));
    assert!(!point_outside_viewport(Point::new(800.0, 600.0), viewport));
    assert!(point_outside_viewport(Point::new(-0.1, 300.0), viewport));
    assert!(point_outside_viewport(Point::new(400.0, 600.1), viewport));
}

#[cfg(target_os = "macos")]
#[test]
fn native_typed_drop_origin_distinguishes_same_and_cross_window_delivery() {
    let source_window = WindowHandle::next();
    let destination_window = WindowHandle::next();
    let source = ElementId::named("source");
    let typed = MacNativeDropPayload::Typed(MacTypedDragPayload::new(
        Arc::new(42_u32),
        TypeId::of::<u32>(),
        source_window,
        source,
    ));
    assert_eq!(
        native_drop_origin(Some(source_window), &typed),
        DragOrigin::Internal(source)
    );
    assert_eq!(
        native_drop_origin(Some(destination_window), &typed),
        DragOrigin::CrossWindow {
            window: source_window,
            source,
        }
    );

    let text = MacNativeDropPayload::Text(Arc::new(ExternalDragText::new("native")));
    assert_eq!(
        native_drop_origin(Some(destination_window), &text),
        DragOrigin::External
    );
}

#[test]
fn native_file_events_are_grouped_until_every_hovered_path_drops() {
    let mut drag = NativeFileDrag::default();
    drag.hover(PathBuf::from("first.txt"));
    drag.hover(PathBuf::from("second.txt"));

    assert!(!drag.drop_path(PathBuf::from("first.txt")));
    assert!(drag.drop_path(PathBuf::from("second.txt")));
    assert_eq!(
        drag.into_dropped_files().paths(),
        [PathBuf::from("first.txt"), PathBuf::from("second.txt")]
    );
}

#[test]
fn invalid_scale_factors_fall_back_to_one() {
    assert_eq!(sane_scale_factor(0.0), 1.0);
    assert_eq!(sane_scale_factor(f64::NAN), 1.0);
    assert_eq!(sane_scale_factor(2.0), 2.0);
}

#[test]
fn native_gesture_values_are_finite_bounded_and_semantic() {
    assert_eq!(bounded_pressure(-1.0), 0.0);
    assert_eq!(bounded_pressure(0.625), 0.625);
    assert_eq!(bounded_pressure(2.0), 1.0);
    assert_eq!(bounded_pressure(f32::NAN), 0.0);

    assert_eq!(bounded_gesture_delta(0.25, 8.0), 0.25);
    assert_eq!(bounded_gesture_delta(99.0, 8.0), 8.0);
    assert_eq!(bounded_gesture_delta(-99.0, 8.0), -8.0);
    assert_eq!(bounded_gesture_delta(f64::INFINITY, 8.0), 0.0);

    assert_eq!(map_pressure_stage(0), PressureStage::Zero);
    assert_eq!(map_pressure_stage(1), PressureStage::Normal);
    assert_eq!(map_pressure_stage(2), PressureStage::Force);
    assert_eq!(map_pressure_stage(7), PressureStage::Other(7));

    assert_eq!(bounded_touch_force(Force::Normalized(0.625)), 0.625);
    assert_eq!(bounded_touch_force(Force::Normalized(2.0)), 1.0);
    assert_eq!(bounded_touch_force(Force::Normalized(f64::NAN)), 0.0);
    assert_eq!(
        bounded_touch_force(Force::Calibrated {
            force: 2.0,
            max_possible_force: 4.0,
            altitude_angle: None,
        }),
        0.5
    );

    assert_eq!(
        map_gesture_phase(winit::event::TouchPhase::Started),
        GesturePhase::Started
    );
    assert_eq!(
        map_gesture_phase(winit::event::TouchPhase::Moved),
        GesturePhase::Moved
    );
    assert_eq!(
        map_gesture_phase(winit::event::TouchPhase::Ended),
        GesturePhase::Ended
    );
    assert_eq!(
        map_gesture_phase(winit::event::TouchPhase::Cancelled),
        GesturePhase::Cancelled
    );
    assert_eq!(
        map_touch_phase(winit::event::TouchPhase::Started),
        TouchPhase::Started
    );
    assert_eq!(
        map_touch_phase(winit::event::TouchPhase::Moved),
        TouchPhase::Moved
    );
    assert_eq!(
        map_touch_phase(winit::event::TouchPhase::Ended),
        TouchPhase::Ended
    );
    assert_eq!(
        map_touch_phase(winit::event::TouchPhase::Cancelled),
        TouchPhase::Cancelled
    );
}

#[test]
fn drawable_geometry_converts_to_logical_points() {
    assert_eq!(
        logical_window_size(PhysicalSize::new(1200, 800), 2.0),
        Size::new(600.0, 400.0)
    );
}

#[test]
fn hidden_inset_window_chrome_options_are_composable() {
    let options = WindowOptions::new("Custom chrome")
        .title_bar_style(TitleBarStyle::HiddenInset)
        .traffic_light_position(16.0, 13.0);

    assert_eq!(options.title_bar_style, TitleBarStyle::HiddenInset);
    assert_eq!(options.traffic_light_position, Some(Point::new(16.0, 13.0)));
    assert_eq!(
        options
            .without_traffic_light_position()
            .traffic_light_position,
        None
    );
}

#[test]
fn modifier_mapping_preserves_all_flags() {
    let mapped =
        map_modifiers(ModifiersState::SHIFT | ModifiersState::CONTROL | ModifiersState::SUPER);
    assert!(mapped.contains(Modifiers::SHIFT));
    assert!(mapped.contains(Modifiers::CONTROL));
    assert!(mapped.contains(Modifiers::SUPER));
    assert!(!mapped.contains(Modifiers::ALT));
}

#[test]
fn platform_primary_modifier_matches_native_shortcuts() {
    if cfg!(target_os = "macos") {
        assert!(primary_modifier(Modifiers::SUPER));
        assert!(!primary_modifier(Modifiers::CONTROL));
        assert!(word_modifier(Modifiers::ALT));
        assert!(!word_modifier(Modifiers::CONTROL));
    } else {
        assert!(primary_modifier(Modifiers::CONTROL));
        assert!(!primary_modifier(Modifiers::SUPER));
        assert!(word_modifier(Modifiers::CONTROL));
        assert!(!word_modifier(Modifiers::ALT));
    }
}

#[cfg(target_os = "macos")]
#[test]
fn default_close_shortcut_is_exact_and_non_repeating() {
    let close = Key::Character("w".to_owned());
    assert!(is_default_close_shortcut(&close, Modifiers::SUPER, false));
    assert!(!is_default_close_shortcut(&close, Modifiers::SUPER, true));
    assert!(!is_default_close_shortcut(
        &close,
        Modifiers::SUPER | Modifiers::SHIFT,
        false,
    ));
    assert!(!is_default_close_shortcut(
        &Key::Character("q".to_owned()),
        Modifiers::SUPER,
        false,
    ));
}
