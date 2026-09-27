use compositor::{Result, invalid, posterize::Posterize};

pub(in crate::ui) fn fields(settings: Posterize) -> Vec<(&'static str, String)> {
    vec![("Levels", settings.levels().to_string())]
}
pub(in crate::ui) fn parse(values: &[String]) -> Result<Posterize> {
    let [levels] = values else {
        return Err(invalid("Enter one Posterize level count from 2 to 256."));
    };
    let levels = levels
        .trim()
        .parse()
        .map_err(|_| invalid("Posterize levels must be a whole number from 2 to 256."))?;
    Posterize::new(levels)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::*;
    use compositor::{adjustment::ExtendedAdjustment, filters::Filter};
    use quickgui::{Application, WindowOptions};
    #[test]
    fn posterize_levels_roundtrip_and_invalid_values_are_rejected() {
        for levels in [2, 3, 4, 255, 256] {
            let filter = Filter::Posterize(Posterize::new(levels).unwrap());
            let (_, fields) = Editor::filter_fields(filter);
            let values: Vec<_> = fields.into_iter().map(|(_, v)| v).collect();
            assert_eq!(Editor::filter_values(filter, &values).unwrap(), filter);
        }
        for levels in ["-1", "0", "1", "257", "4.5", "NaN", ""] {
            assert!(parse(&[levels.into()]).is_err());
        }
    }
    #[test]
    fn posterize_layer_preview_apply_reopen_and_undo() {
        let mut e = Editor::with_test_document();
        compositor::edits::fill(
            &mut e.session_mut().document,
            [150, 150, 150, 255],
            false,
            false,
        )
        .unwrap();
        e.open_extended_adjustment(Some(ExtendedAdjustment::Posterize(
            Posterize::new(2).unwrap(),
        )))
        .unwrap();
        let created = e.session().document.clone();
        assert_eq!(
            compositor::render::sample(&created, [0.5, 0.5]).unwrap(),
            [1.; 4]
        );
        let (mut cx, view) = Application::new()
            .font(crate::UI_FONT)
            .into_test_context(WindowOptions::new("Posterize").size(1180., 780.), e)
            .unwrap();
        let window = view.window_handle();
        cx.update(view, |e, _| e.update_form_field(0, "3")).unwrap();
        cx.read(view, |e| {
            assert_eq!(
                compositor::render::sample(&e.session().document, [0.5, 0.5]).unwrap(),
                [0.5, 0.5, 0.5, 1.]
            )
        })
        .unwrap();
        cx.click(window, "extended-adjustment-preview").unwrap();
        cx.read(view, |e| assert_eq!(e.session().document, created))
            .unwrap();
        cx.click(window, "extended-adjustment-preview").unwrap();
        cx.click(window, "form-apply").unwrap();
        cx.update(view, |e, _| {
            assert_eq!(e.session().undo_label(), Some("Edit Posterize Adjustment"));
            e.open_adjustment(None).unwrap();
            let Some(Form::Edit { fields, .. }) = &e.modal else {
                panic!("Missing Posterize form")
            };
            assert_eq!(fields[0].1, "3");
            e.cancel_adjustment();
            e.session_mut().undo();
            assert_eq!(e.session().document, created);
        })
        .unwrap();
    }
}
