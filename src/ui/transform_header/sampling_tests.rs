use super::*;
use quickgui::{Application, WindowOptions};

#[test]
fn desktop_transform_fields_and_flips_fit_without_partial_controls() {
    for (width, mask) in [
        (1280., false),
        (1280., true),
        (1440., false),
        (1600., false),
        (1920., false),
    ] {
        let mut editor = Editor::with_test_document();
        if mask {
            editor.tools.mask_target = true;
            editor.session_mut().document.layers[0].mask = Some(compositor::document::Mask {
                pixels: Arc::new(image::GrayImage::from_pixel(1, 1, image::Luma([255]))),
                enabled: true,
                linked: false,
                placement: None,
            });
        }
        let (mut cx, view) = Application::new()
            .font(crate::UI_FONT)
            .into_test_context(
                WindowOptions::new("Transform field visibility").size(width, 800.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        let viewport = cx
            .element_bounds(window, "transform-fields-scroll")
            .unwrap();
        for id in [
            "transform-value-0",
            "transform-value-1",
            "transform-value-2",
            "transform-value-3",
            "transform-value-4",
            "transform-value-5",
            "transform-flip-h",
            "transform-flip-v",
        ] {
            let field = cx.element_bounds(window, id).unwrap();
            assert!(
                field.x >= viewport.x && field.right() <= viewport.right(),
                "{id} is clipped at {width}: {field:?}, viewport={viewport:?}"
            );
            assert!(
                field.width >= 40.,
                "{id} must remain usable at {width}: {field:?}"
            );
        }
    }
}

#[test]
fn sampling_picker_is_fully_visible_beside_transform_actions() {
    let mut observations = Vec::new();
    for (width, height) in [(1280., 800.), (800., 594.)] {
        let mut editor = Editor::with_test_document();
        editor.tools.tool = Tool::Move;
        let mut doc = Document::new(32, 32).unwrap();
        compositor::edits::fill(&mut doc, [80, 120, 160, 255], false, false).unwrap();
        editor.tabs = vec![Session::new(doc, None).into()];
        let (mut cx, view) = Application::new()
            .font(crate::UI_FONT)
            .bind_keys(quickgui::select_key_bindings())
            .into_test_context(
                WindowOptions::new("Transform sampling").size(width, height),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        cx.update(view, |e, cx| {
            e.resolve_test_canvas_preview().unwrap();
            cx.invalidate();
        })
        .unwrap();
        let sampling = cx.element_bounds(window, "transform-sampling").unwrap();
        let fields = cx
            .element_bounds(window, "transform-fields-scroll")
            .unwrap();
        let cancel = cx.element_bounds(window, "transform-cancel").unwrap();
        let apply = cx.element_bounds(window, "transform-apply").unwrap();
        if let Ok(prefix) = std::env::var("COMPOSITOR_TRANSFORM_SCREENSHOT") {
            cx.capture_screenshot(window)
                .unwrap()
                .write_png(format!("{prefix}-{}.png", width as u32))
                .unwrap();
        }
        eprintln!(
            "width={width}: sampling={sampling:?}, fields={fields:?}, cancel={cancel:?}, apply={apply:?}"
        );
        observations.push((width, sampling, fields, cancel, apply));
        cx.click(window, "transform-sampling").unwrap();
        cx.simulate_keystrokes(window, "n enter").unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.layers[0].transform.sampling)
                .unwrap(),
            Sampling::Nearest
        );
        cx.click(window, "transform-cancel").unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.layers[0].transform.sampling)
                .unwrap(),
            Sampling::High
        );
        cx.simulate_retained_scroll(
            window,
            "transform-fields-scroll",
            quickgui::Vector::new(-1000., 0.),
        )
        .unwrap();
        assert_eq!(
            cx.element_bounds(window, "transform-sampling").unwrap(),
            sampling
        );
        let flip = cx.element_bounds(window, "transform-flip-v").unwrap();
        assert!(
            flip.x >= fields.x && flip.x + flip.width <= fields.x + fields.width,
            "Scrolled flip controls must remain reachable"
        );
    }
    for (width, sampling, fields, cancel, apply) in observations {
        assert!(
            sampling.x + sampling.width <= cancel.x,
            "Sampling overlaps actions at {width}px"
        );
        assert!(
            sampling.x >= fields.x + fields.width
                || sampling.x + sampling.width <= fields.x + fields.width,
            "Sampling crosses the scrolling fields' clipped edge at {width}px: {sampling:?}, {fields:?}"
        );
        assert!(apply.x + apply.width <= width);
        assert!(
            sampling.width >= 105.,
            "Sampling text and chevron need their full width"
        );
    }
}
