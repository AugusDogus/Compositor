use super::*;
use crate::{
    Display, DisplayEvent, DisplayId, Displays, MAX_DISPLAY_EVENTS, MessageBoxOptions,
    PlatformError, PromptButton, PromptLevel, Rect, ShareItem,
};

struct PlatformProbe;

impl View for PlatformProbe {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        // Observe displays so a simulated reconfiguration exercises the invalidation path too.
        let count = cx.displays().len();
        div().id("platform-probe").child(text(count.to_string()))
    }
}

fn display(id: u64, x: f32) -> Display {
    Display::new(
        DisplayId::new(id),
        format!("Display {id}"),
        Rect::new(x, 0.0, 1_440.0, 900.0),
        Rect::new(x, 24.0, 1_440.0, 876.0),
        2.0,
    )
    .unwrap()
}

#[test]
fn simulated_display_reconfiguration_diffs_into_bounded_granular_events() {
    let (mut cx, _view) = TestAppContext::new(PlatformProbe).unwrap();
    let before = Displays::new(
        cx.displays().to_vec(),
        cx.primary_display().map(Display::id),
    )
    .unwrap();
    assert_eq!(before.len(), 1);

    let primary = display(1, 0.0)
        .with_rotation_degrees(90)
        .with_internal(true)
        .with_color_depth(30);
    let after = Displays::new(
        vec![primary.clone(), display(2, 1_440.0)],
        Some(DisplayId::new(1)),
    )
    .unwrap();
    cx.simulate_displays_change(after.clone()).unwrap();

    assert_eq!(cx.displays().len(), 2);
    let observed = cx.displays()[0].clone();
    assert_eq!(observed.rotation_degrees(), 90);
    assert!(observed.is_internal());
    assert_eq!(observed.color_depth(), Some(30));

    let events = before.diff(&after);
    assert_eq!(
        events,
        vec![
            DisplayEvent::MetricsChanged(after.find(DisplayId::new(1)).unwrap().clone()),
            DisplayEvent::Added(after.find(DisplayId::new(2)).unwrap().clone()),
        ]
    );
    assert!(events.len() <= MAX_DISPLAY_EVENTS);

    // Replaying the same snapshot is a no-op for both the runtime and the diff.
    assert!(cx.simulate_displays_change(after.clone()).is_ok());
    assert!(after.diff(&after).is_empty());
}

#[test]
fn deterministic_contexts_reject_native_platform_requests_before_they_are_retained() {
    let (mut cx, view) = TestAppContext::new(PlatformProbe).unwrap();
    // Option validation runs in the core, so it fails the same way with no native backend present.
    let rejected = cx
        .update(view, |_view, cx| {
            cx.message_box(MessageBoxOptions::new("").buttons([PromptButton::ok("OK")]))
                .err()
        })
        .unwrap();
    assert_eq!(rejected, Some(PlatformError::EmptyPromptMessage));

    let rejected = cx
        .update(view, |_view, cx| {
            cx.message_box(
                MessageBoxOptions::new("Ready")
                    .level(PromptLevel::Info)
                    .default_button(4),
            )
            .err()
        })
        .unwrap();
    assert_eq!(rejected, Some(PlatformError::InvalidButtons));

    let rejected = cx
        .update(view, |_view, cx| {
            cx.share_items(&[], Rect::new(0.0, 0.0, 1.0, 1.0)).err()
        })
        .unwrap();
    assert_eq!(
        rejected,
        Some(if crate::DesktopIntegrationSupport::current().share_sheet {
            PlatformError::InvalidShareItems
        } else {
            PlatformError::Unsupported
        })
    );

    let rejected = cx
        .update(view, |_view, cx| {
            cx.share_items(
                &[ShareItem::Text("Report".into())],
                Rect::new(f32::NAN, 0.0, 1.0, 1.0),
            )
            .err()
        })
        .unwrap();
    assert_eq!(
        rejected,
        Some(if crate::DesktopIntegrationSupport::current().share_sheet {
            PlatformError::InvalidShareAnchor
        } else {
            PlatformError::Unsupported
        })
    );

    let rejected = cx
        .update(view, |_view, cx| cx.preview_file("", None).err())
        .unwrap();
    assert_eq!(
        rejected,
        Some(
            if crate::DesktopIntegrationSupport::current().file_previews {
                PlatformError::InvalidPath
            } else {
                PlatformError::Unsupported
            }
        )
    );

    let rejected = cx
        .update(view, |_view, cx| cx.authenticate_with_biometrics("").err())
        .unwrap();
    assert_eq!(
        rejected,
        Some(
            if crate::DesktopIntegrationSupport::current().biometric_authentication {
                PlatformError::InvalidText
            } else {
                PlatformError::Unsupported
            }
        )
    );
}
