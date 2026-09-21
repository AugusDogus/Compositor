//! Shared native inference environment. Model sessions own their tensors and caches.
use crate::{Result, invalid};
use ort::{
    environment::Environment,
    session::{Session, builder::SessionBuilder},
};
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

mod device;
pub(crate) use device::Device;

pub(crate) fn failed(operation: &str, step: &str, error: impl std::fmt::Display) -> crate::Error {
    invalid(format!(
        "{operation} could not {step}: {error}. The current document is unchanged."
    ))
}

pub(crate) struct Runtime {
    root: PathBuf,
    device: Device,
    operation: &'static str,
}

impl Runtime {
    pub(crate) fn load(operation: &'static str) -> Result<Self> {
        let root = runtime_dir(operation)?;
        let library = root.join("lib/libonnxruntime.so");
        if !library.is_file() {
            return Err(failed(
                operation,
                "load its runtime",
                format!(
                    "{} is missing; download a fresh AppImage, or run scripts/setup-background.sh for a source build",
                    library.display()
                ),
            ));
        }
        let library = library
            .canonicalize()
            .map_err(|e| failed(operation, "locate ONNX Runtime", e))?;
        // Both model workers may initialize concurrently. Keep failures as well as
        // successes: retrying a partially initialized native environment is unsafe.
        static INITIALIZED: OnceLock<std::result::Result<PathBuf, String>> = OnceLock::new();
        let initialized = INITIALIZED
            .get_or_init(|| {
                let committed = ort::init_from(&library)
                    .map_err(|e| e.to_string())?
                    .with_name("Compositor inference")
                    .commit();
                if !committed {
                    return Err("ONNX Runtime was configured outside the shared runtime".into());
                }
                Environment::current().map_err(|e| e.to_string())?;
                Ok(library.clone())
            })
            .as_ref()
            .map_err(|e| {
                failed(
                    operation,
                    "initialize ONNX Runtime (restart after replacing runtime files)",
                    e,
                )
            })?;
        if initialized != &library {
            return Err(failed(
                operation,
                "change its runtime",
                "ONNX Runtime is already loaded from another directory; restart Compositor after changing COMPOSITOR_INFERENCE_DIR",
            ));
        }
        Ok(Self {
            root,
            device: Device::select(operation)?,
            operation,
        })
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn device(&self) -> Device {
        self.device
    }

    pub(crate) fn builder(&self) -> Result<SessionBuilder> {
        let builder = Session::builder()
            .map_err(|e| failed(self.operation, "create an inference session", e))?
            .with_intra_threads(4)
            .map_err(|e| failed(self.operation, "configure inference threads", e))?;
        match self.device {
            Device::Gpu => device::configure_gpu(builder, &self.root, self.operation),
            Device::Cpu => Ok(builder),
        }
    }
}

fn runtime_dir(operation: &str) -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("COMPOSITOR_INFERENCE_DIR") {
        return Ok(path.into());
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .ok_or_else(|| {
            failed(
                operation,
                "locate its runtime",
                "HOME and XDG_DATA_HOME are unset",
            )
        })?;
    Ok(data.join("compositor/inference"))
}
