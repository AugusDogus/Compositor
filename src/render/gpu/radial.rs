//! Radial blur shares the canvas device, error scopes and bounded readback.
use super::Engine;
use crate::{
    Result,
    filters::radial::{Pass, Pixel},
    invalid,
};
use image::RgbaImage;
use wgpu::util::DeviceExt;

pub(super) struct Pipelines {
    blur: wgpu::ComputePipeline,
    encode: wgpu::ComputePipeline,
}
impl Pipelines {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Radial blur"),
            source: wgpu::ShaderSource::Wgsl(include_str!("radial.wgsl").into()),
        });
        let pipeline = |entry| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("Radial blur"),
                layout: None,
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        Self {
            blur: pipeline("blur"),
            encode: pipeline("encode"),
        }
    }
}

pub(crate) fn blur(
    source: &[Pixel],
    size: [u32; 2],
    center: [f32; 2],
    passes: &[Pass],
) -> Result<Option<RgbaImage>> {
    let Ok(engine) = super::engine() else {
        return Ok(None);
    };
    let engine = engine.lock().map_err(|_| invalid("The GPU radial blur worker stopped. Restart the editor; the original pixels are preserved."))?;
    engine.radial_blur(source, size, center, passes)
}

impl Engine {
    fn radial_blur(
        &self,
        source: &[Pixel],
        size: [u32; 2],
        center: [f32; 2],
        passes: &[Pass],
    ) -> Result<Option<RgbaImage>> {
        let row_bytes = (size[0] * 4).div_ceil(256) * 256;
        let bytes = u64::from(row_bytes) * u64::from(size[1]);
        if size
            .iter()
            .any(|v| *v > self.device.limits().max_texture_dimension_2d)
            || bytes > self.device.limits().max_buffer_size
        {
            return Ok(None);
        }
        let errors = crate::gpu::ErrorScopes::new(&self.device);
        let extent = wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        };
        let texture = |label, format, usage| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let work_usage = wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::COPY_DST;
        let work = [
            texture(
                "Radial blur source",
                wgpu::TextureFormat::Rgba16Float,
                work_usage,
            ),
            texture(
                "Radial blur intermediate",
                wgpu::TextureFormat::Rgba16Float,
                work_usage,
            ),
        ];
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &work[0],
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(source),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size[0] * 8),
                rows_per_image: Some(size[1]),
            },
            extent,
        );
        let views = work.each_ref().map(|t| t.create_view(&Default::default()));
        let encoded = texture(
            "Radial blur encoded pixels",
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        );
        let encoded_view = encoded.create_view(&Default::default());
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Radial blur readback"),
            size: bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        for (index, pass) in passes.iter().enumerate() {
            let parameters = [
                center[0],
                center[1],
                0.,
                0.,
                pass.mapping[0],
                pass.mapping[1],
                pass.mapping[2],
                pass.mapping[3],
            ];
            let uniform = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Radial blur settings"),
                    contents: bytemuck::cast_slice(&parameters),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let bindings = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Radial blur bindings"),
                layout: &self.radial.blur.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&views[index % 2]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&views[(index + 1) % 2]),
                    },
                ],
            });
            let mut compute = encoder.begin_compute_pass(&Default::default());
            compute.set_pipeline(&self.radial.blur);
            compute.set_bind_group(0, &bindings, &[]);
            compute.dispatch_workgroups(size[0].div_ceil(16), size[1].div_ceil(16), 1);
        }
        let bindings = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Radial blur encoding bindings"),
            layout: &self.radial.encode.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&views[passes.len() % 2]),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&encoded_view),
                },
            ],
        });
        {
            let mut compute = encoder.begin_compute_pass(&Default::default());
            compute.set_pipeline(&self.radial.encode);
            compute.set_bind_group(0, &bindings, &[]);
            compute.dispatch_workgroups(size[0].div_ceil(16), size[1].div_ceil(16), 1);
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &encoded,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row_bytes),
                    rows_per_image: Some(size[1]),
                },
            },
            extent,
        );
        self.readback(
            encoder,
            &readback,
            errors,
            super::readback::Operation::RadialBlur,
            |bytes| {
                let mut image = RgbaImage::new(size[0], size[1]);
                let pixels: &mut [u8] = image.as_mut();
                for (to, from) in pixels
                    .chunks_exact_mut(size[0] as usize * 4)
                    .zip(bytes.chunks_exact(row_bytes as usize))
                {
                    to.copy_from_slice(&from[..to.len()]);
                }
                image
            },
        )
        .map(Some)
    }
}

#[cfg(test)]
mod tests;
