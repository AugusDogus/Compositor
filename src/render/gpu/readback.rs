//! Complete a full-buffer compute result before releasing its error scopes.
use super::Engine;
use crate::{Result, invalid};
use std::time::Duration;

pub(super) enum Operation {
    Geometry,
    MotionBlur,
    Resize,
    CoverageBlur,
    Effects,
}

impl Operation {
    fn label(&self) -> &'static str {
        match self {
            Self::Geometry => "Camera Raw geometry",
            Self::MotionBlur => "Motion blur",
            Self::Resize => "Preview resizing",
            Self::CoverageBlur => "Coverage blur",
            Self::Effects => "Layer effects",
        }
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(if matches!(self, Self::Resize) { 5 } else { 30 })
    }
}

impl Engine {
    pub(super) fn readback<T>(
        &self,
        encoder: wgpu::CommandEncoder,
        buffer: &wgpu::Buffer,
        errors: crate::gpu::ErrorScopes,
        operation: Operation,
        copy: impl FnOnce(&[u8]) -> T,
    ) -> Result<T> {
        let timeout = operation.timeout();
        let operation = operation.label();
        let submission = self.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = send.send(result);
        });
        let result = self
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(timeout),
            })
            .map_err(|error| {
                invalid(format!(
                    "{operation} GPU processing did not finish: {error}"
                ))
            })
            .and_then(|_| {
                receive
                    .recv_timeout(Duration::from_secs(1))
                    .map_err(|error| invalid(format!("{operation} GPU readback stopped: {error}")))
            })
            .and_then(|result| {
                result.map_err(|error| invalid(format!("{operation} GPU readback failed: {error}")))
            })
            .and_then(|_| {
                slice
                    .get_mapped_range()
                    .map(|pixels| copy(&pixels))
                    .map_err(|error| {
                        invalid(format!("Could not read {operation} GPU output: {error}"))
                    })
            });
        // Release mapped views (or cancel a pending map) even when polling fails.
        buffer.unmap();
        if let Some(error) = errors.finish() {
            return Err(invalid(format!(
                "{operation} GPU processing failed: {error}. Your source is preserved; retry the operation."
            )));
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::util::DeviceExt;

    #[test]
    #[ignore = "Requires a hardware Vulkan adapter"]
    fn failed_submission_releases_mapping_and_error_scopes_before_retry() {
        let engine = Engine::new().unwrap();
        let source = engine
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Readback test source"),
                contents: &[1, 2, 3, 4, 5, 6, 7, 8],
                usage: wgpu::BufferUsages::COPY_SRC,
            });
        let output = engine.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Readback test output"),
            size: 8,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        for invalid_copy in [true, false, false] {
            let errors = crate::gpu::ErrorScopes::new(&engine.device);
            let mut encoder = engine.device.create_command_encoder(&Default::default());
            // The first command deliberately violates the copy alignment rule.
            encoder.copy_buffer_to_buffer(&source, u64::from(invalid_copy), &output, 0, 4);
            let result = engine.readback(
                encoder,
                &output,
                errors,
                Operation::Geometry,
                <[u8]>::to_vec,
            );
            if invalid_copy {
                let error = result.unwrap_err().to_string();
                assert!(
                    error.contains("Camera Raw geometry GPU processing failed"),
                    "{error}"
                );
            } else {
                assert_eq!(result.unwrap(), [1, 2, 3, 4, 0, 0, 0, 0]);
            }
        }
    }
}
