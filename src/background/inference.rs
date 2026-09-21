//! Native BiRefNet inference. One session is shared by the existing background workers.
use crate::{Result, invalid};
use image::{GrayImage, RgbaImage};
use ort::{
    ep,
    session::Session,
    value::{Tensor, TensorElementType},
};
use std::{path::PathBuf, sync::Mutex};

mod model;

static SESSION: Mutex<Option<Engine>> = Mutex::new(None);

#[derive(Clone, Copy, PartialEq)]
enum Device {
    Cuda,
    Cpu,
}

struct Engine {
    session: Session,
    device: Device,
}

fn failed(operation: &str, error: impl std::fmt::Display) -> crate::Error {
    invalid(format!(
        "Background removal could not {operation}: {error}. The layer is unchanged."
    ))
}

fn runtime_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("COMPOSITOR_INFERENCE_DIR") {
        return Ok(path.into());
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .ok_or_else(|| failed("locate its runtime", "HOME and XDG_DATA_HOME are unset"))?;
    Ok(data.join("compositor/inference"))
}

fn load(profile: Option<&std::path::Path>) -> Result<Engine> {
    let root = runtime_dir()?;
    let library = root.join("lib/libonnxruntime.so");
    // Use CPU on machines without the NVIDIA driver. Once CUDA is selected, failures
    // remain visible instead of silently becoming a slow CPU operation.
    let device = match std::env::var("COMPOSITOR_BACKGROUND_DEVICE").as_deref() {
        Ok("cpu") => Device::Cpu,
        Ok("cuda") => Device::Cuda,
        Err(std::env::VarError::NotPresent) => {
            if std::path::Path::new("/proc/driver/nvidia/gpus").is_dir() {
                Device::Cuda
            } else {
                Device::Cpu
            }
        }
        _ => {
            return Err(failed(
                "select an inference device",
                "COMPOSITOR_BACKGROUND_DEVICE must be cuda or cpu",
            ));
        }
    };
    let model = root.join(match device {
        Device::Cuda => "birefnet-cuda.onnx",
        Device::Cpu => "birefnet-cpu.onnx",
    });
    for path in [&library, &model] {
        if !path.is_file() {
            return Err(failed(
                "load its runtime",
                format!(
                    "{} is missing; download a fresh AppImage, or run scripts/setup-background.sh for a source build",
                    path.display()
                ),
            ));
        }
    }
    if device == Device::Cuda {
        for name in [
            "libcudart.so.12",
            "libcublasLt.so.12",
            "libcublas.so.12",
            "libcurand.so.10",
            "libcudnn.so.9",
        ] {
            ort::util::preload_dylib(root.join("lib").join(name))
                .map_err(|e| failed("load CUDA libraries", e))?;
        }
    }
    ort::init_from(&library)
        .map_err(|e| failed("load ONNX Runtime", e))?
        .with_name("Compositor background removal")
        .commit();
    let mut builder = Session::builder()
        .map_err(|e| failed("create an inference session", e))?
        .with_intra_threads(4)
        .map_err(|e| failed("configure inference threads", e))?;
    if device == Device::Cuda {
        builder = builder
            .with_execution_providers([ep::CUDA::default()
                .with_conv_algorithm_search(ep::cuda::ConvAlgorithmSearch::Heuristic)
                .with_conv_max_workspace(false)
                .with_arena_extend_strategy(ep::ArenaExtendStrategy::SameAsRequested)
                .build()
                .error_on_failure()])
            .map_err(|e| failed("start CUDA inference (check your NVIDIA driver)", e))?;
    }
    if let Some(path) = profile {
        builder = builder
            .with_profiling(path)
            .map_err(|e| failed("profile inference", e))?;
    }
    let session = builder
        .commit_from_file(&model)
        .map_err(|e| failed("load BiRefNet", e))?;
    let dtype = match device {
        Device::Cuda => TensorElementType::Float16,
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
    if guard.is_none() {
        *guard = Some(load(None)?);
    }
    let engine = guard
        .as_mut()
        .ok_or_else(|| failed("access its session", "no model is loaded"))?;
    predict(engine, image.dimensions(), input)
}

fn predict(engine: &mut Engine, size: (u32, u32), input: Vec<f32>) -> Result<GrayImage> {
    let shape = [1, 3, model::SIDE as usize, model::SIDE as usize];
    let tensor = match engine.device {
        Device::Cuda => Tensor::from_array((
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
        Device::Cuda => {
            // This CUDA export includes sigmoid. Applying it twice would destroy the mask.
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
    #[ignore = "Requires native CUDA setup and COMPOSITOR_TEST_PHOTO; records real GPU kernel execution"]
    fn cuda_runs_birefnet_repeatedly_and_profiles_gpu_kernels() {
        use ort::AsPointer;
        let photo = std::env::var_os("COMPOSITOR_TEST_PHOTO").expect("Set COMPOSITOR_TEST_PHOTO");
        let image = crate::image_io::read_image(std::path::Path::new(&photo)).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let start = std::time::Instant::now();
        let engine = load(Some(&directory.path().join("inference"))).unwrap();
        eprintln!("Native model load: {:?}", start.elapsed());
        let identity = engine.session.ptr();
        *SESSION.lock().unwrap() = Some(engine);
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
        let mut engine = SESSION.lock().unwrap().take().unwrap();
        assert_eq!(
            engine.session.ptr(),
            identity,
            "Repeated removal must reuse its session"
        );
        let profile = engine.session.end_profiling().unwrap();
        let events: serde_json::Value =
            serde_json::from_slice(&std::fs::read(profile).unwrap()).unwrap();
        let events = events.as_array().unwrap();
        let cuda_nodes = events
            .iter()
            .filter(|event| event["args"]["provider"] == "CUDAExecutionProvider")
            .count();
        let cpu_nodes = events
            .iter()
            .filter(|event| event["args"]["provider"] == "CPUExecutionProvider")
            .count();
        eprintln!("Profile kernel events: CUDA={cuda_nodes}, CPU={cpu_nodes}");
        assert!(cuda_nodes > 0, "Inference must execute on CUDA");
        for operation in ["Conv", "DeformConv", "MatMul"] {
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
                    .all(|event| event["args"]["provider"] == "CUDAExecutionProvider"),
                "{operation} must execute on CUDA"
            );
        }
    }
}
