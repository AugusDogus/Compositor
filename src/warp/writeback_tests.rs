use super::*;
use crate::{geometry::Transform, geometry::projective::Projective, selection::Selection};

// Reference the original full-raster writeback independently of its bounds optimization.
fn full_writeback(stroke: &WarpStroke, document: &Document) -> RgbaImage {
    let layer = document.active_layer().unwrap();
    // Use exactly the same expanded grid, including rotation and perspective.
    let source = stroke.layer.raster().unwrap();
    let dimensions = layer.raster().unwrap().dimensions();
    RgbaImage::from_fn(dimensions.0, dimensions.1, |x, y| {
        let point = layer.transform.point([
            (x as f64 + 0.5) / dimensions.0 as f64,
            (y as f64 + 0.5) / dimensions.1 as f64,
        ]);
        let unit = stroke.layer.transform.unit(point);
        let sx = (unit[0] * source.width() as f64 - 0.5).round() as i64;
        let sy = (unit[1] * source.height() as f64 - 0.5).round() as i64;
        let before =
            if sx >= 0 && sy >= 0 && sx < source.width() as i64 && sy < source.height() as i64 {
                source[(sx as u32, sy as u32)]
            } else {
                Rgba([0; 4])
            };
        if point[0] < 0.
            || point[1] < 0.
            || point[0] >= stroke.width as f64
            || point[1] >= stroke.height as f64
            || !stroke.pixels.touched(point[0] as usize, point[1] as usize)
        {
            return before;
        }
        let coverage = document
            .selection
            .as_ref()
            .map_or(1., |s| s.coverage(point)) as f32;
        let top = stroke.pixels.sample(
            (point[0] - 0.5).clamp(0., (stroke.width - 1) as f64),
            (point[1] - 0.5).clamp(0., (stroke.height - 1) as f64),
        );
        let alpha = before[3] as f32 / 255.;
        let out_alpha = alpha + (top[3] - alpha) * coverage;
        let mut out = [0, 0, 0, (out_alpha * 255.).round() as u8];
        if out_alpha > 0. {
            for k in 0..3 {
                let base = before[k] as f32 / 255. * alpha;
                out[k] = ((base + (top[k] - base) * coverage) / out_alpha * 255.).round() as u8;
            }
        }
        Rgba(out)
    })
}

#[test]
fn bounded_writeback_matches_full_scan_across_transforms_and_selections() {
    for mode in [Mode::Smudge, Mode::Liquify] {
        for placement in 0..4 {
            for selected in [false, true] {
                let mut document = Document::new(80, 60).unwrap();
                let layer = &mut document.layers[0];
                layer.content =
                    LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(48, 36, |x, y| {
                        Rgba([
                            (x * 3) as u8,
                            (y * 5) as u8,
                            110,
                            if x % 5 == 0 { 0 } else { 180 },
                        ])
                    }))));
                layer.transform = Transform::new(48, 36);
                layer.transform.origin = [12., 10.];
                if placement > 0 {
                    layer.transform.rotation = 23.;
                    layer.transform.flip_x = true;
                }
                if placement == 2 {
                    layer.transform.warp = Some(
                        Projective::new([[0.1, 0.], [0.9, 0.1], [1., 1.], [0., 0.8]]).unwrap(),
                    );
                }
                if placement == 3 {
                    layer.transform.origin = [-12., -10.];
                }
                if selected {
                    document.selection =
                        Some(Selection::rectangle(80, 60, [18., 8.], [61., 49.], false));
                }
                let mut stroke = WarpStroke::start(
                    &document,
                    [30., 25.],
                    mode,
                    Brush {
                        diameter: 16.,
                        ..Brush::default()
                    },
                    false,
                )
                .unwrap();
                for point in [[38., 30.], [58., 36.], [72., 41.]] {
                    stroke.to(&mut document, point).unwrap();
                    assert_eq!(
                        document.layers[0].raster().unwrap().as_ref(),
                        &full_writeback(&stroke, &document),
                        "{mode:?}, transform {placement}, selection {selected}, point {point:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn small_stroke_does_not_scan_the_full_screenshot() {
    let [left, top, right, bottom] = write_region(
        Transform::new(3840, 2160),
        (3840, 2160),
        [900., 800., 1420., 1320.],
    );
    assert!(left <= 900 && top <= 800 && right >= 1420 && bottom >= 1320);
    assert!((right - left) * (bottom - top) < 530 * 530);
}

#[test]
fn perspective_horizon_uses_full_scan_and_off_layer_bounds_are_clipped() {
    let mut transform = Transform::new(48, 36);
    transform.warp = Some(Projective::new([[0., 0.], [0.5, 0.], [0.5, 0.5], [0., 1.]]).unwrap());
    assert_eq!(
        write_region(transform, (48, 36), [40., 0., 60., 20.]),
        [0, 0, 48, 36]
    );
    let transform = Transform::new(48, 36);
    assert_eq!(
        write_region(transform, (48, 36), [-30., -30., -20., -20.]),
        [0, 0, 0, 0]
    );
    assert_eq!(
        write_region(transform, (48, 36), [70., 70., 90., 90.]),
        [48, 36, 48, 36]
    );
}

#[test]
#[ignore = "release-mode warp stage profiling"]
fn profile_warp_stages() {
    assert!(!cfg!(debug_assertions), "Run this profiler with --release");
    for mode in [Mode::Smudge, Mode::Liquify] {
        let mut document = Document::new(3840, 2160).unwrap();
        document.layers[0].content =
            LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(3840, 2160, |x, y| {
                Rgba([(x % 251) as u8, (y % 239) as u8, 150, 255])
            }))));
        let mut stroke = WarpStroke::start(
            &document,
            [1700., 970.],
            mode,
            Brush {
                diameter: 520.,
                ..Brush::default()
            },
            false,
        )
        .unwrap();
        let mut from = [1700., 970.];
        for to in [[1720., 980.], [1740., 990.], [1760., 1000.]] {
            let clock = std::time::Instant::now();
            match mode {
                Mode::Smudge => stroke.smudge(to).unwrap(),
                Mode::Liquify => stroke.push(from, to).unwrap(),
            }
            let edit = clock.elapsed();
            let clock = std::time::Instant::now();
            stroke.write_back(&mut document).unwrap();
            eprintln!(
                "{mode:?} {to:?}: update={:.2}ms writeback={:.2}ms",
                edit.as_secs_f64() * 1000.,
                clock.elapsed().as_secs_f64() * 1000.
            );
            from = to;
        }
    }
}
