use super::*;
use quickgui::{Application, Point, WindowOptions};

fn fixture() -> (RawAsset, Arc<DecodedRaw>, RgbaImage) {
    let full = Arc::new(DecodedRaw {
        camera: image::Rgb32FImage::from_fn(64, 48, |x, y| {
            image::Rgb([0.05 + x as f32 / 100., 0.07 + y as f32 / 100., 0.21])
        }),
        as_shot: [1.; 3],
        camera_to_rgb: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        xyz_to_camera: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        metadata: raw::RawMetadata {
            width: 64,
            height: 48,
            camera: "Test camera".into(),
            ..Default::default()
        },
    });
    let asset = RawAsset {
        filename: "Test.NEF".into(),
        metadata: full.metadata.clone(),
        settings: DevelopSettings::default(),
        bytes: Arc::new(vec![1, 2, 3]),
    };
    let pixels = image::RgbaImage::from_fn(64, 48, |x, y| {
        image::Rgba([(x * 4) as u8, (y * 5) as u8, 120, 255])
    });
    (asset, full, pixels)
}
fn ready() -> Develop {
    let (asset, full, pixels) = fixture();
    let mut d = Develop::new(Source::Asset(Arc::new(asset.clone())), Target::New);
    let before = worker::image(pixels.clone()).unwrap();
    let worker::Analysis {
        preview,
        warnings,
        histogram,
        clipping,
    } = worker::analyze(pixels).unwrap();
    d.ready = Some(Ready {
        asset,
        full: full.clone(),
        proxy: full,
        before,
        before_crop: [0., 0., 1., 1.],
        before_full: false,
        preview,
        warnings,
        histogram,
        clipping,
    });
    d.request = None;
    d.rendered = Some(0);
    d
}
#[test]
fn raw_commit_targets_destination_and_undo_restores_source_settings_and_placement() {
    let mut e = Editor::new(Vec::new()).unwrap();
    let (asset, _, pixels) = fixture();
    e.apply_developed(&Target::New, asset.clone(), pixels.clone())
        .unwrap();
    let tab = e.tabs[e.current].id;
    let layer = e.session().document.active.unwrap();
    e.session_mut().document.layers[0].transform.origin = [17., 23.];
    e.session_mut().document.layers[0].opacity = 0.7;
    let original = e.session().document.clone();
    e.add_empty_tab();
    let untouched = e.tabs[e.current].id;
    let mut updated = asset;
    updated.settings.exposure = 1.;
    e.apply_developed(&Target::Existing { tab, layer }, updated, pixels)
        .unwrap();
    assert_eq!(e.tabs[e.current].id, tab);
    assert_eq!(
        e.session().document.layers[0].transform,
        original.layers[0].transform
    );
    assert_eq!(e.session().document.layers[0].opacity, 0.7);
    assert_eq!(
        e.session().document.layers[0]
            .raw
            .as_ref()
            .unwrap()
            .settings
            .exposure,
        1.
    );
    e.session_mut().undo();
    assert_eq!(e.session().document, original);
    assert!(
        e.tabs
            .iter()
            .find(|t| t.id == untouched)
            .unwrap()
            .session()
            .is_none()
    );
}
#[test]
fn stale_preview_and_cancelled_commit_cannot_change_document() {
    let mut e = Editor::with_test_document();
    let original = e.session().document.clone();
    let mut d = ready();
    let id = d.id;
    d.edit(|s| s.exposure = 1.);
    d.running = true;
    e.develop = Some(d);
    e.receive_raw(id, 0, true, Err(invalid("Stale failure")));
    assert!(e.develop.as_ref().unwrap().error.is_none());
    assert!(!e.develop.as_ref().unwrap().running);
    let cancel = e.develop.as_ref().unwrap().cancel.clone();
    e.cancel_develop();
    assert!(cancel.load(Ordering::Relaxed));
    e.receive_raw(
        id,
        1,
        false,
        Ok(worker::Output::Applied {
            settings: DevelopSettings::default(),
            pixels: RgbaImage::new(1, 1),
        }),
    );
    assert_eq!(e.session().document, original);
    assert!(e.develop.is_none());
}
#[test]
fn pointer_gesture_has_one_undo_and_presets_retain_redo() {
    let mut d = ready();
    d.begin_gesture();
    for exposure in [0.2, 0.4, 1.] {
        d.edit(|s| s.exposure = exposure);
    }
    d.finish_gesture(false);
    assert_eq!(d.undo.len(), 1);
    d.history(false);
    assert_eq!(d.settings.exposure, 0.);
    assert_eq!(d.redo.len(), 1);
    d.history(true);
    assert_eq!(d.settings.exposure, 1.);
    d.begin_gesture();
    d.edit(|s| s.exposure = 4.);
    d.finish_gesture(true);
    assert_eq!(d.settings.exposure, 1.);
    assert_eq!(d.undo.len(), 1);
}
#[test]
fn raw_workspace_renders_all_panels_and_controls_are_reachable() {
    let mut editor = Editor::new(Vec::new()).unwrap();
    editor.develop = Some(ready());
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(WindowOptions::new("RAW develop").size(1280., 900.), editor)
        .unwrap();
    let window = view.window_handle();
    for panel in 0..6 {
        cx.click(window, format!("raw-panel-{panel}")).unwrap();
        let frame = cx.capture_screenshot(window).unwrap();
        assert!(frame.width() >= 1280);
    }
    cx.click(window, "raw-panel-0").unwrap();
    cx.click(window, "raw-preset-landscape").unwrap();
    assert!(
        cx.read(view, |e| e.develop.as_ref().unwrap().settings.vibrance > 0.)
            .unwrap()
    );
    cx.click(window, "raw-undo").unwrap();
    assert_eq!(
        cx.read(view, |e| e.develop.as_ref().unwrap().settings.vibrance)
            .unwrap(),
        0.
    );
    cx.click(window, "raw-panel-4").unwrap();
    cx.click(window, "raw-add-Brush").unwrap();
    let bounds = cx.element_bounds(window, "raw-canvas").unwrap();
    let from = Point::new(
        bounds.x + bounds.width * 0.4,
        bounds.y + bounds.height * 0.4,
    );
    let to = Point::new(
        bounds.x + bounds.width * 0.6,
        bounds.y + bounds.height * 0.6,
    );
    cx.simulate_pointer_drag(window, "raw-canvas", from, to)
        .unwrap();
    assert!(
        cx.read(view, |e| !e.develop.as_ref().unwrap().settings.overlays[0]
            .points
            .is_empty())
            .unwrap()
    );
    cx.click(window, "raw-cancel").unwrap();
    assert!(
        cx.read(view, |e| e.develop.is_none() && !e.has_document())
            .unwrap()
    );
}
#[test]
fn sixteen_bit_export_preserves_extra_precision_and_embeds_profile() {
    use image::ImageDecoder;
    let (_, full, _) = fixture();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("develop.tif");
    worker::export_tiff(
        &path,
        &full,
        &DevelopSettings::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    let mut decoder = image::ImageReader::open(&path)
        .unwrap()
        .into_decoder()
        .unwrap();
    assert_eq!(decoder.color_type(), image::ColorType::Rgba16);
    assert_eq!(
        decoder.icc_profile().unwrap().unwrap(),
        include_bytes!("../../../assets/color/sRGB.icc")
    );
    let pixels = image::open(&path).unwrap().to_rgba16();
    assert!(pixels.as_raw().iter().any(|v| v % 257 != 0));
}
#[test]
fn file_open_queues_raw_without_decoding_on_ui_thread() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("camera.NEF");
    std::fs::write(&path, [1, 2, 3]).unwrap();
    let mut e = Editor::new(Vec::new()).unwrap();
    let opened = project_open::OpenedProject::load(path.clone(), &[]).unwrap();
    e.show_opened_projects(vec![opened]).unwrap();
    assert_eq!(e.raw_queue.len(), 1);
    assert!(!e.has_document());
    assert!(matches!(e.raw_queue.front(),Some((Source::Path(p),Target::New))if p==&path));
}
#[test]
#[ignore = "Set COMPOSITOR_TEST_NEF to a real Nikon RAW file"]
fn real_raw_develop_screenshot() {
    let path =
        PathBuf::from(std::env::var_os("COMPOSITOR_TEST_NEF").expect("Set COMPOSITOR_TEST_NEF"));
    let (asset, decoded) = raw::open(&path).unwrap();
    let full = Arc::new(decoded);
    let proxy = Arc::new(full.preview(1600));
    let pixels = raw::render(&proxy, &asset.settings, &AtomicBool::new(false)).unwrap();
    let before = worker::image(pixels.clone()).unwrap();
    let worker::Analysis {
        preview,
        warnings,
        histogram,
        clipping,
    } = worker::analyze(pixels).unwrap();
    let mut d = Develop::new(Source::Asset(Arc::new(asset.clone())), Target::New);
    d.ready = Some(Ready {
        asset,
        full: full.clone(),
        proxy,
        before,
        before_crop: [0., 0., 1., 1.],
        before_full: false,
        preview,
        warnings,
        histogram,
        clipping,
    });
    d.request = None;
    d.rendered = Some(0);
    let mut e = Editor::new(Vec::new()).unwrap();
    e.develop = Some(d);
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(WindowOptions::new("RAW develop").size(1440., 1000.), e)
        .unwrap();
    let shot = cx.capture_screenshot(view.window_handle()).unwrap();
    shot.write_png("/tmp/compositor-raw-develop.png").unwrap();
    cx.simulate_window_resize(view.window_handle(), quickgui::Size::new(1024., 768.))
        .unwrap();
    let small = cx.capture_screenshot(view.window_handle()).unwrap();
    small
        .write_png("/tmp/compositor-raw-develop-small.png")
        .unwrap();
    for id in ["raw-apply", "raw-cancel", "raw-export", "raw-panel-5"] {
        let bounds = cx.element_bounds(view.window_handle(), id).unwrap();
        assert!(
            bounds.x >= 0.
                && bounds.y >= 0.
                && bounds.x + bounds.width <= 1024.
                && bounds.y + bounds.height <= 768.,
            "{id}: {bounds:?}"
        );
    }

    let full_pixels =
        raw::render(&full, &DevelopSettings::default(), &AtomicBool::new(false)).unwrap();
    let full_before = worker::image(full_pixels.clone()).unwrap();
    let worker::Analysis {
        preview,
        warnings,
        histogram,
        clipping,
    } = worker::analyze(full_pixels).unwrap();
    cx.update(view, |e, cx| {
        let d = e.develop.as_mut().unwrap();
        let r = d.ready.as_mut().unwrap();
        r.preview = preview;
        r.before = full_before;
        r.before_full = true;
        r.warnings = warnings;
        r.histogram = histogram;
        r.clipping = clipping;
        d.full_preview = true;
        d.fit = false;
        d.zoom = 1.;
        cx.invalidate();
    })
    .unwrap();
    cx.capture_screenshot(view.window_handle())
        .unwrap()
        .write_png("/tmp/compositor-raw-develop-full.png")
        .unwrap();
}

#[test]
fn numeric_fields_preserve_typed_text_until_commit_and_make_one_undo() {
    let mut editor = Editor::new(Vec::new()).unwrap();
    editor.develop = Some(ready());
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(
            WindowOptions::new("RAW numeric input").size(1280., 900.),
            editor,
        )
        .unwrap();
    let window = view.window_handle();
    cx.focus(window, "raw-field-Temperature").unwrap();
    cx.simulate_keystrokes(window, "ctrl-a").unwrap();
    let mut typed = String::new();
    for digit in ["5", "0", "0", "0"] {
        typed.push_str(digit);
        cx.simulate_input(window, digit).unwrap();
        assert_eq!(
            cx.focused_input_value(window).unwrap().as_deref(),
            Some(typed.as_str())
        );
        assert_eq!(
            cx.read(view, |e| e.develop.as_ref().unwrap().settings.temperature)
                .unwrap(),
            6500.
        );
    }
    cx.simulate_keystrokes(window, "enter").unwrap();
    cx.read(view, |e| {
        let d = e.develop.as_ref().unwrap();
        assert_eq!(d.settings.temperature, 5000.);
        assert_eq!(d.undo.len(), 1);
    })
    .unwrap();
    cx.focus(window, "raw-field-Exposure").unwrap();
    cx.simulate_keystrokes(window, "ctrl-a").unwrap();
    for part in ["-", "1", ".", "2", "5"] {
        cx.simulate_input(window, part).unwrap();
    }
    assert_eq!(
        cx.focused_input_value(window).unwrap().as_deref(),
        Some("-1.25")
    );
    cx.focus(window, "workspace").unwrap();
    cx.read(view, |e| {
        let d = e.develop.as_ref().unwrap();
        assert_eq!(d.settings.exposure, -1.25);
        assert_eq!(d.undo.len(), 2);
    })
    .unwrap();
    cx.click(window, "raw-undo").unwrap();
    assert_eq!(
        cx.read(view, |e| e.develop.as_ref().unwrap().settings.exposure)
            .unwrap(),
        0.
    );
    cx.focus(window, "raw-field-Exposure").unwrap();
    cx.simulate_keystrokes(window, "ctrl-a").unwrap();
    cx.simulate_input(window, "-").unwrap();
    cx.simulate_keystrokes(window, "escape").unwrap();
    assert!(cx.read(view, |e| e.develop.is_some()).unwrap());
    assert_eq!(
        cx.read(view, |e| e.develop.as_ref().unwrap().settings.exposure)
            .unwrap(),
        0.
    );
}

#[test]
fn numeric_field_commands_commit_clamped_values_and_reject_nonfinite_text() {
    let mut editor = Editor::new(Vec::new()).unwrap();
    editor.develop = Some(ready());
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(
            WindowOptions::new("RAW input commands").size(1280., 900.),
            editor,
        )
        .unwrap();
    let window = view.window_handle();
    cx.focus(window, "raw-field-Exposure").unwrap();
    cx.simulate_keystrokes(window, "ctrl-a").unwrap();
    cx.simulate_input(window, "1000").unwrap();
    assert_eq!(
        cx.focused_input_value(window).unwrap().as_deref(),
        Some("1000")
    );
    cx.simulate_keystrokes(window, "enter").unwrap();
    assert_eq!(
        cx.read(view, |e| e.develop.as_ref().unwrap().settings.exposure)
            .unwrap(),
        10.
    );
    cx.focus(window, "raw-field-Exposure").unwrap();
    cx.simulate_keystrokes(window, "ctrl-a").unwrap();
    cx.simulate_input(window, "NaN").unwrap();
    cx.simulate_keystrokes(window, "enter").unwrap();
    cx.read(view, |e| {
        let d = e.develop.as_ref().unwrap();
        assert_eq!(d.settings.exposure, 10.);
        assert_eq!(d.undo.len(), 1);
        assert!(
            d.error
                .as_ref()
                .is_some_and(|e| e.contains("previous setting is unchanged"))
        );
        d.settings.validate().unwrap();
    })
    .unwrap();
    cx.click(window, "raw-apply").unwrap();
    cx.read(view, |e| {
        let d = e.develop.as_ref().unwrap();
        assert!(!d.committing);
        assert!(!matches!(d.request, Some(Request::Apply)));
        assert!(d.numeric_draft.is_some());
    })
    .unwrap();
    cx.focus(window, "raw-field-Exposure").unwrap();
    cx.simulate_keystrokes(window, "ctrl-a").unwrap();
    cx.simulate_input(window, "-2.5").unwrap();
    // Keep the Apply request queued behind a preview, as during real editing.
    cx.update(view, |e, _| e.develop.as_mut().unwrap().running = true)
        .unwrap();
    cx.click(window, "raw-apply").unwrap();
    cx.read(view, |e| {
        let d = e.develop.as_ref().unwrap();
        assert_eq!(d.settings.exposure, -2.5);
        assert_eq!(d.undo.len(), 2);
        assert!(matches!(d.request, Some(Request::Apply)));
        assert!(d.numeric_draft.is_none());
    })
    .unwrap();
}

#[test]
fn numeric_field_commits_before_neutral_picker_without_replacing_sampled_balance() {
    let mut editor = Editor::new(Vec::new()).unwrap();
    let mut develop = ready();
    develop.picker = true;
    editor.develop = Some(develop);
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(
            WindowOptions::new("RAW input before picker").size(1280., 900.),
            editor,
        )
        .unwrap();
    let window = view.window_handle();
    cx.focus(window, "raw-field-Temperature").unwrap();
    cx.simulate_keystrokes(window, "ctrl-a").unwrap();
    cx.simulate_input(window, "5000").unwrap();
    let bounds = cx.element_bounds(window, "raw-canvas").unwrap();
    let point = Point::new(bounds.x + bounds.width / 2., bounds.y + bounds.height / 2.);
    cx.simulate_pointer_drag(window, "raw-canvas", point, point)
        .unwrap();
    cx.read(view, |e| {
        let d = e.develop.as_ref().unwrap();
        assert_eq!(d.settings.temperature, 5000.);
        assert_eq!(d.settings.white_balance, raw::WhiteBalance::Custom);
        assert_eq!(d.undo.len(), 2);
        assert!(d.numeric_draft.is_none());
    })
    .unwrap();
}
