use super::*;
use crate::ui::*;
use compositor::{adjustment::ExtendedAdjustment, filters::Filter};
use quickgui::{Application, WindowOptions};

#[test]
fn shadows_highlights_fields_roundtrip_and_reject_invalid_values() {
    for settings in [
        Settings::default(),
        Settings::new(12.25, 98.5, 500.).unwrap(),
    ] {
        let filter = Filter::ShadowsHighlights(settings);
        let (_, fields) = Editor::filter_fields(filter);
        let values: Vec<_> = fields.into_iter().map(|(_, v)| v).collect();
        assert_eq!(Editor::filter_values(filter, &values).unwrap(), filter);
    }
    for (field, values) in [
        (0, ["-1", "101", "NaN", "inf"]),
        (1, ["-1", "101", "NaN", ""]),
        (2, ["0", "501", "NaN", "inf"]),
    ] {
        for value in values {
            let mut values = vec!["35".into(), "0".into(), "30".into()];
            values[field] = value.into();
            assert!(parse(&values).is_err());
        }
    }
    assert!(parse(&[]).is_err());
}

#[test]
fn shadows_highlights_layer_preview_cancel_apply_reopen_and_undo() {
    let mut e = Editor::with_test_document();
    compositor::edits::fill(
        &mut e.session_mut().document,
        [50, 70, 90, 255],
        false,
        false,
    )
    .unwrap();
    e.open_extended_adjustment(Some(ExtendedAdjustment::ShadowsHighlights(
        Settings::new(0., 0., 3.).unwrap(),
    )))
    .unwrap();
    let created = e.session().document.clone();
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(
            WindowOptions::new("Shadows/Highlights").size(1180., 780.),
            e,
        )
        .unwrap();
    let window = view.window_handle();
    cx.update(view, |e, _| e.update_form_field(0, "80"))
        .unwrap();
    cx.read(view, |e| {
        assert_ne!(
            compositor::render::sample(&e.session().document, [0.5, 0.5]).unwrap(),
            compositor::render::sample(&created, [0.5, 0.5]).unwrap()
        )
    })
    .unwrap();
    cx.click(window, "extended-adjustment-preview").unwrap();
    cx.read(view, |e| assert_eq!(e.session().document, created))
        .unwrap();
    cx.click(window, "extended-adjustment-preview").unwrap();
    cx.update(view, |e, _| e.cancel_adjustment()).unwrap();
    cx.read(view, |e| assert_eq!(e.session().document, created))
        .unwrap();
    cx.update(view, |e, _| e.open_adjustment(None).unwrap())
        .unwrap();
    for (index, value) in ["47.25", "23.75", "12.5"].into_iter().enumerate() {
        cx.update(view, |e, _| e.update_form_field(index, value))
            .unwrap();
    }
    cx.click(window, "form-apply").unwrap();
    cx.update(view, |e, _| {
        assert_eq!(
            e.session().undo_label(),
            Some("Edit Shadows/Highlights Adjustment")
        );
        e.open_adjustment(None).unwrap();
        let Some(Form::Edit { fields, .. }) = &e.modal else {
            panic!("Missing form")
        };
        assert_eq!(fields[0].1, "47.25");
        assert_eq!(fields[1].1, "23.75");
        assert_eq!(fields[2].1, "12.5");
        e.cancel_adjustment();
        e.session_mut().undo();
        assert_eq!(e.session().document, created);
    })
    .unwrap();
}
