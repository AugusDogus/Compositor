use super::*;
use quickgui::{Application, Point, WindowOptions};
#[test]
fn guide_view_shortcuts_leave_pixels_and_pixel_grid_unchanged() {
    let editor = Editor::with_test_document();
    let original = editor.session().document.clone();
    let pixel_grid = editor.tools.pixel_grid;
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Guide shortcuts").size(1280., 850.),
            editor,
        )
        .unwrap();
    let window = view.window_handle();
    cx.simulate_keystrokes(window, "ctrl-r ctrl-' ctrl-; ctrl-shift-;")
        .unwrap();
    cx.read(view, |e| {
        assert!(e.tools.layout.rulers);
        assert!(e.tools.layout.grid);
        assert!(!e.tools.layout.guides);
        assert!(!e.tools.layout.snap);
        assert_eq!(e.tools.pixel_grid, pixel_grid);
        assert_eq!(e.session().document, original);
    })
    .unwrap();
}
#[test]
fn ruler_drag_creates_moves_deletes_and_undoes_guides() {
    let mut editor = Editor::with_test_document();
    editor.tools.layout.rulers = true;
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Guide drags").size(1280., 850.), editor)
        .unwrap();
    let window = view.window_handle();
    let canvas = cx.element_bounds(window, "canvas").unwrap();
    let start = Point::new(canvas.x + 140., canvas.y + 10.);
    let end = Point::new(canvas.x + 140., canvas.y + 180.);
    cx.simulate_pointer_drag(window, "ruler-horizontal", start, end)
        .unwrap();
    let guide = cx
        .read(view, |e| {
            assert_eq!(e.session().document.guides.len(), 1);
            assert_eq!(e.session().undo_label(), Some("Add Guide"));
            e.session().document.guides[0]
        })
        .unwrap();
    assert_eq!(guide.axis, Axis::Horizontal);
    let id = format!("layout-guide-{}", guide.id);
    cx.simulate_pointer_drag(window, id.as_str(), end, Point::new(end.x, end.y + 30.))
        .unwrap();
    cx.read(view, |e| {
        assert_eq!(e.session().document.guides.len(), 1);
        assert!(e.session().document.guides[0].position > guide.position);
        assert_eq!(e.session().undo_label(), Some("Move Guide"));
    })
    .unwrap();
    cx.simulate_pointer_drag(window, id.as_str(), Point::new(end.x, end.y + 30.), start)
        .unwrap();
    cx.read(view, |e| {
        assert!(e.session().document.guides.is_empty());
        assert_eq!(e.session().undo_label(), Some("Delete Guide"));
    })
    .unwrap();
    cx.update(view, |e, _| e.session_mut().undo()).unwrap();
    assert_eq!(
        cx.read(view, |e| e.session().document.guides.len())
            .unwrap(),
        1
    );
}
#[test]
fn locked_guides_cannot_be_created_or_cleared() {
    let mut editor = Editor::with_test_document();
    editor.tools.layout.rulers = true;
    editor.tools.layout.locked = true;
    let original = editor.session().document.clone();
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Locked guides").size(1280., 850.),
            editor,
        )
        .unwrap();
    let window = view.window_handle();
    let bounds = cx.element_bounds(window, "canvas").unwrap();
    cx.simulate_pointer_drag(
        window,
        "ruler-vertical",
        Point::new(bounds.x + 10., bounds.y + 150.),
        Point::new(bounds.x + 150., bounds.y + 150.),
    )
    .unwrap();
    cx.read(view, |e| {
        assert_eq!(e.session().document, original);
        assert!(e.layout_drag.is_none());
        assert!(!e.action_available(Action::ClearGuides));
    })
    .unwrap();
}
#[test]
fn guide_updates_are_atomic_and_preserve_pixels() {
    let mut editor = Editor::with_test_document();
    let original = editor.session().document.clone();
    let guide = Guide {
        id: Uuid::new_v4(),
        axis: Axis::Vertical,
        position: 0.,
    };
    assert!(
        editor
            .commit_guide_drag(Drag {
                guide,
                origin: Origin::Ruler(Axis::Vertical),
                destination: Some(f64::INFINITY)
            })
            .is_err()
    );
    assert_eq!(editor.session().document, original);
    editor
        .commit_guide_drag(Drag {
            guide,
            origin: Origin::Ruler(Axis::Vertical),
            destination: Some(25.),
        })
        .unwrap();
    assert_eq!(editor.session().document.layers, original.layers);
    editor.layout_action(Action::ClearGuides).unwrap();
    assert!(editor.session().document.guides.is_empty());
    editor.session_mut().undo();
    assert_eq!(editor.session().document.guides[0].position, 25.);
}
#[test]
fn ruler_ticks_remain_bounded_and_aligned_after_large_pans() {
    for (zoom, offset) in [(0.01, -2_000_000.), (1., 0.), (64., -2_000_000.)] {
        let ticks = ruler_ticks(1600., zoom, offset);
        assert!(ticks.len() < 2000);
        assert!(!ticks.is_empty());
        for (at, value, _) in ticks {
            assert!((RULER..=1600.).contains(&at));
            assert!((f64::from(at) - (offset + value * zoom)).abs() < 0.001);
        }
    }
}

#[test]
fn rulers_grid_and_guides_render_in_canvas_coordinates() {
    let mut editor = Editor::with_test_document();
    let mut document = Document::new(640, 480).unwrap();
    compositor::edits::fill(&mut document, [201, 132, 86, 255], false, false).unwrap();
    document.guides = vec![
        Guide {
            id: Uuid::new_v4(),
            axis: Axis::Vertical,
            position: 160.,
        },
        Guide {
            id: Uuid::new_v4(),
            axis: Axis::Horizontal,
            position: 120.,
        },
    ];
    editor.tabs = vec![Session::new(document, None).into()];
    editor.tools.layout.rulers = true;
    editor.tools.layout.grid = true;
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Rulers and guides").size(1280., 850.),
            editor,
        )
        .unwrap();
    let window = view.window_handle();
    let canvas = cx.element_bounds(window, "canvas").unwrap();
    let horizontal = cx.element_bounds(window, "ruler-horizontal").unwrap();
    let vertical = cx.element_bounds(window, "ruler-vertical").unwrap();
    assert_eq!(horizontal.height, RULER);
    assert_eq!(vertical.width, RULER);
    assert_eq!(horizontal.y, canvas.y);
    assert_eq!(vertical.x, canvas.x);
    assert!(cx.element_bounds(window, "layout-grid").is_ok());
    let shot = cx.capture_screenshot(window).unwrap();
    if let Some(path) = std::env::var_os("COMPOSITOR_GUIDES_SCREENSHOT") {
        shot.write_png(path).unwrap();
    }
    let scale = shot.width() as f32 / 1280.;
    assert_eq!(
        shot.pixel(
            ((canvas.x + 18.) * scale) as u32,
            ((canvas.y + 18.) * scale) as u32
        ),
        Some([48, 48, 48, 255])
    );
}
