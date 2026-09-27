//! Pointwise filtration shares the canvas device and bounds every storage buffer.
use super::Engine;
use crate::{Result, adjustment::PhotoFilter, invalid};
use image::RgbaImage;
use wgpu::util::DeviceExt;

pub(super) struct Pipeline(wgpu::ComputePipeline);
impl Pipeline {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Photo Filter"),
            source: wgpu::ShaderSource::Wgsl(include_str!("photo_filter.wgsl").into()),
        });
        Self(
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("Photo Filter"),
                layout: None,
                module: &shader,
                entry_point: Some("apply"),
                compilation_options: Default::default(),
                cache: None,
            }),
        )
    }
}

pub(crate) fn apply(image: &RgbaImage, settings: PhotoFilter) -> Result<Option<RgbaImage>> {
    let Ok(engine) = super::engine() else {
        return Ok(None);
    };
    let engine = engine.lock().map_err(|_| invalid("The GPU Photo Filter worker stopped. Restart the editor; the original pixels are preserved."))?;
    engine.photo_filter(image, settings).map(Some)
}

impl Engine {
    fn photo_filter(&self, image: &RgbaImage, settings: PhotoFilter) -> Result<RgbaImage> {
        let limits = self.device.limits();
        // Split large images without downscaling or falling back to the CPU.
        let chunk_pixels = limits
            .max_storage_buffer_binding_size
            .min(limits.max_buffer_size)
            .min(u64::from(limits.max_compute_workgroups_per_dimension) * 256 * 4)
            / 4;
        let chunk_bytes = chunk_pixels as usize * 4;
        let mut output = RgbaImage::new(image.width(), image.height());
        let output_bytes: &mut [u8] = output.as_mut();
        for (source, target) in image
            .as_raw()
            .chunks(chunk_bytes)
            .zip(output_bytes.chunks_mut(chunk_bytes))
        {
            self.photo_filter_chunk(source, target, settings)?;
        }
        Ok(output)
    }

    fn photo_filter_chunk(
        &self,
        source: &[u8],
        target: &mut [u8],
        settings: PhotoFilter,
    ) -> Result<()> {
        let errors = crate::gpu::ErrorScopes::new(&self.device);
        let bytes = source.len() as u64;
        let input = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Photo Filter source"),
                contents: source,
                usage: wgpu::BufferUsages::STORAGE,
            });
        let output = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Photo Filter output"),
            size: bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Photo Filter readback"),
            size: bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let parameters = [
            (settings.color[0] as f32).to_bits(),
            (settings.color[1] as f32).to_bits(),
            (settings.color[2] as f32).to_bits(),
            ((settings.density / 100.) as f32).to_bits(),
            u32::from(settings.preserve_luminosity),
            (source.len() / 4) as u32,
            0,
            0,
        ];
        let uniform = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Photo Filter settings"),
                contents: bytemuck::cast_slice(&parameters),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bindings = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Photo Filter bindings"),
            layout: &self.photo_filter.0.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: input.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: output.as_entire_binding(),
                },
            ],
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut compute = encoder.begin_compute_pass(&Default::default());
            compute.set_pipeline(&self.photo_filter.0);
            compute.set_bind_group(0, &bindings, &[]);
            compute.dispatch_workgroups(parameters[5].div_ceil(256), 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes);
        self.readback(
            encoder,
            &readback,
            errors,
            super::readback::Operation::PhotoFilter,
            |bytes| target.copy_from_slice(bytes),
        )
    }
}

#[cfg(test)]
mod tests;
