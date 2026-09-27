use super::*;
use quickgui::{Application, WindowOptions};

fn fixture() -> Tip {
    compositor::brush::sampled::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gbr/pixel.gbr"),
    )
    .unwrap()
}

#[test]
fn imported_tip_is_selectable_and_spacing_is_validated_without_document_history() {
    let mut editor = Editor::with_test_document();
    editor.tools.tool = Tool::Brush;
    let original = editor.session().document.clone();
    editor.install_brush_tip(fixture()).unwrap();
    editor.install_brush_tip(fixture()).unwrap();
    assert_eq!(editor.brush_presets.tips.len(), 1);
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("Brush Tips").size(1500., 900.), editor)
        .unwrap();
    let window = view.window_handle();
    cx.focus(window, "brush-tip-spacing").unwrap();
    cx.simulate_keystrokes(window, "ctrl-a").unwrap();
    cx.simulate_input(window, "75").unwrap();
    cx.read(view, |e| {
        let Shape::Sampled(brush) = &e.tools.brush_shape else {
            panic!("sampled tip")
        };
        assert_eq!(brush.spacing(), 0.75);
    })
    .unwrap();
    cx.simulate_keystrokes(window, "ctrl-a").unwrap();
    cx.simulate_input(window, "NaN").unwrap();
    cx.read(view, |e| {
        let Shape::Sampled(brush) = &e.tools.brush_shape else {
            panic!("sampled tip")
        };
        assert_eq!(brush.spacing(), 0.75);
    })
    .unwrap();
    cx.click(window, "brush-tip-round").unwrap();
    cx.read(view, |e| {
        assert!(matches!(e.tools.brush_shape, Shape::Round))
    })
    .unwrap();
    cx.click(window, "brush-tip-0").unwrap();
    cx.click(window, "brush-tips-close").unwrap();
    assert!(cx.element_bounds(window, "brush-hardness-slider").is_err());
    cx.click(window, "brush-tip-picker").unwrap();
    cx.read(view, |e| {
        assert!(matches!(e.modal, Some(Form::BrushTips(_))));
        assert_eq!(e.session().document, original);
        assert!(e.session().undo_label().is_none());
    })
    .unwrap();
}

#[test]
fn selected_sampled_tip_paints_through_canvas_and_undo_restores_document() {
    use quickgui::{MouseButton, Point, PointerEvent, PointerPhase, Size, Vector};
    let mut editor = Editor::with_test_document();
    editor.tabs = vec![Session::new(Document::new(64, 64).unwrap(), None).into()];
    editor.session_mut().fit = false;
    editor.session_mut().zoom = 1.;
    editor.tools.tool = Tool::Brush;
    editor.tools.brush.diameter = 20.;
    editor.tools.brush.color = [180, 20, 90, 255];
    editor.install_brush_tip(fixture()).unwrap();
    editor.modal = None;
    let before = editor.session().document.clone();
    for phase in [PointerPhase::Down, PointerPhase::Up] {
        editor
            .pointer(&PointerEvent {
                tablet: None,
                phase,
                position: Point::new(32., 32.),
                origin: Point::new(32., 32.),
                local_position: Point::new(32., 32.),
                local_origin: Point::new(32., 32.),
                delta: Vector::ZERO,
                button: MouseButton::Left,
                modifiers: Modifiers::empty(),
                size: Size::new(64., 64.),
            })
            .unwrap();
    }
    let after = editor.session().document.clone();
    assert_eq!(
        after.layers[0].raster().unwrap()[(24, 24)].0,
        [180, 20, 90, 255]
    );
    assert_eq!(editor.session().undo_label(), Some("Brush Stroke"));
    editor.session_mut().undo();
    assert_eq!(editor.session().document, before);
    editor.session_mut().redo();
    assert_eq!(editor.session().document, after);
}

#[test]
fn real_bristles_picker_keeps_thumbnails_inside_rows() {
    let mut editor = Editor::with_test_document();
    editor.tools.tool = Tool::Brush;
    editor.install_brush_tip(fixture()).unwrap();
    editor
        .install_brush_tip(
            compositor::brush::sampled::read(
                &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/gbr/bristles-01.gbr"),
            )
            .unwrap(),
        )
        .unwrap();
    let (mut cx, view) = Application::new()
        .font(crate::UI_FONT)
        .into_test_context(WindowOptions::new("Brush Tips").size(1500., 900.), editor)
        .unwrap();
    for index in 0..2 {
        let row = cx
            .element_bounds(view.window_handle(), format!("brush-tip-{index}"))
            .unwrap();
        let thumbnail = cx
            .element_bounds(view.window_handle(), format!("brush-tip-thumbnail-{index}"))
            .unwrap();
        assert!(thumbnail.y >= row.y && thumbnail.y + thumbnail.height <= row.y + row.height);
        assert!(thumbnail.x >= row.x && thumbnail.x + thumbnail.width <= row.x + row.width);
    }
}

fn abr_pack() -> compositor::brush::sampled::abr::Pack {
    compositor::brush::sampled::abr::Pack::from_bytes(
        include_bytes!("../../../tests/fixtures/abr/sampled-v2.abr").to_vec(),
    )
    .unwrap()
}
#[test]
fn abr_picker_imports_only_selected_tips_through_file_job_and_keeps_history_clean() {
    let mut editor = Editor::with_test_document();
    editor.open_brush_pack(abr_pack());
    let original = editor.session().document.clone();
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("ABR import").size(1500., 900.), editor)
        .unwrap();
    let window = view.window_handle();
    cx.click(window, "abr-tip-1").unwrap();
    assert!(cx.element_bounds(window, "abr-import").is_ok());
    let job = cx
        .update(view, |e, _| {
            e.import_abr_selection();
            e.file_job.take().unwrap()
        })
        .unwrap();
    let result = job.run(&[]).unwrap();
    cx.update(view, |e, cx| {
        e.pending = false;
        let super::super::file_jobs::Completed::BrushTips(tips) = result else {
            panic!("brush tip import");
        };
        e.install_brush_tips(tips).unwrap();
        cx.invalidate();
    })
    .unwrap();
    cx.read(view, |e| {
        assert_eq!(e.brush_presets.tips.len(), 1);
        assert_eq!(e.brush_presets.tips[0].brush.name(), "Asymmetric dots");
        assert_eq!(e.brush_presets.tips[0].brush.spacing(), 0.75);
        assert_eq!(e.session().document, original);
        assert!(e.session().undo_label().is_none());
    })
    .unwrap();
    cx.click(window, "brush-tip-unload").unwrap();
    cx.read(view, |e| {
        assert!(e.brush_presets.tips.is_empty());
        assert!(matches!(e.tools.brush_shape, Shape::Round));
    })
    .unwrap();
}
#[test]
fn capacity_failure_is_atomic_and_unload_releases_parked_project_references() {
    let mut e = Editor::with_test_document();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tip.gbr");
    for alpha in 0..31 {
        let mut bytes = include_bytes!("../../../tests/fixtures/gbr/pixel.gbr").to_vec();
        *bytes.last_mut().unwrap() = alpha;
        std::fs::write(&path, bytes).unwrap();
        e.install_brush_tip(compositor::brush::sampled::read(&path).unwrap())
            .unwrap();
    }
    let selected = e.tools.brush_shape.clone();
    e.tabs[0].parked_tools.brush_shape = selected;
    let original = e.session().document.clone();
    assert!(
        e.install_brush_tips(abr_pack().decode(&[0, 1]).unwrap())
            .is_err()
    );
    assert_eq!(
        e.brush_presets.tips.len(),
        31,
        "failed pack install cannot retain its first tip"
    );
    e.unload_brush_tip();
    assert_eq!(e.brush_presets.tips.len(), 30);
    assert!(matches!(e.tabs[0].parked_tools.brush_shape, Shape::Round));
    e.install_brush_tips(abr_pack().decode(&[0, 1]).unwrap())
        .unwrap();
    assert_eq!(e.brush_presets.tips.len(), 32);
    assert_eq!(e.session().document, original);
    assert!(e.session().undo_label().is_none());
}

#[test]
fn gih_file_import_keeps_cells_together_and_unloads_all_project_references() {
    use super::super::file_jobs::{Completed, FileJob};
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gih/fine-grain.gih");
    let mut editor = Editor::with_test_document();
    let before = editor.session().document.clone();
    for _ in 0..2 {
        let Completed::BrushHose(hose) = FileJob::BrushTip(path.clone()).run(&[]).unwrap() else {
            panic!("GIH file job must preserve the hose");
        };
        editor.install_brush_hose(hose).unwrap();
    }
    assert_eq!(editor.brush_presets.tips.len(), 1);
    assert_eq!(editor.brush_presets.pixels(), 3 * 256 * 256);
    assert_eq!(editor.brush_presets.tips[0].label(), "Fine Grain · 3 cells");
    let selected = editor.tools.brush_shape.clone();
    editor.tabs[0].parked_tools.brush_shape = selected.clone();
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::new("GIH Brushes").size(1500., 900.), editor)
        .unwrap();
    let window = view.window_handle();
    cx.click(window, "brush-tip-round").unwrap();
    cx.click(window, "brush-tip-0").unwrap();
    cx.read(view, |e| {
        let (Shape::Sampled(a), Shape::Sampled(b)) = (&e.tools.brush_shape, &selected) else {
            panic!("hose selection");
        };
        assert!(a.same_source(b));
        assert_eq!(a.hose().unwrap().cells().len(), 3);
        assert_eq!(a.spacing(), 0.2);
    })
    .unwrap();

    cx.click(window, "brush-tip-unload").unwrap();
    cx.read(view, |e| {
        assert!(e.brush_presets.tips.is_empty());
        assert!(matches!(e.tools.brush_shape, Shape::Round));
        assert!(matches!(e.tabs[0].parked_tools.brush_shape, Shape::Round));
        assert_eq!(e.session().document, before);
        assert!(e.session().undo_label().is_none());
    })
    .unwrap();
}

#[test]
fn gih_library_budget_counts_every_cell_and_keeps_failed_import_atomic() {
    use compositor::brush::sampled::gih::Hose;
    let mut editor = Editor::with_test_document();
    editor.install_brush_tip(fixture()).unwrap();
    for i in 0..4 {
        let mut bytes = format!("Large {i}\n4 dim:1 rank0:4 sel0:incremental\n").into_bytes();
        for _ in 0..4 {
            for value in [30u32, 2, 1024, 1024, 1, 0x47494d50, 20] {
                bytes.extend_from_slice(&value.to_be_bytes());
            }
            bytes.extend_from_slice(b"x\0");
            bytes.resize(bytes.len() + 1024 * 1024, 255);
        }
        let result = editor.install_brush_hose(Hose::from_bytes(&bytes).unwrap());
        if i < 3 {
            result.unwrap();
        } else {
            assert!(result.is_err());
        }
    }
    assert_eq!(editor.brush_presets.tips.len(), 4);
    assert_eq!(editor.brush_presets.pixels(), 12 * 1024 * 1024 + 1);
    let Shape::Sampled(selected) = &editor.tools.brush_shape else {
        panic!("previous brush");
    };
    assert_eq!(selected.name(), "Large 2");
}
