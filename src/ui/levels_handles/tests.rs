use super::*;
use quickgui::{Application, WindowOptions};

#[test]
fn handles_have_named_source_hit_areas_and_visible_black_white_glyphs() {
    for (width, height) in [(1500., 900.), (1920., 1080.)] {
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
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Levels glyphs").size(width, height),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        let screenshot = cx.capture_screenshot(window).unwrap();
        let scale = screenshot.width() as f32 / width;
        let tree = cx.accessibility_update(window).unwrap();
        for (handle, track_id, fraction) in [
            (Handle::InputBlack, "levels-input-handles", 0.),
            (Handle::Gamma, "levels-input-handles", 0.5),
            (Handle::InputWhite, "levels-input-handles", 1.),
            (Handle::OutputBlack, "levels-output-handles", 0.),
            (Handle::OutputWhite, "levels-output-handles", 1.),
        ] {
            let track = cx.element_bounds(window, track_id).unwrap();
            let bounds = cx.element_bounds(window, handle.id()).unwrap();
            assert_eq!(bounds.width, 22.);
            assert_eq!(bounds.height, 20.);
            assert_eq!(bounds.y + bounds.height / 2., track.y + 9.);
            assert_eq!(
                bounds.x + bounds.width / 2.,
                track.x + track.width * fraction
            );
            assert!(
                tree.nodes
                    .iter()
                    .any(|(_, n)| n.label() == Some(handle.label()))
            );
            let pixel = |x: f32, y: f32| {
                screenshot
                    .pixel((x * scale) as u32, (y * scale) as u32)
                    .unwrap()
            };
            let center = pixel(bounds.x + 11., bounds.y + 11.);
            match handle {
                Handle::InputBlack | Handle::OutputBlack => {
                    assert_eq!(center, [0, 0, 0, 255]);
                    // The source shadow gives the black triangle an outline against the panel.
                    let shadow = pixel(bounds.x + 11., bounds.y + 15.5);
                    let background = pixel(bounds.x + 2., bounds.y + 15.5);
                    assert!(shadow[0] > background[0], "Missing gray shadow: {shadow:?}");
                }
                Handle::InputWhite | Handle::OutputWhite => assert_eq!(center, [255; 4]),
                Handle::Gamma => assert_eq!(center, [142, 142, 147, 255]),
            }
        }
    }
}

#[test]
fn outside_half_of_endpoint_targets_drags_the_selected_handle_and_cancel_restores_pixels() {
    let mut editor = Editor::with_test_document();
    editor.tabs[0].set_document(Document::new(8, 8).unwrap(), None);
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
            WindowOptions::new("Levels endpoint targets").size(1500., 900.),
            editor,
        )
        .unwrap();
    let window = view.window_handle();
    let track = cx.element_bounds(window, "levels-input-handles").unwrap();
    cx.simulate_pointer_drag(
        window,
        Handle::InputBlack.id(),
        Point::new(track.x - 8., track.y + 9.),
        Point::new(track.x + track.width * 0.2, track.y + 9.),
    )
    .unwrap();
    cx.read(view, |e| {
        let edit = e.adjustment_edit.as_ref().unwrap();
        assert_eq!(edit.settings.levels.ranges[0].black, 51.);
        assert_eq!(edit.settings.levels.ranges[0].gamma, 1.);
        assert!(edit.levels_handles.active.is_none());
    })
    .unwrap();
    let track = cx.element_bounds(window, "levels-output-handles").unwrap();
    cx.simulate_pointer_drag(
        window,
        Handle::OutputWhite.id(),
        Point::new(track.right() + 8., track.y + 9.),
        Point::new(track.x + track.width * 0.8, track.y + 9.),
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
            .output_white)
            .unwrap(),
        204.
    );
    cx.click(window, "form-cancel").unwrap();
    cx.read(view, |e| {
        assert_eq!(e.session().document, original);
        assert!(e.session().undo_label().is_none());
    })
    .unwrap();
}
