//! Native BiRefNet inference. One session is shared by the existing background workers.
use crate::{
    Result,
    inference::{Device, Runtime},
    invalid,
};
use image::{GrayImage, RgbaImage};
use ort::{
    session::Session,
    value::{Tensor, TensorElementType},
};
use std::sync::Mutex;

mod model;

static SESSION: Mutex<State> = Mutex::new(State::Empty);

enum State {
    Empty,
    Ready(Box<Engine>),
    Stopped,
}

/// Finish any in-flight removal and destroy its native session before driver teardown.
/// Pending workers cannot start another session after shutdown.
pub fn shutdown() {
    let mut state = SESSION.lock().unwrap_or_else(|error| error.into_inner());
    *state = State::Stopped;
}

struct Engine {
    session: Session,
    device: Device,
}

fn failed(operation: &str, error: impl std::fmt::Display) -> crate::Error {
    crate::inference::failed("Background removal", operation, error)
}

fn load(profile: Option<&std::path::Path>) -> Result<Engine> {
    let runtime = Runtime::load("Background removal")?;
    let root = runtime.root();
    let device = runtime.device();
    let model = root.join(match device {
        Device::Gpu => "birefnet-gpu.onnx",
        Device::Cpu => "birefnet-cpu.onnx",
    });
    if !model.is_file() {
        return Err(failed(
            "load its model",
            format!(
                "{} is missing; download a fresh AppImage, or run scripts/setup-background.sh for a source build",
                model.display()
            ),
        ));
    }
    let mut builder = runtime.builder()?;
    if let Some(path) = profile {
        builder = builder
            .with_profiling(path)
            .map_err(|e| failed("profile inference", e))?;
    }
    let session = builder
        .commit_from_file(&model)
        .map_err(|e| failed("load BiRefNet", e))?;
    let dtype = match device {
        Device::Gpu => TensorElementType::Float16,
        Device::Cpu => TensorElementType::Float32,
    };
    if session.inputs().len() != 1
        || session.outputs().len() != 1
        || session.inputs()[0].dtype().tensor_type() != Some(dtype)
        || session.outputs()[0].dtype().tensor_type() != Some(dtype)
    {
        return Err(failed(
            "validate BiRefNet",
            "unexpected model input/output types",
        ));
    }
    Ok(Engine { session, device })
}

pub(super) fn detect(image: &RgbaImage) -> Result<GrayImage> {
    let input = model::normalize(image);
    let mut guard = SESSION.lock().map_err(|_| {
        failed(
            "access its session",
            "inference worker failed; restart Compositor",
        )
    })?;
    if matches!(*guard, State::Empty) {
        *guard = State::Ready(Box::new(load(None)?));
    }
    let State::Ready(engine) = &mut *guard else {
        return Err(failed(
            "start inference",
            "the application is shutting down",
        ));
    };
    predict(engine, image.dimensions(), input)
}

fn predict(engine: &mut Engine, size: (u32, u32), input: Vec<f32>) -> Result<GrayImage> {
    let shape = [1, 3, model::SIDE as usize, model::SIDE as usize];
    let tensor = match engine.device {
        Device::Gpu => Tensor::from_array((
            shape,
            input
                .into_iter()
                .map(half::f16::from_f32)
                .collect::<Vec<_>>(),
        ))
        .map(Tensor::upcast),
        Device::Cpu => Tensor::from_array((shape, input)).map(Tensor::upcast),
    }
    .map_err(|e| failed("prepare its input", e))?;
    let outputs = engine.session.run(ort::inputs![tensor]).map_err(|e| {
        invalid(format!(
            "Background removal failed during inference: {e}. The layer is unchanged. \
         If GPU memory is exhausted, close other GPU-intensive applications and retry."
        ))
    })?;
    let (shape, probabilities) = match engine.device {
        Device::Gpu => {
            // The GPU export includes sigmoid. Applying it twice would destroy the mask.
            let (shape, alpha) = outputs[0]
                .try_extract_tensor::<half::f16>()
                .map_err(|e| failed("read the subject mask", e))?;
            (
                shape,
                alpha.iter().map(|value| value.to_f32()).collect::<Vec<_>>(),
            )
        }
        Device::Cpu => {
            let (shape, logits) = outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(|e| failed("read the subject mask", e))?;
            (
                shape,
                logits
                    .iter()
                    .map(|value| 1. / (1. + (-value).exp()))
                    .collect(),
            )
        }
    };
    if shape.as_ref() != [1, 1, i64::from(model::SIDE), i64::from(model::SIDE)] {
        return Err(failed(
            "read the subject mask",
            format!("unexpected output shape {shape:?}"),
        ));
    }
    model::decode_mask(&probabilities, model::SIDE, size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Requires bundled Vulkan inference files and COMPOSITOR_TEST_PHOTO; records real GPU kernel execution"]
    fn vulkan_runs_birefnet_repeatedly_and_profiles_gpu_kernels() {
        use ort::AsPointer;
        let photo = std::env::var_os("COMPOSITOR_TEST_PHOTO").expect("Set COMPOSITOR_TEST_PHOTO");
        let image = crate::image_io::read_image(std::path::Path::new(&photo)).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let start = std::time::Instant::now();
        let engine = load(Some(&directory.path().join("inference"))).unwrap();
        eprintln!("Native model load: {:?}", start.elapsed());
        assert_eq!(engine.device, Device::Gpu);
        let identity = engine.session.ptr();
        *SESSION.lock().unwrap() = State::Ready(Box::new(engine));
        let mut previous = None;
        for run in 0..2 {
            let start = std::time::Instant::now();
            let mask = detect(&image).unwrap();
            eprintln!("Native GPU removal {run}: {:?}", start.elapsed());
            assert!(mask.pixels().any(|pixel| pixel[0] < 16));
            assert!(mask.pixels().any(|pixel| pixel[0] > 240));
            if let Some(previous) = &previous {
                assert_eq!(&mask, previous);
            }
            if let Some(path) = std::env::var_os("COMPOSITOR_TEST_MASK_OUTPUT") {
                mask.save(path).unwrap();
            }
            previous = Some(mask);
        }
        let State::Ready(mut engine) =
            std::mem::replace(&mut *SESSION.lock().unwrap(), State::Empty)
        else {
            panic!("The cached session is missing");
        };
        assert_eq!(
            engine.session.ptr(),
            identity,
            "Repeated removal must reuse its session"
        );
        let profile = engine.session.end_profiling().unwrap();
        let events: serde_json::Value =
            serde_json::from_slice(&std::fs::read(profile).unwrap()).unwrap();
        let events = events.as_array().unwrap();
        let gpu_nodes = events
            .iter()
            .filter(|event| event["args"]["provider"] == "WebGpuExecutionProvider")
            .count();
        let cpu_nodes = events
            .iter()
            .filter(|event| event["args"]["provider"] == "CPUExecutionProvider")
            .count();
        eprintln!("Profile kernel events: Vulkan={gpu_nodes}, CPU={cpu_nodes}");
        assert!(gpu_nodes > 0, "Inference must execute on Vulkan");
        for operation in ["Conv", "GridSample", "MatMul", "LayerNormalization"] {
            let kernels: Vec<_> = events
                .iter()
                .filter(|event| {
                    event["args"]["op_name"] == operation && event["args"]["provider"].is_string()
                })
                .collect();
            assert!(!kernels.is_empty(), "Missing {operation} profiling events");
            assert!(
                kernels
                    .iter()
                    .all(|event| event["args"]["provider"] == "WebGpuExecutionProvider"),
                "{operation} must execute on Vulkan"
            );
        }
        *SESSION.lock().unwrap() = State::Ready(engine);
        shutdown();
        assert!(
            detect(&image)
                .unwrap_err()
                .to_string()
                .contains("shutting down")
        );
    }
}
