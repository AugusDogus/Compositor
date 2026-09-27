use super::*;
use compositor::{adjustment::ChannelMixer, filters::Filter};
#[test]
fn channel_mixer_fields_round_trip_and_validate_hidden_rows() {
    let filter = Filter::ChannelMixer(ChannelMixer::default());
    let (_, fields) = Editor::filter_fields(filter);
    let mut values: Vec<_> = fields.into_iter().map(|(_, value)| value).collect();
    assert_eq!(Editor::filter_values(filter, &values).unwrap(), filter);
    for (index, invalid) in [
        (0, "201"),
        (5, "NaN"),
        (11, "-201"),
        (12, "2"),
        (13, "cyan"),
    ] {
        let original = std::mem::replace(&mut values[index], invalid.into());
        assert!(Editor::filter_values(filter, &values).is_err());
        values[index] = original;
    }
}
#[test]
fn channel_mixer_output_controls_retain_channels_across_monochrome_and_cancel() {
    use quickgui::{Application, WindowOptions};
    let mut editor = Editor::with_test_document();
    compositor::edits::fill(
        &mut editor.session_mut().document,
        [100, 150, 200, 255],
        false,
        false,
    )
    .unwrap();
    let original = editor.session().document.clone();
    editor
        .open_filter(Filter::ChannelMixer(Default::default()))
        .unwrap();
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(
            WindowOptions::new("Channel Mixer").size(1280., 900.),
            editor,
        )
        .unwrap();
    let window = view.window_handle();
    cx.click(window, "form-choice-13-green").unwrap();
    assert!(cx.element_bounds(window, "parameter-4").is_ok());
    assert!(cx.element_bounds(window, "parameter-0").is_err());
    cx.update(view, |e, _| e.update_form_field(4, "25"))
        .unwrap();
    cx.click(window, 50_012_u64).unwrap();
    assert!(cx.element_bounds(window, "parameter-0").is_ok());
    assert!(cx.element_bounds(window, "parameter-4").is_err());
    cx.click(window, 50_012_u64).unwrap();
    cx.read(view, |e| assert!(matches!(&e.modal,Some(Form::Edit{fields,..}) if fields[4].1=="25" && fields[13].1=="green"))).unwrap();
    cx.click(window, "form-cancel").unwrap();
    cx.read(view, |e| {
        assert_eq!(e.session().document, original);
        assert!(e.session().undo_label().is_none());
    })
    .unwrap();
}
