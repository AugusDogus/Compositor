use super::*;
use quickgui::{Application, WindowOptions};

fn editor() -> Editor {
    let mut editor = Editor::with_test_document();
    let mut document = Document::new(100, 100).unwrap();
    compositor::edits::fill(&mut document, [100, 100, 100, 255], false, false).unwrap();
    document.selection = Some(Selection::rectangle(
        100,
        100,
        [0., 0.],
        [100., 100.],
        false,
    ));
    editor.tabs = vec![Session::new(document, None).into()];
    editor.session_mut().zoom_at(2., [0., 0.]);
    editor
}

#[test]
fn selection_stroke_is_one_point_wide_at_hidpi() {
    let mut editor = editor();
    editor.tools.show_transform_controls = false;
    editor.tools.pixel_grid = false;
    editor.session_mut().document.selection = Some(Selection::rectangle(
        100,
        100,
        [20., 20.],
        [80., 80.],
        false,
    ));
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Selection width").size(1200., 850.),
            editor,
        )
        .unwrap();
    let window = view.window_handle();
    for zoom in [0.5, 2., 8.] {
        cx.update(view, |e, cx| {
            e.session_mut().zoom_at(zoom, [0., 0.]);
            cx.invalidate();
        })
        .unwrap();
        let frame = cx.capture_screenshot(window).unwrap();
        let bounds = cx.element_bounds(window, "canvas").unwrap();
        let (zoom, offset) = cx
            .read(view, |e| {
                let v = e.selection_outline.as_ref().unwrap().viewport;
                (v.zoom, v.offset)
            })
            .unwrap();
        let scale = frame.width() as f64 / 1200.;
        let top = bounds.y as f64 + offset[1] + 20. * zoom;
        for x in 30..70 {
            let x = ((bounds.x as f64 + offset[0] + x as f64 * zoom) * scale) as u32;
            for distance in [-1.25, 1.25] {
                let pixel = frame.pixel(x, ((top + distance) * scale) as u32).unwrap();
                assert!(
                    pixel[..3].iter().all(|v| v.abs_diff(100) < 5),
                    "A one-point selection stroke must not cover pixels {distance} points away: {pixel:?}, zoom {zoom}, scale {scale}, top {top}, x {x}"
                );
            }
        }
    }
}

#[test]
fn unrelated_repaints_reuse_selection_paths_but_selection_changes_invalidate_it() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Selection cache").size(1200., 850.),
            editor(),
        )
        .unwrap();
    let window = view.window_handle();
    cx.capture_screenshot(window).unwrap();
    let allocation = |editor: &Editor| {
        editor
            .selection_outline
            .as_ref()
            .unwrap()
            .outline
            .as_ref()
            .unwrap()
            .white
            .as_ptr() as usize
    };
    let initial = cx.read(view, allocation).unwrap();
    cx.update(view, |e, cx| e.changed(cx)).unwrap();
    cx.capture_screenshot(window).unwrap();
    assert_eq!(
        cx.read(view, allocation).unwrap(),
        initial,
        "An unrelated repaint must not tessellate the unchanged selection again"
    );
    cx.update(view, |e, cx| {
        let selection = e
            .session()
            .document
            .selection
            .as_ref()
            .unwrap()
            .translated([5., 0.])
            .unwrap();
        e.session_mut().document.selection = Some(selection);
        cx.invalidate();
    })
    .unwrap();
    cx.capture_screenshot(window).unwrap();
    let moved = cx.read(view, allocation).unwrap();
    assert_ne!(
        moved, initial,
        "Moving the same coverage must invalidate its outline"
    );
    cx.update(view, |e, cx| {
        e.session_mut().document.selection = Some(Selection::rectangle(
            100,
            100,
            [10., 10.],
            [30., 40.],
            false,
        ));
        cx.invalidate();
    })
    .unwrap();
    cx.capture_screenshot(window).unwrap();
    assert_ne!(
        cx.read(view, allocation).unwrap(),
        moved,
        "Replacing coverage must invalidate the outline even without a global revision change"
    );
}

#[test]
fn selection_outlines_paint_above_transform_frames_and_handles() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Selection stacking").size(1200., 850.),
            editor(),
        )
        .unwrap();
    let window = view.window_handle();
    cx.capture_screenshot(window).unwrap();
    cx.update(view, |e, cx| {
        e.selection_outline.as_mut().unwrap().started = Instant::now() + Duration::from_secs(60);
        cx.invalidate();
    })
    .unwrap();
    let with_handles = cx.capture_screenshot(window).unwrap();
    cx.update(view, |e, cx| {
        e.session_mut().document.selection = None;
        cx.invalidate();
    })
    .unwrap();
    let without_selection = cx.capture_screenshot(window).unwrap();
    let bounds = cx.element_bounds(window, "canvas").unwrap();
    let scale = with_handles.width() as f32 / 1200.;
    let mut intersections = 0;
    for y in (bounds.y * scale) as u32..((bounds.y + bounds.height) * scale) as u32 {
        for x in (bounds.x * scale) as u32..((bounds.x + bounds.width) * scale) as u32 {
            if without_selection.pixel(x, y).unwrap()[..3] == [255, 255, 255]
                && with_handles.pixel(x, y).unwrap()[..3]
                    .iter()
                    .all(|value| *value < 200)
            {
                intersections += 1;
            }
        }
    }
    assert!(
        intersections >= 4,
        "Expected multiple selection/handle intersections"
    );
}

#[test]
fn marching_ants_advance_every_120ms_and_reuse_all_eight_phases() {
    let selection = Selection::rectangle(20, 20, [2., 2.], [18., 18.], false);
    let mut paths = OutlinePaths::new(
        &selection,
        Viewport {
            size: [20., 20.],
            zoom: 1.,
            offset: [0., 0.],
        },
    )
    .unwrap()
    .unwrap();
    let started = Instant::now();
    let white = paths.white.clone();
    let black = paths.black(0).unwrap();
    assert_eq!(animation_frame(started, started), (0, started + FRAME_TIME));
    assert_eq!(
        animation_frame(started, started + FRAME_TIME / 2),
        (0, started + FRAME_TIME)
    );
    for tick in 1..8 {
        assert_eq!(
            animation_frame(started, started + FRAME_TIME * tick),
            (tick as u8, started + FRAME_TIME * (tick + 1))
        );
        let next = paths.black(tick as u8).unwrap();
        assert!(!Arc::ptr_eq(&black, &next));
        assert!(Arc::ptr_eq(&white, &paths.white));
    }
    assert_eq!(
        animation_frame(started, started + FRAME_TIME * 8),
        (0, started + FRAME_TIME * 9)
    );
    assert!(Arc::ptr_eq(&black, &paths.black(0).unwrap()));
}

#[test]
fn panning_a_selection_offscreen_preserves_its_animation_clock() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Selection animation").size(1200., 850.),
            editor(),
        )
        .unwrap();
    let window = view.window_handle();
    cx.capture_screenshot(window).unwrap();
    let started = cx
        .read(view, |e| e.selection_outline.as_ref().unwrap().started)
        .unwrap();
    cx.update(view, |e, cx| {
        e.session_mut().pan = [2000., 0.];
        cx.invalidate();
    })
    .unwrap();
    cx.capture_screenshot(window).unwrap();
    cx.read(view, |e| {
        let cache = e.selection_outline.as_ref().unwrap();
        assert!(cache.outline.is_none());
        assert_eq!(cache.started, started);
    })
    .unwrap();
    cx.update(view, |e, cx| {
        e.session_mut().pan = [0., 0.];
        cx.invalidate();
    })
    .unwrap();
    cx.capture_screenshot(window).unwrap();
    cx.read(view, |e| {
        let cache = e.selection_outline.as_ref().unwrap();
        assert!(cache.outline.is_some());
        assert_eq!(cache.started, started);
    })
    .unwrap();
}
