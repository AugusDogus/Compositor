use compositor::{Result, invalid, threshold::Threshold};

pub(in crate::ui) fn fields(settings: Threshold) -> Vec<(&'static str, String)> {
    vec![("Level", settings.level.to_string())]
}
pub(in crate::ui) fn parse(values: &[String]) -> Result<Threshold> {
    let [level] = values else {
        return Err(invalid("Enter one Threshold level from 0 to 255."));
    };
    let level = level
        .trim()
        .parse()
        .map_err(|_| invalid("Threshold level must be a whole number from 0 to 255."))?;
    Ok(Threshold { level })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::*;
    use compositor::{adjustment::ExtendedAdjustment, filters::Filter};
    use quickgui::{Application, WindowOptions};

    #[test]
    fn threshold_levels_roundtrip_and_invalid_input_is_rejected() {
        for level in [0, 1, 128, 254, 255] {
            let filter = Filter::Threshold(Threshold { level });
            let (_, fields) = Editor::filter_fields(filter);
            let values: Vec<_> = fields.into_iter().map(|(_, v)| v).collect();
            assert_eq!(Editor::filter_values(filter, &values).unwrap(), filter);
        }
        for level in ["-1", "256", "127.5", "NaN", "", "128.0"] {
            assert!(parse(&[level.into()]).is_err());
        }
    }
    #[test]
    fn threshold_layer_preview_apply_reopen_cancel_and_undo() {
        let mut e = Editor::with_test_document();
        compositor::edits::fill(
            &mut e.session_mut().document,
            [150, 150, 150, 255],
            false,
            false,
        )
        .unwrap();
        e.open_extended_adjustment(Some(ExtendedAdjustment::Threshold(Threshold::default())))
            .unwrap();
        let created = e.session().document.clone();
        assert_eq!(
            compositor::render::sample(&created, [0.5, 0.5]).unwrap(),
            [1., 1., 1., 1.]
        );
        let (mut cx, view) = Application::new()
            .font(crate::UI_FONT)
            .into_test_context(WindowOptions::new("Threshold").size(1180., 780.), e)
            .unwrap();
        let window = view.window_handle();
        cx.update(view, |e, _| e.update_form_field(0, "200"))
            .unwrap();
        cx.read(view, |e| {
            assert_eq!(
                compositor::render::sample(&e.session().document, [0.5, 0.5]).unwrap(),
                [0., 0., 0., 1.]
            )
        })
        .unwrap();
        cx.click(window, "extended-adjustment-preview").unwrap();
        cx.read(view, |e| assert_eq!(e.session().document, created))
            .unwrap();
        cx.click(window, "extended-adjustment-preview").unwrap();
        cx.click(window, "form-apply").unwrap();
        cx.update(view, |e, _| {
            assert_eq!(e.session().undo_label(), Some("Edit Threshold Adjustment"));
            e.open_adjustment(None).unwrap();
            let Some(Form::Edit { fields, .. }) = &e.modal else {
                panic!("Missing Threshold form")
            };
            assert_eq!(fields[0].1, "200");
            e.cancel_adjustment();
            e.session_mut().undo();
            assert_eq!(e.session().document, created);
        })
        .unwrap();
    }
    #[test]
    fn invalid_threshold_layer_input_preserves_last_preview_and_cannot_apply() {
        let mut e = Editor::with_test_document();
        e.open_extended_adjustment(Some(ExtendedAdjustment::Threshold(Threshold::default())))
            .unwrap();
        let original = e.session().document.clone();
        e.update_form_field(0, "256");
        assert_eq!(e.session().document, original);
        assert!(e.finish_extended_adjustment().is_err());
        assert_eq!(e.session().document, original);
        e.cancel_adjustment();
        assert_eq!(e.session().document, original);
    }
}
