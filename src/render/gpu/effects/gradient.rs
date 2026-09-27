use crate::{
    Result,
    effects::LayerEffects,
    gradient::stops::MAX_STOPS,
    gradient_overlay::{Geometry, Style},
};

// Base effects occupy twelve vec4 blocks; gradients add four vec4 blocks
// plus one vec4 per color/opacity stop and packed integer boundary keys.
// Mirrors effects.wgsl.
pub(super) const PARAMETER_WORDS: usize = 48 + 16 + MAX_STOPS * 10;

pub(super) fn encode(
    parameters: &mut [u32; PARAMETER_WORDS],
    size: [u32; 2],
    effects: &LayerEffects,
) -> Result<()> {
    let Some(overlay) = &effects.gradient_overlay else {
        return Ok(());
    };
    let geometry = Geometry::padded(overlay, size, effects.margin())?;
    parameters[48] = 1;
    parameters[49] = u32::from(overlay.style == Style::Radial);
    parameters[50] = overlay.stops.as_slice().len() as u32;
    parameters[51] = overlay.opacity_stops.as_slice().len() as u32;
    parameters[52] = (overlay.opacity as f32).to_bits();
    parameters[53] = f32::from(overlay.reverse).to_bits();
    parameters[54] = (geometry.size[0] as f32).to_bits();
    parameters[55] = (geometry.size[1] as f32).to_bits();
    parameters[56] = geometry.direction[0] as u32;
    parameters[57] = geometry.direction[1] as u32;
    parameters[58] = (effects.margin() as f32).to_bits();
    parameters[60] = geometry.extent as u32;
    parameters[61] = geometry.minimum as u32;
    for (index, stop) in overlay.stops.as_slice().iter().enumerate() {
        parameters[64 + MAX_STOPS * 8 + index] = geometry.stop_key(stop.position) as u32;
        let values = [
            stop.position as f32,
            f32::from(stop.color[0]) / 255.,
            f32::from(stop.color[1]) / 255.,
            f32::from(stop.color[2]) / 255.,
        ];
        for (axis, value) in values.into_iter().enumerate() {
            parameters[64 + index * 4 + axis] = value.to_bits();
        }
    }
    for (index, stop) in overlay.opacity_stops.as_slice().iter().enumerate() {
        parameters[64 + MAX_STOPS * 9 + index] = geometry.stop_key(stop.position) as u32;
        parameters[64 + MAX_STOPS * 4 + index * 4] = (stop.position as f32).to_bits();
        parameters[65 + MAX_STOPS * 4 + index * 4] = (stop.opacity as f32).to_bits();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        gradient::stops::{Stop, Stops},
        gradient_overlay::{OpacityStop, OpacityStops, Overlay},
        render::gpu::Engine,
    };
    use image::{Rgba, RgbaImage};
    #[test]
    #[ignore = "Requires a hardware Vulkan adapter"]
    fn gradient_overlay_gpu_matches_cpu_for_angles_radial_hard_edges_and_alpha() {
        let mut engine = Engine::new().unwrap();
        let mut image = RgbaImage::new(36, 22);
        for y in 2..20 {
            for x in 2..34 {
                image.put_pixel(
                    x,
                    y,
                    Rgba([37, 91, 180, if x % 3 == 0 { 128 } else { 255 }]),
                );
            }
        }
        for style in [Style::Linear, Style::Radial] {
            for angle in [0., 45., 90., -135.] {
                for reverse in [false, true] {
                    let overlay = Overlay {
                        style,
                        angle,
                        reverse,
                        opacity: 0.73,
                        stops: Stops::new(vec![
                            Stop {
                                position: 0.,
                                color: [255, 0, 0, 255],
                            },
                            Stop {
                                position: 0.5,
                                color: [0, 255, 0, 255],
                            },
                            Stop {
                                position: 0.5,
                                color: [0, 0, 255, 255],
                            },
                            Stop {
                                position: 1.,
                                color: [255; 4],
                            },
                        ])
                        .unwrap(),
                        opacity_stops: OpacityStops::new(vec![
                            OpacityStop {
                                position: 0.,
                                opacity: 0.123456789,
                            },
                            OpacityStop {
                                position: 0.37,
                                opacity: 0.7,
                            },
                            OpacityStop {
                                position: 1.,
                                opacity: 1.,
                            },
                        ])
                        .unwrap(),
                        ..Default::default()
                    };
                    let effects = LayerEffects {
                        gradient_overlay: Some(Box::new(overlay)),
                        ..Default::default()
                    };
                    let gpu = engine.effects(&image, &effects).unwrap().unwrap();
                    let cpu = crate::effects::cpu::render(&image, &effects).unwrap();
                    for (index, (a, b)) in gpu.as_raw().iter().zip(cpu.as_raw()).enumerate() {
                        assert!(
                            a.abs_diff(*b) <= 1,
                            "{style:?} angle{angle} reverse{reverse} byte{index}:gpu{a}/cpu{b}"
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod boundary_tests {
    use crate::{
        effects::LayerEffects,
        gradient::stops::{Stop, Stops},
        gradient_overlay::{OpacityStop, OpacityStops, Overlay, Style},
        render::gpu::Engine,
    };
    use image::{Rgba, RgbaImage};
    #[test]
    #[ignore = "Requires a hardware Vulkan adapter"]
    fn gradient_overlay_gpu_hard_edges_match_odd_pixel_centers_and_cardinal_angles() {
        let mut engine = Engine::new().unwrap();
        for size in [3, 7, 15, 31] {
            let mut image = RgbaImage::new(size + 4, size + 4);
            for y in 2..size + 2 {
                for x in 2..size + 2 {
                    image.put_pixel(x, y, Rgba([255; 4]));
                }
            }
            for angle in [0., 45., 90., 135., 180., -45., -90., -135., 270., 360.] {
                for edge in [0.1, 0.3, 0.5, 0.7, 0.9] {
                    for reverse in [false, true] {
                        let overlay = Overlay {
                            angle,
                            reverse,
                            stops: Stops::new(vec![
                                Stop {
                                    position: edge,
                                    color: [0, 0, 0, 255],
                                },
                                Stop {
                                    position: edge,
                                    color: [255; 4],
                                },
                            ])
                            .unwrap(),
                            ..Default::default()
                        };
                        let effects = LayerEffects {
                            gradient_overlay: Some(Box::new(overlay)),
                            ..Default::default()
                        };
                        let gpu = engine.effects(&image, &effects).unwrap().unwrap();
                        let cpu = crate::effects::cpu::render(&image, &effects).unwrap();
                        assert_eq!(
                            gpu, cpu,
                            "size{size} angle{angle} edge{edge} reverse{reverse}"
                        );
                        if size == 15 && angle == 0. && edge == 0.3 && !reverse {
                            assert_eq!(gpu[(6, 2)], Rgba([255; 4]));
                        }
                    }
                }
            }
        }
    }
    #[test]
    #[ignore = "Requires a hardware Vulkan adapter"]
    fn gradient_overlay_gpu_opacity_edges_and_radial_centers_match_cpu() {
        let mut engine = Engine::new().unwrap();
        for size in [3, 7, 15, 31] {
            let mut image = RgbaImage::new(size + 4, size + 4);
            for y in 2..size + 2 {
                for x in 2..size + 2 {
                    image.put_pixel(x, y, Rgba([255; 4]));
                }
            }
            for style in [Style::Linear, Style::Radial] {
                for reverse in [false, true] {
                    for edge in [0., 0.1, 0.3, 0.5, 0.7, 0.9, 1.] {
                        for opacity_edge in [false, true] {
                            let overlay = if opacity_edge {
                                Overlay {
                                    style,
                                    reverse,
                                    angle: 0.,
                                    stops: Stops::endpoints([0, 0, 0, 255], [0, 0, 0, 255]),
                                    opacity_stops: OpacityStops::new(vec![
                                        OpacityStop {
                                            position: edge,
                                            opacity: 0.,
                                        },
                                        OpacityStop {
                                            position: edge,
                                            opacity: 1.,
                                        },
                                    ])
                                    .unwrap(),
                                    ..Default::default()
                                }
                            } else {
                                Overlay {
                                    style,
                                    reverse,
                                    angle: 0.,
                                    stops: Stops::new(vec![
                                        Stop {
                                            position: edge,
                                            color: [0, 0, 0, 255],
                                        },
                                        Stop {
                                            position: edge,
                                            color: [255; 4],
                                        },
                                    ])
                                    .unwrap(),
                                    ..Default::default()
                                }
                            };
                            let effects = LayerEffects {
                                gradient_overlay: Some(Box::new(overlay)),
                                ..Default::default()
                            };
                            let gpu = engine.effects(&image, &effects).unwrap().unwrap();
                            let cpu = crate::effects::cpu::render(&image, &effects).unwrap();
                            assert_eq!(
                                gpu, cpu,
                                "size{size} style{style:?} reverse{reverse} edge{edge} opacity{opacity_edge}"
                            );
                        }
                    }
                }
            }
        }
    }
}
