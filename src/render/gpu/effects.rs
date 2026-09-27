use super::Engine;
use crate::{Result, effects::LayerEffects, invalid};
use image::RgbaImage;
use wgpu::util::DeviceExt;

mod gradient;
// Shared by effects and coverage blur, matching the full effects.wgsl layout.
pub(super) const PARAMETER_WORDS: usize = gradient::PARAMETER_WORDS;

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
        let pattern = effects.pattern_overlay.as_ref();
        let pattern_bytes = pattern.map_or(0, |s| s.pattern.pixels().len() as u64);
        if bytes + pattern_bytes > self.device.limits().max_storage_buffer_binding_size {
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
        let mut combined = Vec::new();
        let pixel_bytes = if let Some(pattern) = pattern {
            combined.reserve(image.len() + pattern.pattern.pixels().len());
            combined.extend_from_slice(image.as_raw());
            combined.extend_from_slice(pattern.pattern.pixels().as_raw());
            combined.as_slice()
        } else {
            image.as_raw().as_slice()
        };
        let pixels = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Effects source"),
                contents: pixel_bytes,
                usage: storage,
            });
        let planes: Vec<_> = (0..9)
            .map(|_| allocate("Effects coverage", bytes, storage))
            .collect();
        let [
            shape,
            first,
            second,
            ring,
            shadow,
            inner,
            glow,
            inner_glow,
            scratch,
        ] = [
            &planes[0], &planes[1], &planes[2], &planes[3], &planes[4], &planes[5], &planes[6],
            &planes[7], &planes[8],
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
        let outer = effects
            .outer_glow
            .as_ref()
            .filter(|s| s.size > 0. && s.opacity > 0.);
        let interior_glow = effects
            .inner_glow
            .as_ref()
            .filter(|s| s.size > 0. && s.opacity > 0.);
        let mut params = [0u32; PARAMETER_WORDS];
        gradient::encode(&mut params, [image.width(), image.height()], effects)?;
        if let Some(s) = pattern {
            params[40] = (bytes / 4) as u32;
            params[41] = s.pattern.pixels().width();
            params[42] = s.pattern.pixels().height();
            params[43] = 1;
            params[44] = (s.settings.scale as f32).to_bits();
            params[45] = (s.settings.opacity as f32).to_bits();
            params[46] = (effects.margin() as f32).to_bits();
        }
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
        for (c, v) in outer
            .map_or([0.; 4], |s| [s.red, s.green, s.blue, s.opacity])
            .into_iter()
            .enumerate()
        {
            params[28 + c] = (v as f32).to_bits();
        }
        params[32] = u32::from(outer.is_some());
        params[33] = u32::from(interior_glow.is_some());
        for (c, v) in interior_glow
            .map_or([0.; 4], |s| [s.red, s.green, s.blue, s.opacity])
            .into_iter()
            .enumerate()
        {
            params[36 + c] = (v as f32).to_bits();
        }
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
            let (r, s, i, g) = if phase == 7 {
                (ring, shadow, inner, glow)
            } else {
                (&dummy, &dummy, &dummy, &dummy)
            };
            let bindings = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Effects bindings"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[&uniform, &pixels, input, destination, r, s, i, &output, g]
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
        if let Some(s) = outer {
            if s.size <= 0.02 {
                run(4, [0.; 2], shape, glow);
            } else {
                run(5, [(s.size / 2.) as f32, 0.], shape, first);
                run(6, [(s.size / 2.) as f32, 0.], first, glow);
            }
        }
        if let Some(s) = interior_glow {
            if s.size <= 0.02 {
                run(4, [0.; 2], shape, inner_glow);
            } else {
                run(5, [(s.size / 2.) as f32, 0.], shape, first);
                run(6, [(s.size / 2.) as f32, 0.], first, inner_glow);
            }
        }
        // The composition phase does not otherwise read its input plane.
        run(7, [0.; 2], inner_glow, scratch);
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes);
        let result = self.readback(
            encoder,
            &readback,
            errors,
            super::readback::Operation::Effects,
            <[u8]>::to_vec,
        );
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
    use crate::effects::{
        ColorOverlayEffect, InnerGlowEffect, OuterGlowEffect, ShadowEffect, StrokeEffect,
    };
    use image::Rgba;
    #[test]
    #[ignore = "Requires a hardware Vulkan adapter"]
    fn pattern_gpu_matches_cpu_for_scaled_transparent_tiles_and_effect_padding() {
        let mut engine = Engine::new().unwrap();
        let image = RgbaImage::from_fn(31, 23, |x, y| {
            Rgba([73, 91, 121, if x < 3 || y < 3 { 0 } else { (x * 7) as u8 }])
        });
        let pattern = crate::pattern::Pattern::from_pixels(
            "Alpha tile",
            RgbaImage::from_fn(3, 2, |x, y| {
                Rgba([
                    x as u8 * 120,
                    y as u8 * 240,
                    91,
                    if x == y { 0 } else { 173 },
                ])
            }),
        )
        .unwrap();
        for scale in [0.05, 0.7, 1., 1.7, 20.] {
            let mut overlay = crate::pattern::Overlay::new(pattern.clone());
            overlay.settings.scale = scale;
            overlay.settings.opacity = 0.73;
            let effects = LayerEffects {
                pattern_overlay: Some(Box::new(overlay)),
                color_overlay: Some(ColorOverlayEffect {
                    red: 0.4,
                    opacity: 0.2,
                    ..Default::default()
                }),
                shadow: Some(ShadowEffect {
                    blur: 1.3,
                    distance: 3.7,
                    ..Default::default()
                }),
                ..Default::default()
            };
            let gpu = engine.effects(&image, &effects).unwrap().unwrap();
            let cpu = crate::effects::cpu::render(&image, &effects).unwrap();
            for (i, (a, b)) in gpu.as_raw().iter().zip(cpu.as_raw()).enumerate() {
                assert!(
                    a.abs_diff(*b) <= 1,
                    "scale={scale} byte={i} gpu={a} cpu={b}"
                );
            }
        }
    }
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
                pattern_overlay: None,
                gradient_overlay: None,
                inner_glow: Some(InnerGlowEffect {
                    size: 6.3,
                    red: 0.4,
                    opacity: 0.6,
                    ..Default::default()
                }),
                outer_glow: Some(OuterGlowEffect {
                    size: 5.7,
                    green: 0.3,
                    opacity: 0.8,
                    ..Default::default()
                }),
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
            let cpu = crate::effects::cpu::render(&image, &effects).unwrap();
            for (i, (a, b)) in gpu.as_raw().iter().zip(cpu.as_raw()).enumerate() {
                assert!(
                    a.abs_diff(*b) <= 1,
                    "inside={inside},byte {i}:GPU {a},CPU {b}"
                );
            }
        }
    }
}
