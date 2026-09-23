use super::LayerEffects;
use image::{Rgba, RgbaImage};

fn spread(
    source: &[f32],
    width: usize,
    height: usize,
    reach: usize,
    inside: bool,
    vertical: bool,
) -> Vec<f32> {
    // Monotone windows make even a 500 px stroke linear in image area.
    let mut result = vec![0.; source.len()];
    let (lines, length) = if vertical {
        (width, height)
    } else {
        (height, width)
    };
    for line in 0..lines {
        let index = |position: usize| {
            if vertical {
                position * width + line
            } else {
                line * width + position
            }
        };
        let mut window = std::collections::VecDeque::<usize>::new();
        let mut next = 0;
        for position in 0..length {
            let end = (position + reach + 1).min(length);
            while next < end {
                while window.back().is_some_and(|&back| {
                    if inside {
                        source[index(back)] >= source[index(next)]
                    } else {
                        source[index(back)] <= source[index(next)]
                    }
                }) {
                    window.pop_back();
                }
                window.push_back(next);
                next += 1;
            }
            let begin = position.saturating_sub(reach);
            while window.front().is_some_and(|&front| front < begin) {
                window.pop_front();
            }
            result[index(position)] = if inside && (position < reach || position + reach >= length)
            {
                0.
            } else {
                window.front().map_or(0., |&front| source[index(front)])
            };
        }
    }
    result
}
pub(crate) fn gaussian(source: &[f32], width: usize, height: usize, sigma: f32) -> Vec<f32> {
    if sigma <= 0. {
        return source.to_vec();
    }
    let radius = (sigma * 3.).ceil() as i32;
    let mut weights: Vec<_> = (-radius..=radius)
        .map(|offset| (-(offset as f32).powi(2) / (2. * sigma * sigma)).exp())
        .collect();
    let sum: f32 = weights.iter().sum();
    for weight in &mut weights {
        *weight /= sum;
    }
    let mut input = source.to_vec();
    for vertical in [false, true] {
        let mut output = vec![0.; input.len()];
        for y in 0..height {
            for x in 0..width {
                output[y * width + x] = weights
                    .iter()
                    .enumerate()
                    .map(|(i, weight)| {
                        let offset = i as i32 - radius;
                        let sx = if vertical {
                            x
                        } else {
                            (x as i32 + offset).clamp(0, width as i32 - 1) as usize
                        };
                        let sy = if vertical {
                            (y as i32 + offset).clamp(0, height as i32 - 1) as usize
                        } else {
                            y
                        };
                        input[sy * width + sx] * weight
                    })
                    .sum();
            }
        }
        input = output;
    }
    input
}
fn shift(source: &[f32], width: usize, height: usize, offset: [f32; 2]) -> Vec<f32> {
    let mut output = vec![0.; source.len()];
    for y in 0..height {
        for x in 0..width {
            let sx = x as f32 - offset[0];
            let sy = y as f32 - offset[1];
            if sx < 0. || sy < 0. || sx > (width - 1) as f32 || sy > (height - 1) as f32 {
                continue;
            }
            let x0 = sx.floor() as usize;
            let y0 = sy.floor() as usize;
            let x1 = (x0 + 1).min(width - 1);
            let y1 = (y0 + 1).min(height - 1);
            let fx = sx - x0 as f32;
            let fy = sy - y0 as f32;
            output[y * width + x] =
                (source[y0 * width + x0] * (1. - fx) + source[y0 * width + x1] * fx) * (1. - fy)
                    + (source[y1 * width + x0] * (1. - fx) + source[y1 * width + x1] * fx) * fy;
        }
    }
    output
}
fn over(base: [f32; 4], color: [f64; 3], coverage: f32) -> [f32; 4] {
    [
        color[0] as f32 * coverage + base[0] * (1. - coverage),
        color[1] as f32 * coverage + base[1] * (1. - coverage),
        color[2] as f32 * coverage + base[2] * (1. - coverage),
        coverage + base[3] * (1. - coverage),
    ]
}
pub(crate) fn render(image: &RgbaImage, effects: &LayerEffects) -> RgbaImage {
    let (width, height) = (image.width() as usize, image.height() as usize);
    let shape: Vec<_> = image.pixels().map(|p| p[3] as f32 / 255.).collect();
    let ring = effects.stroke.as_ref().map(|s| {
        let rows = spread(
            &shape,
            width,
            height,
            s.size.ceil() as usize,
            s.inside,
            false,
        );
        let area = spread(&rows, width, height, s.size.ceil() as usize, s.inside, true);
        shape
            .iter()
            .zip(area)
            .map(|(&a, b)| {
                if s.inside {
                    (a - b).max(0.)
                } else {
                    (b - a).max(0.)
                }
            })
            .collect::<Vec<_>>()
    });
    let shadow_plane = |s: &super::ShadowEffect| {
        gaussian(
            &shift(&shape, width, height, s.offset()),
            width,
            height,
            s.blur as f32,
        )
    };
    let shadow = effects.shadow.as_ref().map(shadow_plane);
    let inner = effects.inner_shadow.as_ref().map(shadow_plane);
    let glow = effects
        .outer_glow
        .as_ref()
        .filter(|s| s.size > 0. && s.opacity > 0.)
        .map(|s| {
            if s.size <= 0.02 {
                shape.clone()
            } else {
                gaussian(&shape, width, height, (s.size / 2.) as f32)
            }
        });
    let inside_glow = effects
        .inner_glow
        .as_ref()
        .filter(|s| s.size > 0. && s.opacity > 0.)
        .map(|s| {
            if s.size <= 0.02 {
                shape.clone()
            } else {
                gaussian(&shape, width, height, (s.size / 2.) as f32)
            }
        });
    RgbaImage::from_fn(image.width(), image.height(), |x, y| {
        let i = y as usize * width + x as usize;
        let mut out = [0.; 4];
        if let (Some(s), Some(plane)) = (&effects.shadow, &shadow) {
            out = over(out, [s.red, s.green, s.blue], plane[i] * s.opacity as f32);
        }
        if let (Some(s), Some(plane)) = (&effects.outer_glow, &glow) {
            out = over(
                out,
                [s.red, s.green, s.blue],
                plane[i] * (1. - shape[i]) * s.opacity as f32,
            );
        }
        if let (Some(s), Some(ring)) = (&effects.stroke, &ring)
            && !s.inside
        {
            out = over(out, [s.red, s.green, s.blue], ring[i] * s.opacity as f32);
        }
        let mut source = image[(x, y)].0.map(|v| v as f32 / 255.);
        // Interior effects replace source color without creating additional coverage.
        let tint = |source: &mut [f32; 4], color: [f64; 3], coverage: f32| {
            for c in 0..3 {
                source[c] = source[c] * (1. - coverage) + color[c] as f32 * coverage;
            }
        };
        if let Some(s) = &effects.color_overlay {
            tint(&mut source, [s.red, s.green, s.blue], s.opacity as f32);
        }
        if let (Some(s), Some(plane)) = (&effects.inner_glow, &inside_glow) {
            let coverage = (shape[i] * (1. - plane[i]) * s.opacity as f32).clamp(0., 1.);
            let alpha = coverage + source[3] * (1. - coverage);
            if alpha > 0. {
                let color = [s.red, s.green, s.blue];
                for c in 0..3 {
                    source[c] = (color[c] as f32 * coverage
                        + source[c] * source[3] * (1. - coverage))
                        / alpha;
                }
                source[3] = alpha;
            }
        }
        if let (Some(s), Some(plane)) = (&effects.inner_shadow, &inner) {
            tint(
                &mut source,
                [s.red, s.green, s.blue],
                (1. - plane[i]) * s.opacity as f32,
            );
        }
        if let (Some(s), Some(ring)) = (&effects.stroke, &ring)
            && s.inside
            && shape[i] > 0.
        {
            tint(
                &mut source,
                [s.red, s.green, s.blue],
                ring[i] / shape[i] * s.opacity as f32,
            );
        }
        out = over(
            out,
            [source[0] as f64, source[1] as f64, source[2] as f64],
            source[3],
        );
        if out[3] > 0. {
            for c in 0..3 {
                out[c] /= out[3];
            }
        }
        Rgba(out.map(|v| (v.clamp(0., 1.) * 255.).round() as u8))
    })
}
