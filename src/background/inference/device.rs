use super::failed;
use crate::Result;
use ort::{environment::Environment, session::builder::SessionBuilder};
use std::{path::Path, sync::OnceLock};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Device {
    Gpu,
    Cpu,
}

impl Device {
    pub(super) fn select() -> Result<Self> {
        match std::env::var("COMPOSITOR_BACKGROUND_DEVICE").as_deref() {
            Ok("cpu") => Ok(Self::Cpu),
            // Accept the previous CUDA setting when updating an existing installation.
            Ok("gpu" | "vulkan" | "cuda") => Ok(Self::Gpu),
            Err(std::env::VarError::NotPresent) => {
                let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                    backends: wgpu::Backends::VULKAN,
                    ..wgpu::InstanceDescriptor::new_without_display_handle()
                });
                let adapter =
                    pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                        power_preference: wgpu::PowerPreference::HighPerformance,
                        ..Default::default()
                    }));
                Ok(match adapter {
                    Ok(adapter)
                        if adapter.get_info().device_type != wgpu::DeviceType::Cpu
                            && adapter.features().contains(wgpu::Features::SHADER_F16) =>
                    {
                        Self::Gpu
                    }
                    _ => Self::Cpu,
                })
            }
            _ => Err(failed(
                "select an inference device",
                "COMPOSITOR_BACKGROUND_DEVICE must be gpu or cpu",
            )),
        }
    }
}

pub(super) fn configure_gpu(builder: SessionBuilder, root: &Path) -> Result<SessionBuilder> {
    // Environment registration survives failed model loads and must only happen once.
    static REGISTERED: OnceLock<std::result::Result<(), String>> = OnceLock::new();
    let env = Environment::current().map_err(|e| failed("access ONNX Runtime", e))?;
    REGISTERED
        .get_or_init(|| {
            env.register_ep_library(
                "webgpu",
                root.join("lib/libonnxruntime_providers_webgpu.so"),
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(|e| {
            failed(
                "load its Vulkan inference plugin (restart after replacing runtime files)",
                e,
            )
        })?;
    let device = env
        .devices()
        .find(|device| {
            device
                .ep()
                .is_ok_and(|name| name == "WebGpuExecutionProvider")
        })
        .ok_or_else(|| {
            failed(
                "start GPU inference",
                "no WebGPU device was found; check your Vulkan driver",
            )
        })?;
    // The native plugin reads session entries, not device-provider options.
    // Bucket caching retains oversized buffers and can exhaust 8 GB cards.
    builder
        .with_config_entry("ep.webgpuexecutionprovider.dawnBackendType", "Vulkan")
        .map_err(|e| failed("configure Vulkan inference", e))?
        // GridSample uses NCHW. Keeping convolutions in that layout avoids large
        // temporary transposes around each deformable-convolution replacement.
        .with_config_entry("ep.webgpuexecutionprovider.preferredLayout", "NCHW")
        .map_err(|e| failed("configure the model tensor layout", e))?
        .with_config_entry(
            "ep.webgpuexecutionprovider.storageBufferCacheMode",
            "disabled",
        )
        .map_err(|e| failed("configure GPU memory usage", e))?
        // Submit small batches so temporary tensors can retire while the editor
        // also owns canvas and brush resources on the same GPU.
        .with_config_entry("ep.webgpuexecutionprovider.maxNumPendingDispatches", "4")
        .map_err(|e| failed("configure GPU dispatch batches", e))?
        .with_devices([device], None)
        .map_err(|e| failed("start Vulkan inference (check your graphics driver)", e))
}
