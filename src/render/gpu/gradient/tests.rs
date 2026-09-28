use super::*;
use crate::{
    blend::Blend,
    gradient::{Style, stops::Stop},
    selection::Selection,
};
use image::{Luma, Rgba};

fn reference(pixels: &Pixels<'_>, paint: &Paint<'_>) -> Vec<u8> {
    let (width, height) = pixels.dimensions();
    let delta = [paint.end[0] - paint.start[0], paint.end[1] - paint.start[1]];
    let length = delta[0].hypot(delta[1]);
    let mut output = Vec::new();
    for (index, pixel) in pixels.bytes().chunks_exact(pixels.channels()).enumerate() {
        let point = paint.transform.point([
            ((index % width as usize) as f64 + 0.5) / f64::from(width),
            ((index / width as usize) as f64 + 0.5) / f64::from(height),
        ]);
        let distance = match paint.gradient.shape {
            Shape::Linear => {
                ((point[0] - paint.start[0]) * delta[0] + (point[1] - paint.start[1]) * delta[1])
                    / (length * length)
            }
            Shape::Radial => (point[0] - paint.start[0]).hypot(point[1] - paint.start[1]) / length,
        }
        .clamp(0., 1.);
        let mut color = paint.ramp.sample(if paint.gradient.reversed {
            1. - distance
        } else {
            distance
        });
        let inside = point[0] >= 0.
            && point[1] >= 0.
            && point[0] < paint.canvas[0]
            && point[1] < paint.canvas[1];
        color[3] *= if inside {
            paint.gradient.opacity
                * paint
                    .selection
                    .map_or(1., |selection| selection.coverage(point))
        } else {
            0.
        };
        if color[3] == 0. {
            output.extend_from_slice(pixel);
        } else if pixels.channels() == 1 {
            let gray = crate::gradient::mask_color(color)[0];
            let before = f64::from(pixel[0]) / 255.;
            output.push(((before + (gray - before) * color[3]) * 255.).round() as u8);
        } else {
            let bottom = std::array::from_fn(|channel| f64::from(pixel[channel]) / 255.);
            output.extend(
                Blend::Normal
                    .composite(bottom, color)
                    .map(|v| (v * 255.).round() as u8),
            );
        }
    }
    output
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn gpu_matches_cpu_for_raster_masks_selections_transforms_and_custom_stops() {
    let engine = Engine::new().unwrap();
    let image = RgbaImage::from_fn(97, 73, |x, y| {
        Rgba([x as u8, y as u8, 197, (x * 19 + y * 23) as u8])
    });
    let mask = GrayImage::from_fn(97, 73, |x, y| Luma([(x * 19 + y * 23) as u8]));
    let mut selection = Selection::rectangle(160, 120, [17., 3.], [110., 90.], true);
    selection.origin = [0.35, -0.8];
    for shape in [Shape::Linear, Shape::Radial] {
        for reversed in [false, true] {
            for transformed in [false, true] {
                for selected in [false, true] {
                    let gradient = Gradient {
                        shape,
                        reversed,
                        opacity: 0.73,
                        style: Style::Custom,
                        stops: Stops::new(vec![
                            Stop {
                                position: 0.1,
                                color: [239, 67, 13, 191],
                            },
                            Stop {
                                position: 0.47,
                                color: [23, 171, 209, 0],
                            },
                            Stop {
                                position: 0.9,
                                color: [172, 16, 252, 241],
                            },
                        ])
                        .unwrap(),
                    };
                    let mut transform = Transform::new(97, 73);
                    if transformed {
                        transform.origin = [31.25, -9.5];
                        transform.rotation = 37.;
                        transform.flip_x = true;
                        transform.size = [137.5, 83.];
                    }
                    let paint = Paint {
                        gradient: &gradient,
                        ramp: &gradient.stops,
                        start: [21.5, 13.5],
                        end: [82.3, 66.7],
                        transform,
                        canvas: [160., 120.],
                        selection: selected.then_some(&selection),
                    };
                    assert!(paint.supported());
                    for pixels in [Pixels::Color(&image), Pixels::Mask(&mask)] {
                        let gpu = engine.gradient(&pixels, &paint).unwrap();
                        let cpu = reference(&pixels, &paint);
                        for (index, (actual, expected)) in gpu.iter().zip(&cpu).enumerate() {
                            assert!(
                                actual.abs_diff(*expected) <= 1,
                                "shape{shape:?} reversed{reversed} transformed{transformed} selected{selected} channels{} byte{index}: gpu{actual} cpu{expected}",
                                pixels.channels()
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn hard_edges_use_cpu_without_losing_boundary_precision() {
    for reversed in [false, true] {
        let gradient = Gradient {
            reversed,
            style: Style::Custom,
            stops: Stops::new(vec![
                Stop {
                    position: 0.3,
                    color: [0, 0, 0, 255],
                },
                Stop {
                    position: 0.3,
                    color: [255; 4],
                },
            ])
            .unwrap(),
            ..Default::default()
        };
        let mut doc = crate::document::Document::new(512, 512).unwrap();
        let paint = Paint {
            gradient: &gradient,
            ramp: &gradient.stops,
            start: [0.5, 0.5],
            end: [10.5, 0.5],
            transform: Transform::new(512, 512),
            canvas: [512., 512.],
            selection: None,
        };
        let original = RgbaImage::new(512, 512);
        assert!(apply(Pixels::Color(&original), &paint).unwrap().is_none());
        let expected = reference(&Pixels::Color(&original), &paint);
        gradient
            .apply(&mut doc, paint.start, paint.end, [0; 4], [0; 4], false)
            .unwrap();
        assert_eq!(doc.layers[0].raster().unwrap().as_raw(), &expected);
    }
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn gpu_keeps_transparent_noops_and_crosses_chunk_boundaries() {
    let engine = Engine::new().unwrap();
    let image = RgbaImage::from_pixel(2053, 2045, Rgba([12, 91, 200, 127]));
    let gradient = Gradient {
        style: Style::ForegroundToBackground,
        ..Default::default()
    };
    for ramp in [
        Stops::endpoints([42, 82, 120, 0], [255, 0, 0, 0]),
        Stops::endpoints([0, 0, 0, 255], [255; 4]),
    ] {
        let paint = Paint {
            gradient: &gradient,
            ramp: &ramp,
            start: [0.5, 0.5],
            end: [2052.5, 2044.5],
            transform: Transform::new(2053, 2045),
            canvas: [2053., 2045.],
            selection: None,
        };
        let gpu = engine.gradient(&Pixels::Color(&image), &paint).unwrap();
        let cpu = reference(&Pixels::Color(&image), &paint);
        assert!(gpu.iter().zip(&cpu).all(|(a, b)| a.abs_diff(*b) <= 1));
        if ramp.as_slice().iter().all(|stop| stop.color[3] == 0) {
            assert_eq!(&gpu, image.as_raw());
        }
    }
}

#[test]
#[ignore = "Requires a hardware Vulkan adapter"]
fn gradient_apply_uses_independent_mask_placement_and_preserves_empty_edits() {
    let mut doc = crate::document::Document::new(512, 512).unwrap();
    crate::edits::add_mask(&mut doc, false).unwrap();
    let mut placement = Transform::new(512, 512);
    placement.rotation = -23.;
    placement.flip_y = true;
    placement.origin = [37.5, -17.25];
    let original = GrayImage::from_pixel(512, 512, Luma([155]));
    let mask = doc.layers[0].mask.as_mut().unwrap();
    mask.placement = Some(placement);
    mask.pixels = std::sync::Arc::new(original.clone());
    doc.selection = Some(Selection::rectangle(
        512,
        512,
        [32., 20.],
        [413., 390.],
        true,
    ));
    let gradient = Gradient {
        shape: Shape::Radial,
        opacity: 0.61,
        reversed: true,
        ..Default::default()
    };
    let ramp = gradient.ramp([190, 81, 29, 187], [255; 4]);
    let paint = Paint {
        gradient: &gradient,
        ramp: &ramp,
        start: [91.5, 87.5],
        end: [409.5, 289.5],
        transform: placement,
        canvas: [512., 512.],
        selection: doc.selection.as_ref(),
    };
    let expected = reference(&Pixels::Mask(&original), &paint);
    let (start, end) = (paint.start, paint.end);
    gradient
        .apply(&mut doc, start, end, [190, 81, 29, 187], [255; 4], true)
        .unwrap();
    let actual = &doc.layers[0].mask.as_ref().unwrap().pixels;
    assert!(
        actual
            .as_raw()
            .iter()
            .zip(&expected)
            .all(|(a, b)| a.abs_diff(*b) <= 1)
    );
    assert_eq!(
        doc.layers[0].mask.as_ref().unwrap().placement,
        Some(placement)
    );
    let before = doc.clone();
    gradient
        .apply(
            &mut doc,
            [91.5, 87.5],
            [409.5, 289.5],
            [190, 81, 29, 0],
            [255; 4],
            true,
        )
        .unwrap();
    assert_eq!(doc, before);
}
