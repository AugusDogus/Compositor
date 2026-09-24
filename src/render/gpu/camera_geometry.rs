use crate::{Result, invalid};
use image::RgbaImage;
use wgpu::util::DeviceExt;
pub(in crate::render) fn warp(image: &RgbaImage, matrix: [f32; 9]) -> Result<Option<RgbaImage>> {
    let Ok(engine) = super::engine() else {
        return Ok(None);
    };
    let engine = engine.lock().map_err(|_| {
        invalid(
            "The GPU geometry worker stopped. Restart the editor; the original image is preserved.",
        )
    })?;
    let device = &engine.device;
    let bytes = image.len() as u64;
    if bytes > device.limits().max_storage_buffer_binding_size {
        return Ok(None);
    }
    let errors = crate::gpu::ErrorScopes::new(device);
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Camera Raw geometry"),
        source: wgpu::ShaderSource::Wgsl(include_str!("camera_geometry.wgsl").into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Camera Raw geometry"),
        layout: None,
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let mut params = [0u32; 16];
    params[0] = image.width();
    params[1] = image.height();
    for row in 0..3 {
        for col in 0..3 {
            params[4 + row * 4 + col] = matrix[row * 3 + col].to_bits();
        }
    }
    let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Geometry matrix"),
        contents: bytemuck::cast_slice(&params),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let input = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Geometry source"),
        contents: image.as_raw(),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Geometry output"),
        size: bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Geometry readback"),
        size: bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Geometry bindings"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[&uniform, &input, &output]
            .into_iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: b.as_entire_binding(),
            })
            .collect::<Vec<_>>(),
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.dispatch_workgroups(image.width().div_ceil(16), image.height().div_ceil(16), 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes);
    let submission = engine.queue.submit([encoder.finish()]);
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = send.send(result);
        });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(std::time::Duration::from_secs(30)),
        })
        .map_err(|e| invalid(format!("Camera Raw geometry GPU wait failed: {e}")))?;
    receive
        .recv()
        .map_err(|e| invalid(format!("Camera Raw geometry readback stopped: {e}")))?
        .map_err(|e| invalid(format!("Camera Raw geometry readback failed: {e}")))?;
    let pixels = readback
        .slice(..)
        .get_mapped_range()
        .map_err(|error| invalid(format!("Could not map Camera Raw geometry output: {error}")))?
        .to_vec();
    readback.unmap();
    if let Some(error) = errors.finish() {
        return Err(invalid(format!("Camera Raw geometry failed: {error}")));
    }
    RgbaImage::from_raw(image.width(), image.height(), pixels)
        .map(Some)
        .ok_or_else(|| invalid("Camera Raw geometry returned invalid dimensions."))
}
