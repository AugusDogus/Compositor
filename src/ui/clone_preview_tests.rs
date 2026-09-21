use super::*;
use compositor::document::{Layer, LayerContent, Mask};
use image::{GrayImage, Luma, Rgba};

fn editor() -> Editor {
    let mut e = Editor::with_test_document();
    e.tools.tool = Tool::Clone;
    e.tools.clone_source = Some([32., 32.]);
    e.tools.brush.diameter = 32.;
    e.canvas_pointer = Some([32., 32.]);
    e
}

#[test]
fn preview_uses_rounded_stamp_offset_and_tracks_alignment_and_suppression() {
    let mut e = editor();
    e.tools.clone_source = Some([10.3, 11.8]);
    e.canvas_pointer = Some([40.6, 42.2]);
    let key = e.clone_preview_key(1., [0.; 2]).unwrap();
    for (actual, expected) in key.center.into_iter().zip([10.6, 12.2]) {
        assert!((actual - expected).abs() < 1e-10);
    }
    assert_eq!(e.clone_cursor_source([40.6, 42.2]), e.tools.clone_source);
    e.tools.clone_offset = Some([5., -6.]);
    assert_eq!(e.clone_stroke_offset([40.6, 42.2]), Some([5., -6.]));
    e.tools.clone_aligned = false;
    assert_eq!(e.clone_stroke_offset([40.6, 42.2]), Some([-30., -30.]));
    e.tools.brush.diameter = 2000.;
    assert_eq!(e.clone_preview_key(64., [0.; 2]).unwrap().side, 1024);
    e.keyboard_modifiers = Modifiers::ALT;
    assert!(e.clone_preview_key(1., [0.; 2]).is_none());
    e.keyboard_modifiers = Modifiers::empty();
    e.space_pan = true;
    assert!(e.clone_preview_key(1., [0.; 2]).is_none());
    e.space_pan = false;
    e.modal = Some(Form::Close);
    assert!(e.clone_preview_key(1., [0.; 2]).is_none());
}

#[test]
fn preview_soft_tip_matches_an_actual_clone_click() {
    let e = editor();
    let mut doc = Document::new(64, 64).unwrap();
    doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        64,
        64,
        Rgba([210, 80, 40, 255]),
    ))));
    let target = Layer::blank("Stamp", 64, 64);
    doc.active = Some(target.id);
    doc.selected = [target.id].into();
    doc.layers.push(target);
    for hardness in [0., 0.35, 1.] {
        let key = Key {
            layer: doc.active,
            hardness,
            all_layers: true,
            ..e.clone_preview_key(1., [0.; 2]).unwrap()
        };
        let preview = calculate(doc.clone(), key, &mut Cache::default()).unwrap();
        let mut painted = doc.clone();
        let brush = Brush {
            diameter: 32.,
            hardness,
            opacity: 1.,
            ..Brush::default()
        };
        let mut stroke = Stroke::start(
            &mut painted,
            [32.; 2],
            brush,
            PaintMode::Clone { offset: [0.; 2] },
            false,
            true,
        )
        .unwrap();
        stroke.finish(&mut painted).unwrap();
        let stamp = painted.active_layer().unwrap().raster().unwrap();
        for (x, y, pixel) in preview.enumerate_pixels() {
            let actual = stamp[(x + 16, y + 16)];
            assert_eq!(pixel[3], actual[3], "alpha at {x},{y}, hardness {hardness}");
            if pixel[3] > 0 {
                assert_eq!(*pixel, actual);
            }
        }
    }
}

#[test]
fn active_raster_preview_ignores_compositing_but_respects_transform() {
    let e = editor();
    let mut doc = Document::new(64, 64).unwrap();
    let layer = &mut doc.layers[0];
    layer.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(64, 64, |x, y| {
        Rgba([x as u8 * 3, y as u8 * 3, 40, 255])
    }))));
    layer.transform.rotation = 23.;
    layer.transform.flip_x = true;
    let clean = doc.clone();
    let layer = &mut doc.layers[0];
    layer.visible = false;
    layer.opacity = 0.2;
    layer.mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(1, 1, Luma([0]))),
        enabled: true,
        linked: true,
        placement: None,
    });
    let key = Key {
        layer: doc.active,
        ..e.clone_preview_key(1., [0.; 2]).unwrap()
    };
    let mut cache = Cache::default();
    let raw = calculate(doc.clone(), key, &mut cache).unwrap();
    assert_eq!(raw, calculate(clean, key, &mut cache).unwrap());
    assert!(raw.pixels().any(|p| p[3] > 0));
    let composite = calculate(
        doc,
        Key {
            all_layers: true,
            ..key
        },
        &mut cache,
    )
    .unwrap();
    assert!(composite.pixels().all(|p| p[3] == 0));
}

#[test]
fn stale_clone_workers_cannot_replace_the_latest_preview_or_report_old_errors() {
    let mut e = editor();
    let key = e.clone_preview_key(1., [0.; 2]).unwrap();
    for stale in [
        Key {
            center: [12., 15.],
            ..key
        },
        Key {
            revision: key.revision + 1,
            ..key
        },
        Key {
            session: Uuid::new_v4(),
            ..key
        },
    ] {
        e.clone_preview.desired = Some(key);
        e.clone_preview.work = Work::Running(stale);
        let status = e.status.clone();
        e.receive_clone_preview(stale, Err(invalid("Old failure")));
        assert!(matches!(e.clone_preview.work, Work::Idle));
        assert!(e.clone_preview.ready.is_none());
        assert_eq!(e.status, status);
    }
    e.clone_preview.work = Work::Running(key);
    e.receive_clone_preview(key, Ok(RgbaImage::new(key.side, key.side)));
    assert!(e.clone_preview_image(1., [0.; 2]).is_some());
    e.keyboard_modifiers = Modifiers::ALT;
    assert!(e.clone_preview_image(1., [0.; 2]).is_none());
    e.keyboard_modifiers = Modifiers::empty();
    e.revision += 1;
    assert!(e.clone_preview_image(1., [0.; 2]).is_none());
}

struct PreviewView(Editor);
impl View for PreviewView {
    fn render(&mut self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div()
            .size_full()
            .relative()
            .bg(Color::rgb8(100, 100, 100))
            .child(self.0.brush_cursor(1., [0.; 2]).unwrap())
    }
}

#[test]
fn rendered_preview_obeys_opacity_circle_clipping_and_alt_suppression() {
    let mut e = editor();
    e.canvas_pointer = Some([60., 60.]);
    e.tools.brush.opacity = 0.5;
    let mut doc = Document::new(64, 64).unwrap();
    doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        64,
        64,
        Rgba([220, 40, 60, 255]),
    ))));
    e.tabs = vec![Session::new(doc.clone(), None).into()];
    let key = e.clone_preview_key(1., [0.; 2]).unwrap();
    e.clone_preview.desired = Some(key);
    e.clone_preview.work = Work::Running(key);
    e.receive_clone_preview(key, calculate(doc, key, &mut Cache::default()));
    let (mut cx, view) = quickgui::Application::new()
        .into_test_context(
            quickgui::WindowOptions::new("Clone hover")
                .size(120., 120.)
                .minimum_size(1., 1.),
            PreviewView(e),
        )
        .unwrap();
    let frame = cx.capture_screenshot(view.window_handle()).unwrap();
    let scale = frame.width() as f64 / 120.;
    let pixel = |x, y| frame.pixel((x * scale) as u32, (y * scale) as u32).unwrap();
    assert!(
        pixel(60., 60.)[0].abs_diff(160) <= 1,
        "{:?}",
        pixel(60., 60.)
    );
    assert_eq!(pixel(45., 45.), [100, 100, 100, 255]);
    cx.update(view, |v, cx| {
        v.0.keyboard_modifiers = Modifiers::ALT;
        cx.invalidate();
    })
    .unwrap();
    let frame = cx.capture_screenshot(view.window_handle()).unwrap();
    assert_eq!(
        frame
            .pixel((60. * scale) as u32, (60. * scale) as u32)
            .unwrap(),
        [100, 100, 100, 255]
    );
}

#[test]
fn native_cursor_restores_after_alt_pan_modal_tool_change_and_canvas_exit() {
    let e = editor();
    let (mut cx, view) = quickgui::Application::new()
        .into_test_context(
            quickgui::WindowOptions::new("Clone cursor visibility").size(1280., 850.),
            e,
        )
        .unwrap();
    let window = view.window_handle();
    let bounds = cx.element_bounds(window, "canvas").unwrap();
    cx.update(view, |_, cx| cx.focus_window(window)).unwrap();
    cx.simulate_mouse_move(
        window,
        "canvas",
        quickgui::MouseMoveEvent {
            position: quickgui::Point::new(bounds.x + 100., bounds.y + 100.),
            pressed_button: None,
            modifiers: Modifiers::empty(),
        },
    )
    .unwrap();
    assert!(!cx.window_state(window).unwrap().cursor_visible);
    for state in 0..6 {
        cx.update(view, |e, cx| {
            e.keyboard_modifiers = Modifiers::empty();
            e.space_pan = false;
            e.modal = None;
            e.tools.tool = Tool::Clone;
            e.pending = false;
            e.canvas_pointer = Some([100.; 2]);
            cx.invalidate();
        })
        .unwrap();
        assert!(!cx.window_state(window).unwrap().cursor_visible);
        cx.update(view, move |e, cx| {
            match state {
                0 => e.keyboard_modifiers = Modifiers::ALT,
                1 => e.space_pan = true,
                2 => e.modal = Some(Form::Close),
                3 => e.tools.tool = Tool::Move,
                4 => e.canvas_pointer = None,
                _ => e.pending = true,
            }
            cx.invalidate();
        })
        .unwrap();
        assert!(
            cx.window_state(window).unwrap().cursor_visible,
            "state {state}"
        );
    }
}
