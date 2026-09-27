//! Reuse the effect blur kernels for canvas-aligned grayscale coverage.
use super::Engine;
use crate::{Result, invalid};
use image::{GrayImage, ImageBuffer, Luma};

pub(crate) type FloatPlane = ImageBuffer<Luma<f32>, Vec<f32>>;
use wgpu::util::DeviceExt;

pub(in crate::render) fn blur(image: &GrayImage, sigma: f32) -> Result<Option<GrayImage>> {
    let plane = FloatPlane::from_fn(image.width(), image.height(), |x, y| {
        Luma([f32::from(image[(x, y)][0]) / 255.])
    });
    Ok(blur_float(&plane, sigma)?.map(|blurred| {
        GrayImage::from_fn(image.width(), image.height(), |x, y| {
            Luma([(blurred[(x, y)][0].clamp(0., 1.) * 255.).round() as u8])
        })
    }))
}

pub(in crate::render) fn blur_float(image: &FloatPlane, sigma: f32) -> Result<Option<FloatPlane>> {
    crate::document::validate_size(image.width(), image.height())?;
    let Ok(engine) = super::engine() else {
        return Ok(None);
    };
    let mut engine = engine.lock().map_err(|_| {
        invalid("The GPU coverage worker stopped. Restart the editor; your selection is preserved.")
    })?;
    engine.blur_coverage(image, sigma)
}
impl Engine {
    fn blur_coverage(&mut self, image: &FloatPlane, sigma: f32) -> Result<Option<FloatPlane>> {
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
        let input = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Coverage input"),
                contents: bytemuck::cast_slice(image.as_raw()),
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
                    .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    .collect()
            },
        );
        result
            .and_then(|pixels| {
                FloatPlane::from_raw(image.width(), image.height(), pixels)
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
        let mask = FloatPlane::from_fn(39, 29, |x, y| {
            image::Luma([if x < 15 && y < 11 { 0.371 } else { 0.003 }])
        });
        let gpu = engine.blur_coverage(&mask, 3.7).unwrap().unwrap();
        let expected = crate::effects::cpu::gaussian(mask.as_raw(), 39, 29, 3.7);
        for (a, b) in gpu.as_raw().iter().zip(expected) {
            assert!((a - b).abs() < 0.00001);
        }
        assert!(gpu[(0, 0)][0] > 0.36);
    }
}
