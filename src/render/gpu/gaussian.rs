//! Reuse the effect blur kernels for canvas-aligned grayscale coverage.
use super::Engine;
use crate::{Result, invalid};
use image::GrayImage;
use wgpu::util::DeviceExt;

pub(in crate::render) fn blur(image: &GrayImage, sigma: f32) -> Result<Option<GrayImage>> {
    let Ok(engine) = super::engine() else {
        return Ok(None);
    };
    let mut engine = engine.lock().map_err(|_| {
        invalid("The GPU coverage worker stopped. Restart the editor; your selection is preserved.")
    })?;
    engine.blur_coverage(image, sigma)
}
impl Engine {
    fn blur_coverage(&mut self, image: &GrayImage, sigma: f32) -> Result<Option<GrayImage>> {
        let bytes = image.len() as u64 * 4;
        if bytes > self.device.limits().max_storage_buffer_binding_size {
            return Ok(None);
        }
        let errors = crate::gpu::ErrorScopes::new(&self.device);
        let storage = wgpu::BufferUsages::STORAGE;
        let allocate = |label, size, usage| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        let values: Vec<f32> = image
            .as_raw()
            .iter()
            .map(|v| f32::from(*v) / 255.)
            .collect();
        let input = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Coverage input"),
                contents: bytemuck::cast_slice(&values),
                usage: storage,
            });
        let rows = allocate("Blur rows", bytes, storage);
        let columns = allocate(
            "Blur columns",
            bytes,
            storage | wgpu::BufferUsages::COPY_SRC,
        );
        let readback = allocate(
            "Blur readback",
            bytes,
            wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        );
        let dummy = allocate("Unused blur input", 4, storage);
        let unused_output = allocate("Unused blur pixels", 4, storage);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        for (phase, source, destination) in [(5, &input, &rows), (6, &rows, &columns)] {
            let mut params = [0u32; 40];
            params[0] = image.width();
            params[1] = image.height();
            params[2] = phase;
            params[4] = sigma.to_bits();
            let uniform = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Coverage blur parameters"),
                    contents: bytemuck::cast_slice(&params),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let bindings = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Coverage blur bindings"),
                layout: &self.effects_pipeline.get_bind_group_layout(0),
                entries: &[
                    &uniform,
                    &dummy,
                    source,
                    destination,
                    &dummy,
                    &dummy,
                    &dummy,
                    &unused_output,
                    &dummy,
                ]
                .into_iter()
                .enumerate()
                .map(|(binding, buffer)| wgpu::BindGroupEntry {
                    binding: binding as u32,
                    resource: buffer.as_entire_binding(),
                })
                .collect::<Vec<_>>(),
            });
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.effects_pipeline);
            pass.set_bind_group(0, &bindings, &[]);
            pass.dispatch_workgroups(image.width().div_ceil(16), image.height().div_ceil(16), 1);
        }
        encoder.copy_buffer_to_buffer(&columns, 0, &readback, 0, bytes);
        let result = self.readback(
            encoder,
            &readback,
            errors,
            super::readback::Operation::CoverageBlur,
            |bytes| {
                bytes
                    .chunks_exact(4)
                    .map(|b| {
                        (f32::from_le_bytes([b[0], b[1], b[2], b[3]]).clamp(0., 1.) * 255.).round()
                            as u8
                    })
                    .collect()
            },
        );
        result
            .and_then(|pixels| {
                GrayImage::from_raw(image.width(), image.height(), pixels)
                    .ok_or_else(|| invalid("Coverage blur returned the wrong pixel count."))
            })
            .map(Some)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Requires a hardware Vulkan adapter"]
    fn grayscale_gpu_blur_matches_cpu_and_clamps_canvas_edges() {
        let mut engine = Engine::new().unwrap();
        let mask = GrayImage::from_fn(39, 29, |x, y| {
            image::Luma([if x < 15 && y < 11 { 255 } else { 0 }])
        });
        let gpu = engine.blur_coverage(&mask, 3.7).unwrap().unwrap();
        let values: Vec<_> = mask.as_raw().iter().map(|v| f32::from(*v) / 255.).collect();
        let expected = crate::effects::cpu::gaussian(&values, 39, 29, 3.7);
        for (a, b) in gpu.as_raw().iter().zip(expected) {
            assert!(a.abs_diff((b * 255.).round() as u8) <= 1);
        }
        assert!(gpu[(0, 0)][0] > 250);
    }
}
