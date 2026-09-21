use super::*;
use crate::{MacOsVibrancy, MacOsVisualEffectState};

fn test_typed_payload(value: Arc<dyn Any>) -> MacTypedDragPayload {
    let value_type = value.as_ref().type_id();
    MacTypedDragPayload::new(
        value,
        value_type,
        WindowHandle::next(),
        ElementId::named("typed-source"),
    )
}

#[test]
fn typed_drag_registry_retains_the_exact_shared_value_for_one_session() {
    let registry = MacTypedDragRegistry::new();
    let shared = Arc::new(String::from("process-local"));
    let erased: Arc<dyn Any> = shared.clone();
    let shared_pointer = Arc::as_ptr(&erased) as *const ();
    let payload = test_typed_payload(erased);

    let (token, registration) = registry
        .register(payload.clone())
        .expect("the first typed drag fits the registry");
    assert!(token.len() <= 64);
    assert!(registry.resolve("forged-token").is_none());
    let resolved = registry
        .resolve(&token)
        .expect("the live token resolves inside this application");
    assert_eq!(resolved.value_type, TypeId::of::<String>());
    assert_eq!(Arc::as_ptr(&resolved.value) as *const (), shared_pointer);
    assert_eq!(resolved.value.downcast_ref::<String>(), Some(&*shared));
    assert_eq!(resolved.source_window(), payload.source_window());
    assert_eq!(resolved.source(), payload.source());

    drop(registration);
    assert!(registry.resolve(&token).is_none());
}

#[test]
fn typed_drag_registry_has_a_hard_session_cap_and_recovers_capacity() {
    let registry = MacTypedDragRegistry::new();
    let payload = test_typed_payload(Arc::new(7_u32));
    let mut registrations = Vec::with_capacity(MAX_NATIVE_TYPED_DRAG_SESSIONS);
    for _ in 0..MAX_NATIVE_TYPED_DRAG_SESSIONS {
        let (_, registration) = registry
            .register(payload.clone())
            .expect("every slot up to the documented cap is available");
        registrations.push(registration);
    }
    assert!(registry.register(payload.clone()).is_err());
    drop(registrations.pop());
    let (_, replacement) = registry
        .register(payload)
        .expect("dropping one session immediately returns its registry slot");
    registrations.push(replacement);
    assert_eq!(
        registry.inner.borrow().payloads.len(),
        MAX_NATIVE_TYPED_DRAG_SESSIONS
    );
}

#[test]
fn native_drop_offer_keeps_private_typed_data_ahead_of_public_fallbacks() {
    let typed = test_typed_payload(Arc::new(11_u32));
    let offer = MacNativeDropOffer::new(vec![
        MacNativeDropPayload::Typed(typed),
        MacNativeDropPayload::Text(Arc::new(ExternalDragText::new("fallback"))),
        MacNativeDropPayload::Url(Arc::new(
            ExternalDragUrl::new("https://quickgui.dev").unwrap(),
        )),
    ])
    .expect("the offer is not empty");
    assert_eq!(
        offer
            .iter()
            .map(|(value_type, _)| value_type)
            .collect::<Vec<_>>(),
        vec![
            TypeId::of::<u32>(),
            TypeId::of::<ExternalDragText>(),
            TypeId::of::<ExternalDragUrl>(),
        ]
    );
}

#[test]
fn native_drag_operations_map_to_stable_framework_values() {
    assert_eq!(
        external_drag_operation(NSDragOperation::None),
        ExternalDragOperation::Cancelled
    );
    assert_eq!(
        external_drag_operation(NSDragOperation::Copy),
        ExternalDragOperation::Copied
    );
    assert_eq!(
        external_drag_operation(NSDragOperation::Move),
        ExternalDragOperation::Moved
    );
    assert_eq!(
        external_drag_operation(NSDragOperation::Link),
        ExternalDragOperation::Linked
    );
    assert_eq!(
        external_drag_operation(NSDragOperation::Delete),
        ExternalDragOperation::Deleted
    );
    assert_eq!(
        external_drag_operation(NSDragOperation::Generic),
        ExternalDragOperation::Other
    );
}

#[test]
fn native_drag_boundary_includes_the_content_view_edges() {
    let bounds = NSRect::new(NSPoint::new(10.0, 20.0), NSSize::new(80.0, 60.0));
    assert!(!point_outside_ns_rect(NSPoint::new(10.0, 20.0), bounds));
    assert!(!point_outside_ns_rect(NSPoint::new(90.0, 80.0), bounds));
    assert!(point_outside_ns_rect(NSPoint::new(9.9, 50.0), bounds));
    assert!(point_outside_ns_rect(NSPoint::new(50.0, 80.1), bounds));
}

#[test]
fn system_popover_consumes_only_a_left_press_inside_its_own_anchor() {
    let anchor = NSRect::new(NSPoint::new(20.0, 30.0), NSSize::new(80.0, 40.0));
    let inside = NSPoint::new(99.9, 69.9);

    assert!(should_consume_popover_anchor_press(
        NSEventType::LeftMouseDown,
        true,
        anchor,
        inside,
    ));
    assert!(!should_consume_popover_anchor_press(
        NSEventType::RightMouseDown,
        true,
        anchor,
        inside,
    ));
    assert!(!should_consume_popover_anchor_press(
        NSEventType::LeftMouseDown,
        false,
        anchor,
        inside,
    ));
    assert!(!should_consume_popover_anchor_press(
        NSEventType::LeftMouseDown,
        true,
        anchor,
        NSPoint::new(100.1, 70.1),
    ));
}

#[test]
fn native_text_and_url_writers_create_dragging_items() {
    let frame = NSRect::new(NSPoint::new(4.0, 8.0), NSSize::new(32.0, 32.0));
    let text = NSString::from_str("QuickGUI native text");
    let text_writer: &ProtocolObject<dyn NSPasteboardWriting> =
        ProtocolObject::from_ref(text.as_ref());
    let text_item = external_dragging_item(text_writer, frame, None);
    assert_eq!(unsafe { text_item.draggingFrame() }, frame);

    let url_string = NSString::from_str("https://github.com/egoist/quickgui");
    let url = unsafe { NSURL::URLWithString(&url_string) }.expect("the test URL is valid");
    let url_writer: &ProtocolObject<dyn NSPasteboardWriting> =
        ProtocolObject::from_ref(url.as_ref());
    let url_item = external_dragging_item(url_writer, frame, None);
    assert_eq!(unsafe { url_item.draggingFrame() }, frame);
}

#[test]
fn native_drop_strings_are_bounded_on_utf8_scalar_boundaries() {
    let short = NSString::from_str("QuickGUI");
    assert_eq!(
        bounded_pasteboard_string(&short, 16),
        Some(("QuickGUI".to_owned(), false))
    );

    let long = NSString::from_str("abcéz");
    assert_eq!(
        bounded_pasteboard_string(&long, 4),
        Some(("abc".to_owned(), true))
    );

    let exact = NSString::from_str("éé");
    assert_eq!(
        bounded_pasteboard_string(&exact, 4),
        Some(("éé".to_owned(), false))
    );
}

#[test]
fn traffic_light_layout_keeps_top_left_inset_across_window_resizes() {
    let original_titlebar = NSRect::new(NSPoint::new(0.0, 572.0), NSSize::new(800.0, 28.0));
    let (initial_titlebar, initial_buttons) =
        traffic_light_layout(Point::new(16.0, 13.0), 600.0, original_titlebar, 14.0, 20.0);
    assert_eq!(
        initial_titlebar,
        NSRect::new(NSPoint::new(0.0, 560.0), NSSize::new(800.0, 40.0))
    );
    assert_eq!(
        initial_buttons,
        [
            NSPoint::new(16.0, 13.0),
            NSPoint::new(36.0, 13.0),
            NSPoint::new(56.0, 13.0),
        ]
    );

    let (resized_titlebar, resized_buttons) =
        traffic_light_layout(Point::new(16.0, 13.0), 700.0, initial_titlebar, 14.0, 20.0);
    assert_eq!(resized_titlebar.origin.y, 660.0);
    assert_eq!(resized_buttons, initial_buttons);
}

#[test]
fn electron_compatible_vibrancy_values_map_to_exact_appkit_materials() {
    use super::vibrancy::{native_effect_state, native_material};

    let materials = [
        (MacOsVibrancy::AppearanceBased, 0),
        (MacOsVibrancy::Titlebar, 3),
        (MacOsVibrancy::Selection, 4),
        (MacOsVibrancy::Menu, 5),
        (MacOsVibrancy::Popover, 6),
        (MacOsVibrancy::Sidebar, 7),
        (MacOsVibrancy::Header, 10),
        (MacOsVibrancy::Sheet, 11),
        (MacOsVibrancy::Window, 12),
        (MacOsVibrancy::Hud, 13),
        (MacOsVibrancy::FullscreenUi, 15),
        (MacOsVibrancy::Tooltip, 17),
        (MacOsVibrancy::Content, 18),
        (MacOsVibrancy::UnderWindow, 21),
        (MacOsVibrancy::UnderPage, 22),
    ];
    for (vibrancy, expected) in materials {
        assert_eq!(native_material(vibrancy).0, expected);
    }

    assert_eq!(
        native_effect_state(MacOsVisualEffectState::FollowWindow).0,
        0
    );
    assert_eq!(native_effect_state(MacOsVisualEffectState::Active).0, 1);
    assert_eq!(native_effect_state(MacOsVisualEffectState::Inactive).0, 2);
}
