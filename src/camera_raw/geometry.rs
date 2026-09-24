use super::{Projection, Settings};
use crate::{Result, invalid};
use image::RgbaImage;
#[derive(Clone, Debug, PartialEq)]
pub struct Guide {
    pub start: [f64; 2],
    pub end: [f64; 2],
}
impl Guide {
    pub(super) fn usable(&self) -> bool {
        (self.end[0] - self.start[0]).hypot(self.end[1] - self.start[1]) > 0.01
    }
    pub fn valid(&self) -> bool {
        self.start
            .iter()
            .chain(&self.end)
            .all(|x| x.is_finite() && (0. ..=1.).contains(x))
    }
}

fn corners(s: &Settings, w: f64, h: f64) -> [[f64; 2]; 4] {
    let g = &s.geometry;
    let (mut vertical, mut horizontal, mut rotation) = (g.vertical, g.horizontal, g.rotate);
    if s.guided
        && let Some(first) = s.guides.iter().find(|g| g.usable())
    {
        let dx = first.end[0] - first.start[0];
        let dy = first.end[1] - first.start[1];
        if dx.hypot(dy) > 0.01 {
            let mut rotate = -dy.atan2(dx).to_degrees();
            if rotate > 45. {
                rotate -= 90.;
            } else if rotate < -45. {
                rotate += 90.;
            }
            rotation += rotate;
            if let Some(second) = s.guides.iter().filter(|g| g.usable()).nth(1) {
                let dx = second.end[0] - second.start[0];
                let dy = second.end[1] - second.start[1];
                if dx.hypot(dy) > 0.01 {
                    let a = dy.atan2(dx).to_degrees();
                    if a.abs() > 45. {
                        vertical += if a > 0. { 25. } else { -25. };
                    } else {
                        horizontal += if a > 0. { 25. } else { -25. };
                    }
                }
            }
        }
    }
    let strength = if s.projection == Projection::Perspective {
        1.
    } else {
        0.55
    };
    let v = vertical / 100. * w * 0.18 * strength;
    let hz = horizontal / 100. * h * 0.18 * strength;
    let sx = g.offset_x / 100. * w * 0.15;
    let sy = g.offset_y / 100. * h * 0.15;
    let center = [w / 2. + sx, h / 2. + sy];
    let (sin, cos) = rotation.to_radians().sin_cos();
    let aspect = 1. + g.aspect / 200.;
    let zoom = 1. + g.scale / 100.;
    // Start with Core Image lower-left coordinates, convert output to top-down.
    [
        [-v + sx, h + sy],
        [w + v + sx, h + sy],
        [w + hz + sx, -sy],
        [-hz + sx, -sy],
    ]
    .map(|p| {
        let dx = p[0] - center[0];
        let dy = p[1] - center[1];
        [
            center[0] + (dx * cos - dy * sin) * aspect * zoom,
            h - (center[1] + (dx * sin + dy * cos) / aspect * zoom),
        ]
    })
}
fn inverse_quad(q: [[f64; 2]; 4]) -> Result<[f32; 9]> {
    let [a, b, c, d] = q;
    let dx1 = b[0] - c[0];
    let dx2 = d[0] - c[0];
    let dx3 = a[0] - b[0] + c[0] - d[0];
    let dy1 = b[1] - c[1];
    let dy2 = d[1] - c[1];
    let dy3 = a[1] - b[1] + c[1] - d[1];
    let den = dx1 * dy2 - dx2 * dy1;
    if den.abs() < 1e-10 {
        return Err(invalid(
            "Geometry collapses the image. Reduce the perspective or scale settings.",
        ));
    }
    let g = (dx3 * dy2 - dx2 * dy3) / den;
    let h = (dx1 * dy3 - dx3 * dy1) / den;
    let m = [
        b[0] - a[0] + g * b[0],
        d[0] - a[0] + h * d[0],
        a[0],
        b[1] - a[1] + g * b[1],
        d[1] - a[1] + h * d[1],
        a[1],
        g,
        h,
        1.,
    ];
    let [a, b, c, d, e, f, g, h, i] = m;
    let adj = [
        e * i - f * h,
        c * h - b * i,
        b * f - c * e,
        f * g - d * i,
        a * i - c * g,
        c * d - a * f,
        d * h - e * g,
        b * g - a * h,
        a * e - b * d,
    ];
    let det = a * adj[0] + b * adj[3] + c * adj[6];
    if det.abs() < 1e-10 {
        return Err(invalid(
            "Geometry transform is singular. Reduce the perspective settings.",
        ));
    }
    Ok(adj.map(|x| (x / det) as f32))
}
fn render_cpu(image: &RgbaImage, matrix: [f32; 9]) -> RgbaImage {
    RgbaImage::from_fn(image.width(), image.height(), |x, y| {
        let (x, y) = (x as f32 + 0.5, y as f32 + 0.5);
        let z = matrix[6] * x + matrix[7] * y + matrix[8];
        let uv = [
            f64::from((matrix[0] * x + matrix[1] * y + matrix[2]) / z),
            f64::from((matrix[3] * x + matrix[4] * y + matrix[5]) / z),
        ];
        image::Rgba(
            crate::render::pixel(image, uv, crate::geometry::Sampling::Smooth)
                .map(|v| (v.clamp(0., 1.) * 255.).round() as u8),
        )
    })
}
pub(super) fn render(image: &RgbaImage, s: &Settings) -> Result<RgbaImage> {
    if !s.adjusts(super::Group::Geometry) {
        return Ok(image.clone());
    }
    let matrix = inverse_quad(corners(
        s,
        f64::from(image.width()),
        f64::from(image.height()),
    ))?;
    let mut result = match crate::render::gpu_camera_geometry(image, matrix)? {
        Some(pixels) => pixels,
        None => render_cpu(image, matrix),
    };
    if s.constrain_crop {
        let mut bounds = [image.width(), image.height(), 0, 0];
        for (x, y, p) in result.enumerate_pixels() {
            if p[3] > 0 {
                bounds[0] = bounds[0].min(x);
                bounds[1] = bounds[1].min(y);
                bounds[2] = bounds[2].max(x + 1);
                bounds[3] = bounds[3].max(y + 1);
            }
        }
        if bounds[2] > bounds[0] && bounds[3] > bounds[1] {
            let crop = image::imageops::crop_imm(
                &result,
                bounds[0],
                bounds[1],
                bounds[2] - bounds[0],
                bounds[3] - bounds[1],
            )
            .to_image();
            let scale = (image.width() as f64 / crop.width() as f64)
                .min(image.height() as f64 / crop.height() as f64);
            let (w, h) = (
                (crop.width() as f64 * scale).round() as u32,
                (crop.height() as f64 * scale).round() as u32,
            );
            let resized = image::imageops::resize(
                &crate::native_pixels::premultiply(&crop),
                w,
                h,
                image::imageops::FilterType::Lanczos3,
            );
            result = RgbaImage::new(image.width(), image.height());
            image::imageops::replace(
                &mut result,
                &crate::native_pixels::unpremultiply(resized),
                i64::from((image.width() - w) / 2),
                i64::from((image.height() - h) / 2),
            );
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Requires hardware Vulkan; compares the executed geometry shader with the CPU fallback"]
    fn gpu_geometry_matches_cpu_for_perspective_guides_and_alpha_edges() {
        let image = RgbaImage::from_fn(73, 51, |x, y| {
            image::Rgba([
                (x * 3) as u8,
                (y * 4) as u8,
                177,
                if x < 9 {
                    0
                } else {
                    ((x + y) * 2).min(255) as u8
                },
            ])
        });
        let mut settings = Settings::default();
        settings.geometry.rotate = 17.;
        settings.geometry.vertical = 34.;
        settings.geometry.horizontal = -21.;
        settings.geometry.offset_x = 12.;
        settings.geometry.aspect = 8.;
        settings.guides = vec![Guide {
            start: [0.1, 0.1],
            end: [0.9, 0.3],
        }];
        for (projection, guided) in [
            (Projection::Perspective, false),
            (Projection::Rectilinear, false),
            (Projection::Perspective, true),
        ] {
            settings.projection = projection;
            settings.guided = guided;
            let matrix = inverse_quad(corners(
                &settings,
                image.width() as f64,
                image.height() as f64,
            ))
            .unwrap();
            let expected = render_cpu(&image, matrix);
            let actual = crate::render::gpu_camera_geometry(&image, matrix)
                .unwrap()
                .expect("hardware GPU geometry must execute");
            assert!(actual.pixels().any(|p| p[3] == 0));
            assert!(actual.pixels().any(|p| p[3] > 0));
            for (index, (a, b)) in actual.as_raw().iter().zip(expected.as_raw()).enumerate() {
                assert!(
                    a.abs_diff(*b) <= 1,
                    "{projection:?} guided={guided}, byte {index}: GPU {a}, CPU {b}"
                );
            }
        }
    }
}
