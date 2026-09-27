//! Keep luminance preparation, Gaussian passes, and recombination on the canvas GPU.
use super::Engine;
use crate::{Result, filters::luminosity_sharpen::Settings, invalid};
use image::RgbaImage;
use wgpu::util::DeviceExt;

pub(super) struct Pipelines {
    prepare: wgpu::ComputePipeline,
    combine: wgpu::ComputePipeline,
}
impl Pipelines {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Luminosity Sharpen"),
            source: wgpu::ShaderSource::Wgsl(include_str!("luminosity_sharpen.wgsl").into()),
        });
        let pipeline = |entry| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("Luminosity Sharpen"),
                layout: None,
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        Self {
            prepare: pipeline("prepare"),
            combine: pipeline("combine"),
        }
    }
}
pub(crate) fn apply(image: &RgbaImage, settings: Settings) -> Result<Option<RgbaImage>> {
    let Ok(engine) = super::engine() else {
        return Ok(None);
    };
    let engine = engine.lock().map_err(|_| invalid("The GPU sharpening worker stopped. Restart the editor; your original pixels are preserved."))?;
    engine.luminosity_sharpen(image, settings)
}
impl Engine {
    fn luminosity_sharpen(
        &self,
        image: &RgbaImage,
        settings: Settings,
    ) -> Result<Option<RgbaImage>> {
        let bytes = image.as_raw().len() as u64;
        let limits = self.device.limits();
        if bytes
            > limits
                .max_storage_buffer_binding_size
                .min(limits.max_buffer_size)
            || [image.width(), image.height()]
                .iter()
                .any(|v| v.div_ceil(16) > limits.max_compute_workgroups_per_dimension)
        {
            return Ok(None);
        }
        let errors = crate::gpu::ErrorScopes::new(&self.device);
        let storage = wgpu::BufferUsages::STORAGE;
        let buffer = |label, usage| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes,
                usage,
                mapped_at_creation: false,
            })
        };
        let input = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Sharpen source"),
                contents: image.as_raw(),
                usage: storage,
            });
        let luma = buffer("Premultiplied luminance", storage);
        let alpha = buffer("Sharpen coverage", storage);
        let rows = buffer("Sharpen blur rows", storage);
        let blurred_luma = buffer("Blurred luminance", storage);
        let blurred_alpha = buffer("Blurred coverage", storage);
        let output = buffer("Sharpen result", storage | wgpu::BufferUsages::COPY_SRC);
        let readback = buffer(
            "Sharpen readback",
            wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        );
        let params = [
            image.width(),
            image.height(),
            (settings.amount as f32).to_bits(),
            (settings.noise as f32).to_bits(),
        ];
        let uniform = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Sharpen settings"),
                contents: bytemuck::cast_slice(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let size = [image.width(), image.height()];
        self.sharpen_pass(
            &mut encoder,
            &self.luminosity_sharpen.prepare,
            &[&uniform, &input, &luma, &alpha],
            size,
        );
        self.encode_coverage_blur(
            &mut encoder,
            &luma,
            &rows,
            &blurred_luma,
            size,
            settings.radius as f32,
        );
        self.encode_coverage_blur(
            &mut encoder,
            &alpha,
            &rows,
            &blurred_alpha,
            size,
            settings.radius as f32,
        );
        self.sharpen_pass(
            &mut encoder,
            &self.luminosity_sharpen.combine,
            &[&uniform, &input, &blurred_luma, &blurred_alpha, &output],
            size,
        );
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes);
        let pixels = self.readback(
            encoder,
            &readback,
            errors,
            super::readback::Operation::LuminositySharpen,
            <[u8]>::to_vec,
        )?;
        RgbaImage::from_raw(size[0], size[1], pixels).ok_or_else(|| invalid("Luminosity Sharpen returned an incorrect pixel count. Your original pixels are preserved.")).map(Some)
    }
    fn sharpen_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::ComputePipeline,
        buffers: &[&wgpu::Buffer],
        size: [u32; 2],
    ) {
        let bindings = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Sharpen bindings"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &buffers
                .iter()
                .enumerate()
                .map(|(binding, buffer)| wgpu::BindGroupEntry {
                    binding: binding as u32,
                    resource: buffer.as_entire_binding(),
                })
                .collect::<Vec<_>>(),
        });
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.dispatch_workgroups(size[0].div_ceil(16), size[1].div_ceil(16), 1);
    }
}
#[cfg(test)]
mod tests;
