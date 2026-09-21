use super::super::Kernel;
use super::*;

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn gpu_pixel_application_preserves_stroke_opacity_and_original_color() {
    let engine = Engine::new().unwrap();
    for hardness in [0., 1.] {
        for mode in [PaintMode::Paint, PaintMode::Erase] {
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
                    } else {
                        crate::blend::Blend::Normal.composite(before, top)
                    };
                    expected[(x, y)] =
                        image::Rgba(result.map(|v| (v.clamp(0., 1.) * 255.).round() as u8));
                }
                let operation = if mode == PaintMode::Erase {
                    Operation::Erase
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
    let engine = Engine::new().unwrap();
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
