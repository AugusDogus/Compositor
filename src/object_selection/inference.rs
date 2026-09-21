//! Prompted segmentation with cached image embeddings and a native ONNX decoder.
use crate::{Result, document::Document, geometry::Point, inference::Runtime};
use image::{GrayImage, Luma, RgbaImage};
use ort::{
    session::Session,
    value::{DynValue, Tensor, TensorElementType},
};
use std::{path::Path, sync::Mutex};

mod model;
use model::Spec;

const OPERATION: &str = "Object selection";
const FEATURES: [&str; 3] = [
    "image_embeddings.0",
    "image_embeddings.1",
    "image_embeddings.2",
];
fn failed(step: &str, error: impl std::fmt::Display) -> crate::Error {
    crate::inference::failed(OPERATION, step, error)
}

#[derive(Clone, Copy)]
enum Prompt {
    Point(Point),
    Box { start: Point, end: Point },
}

struct Embeddings {
    source: Document,
    features: [DynValue; 3],
    alpha: GrayImage,
}
struct Engine {
    encoder: Session,
    decoder: Session,
    spec: Spec,
    encoded: Option<Embeddings>,
}
enum State {
    Empty,
    Ready(Box<Engine>),
    Stopped,
}
static ENGINE: Mutex<State> = Mutex::new(State::Empty);

pub(super) fn shutdown() {
    *ENGINE.lock().unwrap_or_else(|error| error.into_inner()) = State::Stopped;
}

impl Engine {
    fn load(runtime: &Runtime, root: &Path, spec: Spec) -> Result<Self> {
        let load = |file: &str| -> Result<Session> {
            let path = root.join(file);
            if !path.is_file() {
                return Err(failed(
                    "load its segmentation model",
                    format!(
                        "{} is missing; download a fresh AppImage or rerun source-build inference setup",
                        path.display()
                    ),
                ));
            }
            let builder = runtime.builder()?;
            #[cfg(test)]
            let builder = if let Some(profile) = std::env::var_os("COMPOSITOR_SEGMENTATION_PROFILE")
            {
                builder
                    .with_profiling(Path::new(&profile).join(file))
                    .map_err(|e| failed("profile segmentation", e))?
            } else {
                builder
            };
            let mut builder = builder;
            builder
                .commit_from_file(&path)
                .map_err(|e| failed("load its segmentation model", e))
        };
        let encoder = load("vision_encoder_fp16.onnx")?;
        let decoder = load("prompt_encoder_mask_decoder_fp16.onnx")?;
        for (session, inputs, outputs) in [
            (&encoder, vec!["pixel_values"], FEATURES.to_vec()),
            (
                &decoder,
                vec![
                    "input_points",
                    "input_labels",
                    "input_boxes",
                    FEATURES[0],
                    FEATURES[1],
                    FEATURES[2],
                ],
                vec!["iou_scores", "pred_masks", "object_score_logits"],
            ),
        ] {
            if session.inputs().len() != inputs.len()
                || session.outputs().len() != outputs.len()
                || inputs.iter().any(|name| {
                    !session.inputs().iter().any(|input| {
                        input.name() == *name
                            && input.dtype().tensor_type()
                                == Some(if *name == "input_labels" {
                                    TensorElementType::Int64
                                } else {
                                    TensorElementType::Float32
                                })
                    })
                })
                || outputs.iter().any(|name| {
                    !session.outputs().iter().any(|output| {
                        output.name() == *name
                            && output.dtype().tensor_type() == Some(TensorElementType::Float32)
                    })
                })
            {
                return Err(failed(
                    "validate its segmentation model",
                    "unexpected model inputs or outputs",
                ));
            }
        }
        Ok(Self {
            encoder,
            decoder,
            spec,
            encoded: None,
        })
    }
    fn encode(&mut self, source: &Document) -> Result<()> {
        if self
            .encoded
            .as_ref()
            .is_some_and(|entry| entry.source == *source)
        {
            return Ok(());
        }
        let pixels = crate::render::region_accelerated(
            source,
            source.width,
            source.height,
            [0.; 2],
            [1.; 2],
            &mut crate::render::DownsampleCache::default(),
        )?;
        let input = Tensor::from_array((
            [1, 3, self.spec.side as usize, self.spec.side as usize],
            model::normalize(&pixels, self.spec),
        ))
        .map_err(|e| failed("prepare image input", e))?;
        let mut output = self
            .encoder
            .run(ort::inputs!["pixel_values" => input])
            .map_err(|e| failed("encode the image", e))?;
        let mut feature = |index: usize| {
            output.remove(FEATURES[index]).ok_or_else(|| {
                failed(
                    "read image embeddings",
                    format!("missing {}", FEATURES[index]),
                )
            })
        };
        let features = [feature(0)?, feature(1)?, feature(2)?];
        for (value, shape) in features.iter().zip(self.spec.feature_shapes()) {
            let (actual, data) = value
                .try_extract_tensor::<f32>()
                .map_err(|e| failed("read image embeddings", e))?;
            if actual.as_ref() != shape || data.iter().any(|v| !v.is_finite()) {
                return Err(failed(
                    "validate image embeddings",
                    "unexpected tensor dimensions or nonfinite values",
                ));
            }
        }
        let alpha = GrayImage::from_fn(pixels.width(), pixels.height(), |x, y| {
            Luma([pixels[(x, y)][3]])
        });
        self.encoded = Some(Embeddings {
            source: source.clone(),
            features,
            alpha,
        });
        Ok(())
    }
    fn predict(&mut self, source: &Document, prompt: Prompt) -> Result<GrayImage> {
        let (points, labels, boxes, point) = match prompt {
            Prompt::Point(point) => {
                if point
                    .iter()
                    .zip([source.width, source.height])
                    .any(|(p, size)| !p.is_finite() || *p < 0. || *p >= f64::from(size))
                {
                    return Err(failed(
                        "place the selection prompt",
                        "click inside the canvas",
                    ));
                }
                (
                    model::prompt(point, [source.width, source.height], self.spec).to_vec(),
                    vec![1_i64],
                    vec![],
                    Some(point),
                )
            }
            Prompt::Box { start, end } => {
                if start.iter().chain(end.iter()).any(|v| !v.is_finite())
                    || start[0] == end[0]
                    || start[1] == end[1]
                {
                    return Err(failed(
                        "place the selection box",
                        "draw a nonempty rectangle inside the canvas",
                    ));
                }
                let low = std::array::from_fn(|i| start[i].min(end[i]));
                let high = std::array::from_fn(|i| start[i].max(end[i]));
                if low.iter().any(|v| *v < 0.)
                    || high[0] > f64::from(source.width)
                    || high[1] > f64::from(source.height)
                {
                    return Err(failed("place the selection box", "draw inside the canvas"));
                }
                let a = model::prompt(low, [source.width, source.height], self.spec);
                let b = model::prompt(high, [source.width, source.height], self.spec);
                (vec![], vec![], vec![a[0], a[1], b[0], b[1]], None)
            }
        };
        self.encode(source)?;
        let encoded = self.encoded.as_ref().ok_or_else(|| {
            failed(
                "read image embeddings",
                "the encoder did not produce a cache entry",
            )
        })?;
        let points = Tensor::from_array(([1, 1, labels.len(), 2], points))
            .map_err(|e| failed("prepare the click", e))?;
        let labels = Tensor::from_array(([1, 1, labels.len()], labels))
            .map_err(|e| failed("prepare the click label", e))?;
        let boxes = Tensor::from_array(([1, boxes.len() / 4, 4], boxes))
            .map_err(|e| failed("prepare box prompts", e))?;
        let output = self.decoder.run(ort::inputs!["input_points" => points,"input_labels" => labels,"input_boxes" => boxes,
            FEATURES[0] => &encoded.features[0], FEATURES[1] => &encoded.features[1], FEATURES[2] => &encoded.features[2]
        ]).map_err(|e| failed("decode the prompted object",e))?;
        let tensor = |name: &str| {
            output
                .get(name)
                .ok_or_else(|| failed("read object masks", format!("missing {name}")))?
                .try_extract_tensor::<f32>()
                .map_err(|e| failed("read object masks", e))
        };
        let (shape, masks) = tensor("pred_masks")?;
        let (score_shape, scores) = tensor("iou_scores")?;
        let (object_shape, object) = tensor("object_score_logits")?;
        if score_shape.as_ref() != [1, 1, 3]
            || object_shape.as_ref() != [1, 1, 1]
            || object.len() != 1
            || !object[0].is_finite()
        {
            return Err(failed(
                "validate object scores",
                "unexpected tensor dimensions or nonfinite values",
            ));
        }
        model::decode(
            shape.as_ref(),
            masks,
            scores,
            object[0],
            point,
            &encoded.alpha,
        )
    }
}

pub(super) fn detect(source: &Document, point: Point) -> Result<GrayImage> {
    detect_prompt(source, Prompt::Point(point))
}
pub(super) fn detect_box(source: &Document, start: Point, end: Point) -> Result<GrayImage> {
    detect_prompt(source, Prompt::Box { start, end })
}
fn detect_prompt(source: &Document, prompt: Prompt) -> Result<GrayImage> {
    let mut state = ENGINE.lock().map_err(|_| {
        failed(
            "access its model",
            "the model worker stopped; restart the editor",
        )
    })?;
    if matches!(*state, State::Empty) {
        let runtime = Runtime::load(OPERATION)?;
        *state = State::Ready(Box::new(Engine::load(
            &runtime,
            &runtime.root().join("object-selection"),
            Spec::SAM3,
        )?));
    }
    match &mut *state {
        State::Ready(engine) => engine.predict(source, prompt),
        _ => Err(failed("start inference", "the editor is shutting down")),
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod unsam_tests;
