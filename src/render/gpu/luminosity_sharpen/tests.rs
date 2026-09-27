use super::*;
#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn gpu_sharpen_matches_cpu_for_fractional_coverage_hidden_colors_and_noise() {
    let engine = Engine::new().unwrap();
    let image = RgbaImage::from_fn(41, 31, |x, y| {
        image::Rgba([
            ((30 + x * 4 + y) % 256) as u8,
            100,
            180,
            [0, 1, 37, 128, 255][(x as usize + y as usize) % 5],
        ])
    });
    for radius in [0.1, 1.7, 7.] {
        for noise in [0., 35., 100.] {
            let settings = Settings {
                radius,
                noise,
                amount: 230.,
            };
            let actual = engine
                .luminosity_sharpen(&image, settings)
                .unwrap()
                .unwrap();
            let expected = crate::filters::luminosity_sharpen::cpu(&image, settings);
            for (got, want) in actual.pixels().zip(expected.pixels()) {
                assert_eq!(got[3], want[3]);
                if want[3] == 0 {
                    assert_eq!(got, want);
                }
                for c in 0..3 {
                    assert!(
                        (i16::from(got[c]) - i16::from(want[c])).abs() <= 1,
                        "{got:?} != {want:?}"
                    );
                }
            }
        }
    }
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn gpu_sharpen_keeps_flat_translucent_colors_at_radius_limits() {
    let engine = Engine::new().unwrap();
    let image = RgbaImage::from_fn(7, 5, |x, _| {
        if x < 2 {
            image::Rgba([250, 0, 250, 0])
        } else {
            image::Rgba([80, 100, 120, 1])
        }
    });
    for radius in [0.1, 250.] {
        let output = engine
            .luminosity_sharpen(
                &image,
                Settings {
                    amount: 500.,
                    radius,
                    noise: 0.,
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(output, image);
    }
}
