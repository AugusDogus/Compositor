use super::*;
use compositor::{
    document::LayerContent,
    selection::{Selection, SelectionMode},
};
use quickgui::{MouseButton, Point, PointerEvent, PointerPhase, Size, Vector};

#[test]
fn wand_queues_without_mutating_pixels_then_commits_the_captured_selection_mode() {
    for mode in [
        SelectionMode::Replace,
        SelectionMode::Add,
        SelectionMode::Subtract,
        SelectionMode::Intersect,
    ] {
        let mut editor = Editor::with_test_document();
        let mut document = Document::new(32, 32).unwrap();
        document.layers[0].content =
            LayerContent::Raster(Some(Arc::new(image::RgbaImage::from_fn(32, 32, |x, _| {
                image::Rgba(if x < 16 {
                    [200, 30, 50, 255]
                } else {
                    [0, 120, 240, 255]
                })
            }))));
        document.selection = Some(Selection::rectangle(32, 32, [8., 0.], [24., 32.], false));
        let settings = compositor::wand::Settings {
            tolerance: 0,
            contiguous: true,
            radius: 0,
            sample_all: false,
        };
        let next = compositor::wand::select(&document, [4., 4.], settings).unwrap();
        let expected = super::canvas::combine(&document.selection, next, mode).unwrap();
        editor.tabs = vec![Session::new(document.clone(), None).into()];
        editor.tools.tool = Tool::Wand;
        editor.tools.selection_mode = mode;
        editor.tools.wand_tolerance = settings.tolerance;
        editor.tools.wand_radius = settings.radius;
        editor.tools.wand_contiguous = settings.contiguous;
        editor.tools.wand_sample_all = settings.sample_all;
        let size = Size::new(600., 600.);
        let (zoom, offset) = editor.viewport(size.width, size.height);
        let point = Point::new(
            (4. * zoom + offset[0]) as f32,
            (4. * zoom + offset[1]) as f32,
        );
        editor
            .pointer(&PointerEvent {
                tablet: None,
                phase: PointerPhase::Down,
                position: point,
                origin: point,
                local_position: point,
                local_origin: point,
                delta: Vector::ZERO,
                button: MouseButton::Left,
                modifiers: Modifiers::empty(),
                size,
            })
            .unwrap();
        assert_eq!(editor.session().document, document);
        assert!(editor.pending);
        assert!(editor.session().undo_label().is_none());
        editor.tools.selection_mode = SelectionMode::Replace;
        let job = editor.job.take().unwrap();
        assert!(matches!(job, jobs::Job::Wand { .. }));
        let completion = job.completion();
        let output = job.run(document.clone());
        editor
            .complete_job(editor.session().id, document.clone(), output, completion)
            .unwrap();
        assert!(!editor.pending);
        assert_eq!(editor.session().document.selection, expected);
        assert_eq!(editor.session().document.layers, document.layers);
        assert_eq!(editor.session().undo_label(), Some("Magic Wand"));
        editor.session_mut().undo();
        assert_eq!(editor.session().document, document);
        editor.session_mut().redo();
        assert_eq!(editor.session().document.selection, expected);
    }
}
