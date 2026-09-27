use super::*;
use crate::filters::radial::{self, Mode, Radial};
#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn radial_gpu_matches_reference_for_modes_centers_and_transparency() {
    let engine = Engine::new().unwrap();
    let image = RgbaImage::from_fn(73, 61, |x, y| {
        image::Rgba([
            ((x * 7 + y) % 256) as u8,
            ((x + y * 5) % 256) as u8,
            90,
            if x < 8 { 0 } else { ((x * y) % 256) as u8 },
        ])
    });
    let source = radial::premultiply(&image);
    for mode in [Mode::Spin, Mode::Zoom] {
        for amount in [0.5, 20., 100.] {
            for center in [[0.5, 0.5], [0., 1.], [0.27, 0.73]] {
                let settings = Radial {
                    mode,
                    amount,
                    center,
                };
                let passes = settings.passes([73, 61]);
                let center = [center[0] as f32 * 73., center[1] as f32 * 61.];
                let cpu = radial::reference(source.clone(), [73, 61], center, &passes);
                let gpu = engine
                    .radial_blur(&source, [73, 61], center, &passes)
                    .unwrap()
                    .unwrap();
                for (index, (a, b)) in cpu.pixels().zip(gpu.pixels()).enumerate() {
                    for channel in 0..4 {
                        if channel < 3 && a[3] == 0 && b[3] == 0 {
                            continue;
                        }
                        assert!(
                            a[channel].abs_diff(b[channel]) <= 2,
                            "{mode:?} amount={amount} center={center:?} pixel={index} channel={channel}: {a:?}/{b:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn radial_gpu_preserves_low_alpha_high_rgb_flat_colors() {
    let engine = Engine::new().unwrap();
    for mode in [Mode::Spin, Mode::Zoom] {
        let settings = Radial {
            mode,
            amount: 100.,
            center: [0.1, 0.8],
        };
        let passes = settings.passes([13, 11]);
        for alpha in [1, 2, 3, 7, 19, 37, 127, 254, 255] {
            for rgb in [
                [1, 128, 254],
                [253, 251, 249],
                [17, 239, 254],
                [0, 255, 255],
            ] {
                let flat =
                    RgbaImage::from_pixel(13, 11, image::Rgba([rgb[0], rgb[1], rgb[2], alpha]));
                let result = engine
                    .radial_blur(&radial::premultiply(&flat), [13, 11], [1.3, 8.8], &passes)
                    .unwrap()
                    .unwrap();
                assert_eq!(result, flat, "{mode:?} rgb={rgb:?} alpha={alpha}");
            }
        }
    }
}

#[test]
#[ignore = "Measures a 4000 by 4000 image on a hardware Vulkan adapter"]
fn radial_gpu_large_image() {
    let engine = Engine::new().unwrap();
    let image = RgbaImage::from_fn(4000, 4000, |x, y| {
        image::Rgba([(x % 256) as u8, (y % 256) as u8, 249, 128])
    });
    let started = std::time::Instant::now();
    let source = radial::premultiply(&image);
    eprintln!("4000x4000 premultiplication: {:?}", started.elapsed());
    for mode in [Mode::Spin, Mode::Zoom] {
        let passes = Radial {
            mode,
            amount: 100.,
            center: [0.5; 2],
        }
        .passes([4000; 2]);
        let started = std::time::Instant::now();
        let result = engine
            .radial_blur(&source, [4000; 2], [2000.; 2], &passes)
            .unwrap()
            .unwrap();
        eprintln!(
            "{mode:?} 4000x4000 ({} passes), upload/compute/readback: {:?}",
            passes.len(),
            started.elapsed()
        );
        assert_eq!(result.dimensions(), (4000, 4000));
        assert!(result.pixels().all(|p| p[3] == 128));
    }
}
