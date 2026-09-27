use super::super::Kernel;
use super::*;

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn gpu_pixel_application_preserves_stroke_opacity_and_original_color() {
    let mut engine = Engine::new().unwrap();
    for hardness in [0., 1.] {
        use crate::brush::tonal::{Range, Tonal};
        for mode in [
            PaintMode::Paint,
            PaintMode::Erase,
            PaintMode::Tonal(Tonal::Dodge(Range::All)),
            PaintMode::Tonal(Tonal::Dodge(Range::Shadows)),
            PaintMode::Tonal(Tonal::Dodge(Range::Midtones)),
            PaintMode::Tonal(Tonal::Dodge(Range::Highlights)),
            PaintMode::Tonal(Tonal::Burn(Range::All)),
            PaintMode::Tonal(Tonal::Burn(Range::Shadows)),
            PaintMode::Tonal(Tonal::Burn(Range::Midtones)),
            PaintMode::Tonal(Tonal::Burn(Range::Highlights)),
            PaintMode::Tonal(Tonal::Saturate),
            PaintMode::Tonal(Tonal::Desaturate),
        ] {
            let brush = Brush {
                diameter: 400.,
                hardness,
                opacity: 0.4,
                color: [210, 30, 160, 180],
            };
            let kernel = Kernel::new(brush);
            let original = image::RgbaImage::from_fn(600, 600, |x, y| {
                image::Rgba([x as u8, y as u8, 80, (x + y) as u8])
            });
            let mut pixels = original.clone();
            let mut plane = Plane::new(600, 600);
            let mut reference_plane = plane.clone();
            let mut expected = original.clone();
            let region = Region {
                projection: None,
                bounds: [31, 27, 570, 581],
                origin: [0.5, 0.5],
                dx: [1., 0.],
                dy: [0., 1.],
                canvas: [600.; 2],
            };
            // Overlapping segments must use the stroke's original pixels, not
            // repeatedly blend the latest pixels and exceed its opacity cap.
            for (start, end) in [([130., 160.], [470., 400.]), ([470., 400.], [100., 180.])] {
                let segment = kernel.segment(start, end, 1.);
                let changed = region
                    .rasterize(&mut reference_plane, &segment, brush)
                    .unwrap();
                for (x, y, coverage) in changed.enumerate_pixels() {
                    if coverage[0] == 0 {
                        continue;
                    }
                    let (x, y) = (x + 31, y + 27);
                    let before = original[(x, y)].0.map(|v| v as f64 / 255.);
                    let amount = coverage[0] as f64 / 255. * brush.opacity;
                    let mut top = brush.color.map(|v| v as f64 / 255.);
                    top[3] *= amount;
                    let result = if mode == PaintMode::Erase {
                        [before[0], before[1], before[2], before[3] * (1. - amount)]
                    } else if let PaintMode::Tonal(operation) = mode {
                        operation.apply(before, amount)
                    } else {
                        crate::blend::Blend::Normal.composite(before, top)
                    };
                    expected[(x, y)] =
                        image::Rgba(result.map(|v| (v.clamp(0., 1.) * 255.).round() as u8));
                }
                let operation = if mode == PaintMode::Erase {
                    Operation::Erase
                } else if let PaintMode::Tonal(operation) = mode {
                    Operation::Tonal(operation)
                } else {
                    Operation::Paint
                };
                engine
                    .process(
                        &region,
                        &mut plane,
                        &segment,
                        brush,
                        &mut Target::Image {
                            pixels: &mut pixels,
                            original: &original,
                            operation,
                        },
                    )
                    .unwrap();
                for (index, (a, b)) in pixels.as_raw().iter().zip(expected.as_raw()).enumerate() {
                    assert!(
                        a.abs_diff(*b) <= 1,
                        "hardness={hardness} mode={mode:?} byte={index}: {a} vs {b}"
                    );
                }
            }
        }
    }
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn gpu_matches_reference_for_clicks_segments_transforms_and_chunk_boundaries() {
    let mut engine = Engine::new().unwrap();
    for hardness in [0., 0.5, 1.] {
        let brush = Brush {
            diameter: 800.,
            hardness,
            ..Brush::default()
        };
        let kernel = Kernel::new(brush);
        for end in [[600., 400.], [1300., 1250.]] {
            for (dx, dy) in [([1., 0.], [0., 1.]), ([-0.8, 0.6], [0.6, 0.8])] {
                // Exceeds one GPU batch, has untouched margins and mixed preexisting density.
                let region = Region {
                    projection: None,
                    bounds: [7, 5, 1500, 900],
                    origin: [900.5, -100.5],
                    dx,
                    dy,
                    canvas: [2400., 1600.],
                };
                let segment = kernel.segment([600., 400.], end, 1.);
                let before = Plane::from_fn(1510, 910, |x, _| {
                    image::Luma([if x < 500 { 0. } else { 0.2 }])
                });
                let mut cpu = before.clone();
                let reference = region.rasterize(&mut cpu, &segment, brush).unwrap();
                let mut gpu = before.clone();
                let actual = engine
                    .rasterize(&region, &mut gpu, &segment, brush)
                    .unwrap();
                for y in 0..910 {
                    for x in 0..1510 {
                        if !(7..1500).contains(&x) || !(5..900).contains(&y) {
                            assert_eq!(gpu[(x, y)], before[(x, y)]);
                        } else {
                            let expected = super::super::alpha(cpu[(x, y)][0], brush);
                            let got = super::super::alpha(gpu[(x, y)][0], brush);
                            assert!(
                                (got - expected).abs() <= 1. / 255.,
                                "hardness={hardness} at {x},{y}: {got} vs {expected}"
                            );
                            // A one-level increment may round away on one backend. Compare
                            // effective coverage rather than the changed-pixel sentinel.
                            let old = (super::super::alpha(before[(x, y)][0], brush) * 255.).round()
                                as u8;
                            let a = actual[(x - 7, y - 5)][0].max(old);
                            let b = reference[(x - 7, y - 5)][0].max(old);
                            assert!(a.abs_diff(b) <= 1, "{a} vs {b}");
                        }
                    }
                }
            }
        }
    }
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn gpu_tilted_tip_matches_reference_with_canvas_clipping() {
    let mut engine = Engine::new().unwrap();
    for hardness in [0., 0.5, 1.] {
        let brush = Brush {
            diameter: 520.,
            hardness,
            ..Brush::default()
        };
        let kernel = Kernel::new(brush);
        let segment = kernel
            .segment([15., 25.], [280., 150.], 1.)
            .with_metric(crate::brush::Tip::new(Some(1.), Some([45., 35.])).metric());
        let region = Region {
            projection: None,
            bounds: [0, 0, 700, 600],
            origin: [-10.5, -20.5],
            dx: [1., 0.],
            dy: [0., 1.],
            canvas: [650., 550.],
        };
        let mut actual = Plane::new(700, 600);
        let mut reference = actual.clone();
        let expected = region.rasterize(&mut reference, &segment, brush).unwrap();
        let result = engine
            .rasterize(&region, &mut actual, &segment, brush)
            .unwrap();
        for (a, b) in result.as_raw().iter().zip(expected.as_raw()) {
            assert!(a.abs_diff(*b) <= 1, "hardness={hardness}: {a} vs {b}");
        }
    }
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn sampled_gpu_matches_cpu_for_real_bristles_tilt_and_pixel_operations() {
    use crate::brush::sampled::{self, Sampled, State};
    use std::{path::PathBuf, sync::Arc};
    let mut engine = Engine::new().unwrap();
    let tip = Arc::new(
        sampled::read(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gbr/bristles-01.gbr"),
        )
        .unwrap(),
    );
    for mode in [PaintMode::Paint, PaintMode::Erase] {
        let brush = Brush {
            diameter: 520.,
            hardness: 1.,
            opacity: 0.4,
            color: [210, 30, 160, 180],
        };
        let mut state = State::new(Sampled::new(tip.clone()));
        let original = image::RgbaImage::from_fn(700, 600, |x, y| {
            image::Rgba([x as u8, y as u8, 80, (x + y) as u8])
        });
        let mut actual = original.clone();
        let mut expected = original.clone();
        let mut plane = Plane::new(700, 600);
        let mut cpu = plane.clone();
        let region = Region {
            projection: None,
            bounds: [0, 0, 700, 600],
            origin: [-10.5, -20.5],
            dx: [1., 0.],
            dy: [0., 1.],
            canvas: [650., 550.],
        };
        for (start, end) in [
            ([15., 25.], [280., 150.]),
            ([280., 150.], [500., 350.]),
            ([500., 350.], [500., 350.]),
        ] {
            let kernel = state
                .kernel(
                    brush.diameter,
                    sampled::gih::Dynamics::new(None, None, [1., 0.]).unwrap(),
                )
                .unwrap();
            let segment = kernel
                .segment(start, end, 1.)
                .with_metric(crate::brush::Tip::new(Some(1.), Some([45., 35.])).metric());
            state.advance((end[0] - start[0]).hypot(end[1] - start[1]), brush.diameter);
            let changed = region.rasterize(&mut cpu, &segment, brush).unwrap();
            for (x, y, coverage) in changed.enumerate_pixels() {
                if coverage[0] == 0 {
                    continue;
                }
                let before = original[(x, y)].0.map(|v| v as f64 / 255.);
                let amount = coverage[0] as f64 / 255. * brush.opacity;
                let mut top = brush.color.map(|v| v as f64 / 255.);
                top[3] *= amount;
                let result = if mode == PaintMode::Erase {
                    [before[0], before[1], before[2], before[3] * (1. - amount)]
                } else {
                    crate::blend::Blend::Normal.composite(before, top)
                };
                expected[(x, y)] =
                    image::Rgba(result.map(|v| (v.clamp(0., 1.) * 255.).round() as u8));
            }
            engine
                .process(
                    &region,
                    &mut plane,
                    &segment,
                    brush,
                    &mut Target::Image {
                        pixels: &mut actual,
                        original: &original,
                        operation: if mode == PaintMode::Erase {
                            Operation::Erase
                        } else {
                            Operation::Paint
                        },
                    },
                )
                .unwrap();
            for (index, (a, b)) in actual.as_raw().iter().zip(expected.as_raw()).enumerate() {
                assert!(a.abs_diff(*b) <= 1, "{mode:?} byte {index}: {a} vs {b}");
            }
        }
    }
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn gih_gpu_matches_cpu_for_mixed_cells_and_all_selection_modes() {
    use crate::brush::sampled::{
        Sampled, State,
        gih::{Dynamics, Hose},
    };
    use std::sync::Arc;
    let mut engine = Engine::new().unwrap();
    let brush = Brush {
        diameter: 120.,
        hardness: 1.,
        ..Brush::default()
    };
    for mode in [
        "incremental",
        "random",
        "angular",
        "pressure",
        "xtilt",
        "ytilt",
    ] {
        let mut bytes = format!("Test\n4 dim:1 rank0:4 sel0:{mode}\n").into_bytes();
        for i in 0..4u32 {
            let (w, h) = (5 + i, 4 + i);
            for value in [30, 2, w, h, 1, 0x47494d50, 20] {
                bytes.extend_from_slice(&value.to_be_bytes());
            }
            bytes.extend_from_slice(b"x\0");
            bytes.extend((0..w * h).map(|p| (p * 11 + i * 53) as u8));
        }
        let mut state = State::new(Sampled::from_hose(Arc::new(
            Hose::from_bytes(&bytes).unwrap(),
        )));
        let mut cpu = Plane::new(400, 400);
        let mut gpu = cpu.clone();
        let region = Region {
            projection: None,
            bounds: [0, 0, 400, 400],
            origin: [0.5, 0.5],
            dx: [1., 0.],
            dy: [0., 1.],
            canvas: [400., 400.],
        };
        for (start, end, pressure, tilt) in [
            ([40., 50.], [220., 150.], 0.2, [-45., 30.]),
            ([220., 150.], [170., 310.], 0.8, [20., -40.]),
        ] {
            let dynamics = Dynamics::new(
                Some(pressure),
                Some(tilt),
                [end[0] - start[0], end[1] - start[1]],
            )
            .unwrap();
            let kernel = state.kernel(brush.diameter, dynamics).unwrap();
            let segment = kernel
                .segment(start, end, 1.)
                .with_metric(crate::brush::Tip::new(Some(1.), Some(tilt)).metric());
            state.advance((end[0] - start[0]).hypot(end[1] - start[1]), brush.diameter);
            let reference = region.rasterize(&mut cpu, &segment, brush).unwrap();
            let actual = engine
                .rasterize(&region, &mut gpu, &segment, brush)
                .unwrap();
            for (index, (a, b)) in actual.as_raw().iter().zip(reference.as_raw()).enumerate() {
                assert!(a.abs_diff(*b) <= 1, "{mode} pixel {index}: {a} vs {b}");
            }
        }
    }
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn projective_brush_coverage_and_local_edge_width_match_cpu() {
    use crate::geometry::{Transform, projective::Projective};
    let transform = Transform {
        origin: [40., 20.],
        rotation: 11.,
        warp: Some(Projective::new([[0., 0.], [1., 0.1], [0.65, 1.1], [0.2, 0.8]]).unwrap()),
        ..Transform::new(400, 360)
    };
    let mapping = crate::brush::placement::pixel_mapping(transform, [400, 360]).unwrap();
    let region = Region {
        bounds: [0, 0, 400, 360],
        projection: Some(mapping),
        origin: [0., 0.],
        dx: [0., 0.],
        dy: [0., 0.],
        canvas: [600., 600.],
    };
    let mut engine = Engine::new().unwrap();
    for hardness in [0., 0.5, 1.] {
        let brush = Brush {
            diameter: 160.,
            hardness,
            opacity: 0.7,
            color: [180, 30, 70, 255],
        };
        let kernel = Kernel::new(brush);
        let mut actual = Plane::new(400, 360);
        let mut expected = actual.clone();
        for (start, end) in [([150., 110.], [270., 225.]), ([270., 225.], [130., 280.])] {
            let segment = kernel.segment(start, end, 1.);
            let reference = region.rasterize(&mut expected, &segment, brush).unwrap();
            let output = engine
                .rasterize(&region, &mut actual, &segment, brush)
                .unwrap();
            for (index, (a, b)) in output.as_raw().iter().zip(reference.as_raw()).enumerate() {
                assert!(
                    a.abs_diff(*b) <= 1,
                    "hardness={hardness} pixel={index}: GPU{a} CPU{b}"
                );
            }
        }
    }
}
