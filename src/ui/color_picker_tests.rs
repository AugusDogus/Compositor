use super::*;
use quickgui::{Application, Point, WindowOptions};

#[test]
fn picker_marker_stays_centered_on_fractional_color_coordinates() {
    let mut editor = Editor::with_test_document();
    editor.open_form(Action::Color);
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Picker marker").size(1500., 900.),
            editor,
        )
        .unwrap();
    let window = view.window_handle();
    let field = cx.element_bounds(window, "color-field").unwrap();
    for offset in [0., 0.25, 0.5, 0.75] {
        let point = Point::new(field.x + 128. + offset, field.y + 128. + offset);
        cx.simulate_pointer_drag(window, "color-field", point, point)
            .unwrap();
        let shot = cx.capture_screenshot(window).unwrap();
        let scale = shot.width() as f32 / 1500.;
        let mut weight = 0.;
        let mut moment = [0.; 2];
        for y in ((point.y - 9.) * scale) as u32..((point.y + 9.) * scale) as u32 {
            for x in ((point.x - 9.) * scale) as u32..((point.x + 9.) * scale) as u32 {
                let px = (x as f32 + 0.5) / scale;
                let py = (y as f32 + 0.5) / scale;
                // At red hue, the unmarked field's green channel is (1-S)*B.
                let background = (1. - (px - field.x) / 256.) * (1. - (py - field.y) / 256.) * 255.;
                let green = f32::from(shot.pixel(x, y).unwrap()[1]);
                let coverage = ((green - background) / (255. - background)).max(0.);
                weight += coverage;
                moment[0] += px * coverage;
                moment[1] += py * coverage;
            }
        }
        assert!(weight > 30. * scale * scale, "Missing white marker");
        for (actual, expected) in moment
            .map(|value| value / weight)
            .into_iter()
            .zip([point.x, point.y])
        {
            assert!(
                (actual - expected).abs() < 0.12,
                "Marker center {actual}, selected coordinate {expected}"
            );
        }
    }
}

#[test]
fn gradient_map_sampling_does_not_resample_its_new_preview_on_pointer_release() {
    let mut e = Editor::with_test_document();
    let mut doc = Document::new(20, 20).unwrap();
    compositor::edits::fill(&mut doc, [128, 128, 128, 255], false, false).unwrap();
    e.tabs = vec![Session::new(doc, None).into()];
    e.session_mut().fit = false;
    e.open_pixel_adjustment(Kind::GradientMap).unwrap();
    e.open_gradient_map_picker().unwrap();
    let mut event = quickgui::PointerEvent {
        phase: quickgui::PointerPhase::Down,
        position: Point::new(10., 10.),
        origin: Point::new(10., 10.),
        local_position: Point::new(10., 10.),
        local_origin: Point::new(10., 10.),
        delta: quickgui::Vector::ZERO,
        button: quickgui::MouseButton::Left,
        modifiers: Modifiers::empty(),
        size: quickgui::Size::new(20., 20.),
    };
    e.sample_picker(&event);
    e.preview_picker().unwrap();
    let sampled = e.adjustment_edit.as_ref().unwrap().settings.clone();
    assert_eq!(
        sampled.gradient_map_settings.unwrap().shadows.red,
        128. / 255.
    );
    event.phase = quickgui::PointerPhase::Up;
    e.sample_picker(&event);
    e.preview_picker().unwrap();
    assert_eq!(e.adjustment_edit.as_ref().unwrap().settings, sampled);
}

#[test]
fn opening_gradient_map_previews_default_colors_and_cancel_restores_pixels() {
    let mut e = Editor::with_test_document();
    let mut doc = Document::new(2, 2).unwrap();
    compositor::edits::fill(&mut doc, [200, 60, 20, 255], false, false).unwrap();
    e.tabs = vec![Session::new(doc.clone(), None).into()];
    e.open_pixel_adjustment(Kind::GradientMap).unwrap();
    let pixel = e
        .session()
        .document
        .active_layer()
        .unwrap()
        .raster()
        .unwrap()[(0, 0)];
    assert_eq!(pixel[0], pixel[1]);
    assert_eq!(pixel[1], pixel[2]);
    assert!(e.session().undo_label().is_none());
    e.cancel_adjustment();
    assert_eq!(e.session().document, doc);
}

#[test]
fn gradient_map_reverse_toggle_updates_the_preview_settings_and_cancels() {
    let mut editor = Editor::with_test_document();
    let mut document = Document::new(8, 8).unwrap();
    compositor::edits::fill(&mut document, [200, 60, 20, 255], false, false).unwrap();
    editor.tabs = vec![Session::new(document, None).into()];
    let original = editor.session().document.clone();
    editor.open_pixel_adjustment(Kind::GradientMap).unwrap();
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Gradient Map").size(1280., 900.), editor)
        .unwrap();
    let window = view.window_handle();
    for reversed in [true, false] {
        cx.click(window, 50_002_u64).unwrap();
        cx.read(view, |editor| {
            let settings = editor
                .adjustment_edit
                .as_ref()
                .unwrap()
                .settings
                .gradient_map_settings
                .unwrap();
            assert_eq!(settings.reversed, reversed);
        })
        .unwrap();
    }
    cx.click(window, "form-cancel").unwrap();
    cx.read(view, |editor| {
        assert_eq!(editor.session().document, original);
        assert!(editor.session().undo_label().is_none());
    })
    .unwrap();
}

#[test]
fn visual_picker_edits_drafts_and_only_apply_changes_the_palette() {
    let mut e = Editor::with_test_document();
    let original = e.tools.brush.color;
    e.open_form(Action::Color);
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Colors").size(1280., 900.), e)
        .unwrap();
    let window = view.window_handle();
    let field = cx.element_bounds(window, "color-field").unwrap();
    let point = Point::new(field.x + field.width / 2., field.y + field.height / 2.);
    cx.simulate_pointer_drag(window, "color-field", point, point)
        .unwrap();
    assert_eq!(cx.read(view, |e| e.tools.brush.color).unwrap(), original);
    cx.click(window, "form-apply").unwrap();
    assert_eq!(
        cx.read(view, |e| e.tools.brush.color).unwrap(),
        [128, 64, 64, 255]
    );
    cx.click(window, "palette-background").unwrap();
    cx.focus(window, 51_003_u64).unwrap();
    cx.simulate_keystroke(
        window,
        quickgui::Keystroke::new(Key::Character("a".into()), Modifiers::CONTROL),
    )
    .unwrap();
    cx.simulate_input(window, "#0f0").unwrap();
    cx.click(window, "form-apply").unwrap();
    assert_eq!(
        cx.read(view, |e| e.tools.background).unwrap(),
        [0, 255, 0, 255]
    );
    assert_eq!(
        cx.read(view, |e| e.tools.brush.color).unwrap(),
        [128, 64, 64, 255]
    );
    cx.click(window, 320_u64).unwrap();
    cx.focus(window, 51_000_u64).unwrap();
    cx.simulate_keystroke(
        window,
        quickgui::Keystroke::new(Key::Character("a".into()), Modifiers::CONTROL),
    )
    .unwrap();
    cx.simulate_input(window, "256").unwrap();
    cx.click(window, "form-apply").unwrap();
    assert!(!cx.read(view, |e| e.picking_color()).unwrap());
    assert_eq!(
        cx.read(view, |e| e.tools.brush.color).unwrap(),
        [255, 64, 64, 255]
    );
}

#[test]
fn movable_picker_samples_canvas_without_painting_and_remembers_position() {
    let mut e = Editor::with_test_document();
    let mut doc = Document::new(4, 4).unwrap();
    doc.layers[0].content = compositor::document::LayerContent::Raster(Some(Arc::new(
        image::RgbaImage::from_fn(4, 4, |_, y| {
            image::Rgba(if y < 2 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            })
        }),
    )));
    e.tabs = vec![Session::new(doc.clone(), None).into()];
    e.color_picker_position = Some([700., 30.]);
    e.open_form(Action::Color);
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Sample color").size(1280., 900.), e)
        .unwrap();
    let window = view.window_handle();
    let title = cx.element_bounds(window, "color-picker-title").unwrap();
    let from = Point::new(title.x + 20., title.y + 10.);
    cx.simulate_pointer_drag(
        window,
        "color-picker-title",
        from,
        Point::new(from.x + 20., from.y + 10.),
    )
    .unwrap();
    let position = cx.read(view, |e| e.color_picker_position).unwrap();
    assert_eq!(position, Some([720., 40.]));
    let bounds = cx.element_bounds(window, "canvas").unwrap();
    let (zoom, offset) = cx
        .update(view, |e, _| e.viewport(bounds.width, bounds.height))
        .unwrap();
    let point = Point::new(
        bounds.x + (offset[0] + 1.5 * zoom) as f32,
        bounds.y + (offset[1] + 3.5 * zoom) as f32,
    );
    cx.simulate_pointer_drag(window, "canvas", point, point)
        .unwrap();
    assert_eq!(
        cx.focused(window).unwrap(),
        Some("color-picker".into()),
        "Releasing a sampled color must return keyboard focus to the picker"
    );
    assert_eq!(
        cx.read(view, |e| e.tools.brush.color).unwrap(),
        [0, 0, 0, 255]
    );
    cx.click(window, "form-cancel").unwrap();
    assert_eq!(
        cx.read(view, |e| e.session().document.clone()).unwrap(),
        doc
    );
    cx.click(window, 320_u64).unwrap();
    assert_eq!(
        cx.read(view, |e| e.color_picker_position).unwrap(),
        position
    );
    cx.focus(window, 51_000_u64).unwrap();
    cx.simulate_pointer_drag(window, "canvas", point, point)
        .unwrap();
    assert_eq!(
        cx.focused(window).unwrap(),
        Some(51_000_u64.into()),
        "Sampling must return to the picker's existing text responder"
    );
    cx.simulate_keystrokes(window, "up").unwrap();
    cx.click(window, "form-apply").unwrap();
    assert_eq!(
        cx.read(view, |e| e.tools.brush.color).unwrap(),
        [1, 0, 255, 255]
    );
    assert_eq!(
        cx.read(view, |e| e.session().document.clone()).unwrap(),
        doc
    );
}

#[test]
fn gradient_map_picker_previews_restores_cancel_and_commits_with_the_adjustment() {
    for pixels in [false, true] {
        let mut e = Editor::with_test_document();
        let mut doc = Document::new(2, 2).unwrap();
        compositor::edits::fill(&mut doc, [255; 4], false, false).unwrap();
        let original = doc.clone();
        e.tabs = vec![Session::new(doc, None).into()];
        e.tools.brush.color = [100, 20, 30, 255];
        if pixels {
            e.open_pixel_adjustment(Kind::GradientMap).unwrap();
        } else {
            e.open_adjustment(Some(Kind::GradientMap)).unwrap();
        }
        let created = e.session().document.clone();
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Gradient colors").size(1280., 900.), e)
            .unwrap();
        let window = view.window_handle();
        for apply in [false, true] {
            cx.click(window, "gradient-map-highlights").unwrap();
            cx.focus(window, 51_003_u64).unwrap();
            cx.simulate_keystroke(
                window,
                quickgui::Keystroke::new(Key::Character("a".into()), Modifiers::CONTROL),
            )
            .unwrap();
            cx.simulate_input(window, "#00f").unwrap();
            assert_eq!(
                cx.read(view, |e| compositor::render::render(
                    &e.session().document,
                    2,
                    2
                )
                .unwrap()[(0, 0)])
                    .unwrap(),
                image::Rgba([255; 4])
            );
            cx.simulate_keystroke(
                window,
                quickgui::Keystroke::new(
                    if apply { Key::Enter } else { Key::Tab },
                    Modifiers::empty(),
                ),
            )
            .unwrap();
            assert!(cx.read(view, |e| e.picking_color()).unwrap());
            cx.read(view, |e| {
                assert_eq!(
                    compositor::render::render(&e.session().document, 2, 2).unwrap()[(0, 0)],
                    image::Rgba([0, 0, 255, 255])
                );
                assert_eq!(e.tools.brush.color, [100, 20, 30, 255]);
                assert_eq!(
                    e.session().undo_label(),
                    if pixels {
                        None
                    } else {
                        Some("New Gradient Map Adjustment")
                    }
                );
            })
            .unwrap();
            cx.click(window, if apply { "form-apply" } else { "form-cancel" })
                .unwrap();
            assert!(!cx.read(view, |e| e.picking_color()).unwrap());
            if !apply {
                assert_eq!(
                    cx.read(view, |e| compositor::render::render(
                        &e.session().document,
                        2,
                        2
                    )
                    .unwrap()[(0, 0)])
                        .unwrap(),
                    image::Rgba([255; 4])
                );
            }
        }
        cx.click(window, "form-apply").unwrap();
        cx.read(view, |e| {
            assert_eq!(
                compositor::render::render(&e.session().document, 2, 2).unwrap()[(0, 0)],
                image::Rgba([0, 0, 255, 255])
            );
            assert!(e.session().undo_label().is_some());
        })
        .unwrap();
        cx.update(view, |e, _| e.session_mut().undo()).unwrap();
        if !pixels {
            assert_eq!(
                cx.read(view, |e| e.session().document.clone()).unwrap(),
                created
            );
            cx.update(view, |e, _| e.session_mut().undo()).unwrap();
        }
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            original
        );
    }
}

#[test]
fn canvas_extension_picker_restores_form_and_only_apply_changes_fill() {
    let mut editor = Editor::with_test_document();
    editor.open_form(Action::CanvasSize);
    editor.update_form_field(0, "640");
    let palette = [editor.tools.brush.color, editor.tools.background];
    let original = editor.session().document.clone();
    let (mut cx, view) = Application::new()
        .bind_keys(quickgui::select_key_bindings())
        .into_test_context(WindowOptions::new("Canvas color").size(1280., 900.), editor)
        .unwrap();
    let window = view.window_handle();
    for (button, expected) in [("form-cancel", "#FFFFFF"), ("form-apply", "#12AB34")] {
        cx.click(window, "size-fill").unwrap();
        assert!(cx.read(view, |e| e.size_menus.fill.is_open()).unwrap());
        cx.simulate_keystrokes(window, "c enter").unwrap();
        assert!(
            cx.read(view, |e| matches!(
                e.modal,
                Some(Form::Edit {
                    action: Action::CanvasSize,
                    ..
                })
            ))
            .unwrap()
        );
        cx.click(window, "canvas-extension-custom").unwrap();
        cx.focus(window, 51_003_u64).unwrap();
        cx.simulate_keystroke(
            window,
            quickgui::Keystroke::new(Key::Character("a".into()), Modifiers::CONTROL),
        )
        .unwrap();
        cx.simulate_input(window, "#12ab34").unwrap();
        cx.click(window, button).unwrap();
        cx.read(view, |e| {
            let Some(Form::Edit {
                action: Action::CanvasSize,
                fields,
                ..
            }) = &e.modal
            else {
                panic!("Canvas size form was not restored")
            };
            assert_eq!(fields[0].1, "640");
            assert_eq!(fields[6].1, expected);
            assert_eq!(e.size_menus.fill.value_text().as_deref(), Some("Custom"));
            assert_eq!([e.tools.brush.color, e.tools.background], palette);
            assert_eq!(e.session().document, original);
        })
        .unwrap();
    }
    cx.click(window, "size-fill").unwrap();
    cx.simulate_keystrokes(window, "w enter").unwrap();
    assert!(
        cx.element_bounds(window, "canvas-extension-custom")
            .is_err()
    );
    cx.click(window, "size-fill").unwrap();
    cx.simulate_keystrokes(window, "c enter").unwrap();
    cx.read(view, |e| {
        let Some(Form::Edit { fields, .. }) = &e.modal else {
            panic!("Canvas Size closed")
        };
        assert_eq!(fields[6].1, "#12AB34");
        assert_eq!(e.size_menus.fill.value_text().as_deref(), Some("Custom"));
    })
    .unwrap();
    cx.click(window, "form-cancel").unwrap();
    cx.update(view, |e, cx| {
        e.open_form(Action::CanvasSize);
        cx.invalidate();
    })
    .unwrap();
    cx.click(window, "size-fill").unwrap();
    cx.simulate_keystrokes(window, "c enter").unwrap();
    cx.read(view, |e| {
        let Some(Form::Edit { fields, .. }) = &e.modal else {
            panic!("Canvas Size closed")
        };
        assert_eq!(fields[6].1, "#FFFFFF");
        assert_eq!(e.session().document, original);
    })
    .unwrap();
}

#[test]
fn picker_takes_keyboard_focus_and_rgb_arrows_clamp_without_changing_canvas() {
    use quickgui::Keystroke;
    let mut editor = Editor::with_test_document();
    editor.tools.brush.color = [250, 80, 60, 255];
    let original = editor.session().document.clone();
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Picker focus").size(1280., 900.), editor)
        .unwrap();
    let window = view.window_handle();
    cx.click(window, 320_u64).unwrap();
    assert_eq!(
        cx.focused(window).unwrap(),
        Some(quickgui::ElementId::from("color-picker"))
    );
    cx.focus(window, 51_000_u64).unwrap();
    cx.simulate_keystroke(window, Keystroke::new(Key::ArrowUp, Modifiers::SHIFT))
        .unwrap();
    assert_eq!(
        cx.read(view, |e| e.tools.brush.color).unwrap(),
        [250, 80, 60, 255]
    );
    cx.simulate_keystroke(window, Keystroke::new(Key::Enter, Modifiers::empty()))
        .unwrap();
    cx.read(view, |e| {
        assert_eq!(e.tools.brush.color, [255, 80, 60, 255]);
        assert_eq!(e.session().document, original);
        assert!(e.session().undo_label().is_none());
        assert!(!e.picking_color());
    })
    .unwrap();
}

#[test]
fn sampling_locks_toolbar_and_layer_descendants_until_picker_closes() {
    let editor = Editor::with_test_document();
    let original = editor.session().document.clone();
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Sampling control lock").size(1500., 900.),
            editor,
        )
        .unwrap();
    let window = view.window_handle();
    cx.click(window, 320_u64).unwrap();
    for id in [
        "transform-auto-select",
        "transform-flip-h",
        "layer-add-mask",
    ] {
        let click = cx.click(window, id);
        assert!(
            matches!(click, Err(quickgui::TestAppError::NotClickable { .. })),
            "{id}: {click:?}"
        );
    }
    let menu = quickgui::Menubar::new("application-menu").item_id(6);
    assert!(matches!(
        cx.click(window, menu),
        Err(quickgui::TestAppError::NotClickable { .. })
    ));
    assert_eq!(
        cx.read(view, |e| e.session().document.clone()).unwrap(),
        original
    );
    assert!(
        cx.read(view, |e| !e.tools.transform_auto_select
            && e.picking_color())
            .unwrap()
    );
    cx.click(window, "form-cancel").unwrap();
    cx.click(window, "transform-auto-select").unwrap();
    assert!(cx.read(view, |e| e.tools.transform_auto_select).unwrap());
}
