// Adapted from Xuan, copyright (c) 2026 Wonder Assembly LLC and Silver Ling.
// Distributed under the MIT license; see licenses/Xuan-MIT.txt.
use super::*;
use image::Rgb;
use std::sync::atomic::AtomicBool;

fn synthetic() -> DecodedRaw {
    DecodedRaw {
        camera: Rgb32FImage::from_fn(64, 48, |x, y| {
            Rgb([0.02 + x as f32 / 40.0, 0.02 + y as f32 / 30.0, 0.2])
        }),
        as_shot: [1.0; 3],
        camera_to_rgb: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        xyz_to_camera: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        metadata: RawMetadata {
            width: 64,
            height: 48,
            ..Default::default()
        },
    }
}

#[test]
fn exposure_recovers_unclipped_raw_values_and_is_repeatable() {
    let raw = synthetic();
    let cancel = AtomicBool::new(false);
    let mut settings = DevelopSettings {
        sharpen: 0.0,
        color_noise: 0.0,
        ..Default::default()
    };
    let before = render(&raw, &settings, &cancel).unwrap();
    settings.exposure = -2.0;
    let recovered = render(&raw, &settings, &cancel).unwrap();
    assert_eq!(before.get_pixel(63, 47)[0], 255);
    assert!(recovered.get_pixel(63, 47)[0] < 230);
    assert!(recovered.get_pixel(63, 47)[0] > recovered.get_pixel(55, 47)[0]);
    assert_eq!(recovered, render(&raw, &settings, &cancel).unwrap());
    assert!(raw.camera.get_pixel(63, 47)[0] > 1.0);
}

#[test]
fn validates_settings_and_cancellation() {
    let raw = synthetic();
    let mut s = DevelopSettings {
        exposure: f32::NAN,
        ..Default::default()
    };
    assert!(s.validate().is_err());
    s = DevelopSettings::default();
    s.crop = [0.5, 0.0, 0.4, 1.0];
    assert!(s.validate().is_err());
    assert!(render(&raw, &DevelopSettings::default(), &AtomicBool::new(true)).is_err());
    assert!(decode(b"broken camera file").is_err());
    assert!(is_raw(Path::new("PHOTO.NEF")));
    assert!(!is_raw(Path::new("photo.tiff")));
}

#[test]
fn crop_and_local_adjustment_are_nondestructive() {
    let raw = synthetic();
    let mut s = DevelopSettings {
        sharpen: 0.0,
        color_noise: 0.0,
        ..Default::default()
    };
    let cancel = AtomicBool::new(false);
    let before = render(&raw, &s, &cancel).unwrap();
    s.overlays.push(Overlay {
        exposure: -2.0,
        start: Point::new(0.0, 0.0),
        end: Point::new(0.0, 0.5),
        ..Default::default()
    });
    let after = render(&raw, &s, &cancel).unwrap();
    assert!(after.get_pixel(20, 0)[0] < before.get_pixel(20, 0)[0]);
    assert_eq!(after.get_pixel(20, 47), before.get_pixel(20, 47));
    s.crop = [0.25, 0.25, 0.75, 0.75];
    assert_eq!(render(&raw, &s, &cancel).unwrap().dimensions(), (32, 24));
}

#[test]
fn sixteen_bit_output_preserves_more_than_eight_bit_steps() {
    let mut raw = synthetic();
    raw.camera = Rgb32FImage::from_fn(1024, 2, |x, _| Rgb([0.1 + x as f32 / 100_000.0; 3]));
    raw.metadata.width = 1024;
    raw.metadata.height = 2;
    let s = DevelopSettings {
        color_noise: 0.0,
        sharpen: 0.0,
        ..Default::default()
    };
    let cancel = AtomicBool::new(false);
    let image = render_16(&raw, &s, &cancel).unwrap();
    let steps: std::collections::HashSet<_> = image.pixels().map(|p| p[0]).collect();
    assert!(steps.len() > 900);
    let mut bytes = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgba16(image.clone())
        .write_to(&mut bytes, image::ImageFormat::Tiff)
        .unwrap();
    let restored = image::load_from_memory(bytes.get_ref()).unwrap();
    assert_eq!(restored.color(), image::ColorType::Rgba16);
    assert_eq!(restored.to_rgba16(), image);
}

#[test]
fn preview_preserves_unclipped_camera_highlights() {
    let raw = synthetic();
    let proxy = raw.preview(16);
    assert_eq!(proxy.camera.dimensions(), (16, 12));
    assert!(proxy.camera.get_pixel(15, 11)[0] > 1.5);
}

#[test]
fn crop_preserves_rotated_flipped_layer_and_mask_placement() {
    let raw = synthetic();
    let asset = RawAsset {
        filename: "test.NEF".into(),
        metadata: raw.metadata,
        settings: DevelopSettings::default(),
        bytes: Arc::new(vec![1]),
    };
    let mut layer = crate::document::Layer::blank("RAW", 64, 48);
    layer.raw = Some(Arc::new(asset.clone()));
    layer.transform.rotation = 25.;
    layer.transform.flip_x = true;
    layer.transform.origin = [50., 20.];
    let original_transform = layer.transform;
    layer.mask = Some(crate::document::Mask {
        pixels: Arc::new(image::GrayImage::from_pixel(64, 48, image::Luma([160]))),
        enabled: true,
        linked: false,
        placement: None,
    });
    let before = layer.transform.point([0.375, 0.5]);
    let mut cropped = asset;
    cropped.settings.crop = [0.25, 0., 0.75, 1.];
    update_layer(&mut layer, cropped, RgbaImage::new(32, 48)).unwrap();
    let after = layer.transform.point([0.25, 0.5]);
    assert_eq!(
        layer.mask.as_ref().unwrap().placement,
        Some(original_transform)
    );
    assert!((before[0] - after[0]).hypot(before[1] - after[1]) < 0.001);
}

#[test]
#[ignore = "Requires hardware Vulkan; exercises real GPU passes against CPU reference"]
fn gpu_development_matches_float_reference() {
    let mut raw = synthetic();
    raw.camera = Rgb32FImage::from_fn(192, 128, |x, y| {
        Rgb([
            0.1 + x as f32 / 180.,
            0.1 + y as f32 / 100.,
            0.05 + (x + y) as f32 / 400.,
        ])
    });
    raw.metadata.width = 192;
    raw.metadata.height = 128;
    let cancel = AtomicBool::new(false);
    let s = DevelopSettings {
        exposure: -1.,
        shadows: 20.,
        saturation: 12.,
        sharpen: 0.,
        color_noise: 0.,
        crop: [0.1, 0.1, 0.9, 0.9],
        ..Default::default()
    };
    let mut detailed = s.clone();
    detailed.sharpen = 30.;
    detailed.texture = 15.;
    detailed.clarity = 10.;
    detailed.luminance_noise = 20.;
    detailed.color_noise = 30.;
    detailed.chromatic_red = 12.;
    detailed.chromatic_blue = -14.;
    detailed.distortion = 8.;
    detailed.vignette = 12.;
    detailed.rotation = 5.;
    detailed.perspective = [5., -8.];
    detailed.hsl[0] = [15., 10., -5.];
    detailed.shadow_tone = [190., 12.];
    detailed.overlays = vec![
        Overlay {
            exposure: 0.2,
            warmth: 20.,
            saturation: -10.,
            ..Default::default()
        },
        Overlay {
            kind: OverlayKind::Radial,
            exposure: -0.3,
            ..Default::default()
        },
        Overlay {
            kind: OverlayKind::Brush,
            exposure: 0.2,
            points: vec![Point::new(0.3, 0.4), Point::new(0.35, 0.4)],
            ..Default::default()
        },
    ];
    for (name, settings) in [("basic", s), ("detail, geometry, and overlays", detailed)] {
        let bytes = gpu::develop(&raw, &settings, raw.as_shot, 8, &cancel)
            .unwrap()
            .expect("hardware GPU required");
        let cpu = process::render_at_depth(&raw, &settings, &cancel, |v| (v * 255.).round() as u8)
            .unwrap();
        assert_eq!(bytes.len(), cpu.as_raw().len());
        let difference = bytes
            .iter()
            .zip(cpu.as_raw())
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(
            difference <= 2,
            "{name}: largest GPU/CPU difference {difference}"
        );
    }
}

#[test]
#[ignore = "Set COMPOSITOR_TEST_NEF using scripts/fetch-raw-fixture.sh"]
fn real_nikon_camera_file_develops_with_unclipped_source() {
    let path = std::env::var_os("COMPOSITOR_TEST_NEF").expect("Set COMPOSITOR_TEST_NEF");
    let (asset, raw) = open(Path::new(&path)).unwrap();
    assert!(asset.metadata.camera.contains("Nikon"));
    assert!(raw.camera.width() > 2000);
    let cancel = AtomicBool::new(false);
    let pixels = render(&raw.preview(1400), &asset.settings, &cancel).unwrap();
    assert!(pixels.pixels().any(|p| p[0].abs_diff(p[1]) > 20));
    let sixteen = render_16(&raw, &asset.settings, &cancel).unwrap();
    assert_eq!(sixteen.dimensions(), raw.camera.dimensions());
    let distinct: std::collections::HashSet<_> = sixteen.pixels().map(|p| p[0]).collect();
    assert!(distinct.len() > 1000);
    println!(
        "Developed {} {:?}: {} distinct red levels",
        asset.metadata.camera,
        sixteen.dimensions(),
        distinct.len()
    );
}

#[test]
fn rejects_special_files_before_opening() {
    assert!(open(Path::new("/dev/zero")).is_err());
    let dir = tempfile::tempdir().unwrap();
    assert!(open(dir.path()).is_err());
}

#[test]
fn crop_rounding_preserves_exact_source_pixel_placement() {
    let raw = synthetic();
    let asset = RawAsset {
        filename: "test.NEF".into(),
        metadata: raw.metadata,
        settings: DevelopSettings::default(),
        bytes: Arc::new(vec![1]),
    };
    let mut layer = crate::document::Layer::blank("RAW", 64, 48);
    layer.raw = Some(Arc::new(asset.clone()));
    let before = layer.transform.point([20.5 / 64., 20.5 / 48.]);
    let mut cropped = asset;
    cropped.settings.crop = [0.123, 0.111, 0.731, 0.777];
    let [left, top, right, bottom] = gpu::crop(&cropped.settings, [64, 48]);
    update_layer(
        &mut layer,
        cropped,
        RgbaImage::new(right - left, bottom - top),
    )
    .unwrap();
    let after = layer.transform.point([
        (20.5 - f64::from(left)) / f64::from(right - left),
        (20.5 - f64::from(top)) / f64::from(bottom - top),
    ]);
    assert!((before[0] - after[0]).hypot(before[1] - after[1]) < 1e-9);
}

#[test]
fn hardware_preflight_bounds_storage_and_dispatch_without_allocation() {
    let mut limits = wgpu::Limits {
        max_storage_buffer_binding_size: 400_000_000,
        max_buffer_size: 800_000_000,
        max_compute_workgroups_per_dimension: 65535,
        ..Default::default()
    };
    assert!(gpu::fits_hardware([6000, 4000], &limits));
    assert!(!gpu::fits_hardware([8256, 5504], &limits));
    limits.max_storage_buffer_binding_size = 800_000_000;
    assert!(gpu::fits_hardware([8256, 5504], &limits));
    limits.max_compute_workgroups_per_dimension = 512;
    assert!(!gpu::fits_hardware([6000, 4000], &limits));
    assert!(!gpu::fits_hardware([0, 1], &limits));
    assert!(!gpu::fits_hardware([u32::MAX, u32::MAX], &limits));
}

#[test]
fn transforming_and_duplicating_raw_keeps_shared_source_and_independent_settings() {
    let decoded = synthetic();
    let asset = Arc::new(RawAsset {
        filename: "test.NEF".into(),
        metadata: decoded.metadata.clone(),
        settings: DevelopSettings::default(),
        bytes: Arc::new(vec![1]),
    });
    let mut doc = crate::document::Document::new(64, 48).unwrap();
    doc.layers[0].raw = Some(asset.clone());
    doc.layers[0].content = crate::document::LayerContent::Raster(Some(Arc::new(
        render(&decoded, &asset.settings, &AtomicBool::new(false)).unwrap(),
    )));
    crate::edits::add_mask(&mut doc, false).unwrap();
    let old = doc.layers[0].transform;
    let mut new = old;
    new.origin = [20., 30.];
    new.size = [96., 72.];
    new.rotation = 15.;
    crate::transform::apply(&mut doc, old, new, false).unwrap();
    assert_eq!(doc.layers[0].transform, new);
    assert!(Arc::ptr_eq(doc.layers[0].raw.as_ref().unwrap(), &asset));
    crate::layer_ops::duplicate_active(&mut doc).unwrap();
    assert_eq!(doc.layers[1].transform, new);
    assert_eq!(doc.layers[1].mask, doc.layers[0].mask);
    Arc::make_mut(doc.layers[1].raw.as_mut().unwrap())
        .settings
        .exposure = -2.;
    assert_eq!(doc.layers[0].raw.as_ref().unwrap().settings.exposure, 0.);
    assert!(Arc::ptr_eq(
        &doc.layers[0].raw.as_ref().unwrap().bytes,
        &doc.layers[1].raw.as_ref().unwrap().bytes
    ));
    doc.validate().unwrap();
}
