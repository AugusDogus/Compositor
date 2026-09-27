use super::*;
use compositor::{adjustment::ExtendedAdjustment, document::LayerContent, filters::Filter};
use quickgui::{Application, WindowOptions};

#[test]
fn all_range_values_round_trip_and_invalid_hidden_values_are_rejected() {
    let mut settings = SelectiveColor {
        mode: Mode::Absolute,
        ..Default::default()
    };
    settings.adjustments[Range::Blacks.index()] = [12.5, -75., 38., 0.];
    let filter = Filter::SelectiveColor(settings);
    let (_, fields) = Editor::filter_fields(filter);
    let mut values: Vec<_> = fields.into_iter().map(|(_, value)| value).collect();
    assert_eq!(Editor::filter_values(filter, &values).unwrap(), filter);
    for (index, invalid) in [
        (0, "101"),
        (35, "NaN"),
        (14, "-101"),
        (MODE, "other"),
        (RANGE, "other"),
    ] {
        let old = std::mem::replace(&mut values[index], invalid.into());
        assert!(Editor::filter_values(filter, &values).is_err());
        values[index] = old;
    }
}

#[test]
fn nine_ranges_expose_all_four_controls_and_cancel_preserves_pixels() {
    let mut e = Editor::with_test_document();
    compositor::edits::fill(
        &mut e.session_mut().document,
        [150, 100, 50, 255],
        false,
        false,
    )
    .unwrap();
    let original = e.session().document.clone();
    e.open_filter(Filter::SelectiveColor(Default::default()))
        .unwrap();
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(WindowOptions::new("Selective Color").size(1180., 780.), e)
        .unwrap();
    let window = view.window_handle();
    for range in Range::ALL {
        cx.click(window, format!("form-choice-{RANGE}-{}", range.name()))
            .unwrap();
        for index in range.index() * 4..range.index() * 4 + 4 {
            let bounds = cx
                .element_bounds(window, format!("parameter-{index}"))
                .unwrap();
            assert!(
                bounds.y >= 0. && bounds.y + bounds.height <= 780.,
                "Control {index} is clipped: {bounds:?}"
            );
        }
        cx.update(view, |e, _| e.update_form_field(range.index() * 4, "25"))
            .unwrap();
    }
    cx.click(window, "form-choice-36-absolute").unwrap();
    cx.read(view, |e| {
        let Some(Form::Edit { fields, .. }) = &e.modal else {
            panic!("Missing Selective Color editor")
        };
        let settings = parse(
            &fields
                .iter()
                .map(|(_, value)| value.clone())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert_eq!(settings.mode, Mode::Absolute);
        assert!(settings.adjustments.iter().all(|row| row[0] == 25.));
    })
    .unwrap();
    if let Some(path) = std::env::var_os("COMPOSITOR_SELECTIVE_COLOR_SCREENSHOT") {
        cx.capture_screenshot(window)
            .unwrap()
            .write_png(path)
            .unwrap();
    }
    cx.click(window, "form-cancel").unwrap();
    cx.read(view, |e| {
        assert_eq!(e.session().document, original);
        assert!(e.session().undo_label().is_none());
    })
    .unwrap();
}

#[test]
fn editable_selective_color_previews_applies_and_undoes_settings() {
    let mut e = Editor::with_test_document();
    compositor::edits::fill(
        &mut e.session_mut().document,
        [180, 90, 60, 255],
        false,
        false,
    )
    .unwrap();
    e.open_extended_adjustment(Some(ExtendedAdjustment::SelectiveColor(Default::default())))
        .unwrap();
    let created = e.session().document.clone();
    let before = compositor::render::sample(&created, [0.5, 0.5]).unwrap();
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(
            WindowOptions::new("Selective Color Layer").size(1180., 780.),
            e,
        )
        .unwrap();
    let window = view.window_handle();
    cx.update(view, |e, _| e.update_form_field(0, "60"))
        .unwrap();
    cx.read(view, |e| {
        assert_ne!(
            compositor::render::sample(&e.session().document, [0.5, 0.5]).unwrap(),
            before
        )
    })
    .unwrap();
    cx.click(window, "extended-adjustment-preview").unwrap();
    cx.read(view, |e| assert_eq!(e.session().document, created))
        .unwrap();
    cx.click(window, "extended-adjustment-preview").unwrap();
    cx.click(window, "form-apply").unwrap();
    cx.update(view, |e, _| {
        assert!(matches!(&e.session().document.active_layer().unwrap().content,
            LayerContent::ExtendedAdjustment(a) if matches!(**a, ExtendedAdjustment::SelectiveColor(s) if s.adjustments[0][0] == 60.)));
        assert_eq!(e.session().undo_label(), Some("Edit Selective Color Adjustment"));
        e.open_adjustment(None).unwrap();
        let Some(Form::Edit {fields,..}) = &e.modal else {panic!("Missing reopened settings")};
        assert_eq!(fields[0].1, "60");
        e.cancel_adjustment();
        e.session_mut().undo();
        assert_eq!(e.session().document, created);
    }).unwrap();
}
