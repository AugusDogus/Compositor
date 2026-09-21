use super::Engine;
use crate::{Result, effects::LayerEffects, invalid};
use image::RgbaImage;
use wgpu::util::DeviceExt;

pub(in crate::render) fn render(
    image: &RgbaImage,
    effects: &LayerEffects,
) -> Result<Option<RgbaImage>> {
    let Ok(engine) = super::engine() else {
        return Ok(None);
    };
    let mut engine = engine.lock().map_err(|_| {
        invalid(
            "The GPU effects worker stopped. Restart the editor; your source pixels are preserved.",
        )
    })?;
    engine.effects(image, effects)
}
impl Engine {
    fn effects(&mut self, image: &RgbaImage, effects: &LayerEffects) -> Result<Option<RgbaImage>> {
        let bytes = image.len() as u64;
        if bytes > self.device.limits().max_storage_buffer_binding_size {
            return Ok(None);
        }
        let errors = crate::gpu::ErrorScopes::new(&self.device);
        let allocate = |label, size, usage| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        let storage = wgpu::BufferUsages::STORAGE;
        let pixels = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Effects source"),
                contents: image.as_raw(),
                usage: storage,
            });
        let planes: Vec<_> = (0..7)
            .map(|_| allocate("Effects coverage", bytes, storage))
            .collect();
        let [shape, first, second, ring, shadow, inner, scratch] = [
            &planes[0], &planes[1], &planes[2], &planes[3], &planes[4], &planes[5], &planes[6],
        ];
        let dummy = allocate("Unused effect plane", 4, storage);
        let output = allocate(
            "Effects pixels",
            bytes,
            storage | wgpu::BufferUsages::COPY_SRC,
        );
        let readback = allocate(
            "Effects readback",
            bytes,
            wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        );
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let stroke = effects.stroke.as_ref();
        let drop = effects.shadow.as_ref();
        let inside = effects.inner_shadow.as_ref();
        let overlay = effects.color_overlay.as_ref();
        let mut params = [0u32; 28];
        params[0] = image.width();
        params[1] = image.height();
        let colors = [
            stroke.map(|s| [s.red, s.green, s.blue, s.opacity]),
            drop.map(|s| [s.red, s.green, s.blue, s.opacity]),
            overlay.map(|s| [s.red, s.green, s.blue, s.opacity]),
            inside.map(|s| [s.red, s.green, s.blue, s.opacity]),
        ];
        for (i, color) in colors.into_iter().enumerate() {
            for (c, v) in color.unwrap_or([0.; 4]).into_iter().enumerate() {
                params[8 + i * 4 + c] = (v as f32).to_bits();
            }
        }
        params[24] = u32::from(stroke.is_some());
        params[25] = u32::from(stroke.is_some_and(|s| s.inside));
        params[26] = u32::from(drop.is_some());
        params[27] = u32::from(inside.is_some());
        let pipeline = &self.effects_pipeline;
        let mut run = |phase: u32,
                       geometry: [f32; 2],
                       input: &wgpu::Buffer,
                       destination: &wgpu::Buffer| {
            params[2] = phase;
            params[4] = geometry[0].to_bits();
            params[5] = geometry[1].to_bits();
            let uniform = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Effects parameters"),
                    contents: bytemuck::cast_slice(&params),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let (r, s, i) = if phase == 7 {
                (ring, shadow, inner)
            } else {
                (&dummy, &dummy, &dummy)
            };
            let bindings = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Effects bindings"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[&uniform, &pixels, input, destination, r, s, i, &output]
                    .into_iter()
                    .enumerate()
                    .map(|(binding, b)| wgpu::BindGroupEntry {
                        binding: binding as u32,
                        resource: b.as_entire_binding(),
                    })
                    .collect::<Vec<_>>(),
            });
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bindings, &[]);
            pass.dispatch_workgroups(image.width().div_ceil(16), image.height().div_ceil(16), 1);
        };
        run(0, [0.; 2], &dummy, shape);
        if let Some(s) = stroke {
            run(1, [s.size.ceil() as f32, 0.], shape, first);
            run(2, [s.size.ceil() as f32, 0.], first, second);
            run(3, [0.; 2], second, ring);
        }
        for (settings, destination) in [(drop, shadow), (inside, inner)] {
            if let Some(s) = settings {
                run(4, s.offset(), shape, first);
                run(5, [s.blur as f32, 0.], first, second);
                run(6, [s.blur as f32, 0.], second, destination);
            }
        }
        run(7, [0.; 2], shape, scratch);
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
                timeout: Some(std::time::Duration::from_secs(30)),
            })
            .map_err(|e| invalid(format!("GPU layer effects did not finish: {e}")))
            .and_then(|_| {
                receive
                    .recv_timeout(std::time::Duration::from_secs(1))
                    .map_err(|e| invalid(format!("GPU effects readback stopped: {e}")))
            })
            .and_then(|r| r.map_err(|e| invalid(format!("GPU effects readback failed: {e}"))))
            .and_then(|_| {
                slice
                    .get_mapped_range()
                    .map(|v| v.to_vec())
                    .map_err(|e| invalid(format!("Could not read GPU effects: {e}")))
            });
        readback.unmap();
        Self::check(errors)?;
        result
            .and_then(|pixels| {
                RgbaImage::from_raw(image.width(), image.height(), pixels)
                    .ok_or_else(|| invalid("GPU effects returned an unexpected pixel count."))
            })
            .map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::{ColorOverlayEffect, ShadowEffect, StrokeEffect};
    use image::Rgba;
    #[test]
    #[ignore = "Requires a hardware Vulkan adapter"]
    fn effects_gpu_matches_reference_for_every_effect_and_fractional_shadow() {
        let mut engine = Engine::new().unwrap();
        let image = RgbaImage::from_fn(67, 59, |x, y| {
            if (15..49).contains(&x) && (12..45).contains(&y) {
                Rgba([
                    32,
                    (x * 4) as u8,
                    (y * 5) as u8,
                    if x < 22 { 128 } else { 255 },
                ])
            } else {
                Rgba([0; 4])
            }
        });
        for inside in [false, true] {
            let effects = LayerEffects {
                stroke: Some(StrokeEffect {
                    size: 3.,
                    inside,
                    red: 1.,
                    green: 0.2,
                    opacity: 0.7,
                    ..Default::default()
                }),
                shadow: Some(ShadowEffect {
                    angle: 43.,
                    distance: 7.5,
                    blur: 2.3,
                    opacity: 0.6,
                    ..Default::default()
                }),
                color_overlay: Some(ColorOverlayEffect {
                    green: 1.,
                    opacity: 0.2,
                    ..Default::default()
                }),
                inner_shadow: Some(ShadowEffect {
                    angle: -27.,
                    distance: 2.7,
                    blur: 1.2,
                    opacity: 0.6,
                    ..ShadowEffect::inner_default()
                }),
            };
            let gpu = engine.effects(&image, &effects).unwrap().unwrap();
            let cpu = crate::effects::cpu::render(&image, &effects);
            for (i, (a, b)) in gpu.as_raw().iter().zip(cpu.as_raw()).enumerate() {
                assert!(
                    a.abs_diff(*b) <= 1,
                    "inside={inside},byte {i}:GPU {a},CPU {b}"
                );
            }
        }
    }
}
