use super::history_tests::history;
use super::*;
use compositor::document::{LayerContent, Mask};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use quickgui::{
    Application, MouseButton, Point, PointerEvent, PointerPhase, Size, Vector, WindowOptions,
};

fn editor() -> Editor {
    let mut e = Editor::with_test_document();
    let mut doc = Document::new(64, 64).unwrap();
    doc.layers[0].content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(64, 64, |x, y| {
            Rgba([
                (x * 4) as u8,
                (y * 4) as u8,
                ((x * y + 19) % 256) as u8,
                if x < 8 { 0 } else { 255 },
            ])
        }))));
    doc.layers[0].mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_fn(64, 64, |x, y| {
            Luma([if x < 32 && y < 48 { 0 } else { 255 }])
        })),
        linked: true,
        enabled: true,
        placement: None,
    });
    e.tabs = vec![Session::new(doc, None).into()];
    e.session_mut().fit = false;
    e.session_mut().zoom = 1.;
    e.tools.brush.diameter = 12.;
    e.tools.brush.hardness = 1.;
    e.tools.brush.opacity = 1.;
    e.tools.brush.color = [220, 40, 70, 255];
    e.tools.healing = compositor::filters::Healing::Proximity;
    e.tools.clone_source = Some([12., 12.]);
    e
}

fn stroke(e: &mut Editor, phase: PointerPhase) -> Result<()> {
    e.pointer(&PointerEvent {
        phase,
        position: Point::new(32., 30.),
        origin: Point::new(32., 30.),
        local_position: Point::new(32., 30.),
        local_origin: Point::new(32., 30.),
        delta: Vector::ZERO,
        button: MouseButton::Left,
        modifiers: Modifiers::empty(),
        size: Size::new(64., 64.),
    })
}

#[test]
fn brush_history_names_the_tool_and_mask_target_and_round_trips_pixels() {
    for (tool, mask, label) in [
        (Tool::Brush, false, "Brush Stroke"),
        (Tool::Erase, false, "Erase"),
        (Tool::Blur, false, "Blur"),
        (Tool::Clone, false, "Clone Stamp"),
        (Tool::Heal, false, "Spot Healing"),
        (Tool::Brush, true, "Paint Mask"),
        (Tool::Erase, true, "Paint Mask"),
        (Tool::Blur, true, "Paint Mask"),
    ] {
        let mut e = editor();
        e.tools.tool = tool;
        e.tools.mask_target = mask;
        let before = e.session().document.clone();
        stroke(&mut e, PointerPhase::Down).unwrap();
        stroke(&mut e, PointerPhase::Up).unwrap();
        history(&mut e, label, before);
    }
}

#[test]
fn clone_and_healing_ignore_mask_strokes_without_starting_history() {
    for (tool, source) in [
        (Tool::Clone, Some([12., 12.])),
        (Tool::Clone, None),
        (Tool::Heal, Some([12., 12.])),
    ] {
        let mut e = editor();
        e.tools.clone_source = source;
        e.tools.tool = tool;
        e.tools.mask_target = true;
        let before = e.session().document.clone();
        for phase in [PointerPhase::Down, PointerPhase::Move, PointerPhase::Up] {
            stroke(&mut e, phase).unwrap();
        }
        assert_eq!(e.session().document, before);
        assert!(e.session().undo_label().is_none());
        assert!(!e.session().has_pending_edit());
        assert!(e.gesture.is_none());
    }
}

#[test]
fn gradient_history_names_the_captured_pixel_or_mask_target() {
    for (mask, label) in [(false, "Gradient"), (true, "Gradient Mask")] {
        let mut e = editor();
        e.tools.tool = Tool::Gradient;
        e.tools.mask_target = mask;
        let before = e.session().document.clone();
        e.begin_gradient([12., 16.]).unwrap();
        e.move_gradient([48., 40.], gradient::Endpoint::End, false)
            .unwrap();
        assert!(e.session().undo_label().is_none());
        e.commit_gradient().unwrap();
        history(&mut e, label, before);
    }
}

#[test]
fn selection_commands_and_control_clicked_thumbnails_name_the_selection_source() {
    let e = editor();
    let id = e.session().document.active.unwrap();
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Selection history").size(1500., 900.), e)
        .unwrap();
    for (action, label) in [
        (Action::LoadAlpha, "Load Layer Selection"),
        (Action::LoadMask, "Load Mask Selection"),
        (Action::InvertSelection, "Inverse"),
    ] {
        cx.update(view, |e, cx| {
            let before = e.session().document.clone();
            e.action(action, cx);
            history(e, label, before);
        })
        .unwrap();
    }
    for (mask, label) in [
        (false, "Load Layer Selection"),
        (true, "Load Mask Selection"),
    ] {
        let before = cx.read(view, |e| e.session().document.clone()).unwrap();
        cx.simulate_mouse_down(
            view.window_handle(),
            format!("layer-thumbnail-{id}-{mask}"),
            quickgui::MouseDownEvent {
                button: MouseButton::Left,
                position: Point::new(1308., 277.),
                modifiers: Modifiers::CONTROL,
                click_count: 1,
                first_mouse: false,
            },
        )
        .unwrap();
        cx.update(view, |e, _| history(e, label, before)).unwrap();
    }
}
