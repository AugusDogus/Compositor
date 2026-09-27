use super::*;

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn posterize_gpu_matches_every_level_byte_and_alpha() {
    let engine = Engine::new().unwrap();
    let source = RgbaImage::from_fn(256, 256, |x, y| {
        image::Rgba([x as u8, 255 - x as u8, (x as u8).wrapping_mul(17), y as u8])
    });
    for levels in 2..=256 {
        let settings = crate::posterize::Posterize::new(levels).unwrap();
        let gpu = engine
            .color_filter(&source, Settings::Posterize(settings))
            .unwrap();
        let cpu = crate::posterize::reference(&source, settings);
        assert_eq!(gpu, cpu, "posterize levels{levels}");
    }
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn threshold_gpu_matches_every_gray_boundary_color_and_alpha() {
    let engine = Engine::new().unwrap();
    let source = RgbaImage::from_fn(256, 11, |x, y| {
        let value = x as u8;
        image::Rgba(match y {
            0..=3 => [value, value, value, (y * 85) as u8],
            4 => [value, 140, 129, 255],
            5 => [100, value, 129, 128],
            6 => [100, 140, value, 255],
            7 => [value, 255 - value, value.wrapping_mul(17), 73],
            8 => [0, 6, 127, value],
            9 => [0, 122, 249, value],
            _ => [0, 208, 236, value],
        })
    });
    for level in 0..=255 {
        let settings = crate::threshold::Threshold { level };
        let gpu = engine
            .color_filter(&source, Settings::Threshold(settings))
            .unwrap();
        let cpu = crate::threshold::reference(&source, settings);
        assert_eq!(gpu, cpu, "threshold {level}");
    }
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn selective_color_gpu_matches_all_ranges_modes_alpha_and_extremes() {
    let engine = Engine::new().unwrap();
    let source = RgbaImage::from_fn(257, 129, |x, y| {
        image::Rgba([
            (x % 256) as u8,
            (y * 2) as u8,
            ((x * y) % 256) as u8,
            (y % 4 * 85) as u8,
        ])
    });
    for mode in [Mode::Relative, Mode::Absolute] {
        for adjustments in [
            [[0.; 4]; 9],
            [[100.; 4]; 9],
            [[-100.; 4]; 9],
            std::array::from_fn(|i| [i as f32 * 20. - 80., 32.5, -42.75, 17.25]),
        ] {
            let settings = SelectiveColor { mode, adjustments };
            let gpu = engine
                .color_filter(&source, Settings::SelectiveColor(settings))
                .unwrap();
            let cpu = crate::selective_color::reference(&source, settings);
            for (a, b) in cpu.pixels().zip(gpu.pixels()) {
                assert_eq!(a[3], b[3]);
                if a[3] == 0 {
                    assert_eq!(a, b);
                }
                for channel in 0..3 {
                    assert!(
                        a[channel].abs_diff(b[channel]) <= 1,
                        "{settings:?}: {a:?}/{b:?}"
                    );
                }
            }
        }
    }
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn photo_filter_gpu_matches_cpu_for_colors_density_luminosity_and_alpha() {
    let engine = Engine::new().unwrap();
    let source = RgbaImage::from_fn(257, 129, |x, y| {
        image::Rgba([
            (x % 256) as u8,
            (y * 2) as u8,
            ((x * y) % 256) as u8,
            (y % 4 * 85) as u8,
        ])
    });
    for color in [[0.925, 0.541, 0.], [0., 0.3, 1.], [0.; 3], [1.; 3]] {
        for density in [0., 25., 100.] {
            for preserve_luminosity in [false, true] {
                let settings = PhotoFilter {
                    color,
                    density,
                    preserve_luminosity,
                };
                let gpu = engine
                    .color_filter(&source, Settings::PhotoFilter(settings))
                    .unwrap();
                let cpu = crate::filters::photo_filter::reference(&source, settings);
                for (a, b) in cpu.pixels().zip(gpu.pixels()) {
                    assert_eq!(a[3], b[3]);
                    if a[3] == 0 {
                        assert_eq!(a, b);
                    }
                    for channel in 0..3 {
                        assert!(
                            a[channel].abs_diff(b[channel]) <= 1,
                            "{settings:?}: {a:?}/{b:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
#[ignore = "Measures GPU upload/compute/readback at 4000 by 4000"]
fn photo_filter_gpu_large_image() {
    let engine = Engine::new().unwrap();
    let source = RgbaImage::from_pixel(4000, 4000, image::Rgba([100, 150, 200, 255]));
    let started = std::time::Instant::now();
    let output = engine
        .color_filter(&source, Settings::PhotoFilter(PhotoFilter::default()))
        .unwrap();
    eprintln!(
        "Photo Filter 4000x4000 upload/compute/readback: {:?}",
        started.elapsed()
    );
    assert_eq!(
        output[(2000, 2000)].0,
        PhotoFilter::default().pixel([100, 150, 200, 255])
    );
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter and a multi-buffer image"]
fn photo_filter_gpu_splits_large_sources_without_losing_boundary_pixels() {
    let engine = Engine::new().unwrap();
    let source = RgbaImage::from_fn(4097, 4097, |x, y| {
        image::Rgba([
            (x % 256) as u8,
            (y % 256) as u8,
            137,
            if x % 3 == 0 { 0 } else { 128 },
        ])
    });
    let settings = PhotoFilter::default();
    let output = engine
        .color_filter(&source, Settings::PhotoFilter(settings))
        .unwrap();
    for (x, y) in [
        (0, 0),
        (4096, 4096),
        (3840, 4094),
        (3841, 4094),
        (3842, 4094),
    ] {
        let expected = settings.pixel(source[(x, y)].0);
        for channel in 0..4 {
            assert!(output[(x, y)][channel].abs_diff(expected[channel]) <= 1);
        }
    }
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn channel_mixer_gpu_matches_cpu_for_signed_coefficients_constants_monochrome_and_alpha() {
    let engine = Engine::new().unwrap();
    let source = RgbaImage::from_fn(257, 129, |x, y| {
        image::Rgba([
            (x % 256) as u8,
            (y * 2) as u8,
            ((x * y) % 256) as u8,
            (y % 4 * 85) as u8,
        ])
    });
    for rows in [
        ChannelMixer::default().rows,
        [[0., 100., 0., 0.], [0., 0., 100., 0.], [100., 0., 0., 0.]],
        [
            [-100., 200., 0., -50.],
            [10., 30., 60., 10.],
            [0., 0., 0., 50.],
        ],
        [[200.; 4], [-200.; 4], [0.; 4]],
    ] {
        for monochrome in [false, true] {
            let settings = ChannelMixer { rows, monochrome };
            let gpu = engine
                .color_filter(&source, Settings::ChannelMixer(settings))
                .unwrap();
            let cpu = crate::filters::channel_mixer::reference(&source, settings);
            for (a, b) in cpu.pixels().zip(gpu.pixels()) {
                assert_eq!(a[3], b[3]);
                if a[3] == 0 {
                    assert_eq!(a, b);
                }
                for channel in 0..3 {
                    assert!(
                        a[channel].abs_diff(b[channel]) <= 1,
                        "{settings:?}: {a:?}/{b:?}"
                    );
                }
            }
        }
    }
}
