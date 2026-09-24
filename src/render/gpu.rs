//! Vulkan viewport compositing. The CPU renderer remains the reference and the
//! fallback for unavailable hardware or scenes exceeding the bounded GPU budget.
mod adjustments;
pub(super) mod camera_geometry;
pub(super) mod effects;
pub(super) mod gaussian;
pub(super) mod motion;
pub(crate) mod raw;
pub(super) mod resize;
pub(super) mod scene;
#[cfg(test)]
mod tests;
use crate::{Result, document::Document, geometry::Point, invalid};
use image::RgbaImage;
use scene::Scene;
use std::sync::{Mutex, OnceLock};
use wgpu::util::DeviceExt;
const BATCH_PIXELS: usize = 1024 * 1024;
static ENGINE: OnceLock<std::result::Result<Mutex<Engine>, String>> = OnceLock::new();

fn engine() -> &'static std::result::Result<Mutex<Engine>, String> {
    ENGINE.get_or_init(|| {
        Engine::new().map(Mutex::new).map_err(|e| {
            let message = e.to_string();
            eprintln!(
                "GPU canvas rendering unavailable: {message}. CPU rendering remains available."
            );
            message
        })
    })
}

pub(super) fn initialize() -> Result<()> {
    engine().as_ref().map(|_| ()).map_err(invalid)
}

pub(super) fn render_surfaces(
    doc: &Document,
    size: [u32; 2],
    origin: Point,
    step: Point,
    surfaces: &super::spatial::Surfaces,
    before: Option<uuid::Uuid>,
) -> Result<Option<RgbaImage>> {
    if (before.is_none() && u64::from(size[0]) * u64::from(size[1]) < 65_536)
        || size[0] as usize > BATCH_PIXELS
    {
        return Ok(None);
    }
    let Ok(engine) = engine() else {
        return Ok(None);
    };
    let Some(scene) = Scene::compile_surfaces(doc, origin, step, surfaces, before) else {
        return Ok(None);
    };
    let mut engine = engine.lock().map_err(|_| {
        invalid("The GPU canvas worker stopped. Your document is preserved; restart the editor.")
    })?;
    engine.render(&scene, size, origin, step).map(Some)
}

pub(super) struct Engine {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    resize_pipeline: wgpu::ComputePipeline,
    motion_pipeline: wgpu::ComputePipeline,
    effects_pipeline: wgpu::ComputePipeline,
    output: wgpu::Buffer,
    readback: wgpu::Buffer,
}
impl Engine {
    pub(super) fn new() -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|e| invalid(format!("Could not select a Vulkan canvas adapter: {e}")))?;
        if adapter.get_info().device_type == wgpu::DeviceType::Cpu {
            return Err(invalid("The Vulkan canvas adapter is a software renderer"));
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Compositor canvas"),
            ..Default::default()
        }))
        .map_err(|e| invalid(format!("Could not create the Vulkan canvas device: {e}")))?;
        let errors = crate::gpu::ErrorScopes::new(&device);
        let source = [
            include_str!("gpu/composite.wgsl"),
            include_str!("gpu/blend.wgsl"),
            include_str!("gpu/adjustments.wgsl"),
        ]
        .join("\n");
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Layer compositor"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Layer compositor"),
            layout: None,
            module: &shader,
            entry_point: Some("composite"),
            compilation_options: Default::default(),
            cache: None,
        });
        let resize_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Lanczos resize"),
            source: wgpu::ShaderSource::Wgsl(include_str!("gpu/resize.wgsl").into()),
        });
        let resize_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Lanczos resize"),
            layout: None,
            module: &resize_shader,
            entry_point: Some("resize"),
            compilation_options: Default::default(),
            cache: None,
        });
        let motion_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Motion blur"),
            source: wgpu::ShaderSource::Wgsl(include_str!("gpu/motion.wgsl").into()),
        });
        let motion_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Motion blur"),
            layout: None,
            module: &motion_shader,
            entry_point: Some("motion"),
            compilation_options: Default::default(),
            cache: None,
        });
        let effects_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Layer effects"),
            source: wgpu::ShaderSource::Wgsl(include_str!("gpu/effects.wgsl").into()),
        });
        let effects_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Layer effects"),
            layout: None,
            module: &effects_shader,
            entry_point: Some("effects"),
            compilation_options: Default::default(),
            cache: None,
        });
        let output = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Canvas pixels"),
            size: (BATCH_PIXELS * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Canvas readback"),
            size: (BATCH_PIXELS * 4) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self::check(errors)?;
        Ok(Self {
            device,
            queue,
            pipeline,
            resize_pipeline,
            motion_pipeline,
            effects_pipeline,
            output,
            readback,
        })
    }
    fn check(scopes: crate::gpu::ErrorScopes) -> Result<()> {
        if let Some(error) = scopes.finish() {
            return Err(invalid(format!(
                "GPU canvas processing failed: {error}. Your project is preserved; retry the preview."
            )));
        }
        Ok(())
    }
    pub(super) fn render(
        &mut self,
        scene: &Scene,
        size: [u32; 2],
        origin: Point,
        step: Point,
    ) -> Result<RgbaImage> {
        let errors = crate::gpu::ErrorScopes::new(&self.device);
        let storage = |label, data: &[u8]| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: data,
                    usage: wgpu::BufferUsages::STORAGE,
                })
        };
        let layers = storage("Canvas layers", bytemuck::cast_slice(&scene.layers));
        let operations = storage(
            "Canvas operations",
            bytemuck::cast_slice(if scene.operations.is_empty() {
                &[[0u32; 2]]
            } else {
                &scene.operations
            }),
        );
        let settings = storage(
            "Canvas adjustments",
            bytemuck::cast_slice(&scene.adjustments),
        );
        let pixels = storage("Canvas source pixels", bytemuck::cast_slice(&scene.pixels));
        Self::check(errors)?;
        let mut result = RgbaImage::new(size[0], size[1]);
        if size[0] == 0 || size[1] == 0 {
            return Ok(result);
        }
        let rows = (BATCH_PIXELS / size[0] as usize).max(1) as u32;
        for top in (0..size[1]).step_by(rows as usize) {
            let height = rows.min(size[1] - top);
            let mut parameters = Vec::with_capacity(32);
            parameters.extend_from_slice(bytemuck::cast_slice(&[
                size[0],
                height,
                top,
                scene.operations.len() as u32,
            ]));
            parameters.extend_from_slice(bytemuck::cast_slice(&[
                origin[0] as f32,
                origin[1] as f32,
                step[0] as f32,
                step[1] as f32,
            ]));
            let errors = crate::gpu::ErrorScopes::new(&self.device);
            let uniform = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Canvas viewport"),
                    contents: &parameters,
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let bindings = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Canvas scene"),
                layout: &self.pipeline.get_bind_group_layout(0),
                entries: &[
                    &uniform,
                    &layers,
                    &operations,
                    &settings,
                    &pixels,
                    &self.output,
                ]
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
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &bindings, &[]);
                pass.dispatch_workgroups(size[0].div_ceil(16), height.div_ceil(16), 1);
            }
            let bytes = u64::from(size[0]) * u64::from(height) * 4;
            encoder.copy_buffer_to_buffer(&self.output, 0, &self.readback, 0, bytes);
            let submission = self.queue.submit([encoder.finish()]);
            let slice = self.readback.slice(..bytes);
            let (send, receive) = std::sync::mpsc::sync_channel(1);
            slice.map_async(wgpu::MapMode::Read, move |result| {
                let _ = send.send(result);
            });
            let mapped=self.device.poll(wgpu::PollType::Wait{submission_index:Some(submission),timeout:Some(std::time::Duration::from_secs(5))})
                .map_err(|e|invalid(format!("GPU canvas rendering did not finish: {e}. Your project is preserved; retry the preview.")))
                .and_then(|_|receive.recv_timeout(std::time::Duration::from_secs(1)).map_err(|e|invalid(format!("GPU canvas readback stopped: {e}"))))
                .and_then(|r|r.map_err(|e|invalid(format!("GPU canvas readback failed: {e}"))));
            if let Err(error) = mapped {
                self.readback.unmap();
                return Err(error);
            }
            let copied = slice
                .get_mapped_range()
                .map(|view| {
                    let start = top as usize * size[0] as usize * 4;
                    let raw: &mut [u8] = result.as_mut();
                    raw[start..start + view.len()].copy_from_slice(&view);
                })
                .map_err(|e| invalid(format!("Could not read GPU canvas output: {e}")));
            self.readback.unmap();
            copied?;
            Self::check(errors)?;
        }
        Ok(result)
    }
}
