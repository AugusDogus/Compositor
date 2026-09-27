//! Pointwise filtration shares the canvas device and bounds every storage buffer.
use super::Engine;
use crate::{
    Result,
    adjustment::{ChannelMixer, PhotoFilter},
    invalid,
    selective_color::{Mode, SelectiveColor},
};
use image::RgbaImage;
use wgpu::util::DeviceExt;

#[derive(Clone, Copy)]
pub(crate) enum Settings {
    PhotoFilter(PhotoFilter),
    ChannelMixer(ChannelMixer),
    SelectiveColor(SelectiveColor),
    Threshold(crate::threshold::Threshold),
}
impl Settings {
    fn operation(self) -> super::readback::Operation {
        match self {
            Self::PhotoFilter(_) => super::readback::Operation::PhotoFilter,
            Self::ChannelMixer(_) => super::readback::Operation::ChannelMixer,
            Self::SelectiveColor(_) => super::readback::Operation::SelectiveColor,
            Self::Threshold(_) => super::readback::Operation::Threshold,
        }
    }
    fn parameters(self, count: u32) -> Vec<u32> {
        match self {
            Self::Threshold(settings) => vec![u32::from(settings.level), count, 0, 0],
            Self::PhotoFilter(settings) => vec![
                (settings.color[0] as f32).to_bits(),
                (settings.color[1] as f32).to_bits(),
                (settings.color[2] as f32).to_bits(),
                ((settings.density / 100.) as f32).to_bits(),
                u32::from(settings.preserve_luminosity),
                count,
                0,
                0,
            ],
            Self::ChannelMixer(settings) => settings
                .coefficients()
                .into_iter()
                .flatten()
                .map(f32::to_bits)
                .chain([count, 0, 0, 0])
                .collect(),
            Self::SelectiveColor(settings) => settings
                .coefficients()
                .into_iter()
                .flatten()
                .map(f32::to_bits)
                .chain([count, u32::from(settings.mode == Mode::Relative), 0, 0])
                .collect(),
        }
    }
}
pub(super) struct Pipelines {
    photo_filter: wgpu::ComputePipeline,
    channel_mixer: wgpu::ComputePipeline,
    selective_color: wgpu::ComputePipeline,
    threshold: wgpu::ComputePipeline,
}
impl Pipelines {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        let pipeline = |source: &str| {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Color filter"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("Color filter"),
                layout: None,
                module: &shader,
                entry_point: Some("apply"),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        Self {
            threshold: pipeline(include_str!("threshold.wgsl")),
            photo_filter: pipeline(include_str!("photo_filter.wgsl")),
            channel_mixer: pipeline(include_str!("channel_mixer.wgsl")),
            selective_color: pipeline(
                &[
                    include_str!("selective_color.wgsl"),
                    include_str!("selective_color_math.wgsl"),
                ]
                .join("\n"),
            ),
        }
    }
    fn get(&self, settings: Settings) -> &wgpu::ComputePipeline {
        match settings {
            Settings::PhotoFilter(_) => &self.photo_filter,
            Settings::ChannelMixer(_) => &self.channel_mixer,
            Settings::SelectiveColor(_) => &self.selective_color,
            Settings::Threshold(_) => &self.threshold,
        }
    }
}

pub(crate) fn apply(image: &RgbaImage, settings: Settings) -> Result<Option<RgbaImage>> {
    let Ok(engine) = super::engine() else {
        return Ok(None);
    };
    let engine = engine.lock().map_err(|_| invalid("The GPU color filter worker stopped. Restart the editor; the original pixels are preserved."))?;
    engine.color_filter(image, settings).map(Some)
}

impl Engine {
    fn color_filter(&self, image: &RgbaImage, settings: Settings) -> Result<RgbaImage> {
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
            self.color_filter_chunk(source, target, settings)?;
        }
        Ok(output)
    }

    fn color_filter_chunk(
        &self,
        source: &[u8],
        target: &mut [u8],
        settings: Settings,
    ) -> Result<()> {
        let errors = crate::gpu::ErrorScopes::new(&self.device);
        let bytes = source.len() as u64;
        let input = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Color filter source"),
                contents: source,
                usage: wgpu::BufferUsages::STORAGE,
            });
        let output = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Color filter output"),
            size: bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Color filter readback"),
            size: bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let count = (source.len() / 4) as u32;
        let parameters = settings.parameters(count);
        let pipeline = self.color_filter.get(settings);
        let uniform = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Color filter settings"),
                contents: bytemuck::cast_slice(&parameters),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bindings = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Color filter bindings"),
            layout: &pipeline.get_bind_group_layout(0),
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
            compute.set_pipeline(pipeline);
            compute.set_bind_group(0, &bindings, &[]);
            compute.dispatch_workgroups(count.div_ceil(256), 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes);
        self.readback(encoder, &readback, errors, settings.operation(), |bytes| {
            target.copy_from_slice(bytes)
        })
    }
}

#[cfg(test)]
mod tests;
