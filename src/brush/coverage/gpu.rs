//! Bounded GPU coverage processing. The document remains in the existing CPU
//! editing model; deposition and ordinary paint/erase blending run on Vulkan.
use super::{Brush, Plane, Region, Segment};
use crate::brush::{Destination, PaintMode};
use crate::{Result, invalid};
use std::sync::{Mutex, OnceLock};
use wgpu::util::DeviceExt;

const MAX_PIXELS: usize = 1024 * 1024;
static ENGINE: OnceLock<std::result::Result<Mutex<Engine>, String>> = OnceLock::new();

fn engine() -> &'static std::result::Result<Mutex<Engine>, String> {
    ENGINE.get_or_init(|| {
        Engine::new().map(Mutex::new).map_err(|error| {
            let message = error.to_string();
            eprintln!("GPU brush coverage unavailable: {message}. CPU painting remains available.");
            message
        })
    })
}

pub(crate) fn initialize() -> Result<String> {
    let engine = engine().as_ref().map_err(invalid)?;
    Ok(engine
        .lock()
        .map_err(|_| invalid("The GPU brush worker stopped."))?
        .name
        .clone())
}

pub(in crate::brush) fn rasterize(
    region: &Region,
    plane: &mut Plane,
    segment: &Segment<'_>,
    brush: Brush,
) -> Result<image::GrayImage> {
    let [left, top, right, bottom] = region.bounds;
    // Small tips avoid submission/readback overhead. Unavailable hardware keeps
    // using the reference path; failures after dispatch are reported to the editor.
    if right.saturating_sub(left) as u64 * (bottom.saturating_sub(top) as u64) < 65_536 {
        return region.rasterize(plane, segment, brush);
    }
    let Ok(engine) = engine() else {
        return region.rasterize(plane, segment, brush);
    };
    engine
        .lock()
        .map_err(|_| {
            invalid("GPU brush processing stopped. Cancel the stroke and restart the editor.")
        })?
        .rasterize(region, plane, segment, brush)
}

/// Full pixel application for unselected Paint/Eraser strokes. Other tools keep
/// their source sampling and selection semantics, using GPU coverage alone.
pub(in crate::brush) fn paint(
    region: &Region,
    plane: &mut Plane,
    segment: &Segment<'_>,
    brush: Brush,
    destination: &mut Destination<'_>,
    mode: PaintMode,
) -> Result<Option<bool>> {
    let [left, top, right, bottom] = region.bounds;
    if right.saturating_sub(left) as u64 * (bottom.saturating_sub(top) as u64) < 65_536 {
        return Ok(None);
    }
    let Destination::Image { pixels, original } = destination else {
        return Ok(None);
    };
    let operation = match mode {
        PaintMode::Paint => Operation::Paint,
        PaintMode::Erase => Operation::Erase,
        _ => return Ok(None),
    };
    let Ok(engine) = engine() else {
        return Ok(None);
    };
    let mut target = Target::Image {
        pixels,
        original,
        operation,
    };
    let result = engine
        .lock()
        .map_err(|_| {
            invalid("GPU brush processing stopped. Cancel the stroke and restart the editor.")
        })?
        .process(region, plane, segment, brush, &mut target)?;
    Ok(Some(result.pixels().any(|p| p[0] > 0)))
}

enum Operation {
    Paint,
    Erase,
}
enum Target<'a> {
    Coverage,
    Image {
        pixels: &'a mut image::RgbaImage,
        original: &'a image::RgbaImage,
        operation: Operation,
    },
}

struct Engine {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    input: wgpu::Buffer,
    output: wgpu::Buffer,
    original: wgpu::Buffer,
    readback: wgpu::Buffer,
    name: String,
}

impl Engine {
    fn new() -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|error| invalid(format!("Could not select a Vulkan adapter: {error}")))?;
        if adapter.get_info().device_type == wgpu::DeviceType::Cpu {
            return Err(invalid("The Vulkan adapter is a software renderer"));
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Compositor brush compute"),
            ..Default::default()
        }))
        .map_err(|error| invalid(format!("Could not create a GPU brush device: {error}")))?;
        let errors = crate::gpu::ErrorScopes::new(&device);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Continuous brush"),
            source: wgpu::ShaderSource::Wgsl(include_str!("brush.wgsl").into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Brush coverage"),
            layout: None,
            module: &shader,
            entry_point: Some("brush"),
            compilation_options: Default::default(),
            cache: None,
        });
        let buffer = |label, size, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        let input = buffer(
            "Brush input",
            (MAX_PIXELS * 4) as u64,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        let original = buffer(
            "Brush original pixels",
            (MAX_PIXELS * 4) as u64,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        let output = buffer(
            "Brush output",
            (MAX_PIXELS * 12) as u64,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        );
        let readback = buffer(
            "Brush readback",
            (MAX_PIXELS * 12) as u64,
            wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        );
        if let Some(error) = errors.finish() {
            return Err(invalid(format!(
                "Could not initialize the GPU brush pipeline: {error}"
            )));
        }
        Ok(Self {
            device,
            queue,
            pipeline,
            input,
            output,
            original,
            readback,
            name: adapter.get_info().name,
        })
    }

    fn rasterize(
        &self,
        region: &Region,
        plane: &mut Plane,
        segment: &Segment<'_>,
        brush: Brush,
    ) -> Result<image::GrayImage> {
        self.process(region, plane, segment, brush, &mut Target::Coverage)
    }

    fn process(
        &self,
        region: &Region,
        plane: &mut Plane,
        segment: &Segment<'_>,
        brush: Brush,
        target: &mut Target<'_>,
    ) -> Result<image::GrayImage> {
        let [left, top, right, bottom] = region.bounds;
        let width = right.saturating_sub(left);
        let height = bottom.saturating_sub(top);
        let mut changed = image::GrayImage::new(width, height);
        if width == 0 || height == 0 {
            return Ok(changed);
        }
        let chunk_rows = (MAX_PIXELS / width as usize).max(1) as u32;
        for y in (top..bottom).step_by(chunk_rows as usize) {
            let rows = chunk_rows.min(bottom - y);
            let mut values = Vec::with_capacity(width as usize * rows as usize);
            for row in y..y + rows {
                let start = row as usize * plane.width() as usize + left as usize;
                values.extend_from_slice(&plane.as_raw()[start..start + width as usize]);
            }
            let origin = region.point(left, y);
            let mut parameters = Vec::with_capacity(112);
            for field in [
                [region.dx[0], region.dx[1], region.dy[0], region.dy[1]],
                [
                    origin[0] - segment.start[0],
                    origin[1] - segment.start[1],
                    brush.diameter / 2.,
                    brush.hardness,
                ],
                [
                    -segment.start[0],
                    -segment.start[1],
                    region.canvas[0] - segment.start[0],
                    region.canvas[1] - segment.start[1],
                ],
                [
                    segment.direction[0],
                    segment.direction[1],
                    segment.length,
                    segment.antialias,
                ],
            ] {
                parameters.extend_from_slice(bytemuck::cast_slice(&field.map(|v| v as f32)));
            }
            let operation = match target {
                Target::Coverage => 0,
                Target::Image {
                    operation: Operation::Paint,
                    ..
                } => 1,
                Target::Image {
                    operation: Operation::Erase,
                    ..
                } => 2,
            };
            parameters.extend_from_slice(bytemuck::cast_slice(&[width, rows, operation, 0]));
            parameters.extend_from_slice(bytemuck::cast_slice(&[
                (brush.diameter * 0.025).max(0.25) as f32,
                brush.opacity as f32,
                0.,
                0.,
            ]));
            parameters
                .extend_from_slice(bytemuck::cast_slice(&brush.color.map(|v| v as f32 / 255.)));
            let original = match target {
                Target::Coverage => None,
                Target::Image { original, .. } => {
                    let mut data = Vec::with_capacity(values.len());
                    for row in y..y + rows {
                        for x in left..right {
                            data.push(
                                original
                                    .get_pixel_checked(x, row)
                                    .map_or(0, |p| u32::from_le_bytes(p.0)),
                            );
                        }
                    }
                    Some(data)
                }
            };
            let bytes = self.dispatch(&parameters, &values, original.as_deref(), width, rows)?;
            // Read back contiguous rows instead of dividing and bounds-checking
            // document coordinates for every pixel of a large brush.
            for (row, bytes) in bytes.chunks_exact(width as usize * 12).enumerate() {
                let plane_start = (y as usize + row) * plane.width() as usize + left as usize;
                let changed_start = (y as usize - top as usize + row) * width as usize;
                let densities: &mut [f32] = plane.as_mut();
                let densities = &mut densities[plane_start..plane_start + width as usize];
                let coverage: &mut [u8] = changed.as_mut();
                let coverage = &mut coverage[changed_start..changed_start + width as usize];
                for ((density, changed), pixel) in densities
                    .iter_mut()
                    .zip(coverage)
                    .zip(bytes.chunks_exact(12))
                {
                    let value = f32::from_ne_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]);
                    let coverage = u32::from_ne_bytes([pixel[4], pixel[5], pixel[6], pixel[7]]);
                    if !value.is_finite() || value < 0. || coverage > 255 {
                        return Err(invalid(
                            "The GPU returned invalid brush coverage. Cancel the stroke and restart the editor.",
                        ));
                    }
                    *density = value;
                    *changed = coverage as u8;
                }
                if let Target::Image { pixels, .. } = target {
                    let start = ((y as usize + row) * pixels.width() as usize + left as usize) * 4;
                    let pixels: &mut [u8] = pixels.as_mut();
                    for (destination, pixel) in pixels[start..start + width as usize * 4]
                        .chunks_exact_mut(4)
                        .zip(bytes.chunks_exact(12))
                    {
                        if pixel[4] > 0 {
                            destination.copy_from_slice(&pixel[8..12]);
                        }
                    }
                }
            }
        }
        Ok(changed)
    }

    fn dispatch(
        &self,
        parameters: &[u8],
        values: &[f32],
        original: Option<&[u32]>,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>> {
        let errors = crate::gpu::ErrorScopes::new(&self.device);
        let uniform = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Brush parameters"),
                contents: parameters,
                usage: wgpu::BufferUsages::UNIFORM,
            });
        self.queue
            .write_buffer(&self.input, 0, bytemuck::cast_slice(values));
        if let Some(original) = original {
            self.queue
                .write_buffer(&self.original, 0, bytemuck::cast_slice(original));
        }
        let bindings = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Brush bindings"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.input.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.output.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.original.as_entire_binding(),
                },
            ],
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bindings, &[]);
            pass.dispatch_workgroups(width.div_ceil(16), height.div_ceil(16), 1);
        }
        let size = (values.len() * 12) as u64;
        encoder.copy_buffer_to_buffer(&self.output, 0, &self.readback, 0, size);
        let submission = self.queue.submit([encoder.finish()]);
        let slice = self.readback.slice(..size);
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = send.send(result);
        });
        let waited = self.device.poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(std::time::Duration::from_secs(5)),
        });
        if let Err(error) = waited {
            self.readback.unmap();
            return Err(invalid(format!(
                "GPU brush processing did not finish: {error}. Cancel the stroke and restart the editor."
            )));
        }
        let mapped = receive
            .recv_timeout(std::time::Duration::from_secs(1))
            .map_err(|e| invalid(format!("GPU brush readback stopped: {e}")))
            .and_then(|result| {
                result.map_err(|e| invalid(format!("GPU brush readback failed: {e}")))
            });
        if let Err(error) = mapped {
            self.readback.unmap();
            return Err(error);
        }
        let bytes = slice
            .get_mapped_range()
            .map(|view| view.to_vec())
            .map_err(|error| invalid(format!("Could not read GPU brush output: {error}")));
        self.readback.unmap();
        if let Some(error) = errors.finish() {
            return Err(invalid(format!(
                "GPU brush dispatch failed: {error}. Cancel the stroke and restart the editor."
            )));
        }
        bytes
    }
}

#[cfg(test)]
mod tests;
