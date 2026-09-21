use super::*;
use quickgui::{Application, WindowOptions};

#[test]
fn minimum_window_preserves_content_height_and_scrolls_to_the_palette() {
    let options = crate::window_layout::options(None, &quickgui::Displays::default());
    let minimum = options.minimum_size.unwrap();
    assert_eq!(minimum.width, 800.);
    assert_eq!(minimum.height - 28. - 46., 520.);
    let editor = Editor::with_test_document();
    let original = editor.session().document.clone();
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(
            WindowOptions::new("Minimum editor").size(minimum.width, minimum.height),
            editor,
        )
        .unwrap();
    let window = view.window_handle();
    let rail = cx.element_bounds(window, "tool-rail").unwrap();
    assert_eq!(rail.y, 116.);
    assert_eq!(rail.height, 448.);
    assert!(cx.element_bounds(window, "palette-reset").unwrap().y > rail.y + rail.height);
    assert!(
        cx.simulate_retained_scroll(window, "tool-rail", quickgui::Vector::new(0., -1000.))
            .unwrap()
    );
    for id in [
        quickgui::ElementId::from(320_u64),
        "palette-swap".into(),
        "palette-reset".into(),
    ] {
        let bounds = cx.element_bounds(window, id).unwrap();
        assert!(bounds.y >= rail.y && bounds.y + bounds.height <= rail.y + rail.height);
    }
    let status = cx.element_bounds(window, "status-message").unwrap();
    assert!(status.y >= rail.y + rail.height && status.y + status.height <= minimum.height);
    cx.click(window, 320_u64).unwrap();
    assert!(
        cx.read(view, |e| matches!(e.modal, Some(Form::Color(_))))
            .unwrap()
    );
    cx.update(view, |e, cx| e.cancel_form(cx)).unwrap();
    assert!(
        cx.simulate_retained_scroll(window, "tool-rail", quickgui::Vector::new(0., 1000.))
            .unwrap()
    );
    assert_eq!(
        cx.retained_scroll_offset(window, "tool-rail").unwrap(),
        quickgui::Vector::ZERO
    );
    assert_eq!(
        cx.read(view, |e| e.session().document.clone()).unwrap(),
        original
    );
}

struct SegmentSample;
impl View for SegmentSample {
    fn render(&mut self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(Color::rgb8(25, 25, 25))
            .text_color(Color::WHITE)
            .font_family("Inter Variable")
            .flex_col()
            .items_start()
            .gap(12.)
            .p(12.)
            .child(Editor::segment("Rectangle", true).id("active").w(160.))
            .child(Editor::segment("Ellipse", false).id("inactive").w(120.))
    }
}

#[test]
fn segment_labels_stay_centered_inside_different_control_widths() {
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(
            WindowOptions::new("Segment labels").size(200., 100.),
            SegmentSample,
        )
        .unwrap();
    let window = view.window_handle();
    let frame = cx.capture_screenshot(window).unwrap();
    let scale = frame.width() as f32 / 200.;
    for id in ["active", "inactive"] {
        let bounds = cx.element_bounds(window, id).unwrap();
        let mut ink = [f32::INFINITY, f32::INFINITY, 0_f32, 0_f32];
        for y in (bounds.y * scale) as u32..((bounds.y + bounds.height) * scale) as u32 {
            for x in (bounds.x * scale) as u32..((bounds.x + bounds.width) * scale) as u32 {
                if frame.pixel(x, y).is_some_and(|pixel| pixel[0] > 200) {
                    ink[0] = ink[0].min(x as f32 / scale);
                    ink[1] = ink[1].min(y as f32 / scale);
                    ink[2] = ink[2].max((x + 1) as f32 / scale);
                    ink[3] = ink[3].max((y + 1) as f32 / scale);
                }
            }
        }
        let offset = [
            (ink[0] + ink[2]) / 2. - (bounds.x + bounds.width / 2.),
            (ink[1] + ink[3]) / 2. - (bounds.y + bounds.height / 2.),
        ];
        assert!(
            offset.into_iter().all(|value| value.abs() <= 2.),
            "{id} label must be centered in its segment: offset={offset:?}, ink={ink:?}"
        );
    }
}

#[test]
fn healing_type_group_keeps_all_choices_operable_at_wide_and_narrow_window_sizes() {
    let mut title_width = None;
    for width in [1500., 900.] {
        let mut editor = Editor::with_test_document();
        editor.tools.tool = Tool::Heal;
        let original = editor.session().document.clone();
        let (mut cx, view) = Application::new()
            .font(crate::UI_FONT)
            .into_test_context(WindowOptions::new("Healing type").size(width, 900.), editor)
            .unwrap();
        let window = view.window_handle();
        let title = cx.element_bounds(window, "tool-header-title").unwrap();
        if let Some(expected) = title_width {
            assert_eq!(title.width, expected, "The title must scroll, not collapse");
        } else {
            title_width = Some(title.width);
        }
        let group = cx.element_bounds(window, "healing-type").unwrap();
        assert_eq!(group.width, 330.);
        let frame = cx.capture_screenshot(window).unwrap();
        let scale = frame.width() as f32 / width;
        for (label, mode) in [
            (
                "Create Texture",
                compositor::filters::Healing::CreateTexture,
            ),
            ("Proximity Match", compositor::filters::Healing::Proximity),
            ("Content-Aware", compositor::filters::Healing::ContentAware),
        ] {
            let id = format!("healing-{label}");
            let choice = cx.element_bounds(window, id.clone()).unwrap();
            assert!(choice.x >= group.x);
            assert!(choice.x + choice.width <= group.x + group.width + 0.5);
            let mut ink_rows = Vec::new();
            for y in (choice.y * scale) as u32..((choice.y + choice.height) * scale) as u32 {
                if ((choice.x * scale) as u32..((choice.x + choice.width) * scale) as u32)
                    .any(|x| frame.pixel(x, y).is_some_and(|pixel| pixel[0] > 180))
                {
                    ink_rows.push(y);
                }
            }
            let first = ink_rows.first().expect("Healing label must be visible");
            let last = ink_rows.last().unwrap();
            assert!(
                (last - first + 1) as f32 <= 14. * scale,
                "{label} must remain on one line inside its segment"
            );
            cx.click(window, id).unwrap();
            cx.read(view, |e| {
                assert_eq!(e.tools.healing, mode);
                assert_eq!(e.session().document, original);
                assert!(e.session().undo_label().is_none());
            })
            .unwrap();
        }
    }
}

#[test]
fn tool_rail_and_segments_expose_the_current_choice() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Tool selection semantics").size(1500., 900.),
            Editor::with_test_document(),
        )
        .unwrap();
    let window = view.window_handle();
    for (tool, selected_label, selected_segment) in [
        (Tool::Brush, "Brush (B) · Eraser (E)", "Paint"),
        (Tool::Erase, "Brush (B) · Eraser (E)", "Erase"),
        (
            Tool::Shape,
            "Shape (U) · Shift-U switches Rectangle/Ellipse",
            "Rectangle",
        ),
    ] {
        cx.update(view, |e, cx| e.select_tool(tool, cx)).unwrap();
        let tree = cx.accessibility_update(window).unwrap();
        for expected in [selected_label, selected_segment] {
            assert!(
                tree.nodes.iter().any(|(_, node)| {
                    node.label() == Some(expected) && node.is_selected() == Some(true)
                }),
                "Missing selected control: {expected}"
            );
        }
        assert!(tree.nodes.iter().any(|(_, node)| {
            node.label() == Some("Move / Transform (V)") && node.is_selected() != Some(true)
        }));
    }
}

#[test]
fn held_selection_keys_preview_mode_but_do_not_change_a_started_outline() {
    use compositor::selection::SelectionMode::{Add, Replace, Subtract};
    let mut editor = Editor::with_test_document();
    editor.tools.tool = Tool::Polygon;
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Held selection keys").size(1500., 900.),
            editor,
        )
        .unwrap();
    cx.update(view, |e, cx| {
        for (keys, expected) in [
            (Modifiers::SHIFT, Add),
            (Modifiers::SHIFT | Modifiers::ALT, Subtract),
            (Modifiers::empty(), Replace),
        ] {
            e.event(&Event::ModifiersChanged(keys), cx);
            assert_eq!(e.displayed_selection_mode(), expected);
            assert_eq!(e.tools.selection_mode, Replace);
        }
        e.event(&Event::ModifiersChanged(Modifiers::ALT), cx);
        e.polygon_click([10., 10.], 1., Subtract).unwrap();
        e.event(&Event::ModifiersChanged(Modifiers::SHIFT), cx);
        assert_eq!(e.displayed_selection_mode(), Subtract);
        e.tools.polygon = None;
        assert_eq!(e.displayed_selection_mode(), Add);
        e.event(&Event::Focused(false), cx);
        assert_eq!(e.displayed_selection_mode(), Replace);
    })
    .unwrap();
}

#[test]
fn gradient_header_only_shows_commit_controls_for_a_preview() {
    let mut editor = Editor::with_test_document();
    editor.tools.tool = Tool::Gradient;
    let original = editor.session().document.clone();
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Gradient header").size(1500., 900.),
            editor,
        )
        .unwrap();
    let window = view.window_handle();
    assert!(cx.element_bounds(window, "gradient-apply").is_err());
    assert!(cx.element_bounds(window, "gradient-cancel").is_err());
    assert_eq!(
        cx.element_bounds(window, "gradient-opacity-slider")
            .unwrap()
            .width,
        100.
    );
    assert_eq!(
        cx.element_bounds(window, "gradient-opacity").unwrap().width,
        42.
    );
    cx.click(window, "gradient-style").unwrap();
    cx.simulate_keystrokes(window, "home escape").unwrap();
    assert_eq!(
        cx.read(view, |e| e.tools.gradient.style).unwrap(),
        compositor::gradient::Style::ForegroundToTransparent
    );
    cx.update(view, |e, cx| {
        e.begin_gradient([0., 0.]).unwrap();
        e.changed(cx);
    })
    .unwrap();
    let cancel = cx.element_bounds(window, "gradient-cancel").unwrap();
    let apply = cx.element_bounds(window, "gradient-apply").unwrap();
    assert!(cancel.x < apply.x);
    assert!((apply.x + apply.width - 1482.).abs() < 1.);
    cx.click(window, "gradient-cancel").unwrap();
    assert_eq!(
        cx.read(view, |e| e.session().document.clone()).unwrap(),
        original
    );
}

#[test]
fn wand_sampling_is_inline_and_cancelling_keeps_the_previous_choice() {
    let mut editor = Editor::with_test_document();
    editor.tools.tool = Tool::Wand;
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Wand header").size(1800., 900.), editor)
        .unwrap();
    let window = view.window_handle();
    cx.click(window, "wand-sample").unwrap();
    cx.simulate_keystrokes(window, "down enter").unwrap();
    assert_eq!(cx.read(view, |e| e.tools.wand_radius).unwrap(), 1);
    assert!(cx.read(view, |e| e.modal.is_none()).unwrap());
    cx.click(window, "wand-sample").unwrap();
    cx.simulate_keystrokes(window, "down escape").unwrap();
    assert_eq!(cx.read(view, |e| e.tools.wand_radius).unwrap(), 1);
    cx.focus(window, "wand-tolerance").unwrap();
    cx.simulate_keystrokes(window, "ctrl-a").unwrap();
    cx.simulate_input(window, "300").unwrap();
    assert_eq!(cx.read(view, |e| e.tools.wand_tolerance).unwrap(), 255);
    cx.click(window, "sample-layers-true").unwrap();
    cx.click(window, "wand-contiguous").unwrap();
    cx.read(view, |e| {
        assert!(e.tools.wand_sample_all);
        assert!(!e.tools.wand_contiguous);
        assert!(e.session().undo_label().is_none());
    })
    .unwrap();
}

#[test]
fn selection_header_modifies_directly_and_hides_rectangle_antialias() {
    let mut editor = Editor::with_test_document();
    let mut doc = Document::new(100, 80).unwrap();
    doc.selection = Some(compositor::selection::Selection::rectangle(
        100,
        80,
        [20., 20.],
        [60., 60.],
        false,
    ));
    editor.tabs = vec![Session::new(doc.clone(), None).into()];
    editor.tools.tool = Tool::Rectangle;
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::new("Selection header").size(1500., 900.),
            editor,
        )
        .unwrap();
    let window = view.window_handle();
    assert!(cx.element_bounds(window, "selection-antialias").is_err());
    cx.focus(window, "selection-expand-amount").unwrap();
    cx.simulate_keystrokes(window, "ctrl-a").unwrap();
    cx.simulate_input(window, "3").unwrap();
    cx.click(window, 413_u64).unwrap();
    cx.read(view, |e| {
        assert!(e.modal.is_none());
        assert_eq!(e.tools.selection_expand_amount, 3);
        assert_eq!(
            e.session().document.selection.as_ref().unwrap().bounds(),
            Some([17., 17., 63., 63.])
        );
        assert_eq!(e.session().undo_label(), Some("Expand Selection"));
    })
    .unwrap();
    cx.update(view, |e, cx| e.action(Action::Undo, cx)).unwrap();
    assert_eq!(
        cx.read(view, |e| e.session().document.clone()).unwrap(),
        doc
    );
    cx.click(window, "tool-mode-Ellipse").unwrap();
    assert!(cx.element_bounds(window, "selection-antialias").is_ok());
    cx.click(window, "header-deselect").unwrap();
    assert!(
        cx.read(view, |e| e.session().document.selection.is_none())
            .unwrap()
    );
    assert!(matches!(
        cx.click(window, 413_u64),
        Err(quickgui::TestAppError::NotClickable { .. })
    ));
}

#[test]
fn mask_paint_picker_and_shortcuts_agree_without_changing_the_pixel_palette() {
    let mut editor = Editor::with_test_document();
    editor.tools.tool = Tool::Brush;
    compositor::edits::add_mask(&mut editor.session_mut().document, false).unwrap();
    editor.tools.mask_target = true;
    let original = editor.session().document.clone();
    let colors = editor.palette_colors(false);
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Mask paint").size(1500., 900.), editor)
        .unwrap();
    let window = view.window_handle();
    cx.click(window, "mask-paint").unwrap();
    cx.simulate_keystrokes(window, "down enter").unwrap();
    assert!(cx.read(view, |e| e.tools.mask_paint_white).unwrap());
    cx.focus(window, "workspace").unwrap();
    cx.simulate_keystrokes(window, "x").unwrap();
    assert!(!cx.read(view, |e| e.tools.mask_paint_white).unwrap());
    cx.click(window, "mask-paint").unwrap();
    cx.simulate_keystrokes(window, "enter").unwrap();
    cx.read(view, |e| {
        assert!(!e.tools.mask_paint_white);
        assert_eq!(e.palette_colors(false), colors);
        assert_eq!(e.session().document, original);
    })
    .unwrap();
}

#[test]
fn brush_header_matches_field_metrics_and_opens_foreground_picker() {
    let mut editor = Editor::with_test_document();
    editor.tools.tool = Tool::Brush;
    let original = editor.session().document.clone();
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Brush header").size(1500., 900.), editor)
        .unwrap();
    let window = view.window_handle();
    for (id, width) in [
        ("brush-size", 48.),
        ("brush-hardness", 42.),
        ("brush-opacity", 42.),
        ("brush-hardness-slider", 100.),
        ("brush-opacity-slider", 100.),
    ] {
        assert_eq!(cx.element_bounds(window, id).unwrap().width, width, "{id}");
    }
    cx.click(window, "header-foreground").unwrap();
    assert!(
        cx.read(view, |e| matches!(e.modal, Some(Form::Color(_))))
            .unwrap()
    );
    cx.simulate_keystrokes(window, "escape").unwrap();
    assert_eq!(
        cx.read(view, |e| e.session().document.clone()).unwrap(),
        original
    );
}

#[test]
fn shape_header_has_rectangle_radius_and_fill_without_an_extra_settings_sheet() {
    let mut editor = Editor::with_test_document();
    editor.tools.tool = Tool::Shape;
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Shape header").size(1500., 900.), editor)
        .unwrap();
    let window = view.window_handle();
    assert_eq!(
        cx.element_bounds(window, "shape-radius-slider")
            .unwrap()
            .width,
        100.
    );
    cx.focus(window, "shape-radius").unwrap();
    cx.simulate_keystrokes(window, "ctrl-a").unwrap();
    cx.simulate_input(window, "6000").unwrap();
    assert_eq!(cx.read(view, |e| e.tools.shape_radius).unwrap(), 5000.);
    cx.simulate_keystrokes(window, "enter").unwrap();
    assert_eq!(
        cx.focused(window).unwrap(),
        Some(quickgui::ElementId::from("workspace"))
    );
    cx.click(window, "shape-Ellipse").unwrap();
    assert!(cx.element_bounds(window, "shape-radius").is_err());
    cx.click(window, "header-foreground").unwrap();
    assert!(
        cx.read(view, |e| matches!(e.modal, Some(Form::Color(_))))
            .unwrap()
    );
    cx.simulate_keystrokes(window, "escape").unwrap();
    cx.click(window, "shape-Rectangle").unwrap();
    assert_eq!(cx.read(view, |e| e.tools.shape_radius).unwrap(), 5000.);
    assert!(
        cx.read(view, |e| e.session().undo_label().is_none())
            .unwrap()
    );
}
