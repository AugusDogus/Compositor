//! Opt-in pointer-handler profiling. This does not measure GPU presentation or model inference.
use super::*;
use compositor::document::LayerContent;
use image::{Rgba, RgbaImage};
use quickgui::{Application, MouseButton, PointerEvent, PointerPhase, Size, Vector, WindowOptions};
use std::{sync::Arc, time::Instant};

fn editor(width: u32, height: u32, tool: Tool) -> Editor {
    let mut e = Editor::with_test_document();
    let mut document = Document::new(width, height).unwrap();
    // Colored edges and fine detail make blur, heal, clone and warp perform real work.
    document.layers[0].content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(width, height, |x, y| {
            Rgba([
                40 + (x % 151) as u8,
                30 + (y % 173) as u8,
                if (x / 37 + y / 29) % 2 == 0 { 50 } else { 200 },
                255,
            ])
        }))));
    e.tabs = vec![Session::new(document, None).into()];
    e.session_mut().fit = false;
    e.session_mut().zoom = 1.;
    e.tools.tool = tool;
    e.tools.brush.diameter = 520.;
    e.tools.brush.hardness = 1.;
    e.tools.brush.color = [240, 10, 20, 255];
    e.tools.clone_source = Some([100., 100.]);
    e.tools.show_transform_controls = false;
    e
}

fn pointer(e: &mut Editor, phase: PointerPhase, point: [f64; 2]) -> f64 {
    let size = Size::new(1280., 720.);
    let (zoom, offset) = e.viewport(size.width, size.height);
    let point = quickgui::Point::new(
        (point[0] * zoom + offset[0]) as f32,
        (point[1] * zoom + offset[1]) as f32,
    );
    let start = Instant::now();
    e.pointer(&PointerEvent {
        tablet: None,
        phase,
        position: point,
        origin: point,
        local_position: point,
        local_origin: point,
        delta: Vector::new(10., 10.),
        button: MouseButton::Left,
        modifiers: Modifiers::empty(),
        size,
    })
    .unwrap();
    start.elapsed().as_secs_f64() * 1000.
}

/// Run serially in release mode with --ignored --nocapture; timings are observations,
/// not CI thresholds. A short stroke bounds work even when an event is expensive.
#[test]
#[ignore = "release-mode screenshot-size tool responsiveness audit"]
fn screenshot_size_pointer_handlers() {
    if cfg!(debug_assertions) {
        panic!("Run this profiler with --release");
    }
    // The native launcher performs these initializations in its startup worker.
    // Report startup separately so first-tool timing represents a ready window.
    let clock = Instant::now();
    let brush = compositor::brush::initialize_gpu();
    let rendering = compositor::render::initialize_gpu();
    eprintln!(
        "Graphics warmup: {:.2} ms; brush={brush:?}, renderer={rendering:?}",
        clock.elapsed().as_secs_f64() * 1000.
    );
    for (width, height) in [(1920, 1080), (3840, 2160)] {
        for (tool, _) in Tool::ALL {
            if tool == Tool::Gradient {
                continue;
            }
            let (mut cx, view) = Application::new()
                .into_test_context(
                    WindowOptions::new("Tool responsiveness").size(1500., 900.),
                    editor(width, height, tool),
                )
                .unwrap();
            cx.update(view, |e, _| {
                let original = e.session().document.clone();
                let color = e.tools.brush.color;
                let start = [width as f64 * 0.45, height as f64 * 0.45];
                let end = [start[0] + 40., start[1] + 30.];
                let down = pointer(e, PointerPhase::Down, start);
                let mut moves = Vec::new();
                let up;
                let mut apply = 0.;
                let mut worker = 0.;
                if tool == Tool::Polygon {
                    pointer(e, PointerPhase::Up, start);
                    moves.push(pointer(e, PointerPhase::Down, [end[0], start[1]]));
                    pointer(e, PointerPhase::Up, [end[0], start[1]]);
                    moves.push(pointer(e, PointerPhase::Down, end));
                    up = pointer(e, PointerPhase::Up, end);
                    let clock = Instant::now();
                    e.commit_polygon().unwrap();
                    apply = clock.elapsed().as_secs_f64() * 1000.;
                } else {
                    for point in [
                        [start[0] + 10., start[1]],
                        [start[0] + 20., start[1] + 5.],
                        [end[0], start[1] + 10.],
                        end,
                    ] {
                        moves.push(pointer(e, PointerPhase::Move, point));
                    }
                    up = pointer(e, PointerPhase::Up, end);
                }
                if tool == Tool::Heal {
                    let job = e.job.take().expect("Healing finishes on a worker");
                    let initial = e.job_source(&job).unwrap();
                    let completion = job.completion();
                    let clock = Instant::now();
                    let result = job.run(initial.clone());
                    worker = clock.elapsed().as_secs_f64() * 1000.;
                    e.complete_job(e.tabs[e.current].id, initial, result, completion).unwrap();
                }
                match tool {
                    Tool::Rectangle | Tool::Ellipse | Tool::Lasso | Tool::Polygon => {
                        assert!(e.session().document.selection.as_ref().unwrap().bounds().is_some());
                        assert!(e.session().undo_label().is_some());
                    }
                    Tool::Wand => {
                        assert_eq!(e.session().document, original);
                        let job = e.job.take().expect("Wand queues work instead of blocking the pointer");
                        assert!(matches!(job, jobs::Job::Wand { .. }));
                        let completion = job.completion();
                        let clock = Instant::now();
                        let result = job.run(original.clone());
                        worker = clock.elapsed().as_secs_f64() * 1000.;
                        e.complete_job(e.tabs[e.current].id, original.clone(), result, completion).unwrap();
                        assert!(e.session().document.selection.as_ref().unwrap().bounds().is_some());
                        assert!(e.session().undo_label().is_some());
                    }
                    Tool::Object => {
                        assert!(matches!(e.job.take(), Some(jobs::Job::SelectForeground(_))));
                        assert_eq!(e.session().document, original);
                    }
                    Tool::Crop => {
                        assert!(e.tools.pending_crop.is_some());
                        let clock = Instant::now();
                        e.commit_crop().unwrap();
                        apply = clock.elapsed().as_secs_f64() * 1000.;
                        assert!(e.session().document.width < width);
                        assert!(e.session().document.height < height);
                    }
                    Tool::Text => assert!(matches!(e.modal, Some(Form::Text(_)))),
                    Tool::Pen => assert_eq!(e.session().document.paths.len(), 1),
                    Tool::Eyedropper => assert_ne!(e.tools.brush.color, color),
                    Tool::Hand => assert_ne!(e.session().pan, [0., 0.]),
                    Tool::Zoom => assert_ne!(e.session().zoom, 1.),
                    Tool::Shape => assert_eq!(e.session().document.layers.len(), 2),
                    Tool::Move => assert_ne!(e.session().document.layers[0].transform, original.layers[0].transform),
                    _ => {
                        assert_ne!(e.session().document.layers[0].raster(), original.layers[0].raster(), "{tool:?} must change pixels");
                        assert!(e.session().undo_label().is_some(), "{tool:?}");
                        e.session_mut().undo();
                        assert_eq!(e.session().document, original, "{tool:?} undo");
                    }
                }
                assert!(e.errors.is_empty(), "{tool:?}");
                let worst_move = moves.into_iter().fold(0_f64, f64::max);
                eprintln!("{width}x{height} {tool:?}: down={down:.2} ms, worst_move={worst_move:.2} ms, up={up:.2} ms, apply={apply:.2} ms, worker={worker:.2} ms");
            })
            .unwrap();
        }
    }
}
