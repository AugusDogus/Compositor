//! Reproducible research harness for the granularity-conditioned UnSAMv2+ model.
use super::*;

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
#[ignore = "Requires exported UnSAMv2+ models, native runtime and external labeled fixtures"]
fn benchmark_unsam_objects() {
    let root = std::env::var_os("COMPOSITOR_SEGMENTATION_MODEL").expect("Set model directory");
    let cases_path = std::env::var_os("COMPOSITOR_SEGMENTATION_CASES").expect("Set cases.json");
    let cases_path = Path::new(&cases_path);
    let fixture_root = cases_path.parent().unwrap();
    let cases: Cases = serde_json::from_slice(&std::fs::read(cases_path).unwrap()).unwrap();
    let output = std::path::PathBuf::from(
        std::env::var_os("COMPOSITOR_SEGMENTATION_OUTPUT").expect("Set output directory"),
    );
    std::fs::create_dir_all(&output).unwrap();
    let granularity: f32 = std::env::var("COMPOSITOR_UNSAM_GRANULARITY")
        .unwrap_or_else(|_| "0.5".into())
        .parse()
        .unwrap();
    assert!((0.0..=1.0).contains(&granularity));
    let use_box = std::env::var("COMPOSITOR_SEGMENTATION_PROMPT").as_deref() == Ok("box");
    let refine = std::env::var_os("COMPOSITOR_UNSAM_REFINE").is_some();
    let runtime = Runtime::load(OPERATION).unwrap();
    let mut encoder = runtime
        .builder()
        .unwrap()
        .commit_from_file(Path::new(&root).join("encoder.fp16.onnx"))
        .unwrap();
    let mut decoder = runtime
        .builder()
        .unwrap()
        .commit_from_file(Path::new(&root).join("decoder.fp16.onnx"))
        .unwrap();
    eprintln!(
        "UnSAMv2+ {:?}, granularity {granularity}, box {use_box}, refine {refine}",
        runtime.device()
    );
    let mut cached: Option<(String, RgbaImage, [DynValue; 3])> = None;
    let mut results = Vec::new();
    for case in cases.cases {
        let start = std::time::Instant::now();
        if cached.as_ref().is_none_or(|c| c.0 != case.image) {
            let pixels = image::open(fixture_root.join(&case.image))
                .unwrap()
                .into_rgba8();
            let tensor =
                Tensor::from_array(([1, 3, 1024, 1024], model::normalize(&pixels, Spec::SAM2)))
                    .unwrap();
            let mut encoded = encoder.run(ort::inputs!["image" => tensor]).unwrap();
            let features = ["high_res_feats_0", "high_res_feats_1", "image_embed"]
                .map(|name| encoded.remove(name).unwrap());
            cached = Some((case.image.clone(), pixels, features));
        }
        let (_, pixels, features) = cached.as_ref().unwrap();
        let size = [pixels.width(), pixels.height()];
        let (points, labels) = if use_box {
            let b = case.box_xyxy;
            let a = model::prompt([b[0], b[1]], size, Spec::SAM2);
            let b = model::prompt([b[2], b[3]], size, Spec::SAM2);
            (vec![a[0], a[1], b[0], b[1]], vec![2_i64, 3])
        } else {
            (
                model::prompt(case.positive_point, size, Spec::SAM2).to_vec(),
                vec![1_i64],
            )
        };
        let mut logits = vec![0_f32; 256 * 256];
        for pass in 0..if refine { 2 } else { 1 } {
            let points = Tensor::from_array(([1, labels.len(), 2], points.clone())).unwrap();
            let labels = Tensor::from_array(([1, labels.len()], labels.clone())).unwrap();
            let gra = Tensor::from_array(([1, 1], vec![granularity])).unwrap();
            let prior = Tensor::from_array(([1, 1, 256, 256], logits)).unwrap();
            let has =
                Tensor::from_array(([1], vec![if pass == 0 { 0_f32 } else { 1_f32 }])).unwrap();
            let decoded = decoder.run(ort::inputs![
                "high_res_feats_0" => &features[0], "high_res_feats_1" => &features[1], "image_embed" => &features[2],
                "point_coords" => points, "point_labels" => labels, "granularity" => gra, "mask_input" => prior, "has_mask_input" => has
            ]).unwrap();
            let (shape, values) = decoded["masks"].try_extract_tensor::<f32>().unwrap();
            assert_eq!(shape.as_ref(), [1, 1, 256, 256]);
            assert!(values.iter().all(|v| v.is_finite()));
            logits = values.to_vec();
        }
        // Repeat the one mask only to reuse production interpolation, without mask ranking or click filtering.
        let alpha = GrayImage::from_fn(size[0], size[1], |x, y| Luma([pixels[(x, y)][3]]));
        let mask = model::decode(
            &[1, 1, 3, 256, 256],
            &logits.repeat(3),
            &[1.; 3],
            1.,
            None,
            &alpha,
        )
        .unwrap();
        let elapsed = start.elapsed().as_secs_f64();
        let truth = image::open(fixture_root.join(&case.mask_png))
            .unwrap()
            .into_luma8();
        assert_eq!(mask.dimensions(), truth.dimensions());
        let (mut intersection, mut union, mut predicted, mut target) = (0_u64, 0_u64, 0_u64, 0_u64);
        for (p, t) in mask.pixels().zip(truth.pixels()) {
            let p = p[0] >= 128;
            let t = t[0] >= 128;
            intersection += u64::from(p && t);
            union += u64::from(p || t);
            predicted += u64::from(p);
            target += u64::from(t);
        }
        let iou = intersection as f64 / union.max(1) as f64;
        eprintln!("{}: IoU {iou:.4}, {elapsed:.4}s", case.id);
        mask.save(output.join(format!("{}.png", case.id))).unwrap();
        results.push(serde_json::json!({"id":case.id,"iou":iou,"seconds":elapsed,"granularity":granularity,"box":use_box,"refine":refine,"precision":intersection as f64/predicted.max(1) as f64,"recall":intersection as f64/target.max(1) as f64}));
    }
    std::fs::write(
        output.join("results.json"),
        serde_json::to_vec_pretty(&results).unwrap(),
    )
    .unwrap();
}
