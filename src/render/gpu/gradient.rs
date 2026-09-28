//! Raster gradients share the canvas device. Coverage remains in document-space
//! double precision so transformed selection and canvas edges stay exact.
use super::Engine;
use crate::{
    Result,
    geometry::{Point, Transform},
    gradient::{Gradient, Shape, stops::Stops},
    invalid,
    selection::Selection,
};
use image::{GrayImage, RgbaImage};
use wgpu::util::DeviceExt;

pub(crate) struct Paint<'a> {
    pub gradient: &'a Gradient,
    pub ramp: &'a Stops,
    pub start: Point,
    pub end: Point,
    pub transform: Transform,
    pub canvas: Point,
    pub selection: Option<&'a Selection>,
}

pub(crate) enum Pixels<'a> {
    Color(&'a RgbaImage),
    Mask(&'a GrayImage),
}
impl Pixels<'_> {
    fn dimensions(&self) -> (u32, u32) {
        match self {
            Self::Color(pixels) => pixels.dimensions(),
            Self::Mask(pixels) => pixels.dimensions(),
        }
    }
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Color(pixels) => pixels.as_raw(),
            Self::Mask(pixels) => pixels.as_raw(),
        }
    }
    fn channels(&self) -> usize {
        match self {
            Self::Color(_) => 4,
            Self::Mask(_) => 1,
        }
    }
}

pub(super) fn pipeline(device: &wgpu::Device) -> wgpu::ComputePipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Raster gradient"),
        source: wgpu::ShaderSource::Wgsl(include_str!("gradient.wgsl").into()),
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Raster gradient"),
        layout: None,
        module: &shader,
        entry_point: Some("apply"),
        compilation_options: Default::default(),
        cache: None,
    })
}

impl Paint<'_> {
    fn supported(&self) -> bool {
        // f32 cannot preserve which side of a coincident/near-coincident stop a
        // pixel occupies. Keep those ramps and projective placements on the CPU.
        if self.transform.warp.is_some() {
            return false;
        }
        let Ok(mapping) = self.transform.affine_mapping() else {
            return false;
        };
        let matrix = mapping.matrix();
        let matrix = matrix.map(|value| value / matrix[8]);
        let length = (self.end[0] - self.start[0]).hypot(self.end[1] - self.start[1]);
        let extent = matrix[0].abs()
            + matrix[1].abs()
            + (matrix[2] - self.start[0]).abs()
            + matrix[3].abs()
            + matrix[4].abs()
            + (matrix[5] - self.start[1]).abs();
        let error = 8. * f64::from(f32::EPSILON) * extent / length;
        self.ramp.as_slice().windows(2).all(|stops| {
            let gap = stops[1].position - stops[0].position;
            gap >= 0.001 && error / gap < 0.001
        })
    }

    fn full_coverage(&self) -> bool {
        let bounds = self.transform.bounds();
        self.selection.is_none()
            && bounds[0] >= 0.
            && bounds[1] >= 0.
            && bounds[2] <= self.canvas[0]
            && bounds[3] <= self.canvas[1]
    }

    fn coverage(&self, index: usize, width: u32, height: u32) -> u32 {
        let point = self.transform.point([
            ((index % width as usize) as f64 + 0.5) / f64::from(width),
            ((index / width as usize) as f64 + 0.5) / f64::from(height),
        ]);
        if point[0] < 0.
            || point[1] < 0.
            || point[0] >= self.canvas[0]
            || point[1] >= self.canvas[1]
        {
            0
        } else {
            (self
                .selection
                .map_or(1., |selection| selection.coverage(point))
                * 255.)
                .round() as u32
        }
    }
}

pub(crate) fn apply(pixels: Pixels<'_>, paint: &Paint<'_>) -> Result<Option<Vec<u8>>> {
    let (width, height) = pixels.dimensions();
    if u64::from(width) * u64::from(height) < 65_536 || !paint.supported() {
        return Ok(None);
    }
    let Ok(engine) = super::engine() else {
        return Ok(None);
    };
    let engine = engine.lock().map_err(|_| invalid(
        "The GPU gradient worker stopped. Restart the editor; the original pixels are preserved.",
    ))?;
    engine.gradient(&pixels, paint).map(Some)
}

impl Engine {
    fn gradient(&self, pixels: &Pixels<'_>, paint: &Paint<'_>) -> Result<Vec<u8>> {
        let limits = self.device.limits();
        // At most 16 MiB per storage buffer, independent of canvas size.
        let chunk_pixels = limits
            .max_storage_buffer_binding_size
            .min(limits.max_buffer_size)
            .min(u64::from(limits.max_compute_workgroups_per_dimension) * 256 * 4)
            .min(16 * 1024 * 1024) as usize
            / 4;
        let channels = pixels.channels();
        let mut output = vec![0; pixels.bytes().len()];
        for (chunk, (source, target)) in pixels
            .bytes()
            .chunks(chunk_pixels * channels)
            .zip(output.chunks_mut(chunk_pixels * channels))
            .enumerate()
        {
            self.gradient_chunk(source, target, pixels, paint, chunk * chunk_pixels)?;
        }
        Ok(output)
    }

    fn gradient_chunk(
        &self,
        source: &[u8],
        target: &mut [u8],
        pixels: &Pixels<'_>,
        paint: &Paint<'_>,
        offset: usize,
    ) -> Result<()> {
        let errors = crate::gpu::ErrorScopes::new(&self.device);
        let (width, height) = pixels.dimensions();
        let channels = pixels.channels();
        let count = source.len() / channels;
        let full_coverage = paint.full_coverage();
        let mask_source;
        let source = if channels == 1 {
            mask_source = source
                .iter()
                .map(|value| u32::from(*value))
                .collect::<Vec<_>>();
            bytemuck::cast_slice(&mask_source)
        } else {
            source
        };
        let input = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Gradient source"),
                contents: source,
                usage: wgpu::BufferUsages::STORAGE,
            });
        let coverage = if full_coverage {
            vec![255]
        } else {
            (offset..offset + count)
                .map(|index| paint.coverage(index, width, height))
                .collect()
        };
        let coverage = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Gradient selection"),
                contents: bytemuck::cast_slice(&coverage),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let bytes = (count * 4) as u64;
        let output = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Gradient output"),
            size: bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Gradient readback"),
            size: bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut parameters = vec![width, offset as u32, count as u32, u32::from(channels == 1)];
        parameters.extend([
            u32::from(paint.gradient.shape == Shape::Radial),
            u32::from(paint.gradient.reversed),
            paint.ramp.as_slice().len() as u32,
            u32::from(full_coverage),
        ]);
        let matrix = paint
            .transform
            .affine_mapping()
            .map_err(|error| invalid(error.to_string()))?
            .matrix();
        // Homographies normalize all coefficients, including the denominator.
        let matrix = matrix.map(|value| value / matrix[8]);
        let dx = paint.end[0] - paint.start[0];
        let dy = paint.end[1] - paint.start[1];
        let length = dx.hypot(dy);
        // Translate before casting, avoiding cancellation for layers far from the origin.
        let floats = [
            matrix[0] / f64::from(width),
            matrix[1] / f64::from(height),
            matrix[2] - paint.start[0],
            0.,
            matrix[3] / f64::from(width),
            matrix[4] / f64::from(height),
            matrix[5] - paint.start[1],
            0.,
            dx / length,
            dy / length,
            1. / length,
            paint.gradient.opacity,
        ];
        parameters.extend(floats.map(|value| (value as f32).to_bits()));
        let mut stops = Vec::with_capacity(paint.ramp.as_slice().len() * 8);
        for stop in paint.ramp.as_slice() {
            stops.extend(stop.color.map(|value| f32::from(value) / 255.));
            stops.extend([stop.position as f32, 0., 0., 0.]);
        }
        let stops = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Gradient stops"),
                contents: bytemuck::cast_slice(&stops),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let uniform = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Gradient geometry"),
                contents: bytemuck::cast_slice(&parameters),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bindings = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Gradient bindings"),
            layout: &self.gradient_pipeline.get_bind_group_layout(0),
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
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: stops.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: coverage.as_entire_binding(),
                },
            ],
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut compute = encoder.begin_compute_pass(&Default::default());
            compute.set_pipeline(&self.gradient_pipeline);
            compute.set_bind_group(0, &bindings, &[]);
            compute.dispatch_workgroups((count as u32).div_ceil(256), 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes);
        self.readback(
            encoder,
            &readback,
            errors,
            super::readback::Operation::Gradient,
            |bytes| {
                if channels == 4 {
                    target.copy_from_slice(bytes);
                } else {
                    for (target, pixel) in target.iter_mut().zip(bytes.chunks_exact(4)) {
                        *target = pixel[0];
                    }
                }
            },
        )
    }
}

#[cfg(test)]
mod tests;
