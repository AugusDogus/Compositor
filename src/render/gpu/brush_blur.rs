//! The brush reference uses image's Gaussian kernel, not the effect kernel's
//! three-sigma cutoff. Keep its weights and premultiplied byte quantization.
use crate::{Result, invalid};
use image::RgbaImage;
use wgpu::util::DeviceExt;

pub(super) fn pipeline(device: &wgpu::Device) -> wgpu::ComputePipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Brush Gaussian"),
        source: wgpu::ShaderSource::Wgsl(include_str!("brush_blur.wgsl").into()),
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Brush Gaussian"),
        layout: None,
        module: &shader,
        entry_point: Some("blur"),
        compilation_options: Default::default(),
        cache: None,
    })
}

pub(in crate::render) fn blur(image: &RgbaImage, sigma: f32) -> Result<Option<RgbaImage>> {
    let Ok(engine) = super::engine() else {
        return Ok(None);
    };
    let engine = engine.lock().map_err(|_| {
        invalid(
            "The GPU blur worker stopped. Restart the editor; the original pixels are preserved.",
        )
    })?;
    let device = &engine.device;
    let bytes = image.len() as u64 * 4;
    if bytes > device.limits().max_storage_buffer_binding_size {
        return Ok(None);
    }
    // Match image 0.25's GaussianBlurParameters::new_from_sigma.
    let mut width = (((((sigma - 0.8) / 0.3) + 1.) * 2.) + 1.).max(3.) as u32;
    width |= 1;
    let radius = width / 2;
    let scale = 1. / ((2. * std::f32::consts::PI).sqrt() * sigma);
    let mut weights: Vec<f32> = (0..width)
        .map(|x| (-0.5 * ((x as f32 - radius as f32) / sigma).powf(2.)).exp() * scale)
        .collect();
    let normalizer = 1. / weights.iter().sum::<f32>();
    for weight in &mut weights {
        *weight *= normalizer;
    }
    let errors = crate::gpu::ErrorScopes::new(device);
    let input: Vec<f32> = image.as_raw().iter().map(|v| f32::from(*v)).collect();
    let input = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Blur input"),
        contents: bytemuck::cast_slice(&input),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let weights = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Blur weights"),
        contents: bytemuck::cast_slice(&weights),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let allocate = |label, usage| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: bytes,
            usage,
            mapped_at_creation: false,
        })
    };
    let rows = allocate("Blur rows", wgpu::BufferUsages::STORAGE);
    let output = allocate(
        "Blur output",
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    );
    let readback = allocate(
        "Blur readback",
        wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
    );
    let mut encoder = device.create_command_encoder(&Default::default());
    for (vertical, source, destination) in [(0, &input, &rows), (1, &rows, &output)] {
        let params = [image.width(), image.height(), radius, vertical];
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Blur parameters"),
            contents: bytemuck::cast_slice(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Blur bindings"),
            layout: &engine.brush_blur_pipeline.get_bind_group_layout(0),
            entries: &[&uniform, source, &weights, destination]
                .into_iter()
                .enumerate()
                .map(|(binding, buffer)| wgpu::BindGroupEntry {
                    binding: binding as u32,
                    resource: buffer.as_entire_binding(),
                })
                .collect::<Vec<_>>(),
        });
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&engine.brush_blur_pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.dispatch_workgroups(image.width().div_ceil(16), image.height().div_ceil(16), 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes);
    let pixels = engine.readback(
        encoder,
        &readback,
        errors,
        super::readback::Operation::BrushBlur,
        |bytes| {
            bytes
                .chunks_exact(4)
                .map(|v| {
                    f32::from_le_bytes([v[0], v[1], v[2], v[3]])
                        .round()
                        .clamp(0., 255.) as u8
                })
                .collect()
        },
    )?;
    RgbaImage::from_raw(image.width(), image.height(), pixels)
        .map(Some)
        .ok_or_else(|| invalid("GPU brush blur returned the wrong pixel count."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Requires a hardware Vulkan adapter"]
    fn brush_gaussian_matches_reference_kernel_and_canvas_edges() {
        let source = RgbaImage::from_fn(129, 117, |x, y| {
            image::Rgba([
                (x * 31 + y * 11) as u8,
                (x * 7 + y * 41) as u8,
                if x < 64 { 0 } else { 255 },
                if y < 45 { 7 } else { 255 },
            ])
        });
        let source = crate::native_pixels::premultiply(&source);
        for sigma in [1.5, 9.7, 30.] {
            let expected = image::imageops::blur(&source, sigma);
            let actual = blur(&source, sigma)
                .unwrap()
                .expect("Hardware GPU required");
            let largest = actual
                .as_raw()
                .iter()
                .zip(expected.as_raw())
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap();
            assert!(largest <= 1, "sigma {sigma}: byte error {largest}");
        }
    }
}
