use super::*;

#[test]
#[ignore = "Requires the packaged native runtime and object-selection model"]
fn packaged_object_selection() {
    use crate::document::{Layer, LayerContent};
    use std::sync::Arc;
    let pixels = RgbaImage::from_fn(160, 96, |x, y| {
        let inside = |cx: i32| (x as i32 - cx).pow(2) + (y as i32 - 48).pow(2) <= 28 * 28;
        image::Rgba(if inside(48) {
            [230, 70, 20, 255]
        } else if inside(104) {
            [20, 70, 230, 255]
        } else {
            [235, 235, 235, 255]
        })
    });
    let mut source = Document::new(160, 96).unwrap();
    source.layers.clear();
    let mut layer = Layer::blank("Touching circles", 160, 96);
    layer.content = LayerContent::Raster(Some(Arc::new(pixels)));
    source.add(layer).unwrap();
    crate::object_selection::select(
        &mut source,
        crate::object_selection::Settings {
            target: crate::object_selection::Target::ObjectBox {
                start: [20., 20.],
                end: [76., 76.],
            },
            sample_all: true,
            antialiased: true,
            mode: crate::selection::SelectionMode::Replace,
        },
    )
    .unwrap();
    let selection = source
        .selection
        .as_ref()
        .expect("No object selection was created");
    assert!(
        selection.coverage([48.5, 48.5]) >= 0.5,
        "Prompted circle was not selected"
    );
    assert!(
        selection.coverage([104.5, 48.5]) < 0.5,
        "Neighboring circle was selected"
    );
    assert!(
        selection.coverage([0.5, 0.5]) < 0.5,
        "Background was selected"
    );
}

#[test]
#[ignore = "Requires installed native inference, candidate ONNX weights and COMPOSITOR_SEGMENTATION_PHOTO"]
fn benchmark_native_prompt_model() {
    let root =
        std::env::var_os("COMPOSITOR_SEGMENTATION_MODEL").expect("Set candidate model directory");
    let photo = std::env::var_os("COMPOSITOR_SEGMENTATION_PHOTO").expect("Set photo");
    let spec = if std::env::var("COMPOSITOR_SEGMENTATION_ARCH").as_deref() == Ok("sam3") {
        Spec::SAM3
    } else {
        Spec::SAM2
    };
    let runtime = Runtime::load(OPERATION).unwrap();
    let start = std::time::Instant::now();
    let mut engine = Engine::load(&runtime, Path::new(&root), spec).unwrap();
    eprintln!("Load {:?}: {:?}", runtime.device(), start.elapsed());
    let layer = crate::image_io::import(Path::new(&photo)).unwrap();
    let pixels = layer.raster().unwrap();
    let mut source = Document::new(pixels.width(), pixels.height()).unwrap();
    source.layers.clear();
    source.add(layer).unwrap();
    if std::env::var_os("COMPOSITOR_SEGMENTATION_WITH_BACKGROUND").is_some() {
        let pixels = source.layers[0].raster().unwrap();
        let start = std::time::Instant::now();
        let mask = crate::background::foreground_mask(pixels).unwrap();
        assert!(mask.pixels().any(|p| p[0] >= 128));
        eprintln!(
            "Background model with object sessions loaded: {:?}",
            start.elapsed()
        );
    }
    let points = std::env::var("COMPOSITOR_SEGMENTATION_POINTS")
        .unwrap_or_else(|_| format!("{},{}", source.width / 2, source.height / 2));
    let output = std::env::var_os("COMPOSITOR_SEGMENTATION_OUTPUT").map(std::path::PathBuf::from);
    if let Some(path) = &output {
        std::fs::create_dir_all(path).unwrap();
    }
    for (index, text) in points.split(';').enumerate() {
        let (x, y) = text.split_once(',').unwrap();
        let point = [x.parse().unwrap(), y.parse().unwrap()];
        let start = std::time::Instant::now();
        let mask = engine.predict(&source, Prompt::Point(point)).unwrap();
        eprintln!(
            "Click {point:?}: {:?}, selected {} pixels",
            start.elapsed(),
            mask.pixels().filter(|p| p[0] >= 128).count()
        );
        assert!(mask.pixels().any(|p| p[0] >= 128), "No object found");
        if let Some(path) = &output {
            mask.save(path.join(format!("{index}.png"))).unwrap();
        }
    }
    if std::env::var_os("COMPOSITOR_SEGMENTATION_PROFILE").is_some() {
        eprintln!(
            "Encoder profile: {}",
            engine.encoder.end_profiling().unwrap()
        );
        eprintln!(
            "Decoder profile: {}",
            engine.decoder.end_profiling().unwrap()
        );
    }
}

#[derive(serde::Deserialize)]
struct Cases {
    cases: Vec<Case>,
}
#[derive(serde::Deserialize)]
struct Case {
    id: String,
    image: String,
    mask_png: String,
    positive_point: Point,
    box_xyxy: [f64; 4],
}
#[test]
#[ignore = "Requires candidate models and labeled external object fixtures"]
fn benchmark_labeled_objects() {
    let root = std::env::var_os("COMPOSITOR_SEGMENTATION_MODEL").expect("Set model directory");
    let cases_path =
        std::env::var_os("COMPOSITOR_SEGMENTATION_CASES").expect("Set labeled cases.json");
    let cases_path = Path::new(&cases_path);
    let fixture_root = cases_path.parent().unwrap();
    let cases: Cases = serde_json::from_slice(&std::fs::read(cases_path).unwrap()).unwrap();
    let spec = if std::env::var("COMPOSITOR_SEGMENTATION_ARCH").as_deref() == Ok("sam3") {
        Spec::SAM3
    } else {
        Spec::SAM2
    };
    let runtime = Runtime::load(OPERATION).unwrap();
    let start = std::time::Instant::now();
    let mut engine = Engine::load(&runtime, Path::new(&root), spec).unwrap();
    eprintln!("Load {:?}: {:?}", runtime.device(), start.elapsed());
    let output = std::path::PathBuf::from(
        std::env::var_os("COMPOSITOR_SEGMENTATION_OUTPUT").expect("Set output directory"),
    );
    std::fs::create_dir_all(&output).unwrap();
    let mut source: Option<(String, Document)> = None;
    let mut results = Vec::new();
    for case in cases.cases {
        if source
            .as_ref()
            .is_none_or(|(image, _)| *image != case.image)
        {
            let layer = crate::image_io::import(&fixture_root.join(&case.image)).unwrap();
            let pixels = layer.raster().unwrap();
            let mut doc = Document::new(pixels.width(), pixels.height()).unwrap();
            doc.layers.clear();
            doc.add(layer).unwrap();
            source = Some((case.image.clone(), doc));
        }
        let doc = &source.as_ref().unwrap().1;
        let prompt = if std::env::var("COMPOSITOR_SEGMENTATION_PROMPT").as_deref() == Ok("box") {
            let b = case.box_xyxy;
            Prompt::Box {
                start: [b[0], b[1]],
                end: [b[2], b[3]],
            }
        } else {
            Prompt::Point(case.positive_point)
        };
        let start = std::time::Instant::now();
        let mask = engine.predict(doc, prompt).unwrap();
        let elapsed = start.elapsed().as_secs_f64();
        let truth = image::open(fixture_root.join(&case.mask_png))
            .unwrap()
            .into_luma8();
        assert_eq!(mask.dimensions(), truth.dimensions());
        let (mut intersection, mut union, mut prediction, mut target) =
            (0_u64, 0_u64, 0_u64, 0_u64);
        for (p, t) in mask.pixels().zip(truth.pixels()) {
            let p = p[0] >= 128;
            let t = t[0] >= 128;
            intersection += u64::from(p && t);
            union += u64::from(p || t);
            prediction += u64::from(p);
            target += u64::from(t);
        }
        let iou = intersection as f64 / union.max(1) as f64;
        eprintln!("{}: IoU {iou:.4}, {elapsed:.4}s", case.id);
        mask.save(output.join(format!("{}.png", case.id))).unwrap();
        results.push(serde_json::json!({"id":case.id,"iou":iou,"seconds":elapsed,"precision":intersection as f64/prediction.max(1) as f64,"recall":intersection as f64/target.max(1) as f64}));
    }
    std::fs::write(
        output.join("results.json"),
        serde_json::to_vec_pretty(&results).unwrap(),
    )
    .unwrap();
}
