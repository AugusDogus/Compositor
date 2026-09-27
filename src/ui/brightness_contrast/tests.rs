use super::*;
use compositor::document::LayerContent;
use quickgui::{Application, WindowOptions};

#[test]
fn dedicated_editor_previews_applies_reopens_and_undoes_native_levels() {
    let mut editor = Editor::with_test_document();
    editor.open_brightness_contrast(true).unwrap();
    let created = editor.session().document.clone();
    editor.update_form_field(0, "45.5");
    editor.update_form_field(1, "-12");
    let expected = BrightnessContrast {
        brightness: 45.5,
        contrast: -12.,
    };
    assert_eq!(
        editor
            .adjustment_edit
            .as_ref()
            .unwrap()
            .settings
            .brightness_contrast(),
        Some(expected)
    );
    editor.adjustment_edit.as_mut().unwrap().preview = false;
    editor.preview_adjustment().unwrap();
    assert_eq!(editor.session().document, created);
    editor.finish_adjustment().unwrap();
    let committed = editor.session().document.clone();
    editor.open_adjustment(None).unwrap();
    assert!(
        matches!(&editor.modal, Some(Form::Edit { title: "Brightness/Contrast", fields, .. }) if fields.len() == 2)
    );
    editor.cancel_adjustment();
    editor.session_mut().undo();
    assert_eq!(editor.session().document, created);
    editor.session_mut().redo();
    assert_eq!(editor.session().document, committed);
    assert!(
        matches!(&committed.active_layer().unwrap().content, LayerContent::Adjustment(a) if a.kind == Kind::Levels)
    );
}

#[test]
fn ordinary_or_stale_levels_keep_native_controls_regardless_of_layer_name() {
    let mut editor = Editor::with_test_document();
    editor.open_adjustment(Some(Kind::Levels)).unwrap();
    assert!(!editor.editing_brightness_contrast());
    editor.cancel_adjustment();
    editor.open_brightness_contrast(true).unwrap();
    editor.cancel_adjustment();
    if let LayerContent::Adjustment(settings) = &mut editor
        .session_mut()
        .document
        .layers
        .last_mut()
        .unwrap()
        .content
    {
        settings.levels.ranges[0].gamma = 1.2;
    }
    editor.open_adjustment(None).unwrap();
    assert!(!editor.editing_brightness_contrast());
    assert!(
        matches!(&editor.modal, Some(Form::Edit { title: "Levels", fields, .. }) if fields.len() == 5)
    );
    editor.update_form_field(1, "1");
    assert!(!editor.editing_brightness_contrast());
}

#[test]
fn brightness_controls_mount_without_levels_controls_and_reset_keeps_editor() {
    let mut editor = Editor::with_test_document();
    editor.open_brightness_contrast(true).unwrap();
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(
            WindowOptions::new("Brightness/Contrast").size(1280., 900.),
            editor,
        )
        .unwrap();
    let window = view.window_handle();
    assert!(cx.element_bounds(window, "parameter-0").is_ok());
    assert!(cx.element_bounds(window, "parameter-1").is_ok());
    assert!(cx.element_bounds(window, "adjustment-channel").is_err());
    cx.update(view, |editor, _| editor.update_form_field(0, "75"))
        .unwrap();
    cx.click(window, "adjustment-reset").unwrap();
    cx.read(view, |editor| {
        assert_eq!(
            editor
                .adjustment_edit
                .as_ref()
                .unwrap()
                .settings
                .brightness_contrast(),
            Some(BrightnessContrast::default())
        );
    })
    .unwrap();
    cx.click(window, "form-cancel").unwrap();
}

#[test]
fn pixel_adjustment_cancel_identity_and_selection_keep_sources_intact() {
    let mut editor = Editor::with_test_document();
    editor.tabs = vec![Session::new(Document::new(4, 2).unwrap(), None).into()];
    compositor::edits::fill(
        &mut editor.session_mut().document,
        [60, 110, 170, 255],
        false,
        false,
    )
    .unwrap();
    editor.session_mut().document.selection = Some(compositor::selection::Selection::rectangle(
        4,
        2,
        [0., 0.],
        [2., 2.],
        false,
    ));
    let original = editor.session().document.clone();
    editor.open_brightness_contrast(false).unwrap();
    editor.finish_adjustment().unwrap();
    assert_eq!(editor.session().document, original);
    editor.open_brightness_contrast(false).unwrap();
    editor.update_form_field(0, "50");
    assert_ne!(editor.session().document, original);
    let pixels = editor
        .session()
        .document
        .active_layer()
        .unwrap()
        .raster()
        .unwrap();
    assert_ne!(pixels[(0, 0)].0, [60, 110, 170, 255]);
    assert_eq!(pixels[(3, 0)].0, [60, 110, 170, 255]);
    editor.cancel_adjustment();
    assert_eq!(editor.session().document, original);
}

#[test]
fn recovery_store_restores_editor_hints() {
    let mut editor = Editor::with_test_document();
    editor.open_brightness_contrast(true).unwrap();
    editor.update_form_field(1, "35");
    editor.finish_adjustment().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let startup = super::super::recovery_store::Store::initialize(temp.path()).unwrap();
    startup
        .store
        .write(
            editor.session().id,
            "Brightness document",
            &editor.session().document,
        )
        .unwrap();
    drop(startup);
    let recovered = super::super::recovery_store::Store::initialize(temp.path()).unwrap();
    assert!(recovered.warnings.is_empty());
    assert_eq!(recovered.recovered.len(), 1);
    let LayerContent::Adjustment(settings) = &recovered.recovered[0]
        .document
        .active_layer()
        .unwrap()
        .content
    else {
        panic!("adjustment missing");
    };
    assert_eq!(
        settings.brightness_contrast(),
        Some(BrightnessContrast {
            brightness: 0.,
            contrast: 35.
        })
    );
}
