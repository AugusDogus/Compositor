use super::*;
use crate::{Color, PathBuilder, PathPrimitive};

#[test]
fn stroke_fringe_preserves_coverage_through_scaling_clipping_and_opacity() {
    use crate::{Svg, SvgPrimitive};
    let fonts = create_shared_font_system(&Assets::default(), &[]).unwrap();
    let mut renderer =
        pollster::block_on(OffscreenRenderer::new(PerformanceProfile::Balanced, fonts)).unwrap();
    let shapes = [
        vec![Point::new(24., 32.), Point::new(200., 144.)],
        vec![
            Point::new(24., 32.),
            Point::new(140., 32.),
            Point::new(140., 168.),
        ],
        (0..=64)
            .map(|i| {
                let angle = i as f32 * std::f32::consts::TAU / 64.;
                Point::new(112. + 56. * angle.cos(), 104. + 56. * angle.sin())
            })
            .collect(),
    ];
    for (shape, points) in shapes.iter().enumerate() {
        for width in [0.5, 1., 2., 12.] {
            for (scale, transform, translation) in [
                (1., [1., 1.], [0., 0.]),
                (1.5, [-0.75, 0.9], [210., 0.]),
                (2., [0.4, 1.], [25., 0.]),
            ] {
                let mut builder = PathBuilder::stroke(width);
                let mut commands = String::new();
                for (i, point) in points.iter().enumerate() {
                    use std::fmt::Write;
                    if i == 0 {
                        builder.move_to(*point);
                    } else {
                        builder.line_to(*point);
                    }
                    write!(
                        commands,
                        "{}{} {} ",
                        if i == 0 { "M" } else { "L" },
                        point.x,
                        point.y
                    )
                    .unwrap();
                }
                if shape == 2 {
                    builder.close();
                    commands.push('Z');
                }
                let svg = Svg::from_svg(format!(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" width="240" height="208"><path d="{commands}" fill="none" stroke="white" stroke-width="{width}" transform="matrix({} 0 0 {} {} {})"/></svg>"#,
                    transform[0], transform[1], translation[0], translation[1],
                )).unwrap();
                let clip = Rect::new(37.25, 18.5, 160.5, 166.);
                let color = Color::rgba8(255, 255, 255, 128);
                let mut scene = Scene::new();
                scene.clear(Color::BLACK);
                scene.push_path(
                    PathPrimitive::new(builder.build().unwrap(), color)
                        .scale_xy(transform[0], transform[1])
                        .translate(translation[0], translation[1])
                        .clip(clip),
                );
                scene.finish();
                let actual = renderer
                    .render_to_snapshot(&scene, Size::new(240., 208.), scale)
                    .unwrap();
                let mut scene = Scene::new();
                scene.clear(Color::BLACK);
                scene.push_svg(
                    SvgPrimitive::new(svg, Rect::new(0., 0., 240., 208.), color).clip(clip),
                );
                scene.finish();
                let expected = renderer
                    .render_to_snapshot(&scene, Size::new(240., 208.), scale)
                    .unwrap();
                let mut actual_ink = 0_u64;
                let mut expected_ink = 0_u64;
                let mut difference = 0_u64;
                for y in 0..actual.height() {
                    for x in 0..actual.width() {
                        let value = actual.pixel(x, y).unwrap()[0];
                        let reference = expected.pixel(x, y).unwrap()[0];
                        actual_ink += u64::from(value);
                        expected_ink += u64::from(reference);
                        difference += u64::from(value.abs_diff(reference));
                        assert!(
                            value <= 130,
                            "fringe overlaps at {x},{y}: {value}, shape={shape}, width={width}, scale={scale}"
                        );
                    }
                }
                let expected_area: f64 = points
                    .windows(2)
                    .map(|pair| {
                        clipped_fraction(pair, transform, translation, clip)
                            * f64::from((pair[1].x - pair[0].x).hypot(pair[1].y - pair[0].y))
                            * f64::from(width * (transform[0] * transform[1]).abs() * scale * scale)
                    })
                    .sum();
                let error = (actual_ink as f64 / 128. - expected_area).abs() / expected_area;
                assert!(
                    error < 0.05,
                    "stroke area differs by {error:.3}: actual={}, expected={expected_area}, shape={shape}, width={width}, scale={scale}",
                    actual_ink as f64 / 128.
                );
                // Tiny-skia uses a modulated hairline at <=1 device pixel.
                // Thin strokes also magnify differences between AA kernels, so
                // their oracle is geometric area, not another rasterizer.
                if width * transform[0].abs().min(transform[1].abs()) * scale >= 2. {
                    let error = difference as f64 / expected_ink as f64;
                    assert!(
                        error < 0.15,
                        "stroke coverage differs by {error:.3}: shape={shape}, width={width}, scale={scale}"
                    );
                }
            }
        }
    }
}

fn clipped_fraction(
    points: &[Point],
    transform: [f32; 2],
    translation: [f32; 2],
    clip: Rect,
) -> f64 {
    let mut start = 0_f64;
    let mut end = 1_f64;
    for (first, second, factor, offset, low, high) in [
        (
            points[0].x,
            points[1].x,
            transform[0],
            translation[0],
            clip.x,
            clip.right(),
        ),
        (
            points[0].y,
            points[1].y,
            transform[1],
            translation[1],
            clip.y,
            clip.bottom(),
        ),
    ] {
        let first = f64::from(first * factor + offset);
        let second = f64::from(second * factor + offset);
        let delta = second - first;
        if delta.abs() < 1e-12 {
            if first < f64::from(low) || first > f64::from(high) {
                return 0.;
            }
        } else {
            let a = (f64::from(low) - first) / delta;
            let b = (f64::from(high) - first) / delta;
            start = start.max(a.min(b));
            end = end.min(a.max(b));
        }
    }
    (end - start).max(0.)
}

#[test]
fn path_antialiasing_preserves_stroke_width_and_collinear_subdivision() {
    let fonts = create_shared_font_system(&Assets::default(), &[]).unwrap();
    let mut renderer =
        pollster::block_on(OffscreenRenderer::new(PerformanceProfile::Balanced, fonts)).unwrap();
    for scale in [1., 1.5, 2.] {
        for end in [Point::new(220.25, 24.5), Point::new(220.25, 184.5)] {
            let start = Point::new(20.25, 24.5);
            let length = (end.x - start.x).hypot(end.y - start.y);
            let mut snapshots = Vec::new();
            for segments in [1, 255] {
                let mut path = PathBuilder::stroke(2.);
                path.move_to(start);
                for i in 1..=segments {
                    let t = i as f32 / segments as f32;
                    path.line_to(Point::new(
                        start.x + t * (end.x - start.x),
                        start.y + t * (end.y - start.y),
                    ));
                }
                let mut scene = Scene::new();
                scene.clear(Color::BLACK);
                scene.push_path(PathPrimitive::new(path.build().unwrap(), Color::WHITE));
                scene.finish();
                let frame = renderer
                    .render_to_snapshot(&scene, Size::new(240., 208.), scale)
                    .unwrap();
                let coverage: f32 = (0..frame.height())
                    .flat_map(|y| (0..frame.width()).map(move |x| (x, y)))
                    .map(|(x, y)| f32::from(frame.pixel(x, y).unwrap()[0]) / 255.)
                    .sum();
                let expected = length * 2. * scale * scale;
                assert!(
                    (coverage - expected).abs() < expected * 0.04,
                    "scale={scale}, end={end:?}, segments={segments}: {coverage} covered pixels, expected {expected}"
                );
                snapshots.push(frame);
            }
            let difference: u64 = (0..snapshots[0].height())
                .flat_map(|y| (0..snapshots[0].width()).map(move |x| (x, y)))
                .map(|(x, y)| {
                    u64::from(
                        snapshots[0].pixel(x, y).unwrap()[0]
                            .abs_diff(snapshots[1].pixel(x, y).unwrap()[0]),
                    )
                })
                .sum();
            assert!(
                difference as f32 / 255. < length * scale * scale * 0.02,
                "subdividing the same line changed {difference} intensity levels at scale {scale}"
            );
        }
    }
}
