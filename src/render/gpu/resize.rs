use super::{Engine, scene::ASSET_BYTES};
use crate::{Result, invalid};
use wgpu::util::DeviceExt;

/// The same f32 Lanczos3 weights as image::imageops::resize, shared by all rows.
fn axis(source: u32, destination: u32, kernels: &mut Vec<[u32; 4]>, weights: &mut Vec<f32>) {
    let ratio = source as f32 / destination as f32;
    let scale = ratio.max(1.);
    let sinc = |x: f32| {
        let p = x * std::f32::consts::PI;
        if x == 0. { 1. } else { p.sin() / p }
    };
    for index in 0..destination {
        let center = (index as f32 + 0.5) * ratio;
        let left = (center - 3. * scale).floor().clamp(0., (source - 1) as f32) as u32;
        let right = (center + 3. * scale)
            .ceil()
            .clamp((left + 1) as f32, source as f32) as u32;
        let offset = weights.len();
        let mut sum = 0.;
        for i in left..right {
            let x = (i as f32 - (center - 0.5)) / scale;
            let weight = if x.abs() < 3. {
                sinc(x) * sinc(x / 3.)
            } else {
                0.
            };
            weights.push(weight);
            sum += weight;
        }
        for weight in &mut weights[offset..] {
            *weight /= sum;
        }
        kernels.push([left, right - left, offset as u32, 0]);
    }
}

impl Engine {
    pub(super) fn resize(
        &mut self,
        source: &[u8],
        from: [u32; 2],
        to: [u32; 2],
        mask: bool,
    ) -> Result<Option<Vec<u8>>> {
        if from.contains(&0)
            || to.contains(&0)
            || source.len() > ASSET_BYTES
            || u64::from(from[0]) * u64::from(to[1]) * 16 > ASSET_BYTES as u64
            || u64::from(to[0]) * u64::from(to[1]) * 4 > ASSET_BYTES as u64
        {
            return Ok(None);
        }
        let errors = crate::gpu::ErrorScopes::new(&self.device);
        let mut kernels = Vec::new();
        let mut weights = Vec::new();
        axis(from[1], to[1], &mut kernels, &mut weights);
        axis(from[0], to[0], &mut kernels, &mut weights);
        let pipeline = &self.resize_pipeline;
        let upload = |label, data: &[u8]| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: data,
                    usage: wgpu::BufferUsages::STORAGE,
                })
        };
        let input = upload("Resize source", source);
        let coefficients = upload("Resize coefficients", bytemuck::cast_slice(&weights));
        let ranges = upload("Resize ranges", bytemuck::cast_slice(&kernels));
        let allocate = |label, size, usage| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        let intermediate = allocate(
            "Resize intermediate",
            u64::from(from[0]) * u64::from(to[1]) * 16,
            wgpu::BufferUsages::STORAGE,
        );
        let bytes = u64::from(to[0]) * u64::from(to[1]) * 4;
        let output = allocate(
            "Resized pixels",
            bytes,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        );
        let readback = allocate(
            "Resize readback",
            bytes,
            wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        );
        let mut encoder = self.device.create_command_encoder(&Default::default());
        for phase in 0..2 {
            let parameters = [from[0], from[1], to[0], to[1], phase, u32::from(mask), 0, 0];
            let uniform = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Resize parameters"),
                    contents: bytemuck::cast_slice(&parameters),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let bindings = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Resize bindings"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    &uniform,
                    &input,
                    &ranges,
                    &coefficients,
                    &intermediate,
                    &output,
                ]
                .into_iter()
                .enumerate()
                .map(|(i, b)| wgpu::BindGroupEntry {
                    binding: i as u32,
                    resource: b.as_entire_binding(),
                })
                .collect::<Vec<_>>(),
            });
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bindings, &[]);
            let width = if phase == 0 { from[0] } else { to[0] };
            pass.dispatch_workgroups(width.div_ceil(16), to[1].div_ceil(16), 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes);
        let submission = self.queue.submit([encoder.finish()]);
        let slice = readback.slice(..);
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = send.send(r);
        });
        let result = self
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(std::time::Duration::from_secs(5)),
            })
            .map_err(|e| invalid(format!("GPU preview resizing did not finish: {e}")))
            .and_then(|_| {
                receive
                    .recv_timeout(std::time::Duration::from_secs(1))
                    .map_err(|e| invalid(format!("GPU preview resize readback stopped: {e}")))
            })
            .and_then(|r| {
                r.map_err(|e| invalid(format!("GPU preview resize readback failed: {e}")))
            })
            .and_then(|_| {
                slice
                    .get_mapped_range()
                    .map(|v| v.to_vec())
                    .map_err(|e| invalid(format!("Could not read the resized preview: {e}")))
            });
        readback.unmap();
        Self::check(errors)?;
        result.map(Some)
    }
}

pub(in crate::render) fn color(
    image: &image::RgbaImage,
    size: (u32, u32),
) -> Result<Option<image::RgbaImage>> {
    if image.len() > ASSET_BYTES {
        return Ok(None);
    }
    let Ok(engine) = super::engine() else {
        return Ok(None);
    };
    let mut engine = engine.lock().map_err(|_| {
        invalid("The GPU canvas worker stopped. Restart the editor; your project is preserved.")
    })?;
    engine
        .resize(
            image.as_raw(),
            [image.width(), image.height()],
            [size.0, size.1],
            false,
        )?
        .map(|bytes| {
            image::RgbaImage::from_raw(size.0, size.1, bytes)
                .ok_or_else(|| invalid("GPU preview resizing returned the wrong pixel count."))
        })
        .transpose()
}
pub(in crate::render) fn mask(
    image: &image::GrayImage,
    size: (u32, u32),
) -> Result<Option<image::GrayImage>> {
    if image.len() * 4 > ASSET_BYTES {
        return Ok(None);
    }
    let Ok(engine) = super::engine() else {
        return Ok(None);
    };
    let mut engine = engine.lock().map_err(|_| {
        invalid("The GPU canvas worker stopped. Restart the editor; your project is preserved.")
    })?;
    let pixels: Vec<u32> = image.as_raw().iter().map(|p| u32::from(*p)).collect();
    engine
        .resize(
            bytemuck::cast_slice(&pixels),
            [image.width(), image.height()],
            [size.0, size.1],
            true,
        )?
        .map(|bytes| {
            image::GrayImage::from_raw(
                size.0,
                size.1,
                bytes.chunks_exact(4).map(|p| p[0]).collect(),
            )
            .ok_or_else(|| invalid("GPU mask resizing returned the wrong pixel count."))
        })
        .transpose()
}
