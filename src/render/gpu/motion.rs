//! Alpha-safe directional blur on the same Vulkan adapter as canvas compositing.
use super::Engine;
use crate::{Result, invalid};
use image::RgbaImage;
use wgpu::util::DeviceExt;

pub(in crate::render) fn blur(
    image: &RgbaImage,
    distance: f64,
    angle: f64,
) -> Result<Option<RgbaImage>> {
    let Ok(engine) = super::engine() else {
        return Ok(None);
    };
    let engine = engine.lock().map_err(|_| {
        invalid("The GPU blur worker stopped. Restart the editor; your document is preserved.")
    })?;
    engine.motion(image, distance, angle)
}
impl Engine {
    fn motion(&self, image: &RgbaImage, distance: f64, angle: f64) -> Result<Option<RgbaImage>> {
        let bytes = image.as_raw().len() as u64;
        if bytes > self.device.limits().max_storage_buffer_binding_size {
            return Ok(None);
        }
        let errors = crate::gpu::ErrorScopes::new(&self.device);
        let (sin, cos) = (-angle).to_radians().sin_cos();
        let params = [
            image.width(),
            image.height(),
            distance.ceil().max(1.) as u32 + 1,
            0,
            (distance as f32).to_bits(),
            (cos as f32).to_bits(),
            (sin as f32).to_bits(),
            0,
        ];
        let uniform = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Motion blur settings"),
                contents: bytemuck::cast_slice(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let input = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Motion blur source"),
                contents: image.as_raw(),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let output = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Motion blur pixels"),
            size: bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Motion blur readback"),
            size: bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bindings = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Motion blur bindings"),
            layout: &self.motion_pipeline.get_bind_group_layout(0),
            entries: &[&uniform, &input, &output]
                .into_iter()
                .enumerate()
                .map(|(binding, buffer)| wgpu::BindGroupEntry {
                    binding: binding as u32,
                    resource: buffer.as_entire_binding(),
                })
                .collect::<Vec<_>>(),
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.motion_pipeline);
            pass.set_bind_group(0, &bindings, &[]);
            pass.dispatch_workgroups(image.width().div_ceil(16), image.height().div_ceil(16), 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes);
        let result = self.readback(
            encoder,
            &readback,
            errors,
            super::readback::Operation::MotionBlur,
            <[u8]>::to_vec,
        );
        result
            .and_then(|pixels| {
                RgbaImage::from_raw(image.width(), image.height(), pixels)
                    .ok_or_else(|| invalid("Motion blur returned the wrong number of pixels."))
            })
            .map(Some)
    }
}
