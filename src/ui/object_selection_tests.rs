use super::*;
use compositor::{
    object_selection::{Settings, Target},
    selection::SelectionMode,
};
use quickgui::{
    Application, MouseButton, Point, PointerEvent, PointerPhase, Size, Vector, WindowOptions,
};
#[test]
fn object_shortcuts_modes_and_sampling_are_exposed() {
    let editor = Editor::with_test_document();
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Object selection").size(1500., 900.),
            editor,
        )
        .unwrap();
    let window = view.window_handle();
    cx.simulate_keystrokes(window, "o").unwrap();
    assert_eq!(cx.read(view, |e| e.tools.tool).unwrap(), Tool::Object);
    cx.click(window, "sample-layers-false").unwrap();
    assert!(!cx.read(view, |e| e.tools.object_sample_all).unwrap());
    cx.click(window, "selection-mode-Add").unwrap();
    assert_eq!(
        cx.read(view, |e| e.tools.selection_mode).unwrap(),
        SelectionMode::Add
    );
    cx.focus(window, "workspace").unwrap();
    cx.simulate_keystrokes(window, "tab").unwrap();
    assert_eq!(cx.read(view, |e| e.tools.tool).unwrap(), Tool::Wand);
    cx.simulate_keystrokes(window, "tab").unwrap();
    assert_eq!(cx.read(view, |e| e.tools.tool).unwrap(), Tool::Object);
    cx.simulate_keystrokes(window, "w").unwrap();
    assert_eq!(cx.read(view, |e| e.tools.tool).unwrap(), Tool::Wand);
    cx.simulate_keystrokes(window, "w").unwrap();
    assert_eq!(cx.read(view, |e| e.tools.tool).unwrap(), Tool::Object);
}
#[test]
fn object_click_queues_inference_and_completion_is_undoable() {
    let mut editor = Editor::with_test_document();
    editor.tabs = vec![Session::new(Document::new(100, 100).unwrap(), None).into()];
    editor.tools.tool = Tool::Object;
    editor.session_mut().fit = false;
    editor.session_mut().zoom = 1.;
    editor.tools.object_edge_offset = -3;
    let before = editor.session().document.clone();
    let p = Point::new(30., 40.);
    object_pointer(&mut editor, PointerPhase::Down, p, p, Modifiers::SHIFT);
    assert!(editor.job.is_none());
    assert!(!editor.pending);
    assert_eq!(editor.session().document, before);
    editor.tools.object_edge_offset = 5;
    object_pointer(&mut editor, PointerPhase::Up, p, p, Modifiers::empty());
    let job = editor.job.take().unwrap();
    let completion = job.completion();
    let jobs::Job::SelectForeground(settings) = job else {
        panic!("object inference job");
    };
    assert_eq!(
        settings,
        Settings {
            target: Target::Object([30., 40.]),
            sample_all: true,
            antialiased: true,
            edge_offset: -3,
            mode: SelectionMode::Add
        }
    );
    let mut result = before.clone();
    let mask = image::GrayImage::from_fn(100, 100, |x, y| {
        image::Luma([if (20..50).contains(&x) && (20..60).contains(&y) {
            255
        } else {
            0
        }])
    });
    compositor::object_selection::select_from_mask(&mut result, &mask, settings).unwrap();
    let id = editor.tabs[editor.current].id;
    editor
        .complete_job(id, before.clone(), Ok(result), completion)
        .unwrap();
    assert!(editor.session().document.selection.is_some());
    assert!(!editor.pending);
    editor.session_mut().undo();
    assert_eq!(editor.session().document, before);
}

fn object_pointer(
    editor: &mut Editor,
    phase: PointerPhase,
    start: Point,
    position: Point,
    modifiers: Modifiers,
) {
    editor
        .pointer(&PointerEvent {
            phase,
            position,
            origin: start,
            local_position: position,
            local_origin: start,
            delta: Vector::ZERO,
            button: MouseButton::Left,
            modifiers,
            size: Size::new(100., 100.),
        })
        .unwrap();
}
fn object_editor() -> Editor {
    let mut editor = Editor::with_test_document();
    editor.tabs = vec![Session::new(Document::new(100, 100).unwrap(), None).into()];
    editor.tools.tool = Tool::Object;
    editor.session_mut().fit = false;
    editor.session_mut().zoom = 1.;
    editor
}
#[test]
fn object_box_drag_clamps_to_canvas_and_preserves_mode_until_release() {
    let mut editor = object_editor();
    editor.session_mut().document.selection = Some(compositor::selection::Selection::from_mask(
        image::GrayImage::from_pixel(100, 100, image::Luma([255])),
    ));
    let original = editor.session().document.clone();
    let start = Point::new(80., 75.);
    let end = Point::new(-10., -20.);
    object_pointer(
        &mut editor,
        PointerPhase::Down,
        start,
        start,
        Modifiers::ALT,
    );
    object_pointer(
        &mut editor,
        PointerPhase::Move,
        start,
        end,
        Modifiers::empty(),
    );
    assert!(editor.job.is_none());
    assert_eq!(editor.session().document, original);
    assert_eq!(editor.displayed_selection_mode(), SelectionMode::Subtract);
    assert!(matches!(
        editor.gesture,
        Some(Gesture::Object {
            start: [80., 75.],
            end: [0., 0.],
            ..
        })
    ));
    object_pointer(
        &mut editor,
        PointerPhase::Up,
        start,
        end,
        Modifiers::empty(),
    );
    let Some(jobs::Job::SelectForeground(settings)) = editor.job else {
        panic!("Object box job was not queued");
    };
    assert_eq!(
        settings.target,
        Target::ObjectBox {
            start: [80., 75.],
            end: [0., 0.]
        }
    );
    assert_eq!(settings.mode, SelectionMode::Subtract);
    assert_eq!(editor.session().document, original);
}
#[test]
fn object_gesture_cancel_and_zero_height_drag_do_not_change_selection() {
    for cancel in [true, false] {
        let mut editor = object_editor();
        let original = editor.session().document.clone();
        let start = Point::new(20., 30.);
        let end = if cancel {
            Point::new(80., 90.)
        } else {
            Point::new(80., 30.)
        };
        object_pointer(
            &mut editor,
            PointerPhase::Down,
            start,
            start,
            Modifiers::empty(),
        );
        object_pointer(
            &mut editor,
            PointerPhase::Move,
            start,
            end,
            Modifiers::empty(),
        );
        if cancel {
            object_pointer(
                &mut editor,
                PointerPhase::Cancel,
                start,
                end,
                Modifiers::empty(),
            );
        }
        object_pointer(
            &mut editor,
            PointerPhase::Up,
            start,
            end,
            Modifiers::empty(),
        );
        assert!(editor.gesture.is_none());
        assert!(editor.job.is_none());
        assert!(!editor.pending);
        assert_eq!(editor.session().document, original);
    }
}
#[test]
fn object_click_jitter_keeps_point_prompt_even_inside_existing_selection() {
    let mut editor = object_editor();
    editor.session_mut().document.selection = Some(compositor::selection::Selection::from_mask(
        image::GrayImage::from_pixel(100, 100, image::Luma([255])),
    ));
    let start = Point::new(30., 40.);
    object_pointer(
        &mut editor,
        PointerPhase::Down,
        start,
        start,
        Modifiers::empty(),
    );
    object_pointer(
        &mut editor,
        PointerPhase::Up,
        start,
        Point::new(31., 41.),
        Modifiers::empty(),
    );
    let Some(jobs::Job::SelectForeground(settings)) = editor.job else {
        panic!("Object click job was not queued");
    };
    assert_eq!(settings.target, Target::Object([30., 40.]));
}

#[test]
fn object_edge_control_clamps_signed_values_and_text_tab_does_not_switch_tools() {
    let editor = object_editor();
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Object edge").size(1500., 900.), editor)
        .unwrap();
    let window = view.window_handle();
    for (input, expected) in [("-4", -4), ("-99", -10), ("99", 10)] {
        cx.focus(window, "object-edge-offset").unwrap();
        cx.simulate_keystrokes(window, "ctrl-a").unwrap();
        cx.simulate_input(window, input).unwrap();
        assert_eq!(
            cx.read(view, |e| e.tools.object_edge_offset).unwrap(),
            expected
        );
    }
    cx.simulate_keystrokes(window, "tab").unwrap();
    assert_eq!(cx.read(view, |e| e.tools.tool).unwrap(), Tool::Object);
    cx.focus(window, "workspace").unwrap();
    cx.simulate_keystrokes(window, "tab").unwrap();
    assert_eq!(cx.read(view, |e| e.tools.tool).unwrap(), Tool::Wand);
    assert!(cx.element_bounds(window, "object-edge-offset").is_err());
}
