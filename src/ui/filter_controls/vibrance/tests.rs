use super::*;
use crate::ui::*;
use compositor::{adjustment::ExtendedAdjustment, filters::Filter};
use quickgui::{Application, WindowOptions};

#[test]
fn vibrance_fields_roundtrip_and_reject_invalid_values() {
    for (vibrance, saturation) in [(-100., 100.), (47.25, -23.75), (0., 0.)] {
        let filter = Filter::Vibrance(Vibrance::new(vibrance, saturation).unwrap());
        let (_, fields) = Editor::filter_fields(filter);
        let values: Vec<_> = fields.into_iter().map(|(_, v)| v).collect();
        assert_eq!(Editor::filter_values(filter, &values).unwrap(), filter);
    }
    for value in ["-101", "101", "NaN", "inf", ""] {
        assert!(parse(&[value.into(), "0".into()]).is_err());
        assert!(parse(&["0".into(), value.into()]).is_err());
    }
    assert!(parse(&[]).is_err());
}

#[test]
fn vibrance_layer_preview_cancel_apply_reopen_and_undo() {
    let mut e = Editor::with_test_document();
    compositor::edits::fill(
        &mut e.session_mut().document,
        [160, 100, 80, 255],
        false,
        false,
    )
    .unwrap();
    e.open_extended_adjustment(Some(ExtendedAdjustment::Vibrance(Vibrance::default())))
        .unwrap();
    let created = e.session().document.clone();
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(WindowOptions::new("Vibrance").size(1180., 780.), e)
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
    cx.update(view, |e, _| e.update_form_field(0, "47.25"))
        .unwrap();
    cx.update(view, |e, _| e.update_form_field(1, "-23.75"))
        .unwrap();
    cx.click(window, "form-apply").unwrap();
    cx.update(view, |e, _| {
        assert_eq!(e.session().undo_label(), Some("Edit Vibrance Adjustment"));
        e.open_adjustment(None).unwrap();
        let Some(Form::Edit { fields, .. }) = &e.modal else {
            panic!("Missing Vibrance form")
        };
        assert_eq!(fields[0].1, "47.25");
        assert_eq!(fields[1].1, "-23.75");
        e.cancel_adjustment();
        e.session_mut().undo();
        assert_eq!(e.session().document, created);
    })
    .unwrap();
}
