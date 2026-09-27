use super::*;
use quickgui::{Application, WindowOptions};

fn fixture() -> Tip {
    compositor::brush::sampled::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gbr/pixel.gbr"),
    )
    .unwrap()
}

#[test]
fn imported_tip_is_selectable_and_spacing_is_validated_without_document_history() {
    let mut editor = Editor::with_test_document();
    editor.tools.tool = Tool::Brush;
    let original = editor.session().document.clone();
    editor.install_brush_tip(fixture()).unwrap();
    editor.install_brush_tip(fixture()).unwrap();
    assert_eq!(editor.brush_presets.tips.len(), 1);
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Brush Tips").size(1500., 900.), editor)
        .unwrap();
    let window = view.window_handle();
    cx.focus(window, "brush-tip-spacing").unwrap();
    cx.simulate_keystrokes(window, "ctrl-a").unwrap();
    cx.simulate_input(window, "75").unwrap();
    cx.read(view, |e| {
        let Shape::Sampled(brush) = &e.tools.brush_shape else {
            panic!("sampled tip")
        };
        assert_eq!(brush.spacing(), 0.75);
    })
    .unwrap();
    cx.simulate_keystrokes(window, "ctrl-a").unwrap();
    cx.simulate_input(window, "NaN").unwrap();
    cx.read(view, |e| {
        let Shape::Sampled(brush) = &e.tools.brush_shape else {
            panic!("sampled tip")
        };
        assert_eq!(brush.spacing(), 0.75);
    })
    .unwrap();
    cx.click(window, "brush-tip-round").unwrap();
    cx.read(view, |e| {
        assert!(matches!(e.tools.brush_shape, Shape::Round))
    })
    .unwrap();
    cx.click(window, "brush-tip-0").unwrap();
    cx.click(window, "brush-tips-close").unwrap();
    assert!(cx.element_bounds(window, "brush-hardness-slider").is_err());
    cx.click(window, "brush-tip-picker").unwrap();
    cx.read(view, |e| {
        assert!(matches!(e.modal, Some(Form::BrushTips(_))));
        assert_eq!(e.session().document, original);
        assert!(e.session().undo_label().is_none());
    })
    .unwrap();
}

#[test]
fn selected_sampled_tip_paints_through_canvas_and_undo_restores_document() {
    use quickgui::{MouseButton, Point, PointerEvent, PointerPhase, Size, Vector};
    let mut editor = Editor::with_test_document();
    editor.tabs = vec![Session::new(Document::new(64, 64).unwrap(), None).into()];
    editor.session_mut().fit = false;
    editor.session_mut().zoom = 1.;
    editor.tools.tool = Tool::Brush;
    editor.tools.brush.diameter = 20.;
    editor.tools.brush.color = [180, 20, 90, 255];
    editor.install_brush_tip(fixture()).unwrap();
    editor.modal = None;
    let before = editor.session().document.clone();
    for phase in [PointerPhase::Down, PointerPhase::Up] {
        editor
            .pointer(&PointerEvent {
                tablet: None,
                phase,
                position: Point::new(32., 32.),
                origin: Point::new(32., 32.),
                local_position: Point::new(32., 32.),
                local_origin: Point::new(32., 32.),
                delta: Vector::ZERO,
                button: MouseButton::Left,
                modifiers: Modifiers::empty(),
                size: Size::new(64., 64.),
            })
            .unwrap();
    }
    let after = editor.session().document.clone();
    assert_eq!(
        after.layers[0].raster().unwrap()[(24, 24)].0,
        [180, 20, 90, 255]
    );
    assert_eq!(editor.session().undo_label(), Some("Brush Stroke"));
    editor.session_mut().undo();
    assert_eq!(editor.session().document, before);
    editor.session_mut().redo();
    assert_eq!(editor.session().document, after);
}

#[test]
fn real_bristles_picker_keeps_thumbnails_inside_rows() {
    let mut editor = Editor::with_test_document();
    editor.tools.tool = Tool::Brush;
    editor.install_brush_tip(fixture()).unwrap();
    editor
        .install_brush_tip(
            compositor::brush::sampled::read(
                &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/gbr/bristles-01.gbr"),
            )
            .unwrap(),
        )
        .unwrap();
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(WindowOptions::new("Brush Tips").size(1500., 900.), editor)
        .unwrap();
    for index in 0..2 {
        let row = cx
            .element_bounds(view.window_handle(), format!("brush-tip-{index}"))
            .unwrap();
        let thumbnail = cx
            .element_bounds(view.window_handle(), format!("brush-tip-thumbnail-{index}"))
            .unwrap();
        assert!(thumbnail.y >= row.y && thumbnail.y + thumbnail.height <= row.y + row.height);
        assert!(thumbnail.x >= row.x && thumbnail.x + thumbnail.width <= row.x + row.width);
    }
}
