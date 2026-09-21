// Adapted from Xuan, copyright (c) 2026 Wonder Assembly LLC and Silver Ling.
// Distributed under the MIT license; see licenses/Xuan-MIT.txt.
use crate::{Result, invalid, raw::ensure};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::Duration,
};
use wgpu::util::DeviceExt;

static ENGINE: OnceLock<std::result::Result<Mutex<Processor>, String>> = OnceLock::new();
pub(super) fn engine() -> &'static std::result::Result<Mutex<Processor>, String> {
    ENGINE.get_or_init(|| {
        Processor::new().map(Mutex::new).map_err(|e| {
            let message = e.to_string();
            eprintln!("RAW GPU unavailable: {message}. Using native CPU development.");
            message
        })
    })
}
pub(super) struct Processor {
    pub(super) device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    pipelines: Mutex<HashMap<&'static str, wgpu::ComputePipeline>>,
}
impl Processor {
    fn new() -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|e| invalid(format!("Could not select a Vulkan RAW adapter: {e}")))?;
        ensure(
            adapter.get_info().device_type != wgpu::DeviceType::Cpu,
            "The Vulkan RAW adapter is a software renderer",
        )?;
        let limits = adapter.limits();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Compositor RAW development"),
            required_limits: wgpu::Limits {
                max_storage_buffer_binding_size:
                    limits.max_storage_buffer_binding_size.min(1_600_000_000),
                max_buffer_size: limits.max_buffer_size.min(1_600_000_000),
                ..Default::default()
            },
            ..Default::default()
        }))
        .map_err(|e| invalid(format!("Could not create the RAW Vulkan device: {e}")))?;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("RAW processing buffers"),
            entries: &std::array::from_fn::<_, 4, _>(|index| wgpu::BindGroupLayoutEntry {
                binding: index as u32,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage {
                        read_only: index != 2,
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }),
        });
        Ok(Self {
            device,
            queue,
            layout,
            pipelines: Mutex::new(HashMap::new()),
        })
    }
    pub(super) fn buffer(&self, bytes: &[u8]) -> Result<wgpu::Buffer> {
        self.check_size(bytes.len() as u64)?;
        Ok(self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("processing input"),
                contents: if bytes.is_empty() { &[0; 16] } else { bytes },
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            }))
    }

    pub(super) fn empty(&self, size: u64) -> Result<wgpu::Buffer> {
        self.check_size(size)?;
        Ok(self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("processing intermediate"),
            size: size.max(16),
            mapped_at_creation: false,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        }))
    }

    fn check_size(&self, size: u64) -> Result<()> {
        let limits = self.device.limits();
        ensure(
            size <= limits.max_storage_buffer_binding_size && size <= limits.max_buffer_size,
            "RAW image exceeds GPU buffer limits. Reduce the RAW image dimensions before developing.",
        )?;
        Ok(())
    }

    pub(super) fn encoder(&self) -> wgpu::CommandEncoder {
        self.device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("image processing"),
            })
    }

    pub(super) fn dispatch(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        entry: &'static str,
        shader: &str,
        buffers: [&wgpu::Buffer; 3],
        config: &[[f32; 4]],
        size: [u32; 2],
    ) -> Result<()> {
        let limit = self.device.limits().max_compute_workgroups_per_dimension;
        ensure(
            size[0].div_ceil(8) <= limit && size[1].div_ceil(8) <= limit,
            "RAW image exceeds GPU dispatch limits.",
        )?;
        let mut pipelines = self.pipelines.lock().unwrap_or_else(|p| p.into_inner());
        let pipeline = pipelines
            .entry(entry)
            .or_insert_with(|| {
                let module = self
                    .device
                    .create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some(entry),
                        source: wgpu::ShaderSource::Wgsl(shader.into()),
                    });
                let layout = self
                    .device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("processing pipeline"),
                        bind_group_layouts: &[Some(&self.layout)],
                        immediate_size: 0,
                    });
                self.device
                    .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                        label: Some(entry),
                        layout: Some(&layout),
                        module: &module,
                        entry_point: Some(entry),
                        compilation_options: Default::default(),
                        cache: None,
                    })
            })
            .clone();
        drop(pipelines);
        let params = self.buffer(bytemuck::cast_slice(config))?;
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(entry),
            layout: &self.layout,
            entries: &std::array::from_fn::<_, 4, _>(|index| wgpu::BindGroupEntry {
                binding: index as u32,
                resource: if index == 3 {
                    params.as_entire_binding()
                } else {
                    buffers[index].as_entire_binding()
                },
            }),
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(entry),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(size[0].div_ceil(8), size[1].div_ceil(8), 1);
        Ok(())
    }

    pub(super) fn read(
        &self,
        mut encoder: wgpu::CommandEncoder,
        buffer: &wgpu::Buffer,
        size: u64,
    ) -> Result<Vec<u8>> {
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("processing readback"),
            size,
            mapped_at_creation: false,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        });
        encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, size);
        self.queue.submit([encoder.finish()]);
        self.map(&staging)
    }

    pub(super) fn blur_passes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        input: &wgpu::Buffer,
        scratch: &wgpu::Buffer,
        output: &wgpu::Buffer,
        size: [u32; 2],
        sigma: f32,
    ) -> Result<()> {
        let possible = (((sigma - 0.8) / 0.3 + 1.0) * 2.0 + 1.0).max(3.0) as u32;
        let length = possible | 1;
        ensure(length <= 4095, "Gaussian kernel exceeds GPU work budget")?;
        let mut kernel: Vec<f32> = (0..length)
            .map(|i| {
                (-0.5 * ((i as f32 - (length / 2) as f32) / sigma).powi(2)).exp()
                    / (std::f32::consts::TAU.sqrt() * sigma)
            })
            .collect();
        let scale = 1.0 / kernel.iter().sum::<f32>();
        kernel.iter_mut().for_each(|weight| *weight *= scale);
        for (source, target, direction) in
            [(input, scratch, [1.0, 0.0]), (scratch, output, [0.0, 1.0])]
        {
            let mut config = vec![
                [size[0] as f32, size[1] as f32, 0.0, 0.0],
                [(length / 2) as f32, direction[0], direction[1], 0.0],
            ];
            config.extend(kernel.iter().map(|w| [*w, 0.0, 0.0, 0.0]));
            self.dispatch(
                encoder,
                "gaussian",
                super::SHADER,
                [source, source, target],
                &config,
                size,
            )?;
        }
        Ok(())
    }

    fn map(&self, buffer: &wgpu::Buffer) -> Result<Vec<u8>> {
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = send.send(result);
        });
        let result = self
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(120)),
            })
            .map_err(|e| invalid(format!("RAW GPU development timed out or failed: {e}")))
            .and_then(|_| {
                receive
                    .recv_timeout(Duration::from_secs(1))
                    .map_err(|e| invalid(format!("RAW GPU readback stopped: {e}")))
            })
            .and_then(|r| r.map_err(|e| invalid(format!("RAW GPU readback failed: {e}"))))
            .and_then(|_| {
                slice
                    .get_mapped_range()
                    .map(|bytes| bytes.to_vec())
                    .map_err(|e| invalid(format!("Cannot read developed RAW pixels: {e}")))
            });
        buffer.unmap();
        result
    }
}
