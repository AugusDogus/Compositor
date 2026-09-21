use super::*;
use image::imageops::FilterType;

#[derive(Clone, Copy)]
pub(super) struct Spec {
    pub side: u32,
    mean: [f32; 3],
    std: [f32; 3],
    grid: i64,
}
impl Spec {
    #[cfg(test)]
    pub const SAM2: Self = Self {
        side: 1024,
        mean: [0.485, 0.456, 0.406],
        std: [0.229, 0.224, 0.225],
        grid: 64,
    };
    pub const SAM3: Self = Self {
        side: 1008,
        mean: [0.5; 3],
        std: [0.5; 3],
        grid: 72,
    };
    pub fn feature_shapes(self) -> [[i64; 4]; 3] {
        [
            [1, 32, self.grid * 4, self.grid * 4],
            [1, 64, self.grid * 2, self.grid * 2],
            [1, 256, self.grid, self.grid],
        ]
    }
}
pub(super) fn normalize(image: &RgbaImage, spec: Spec) -> Vec<f32> {
    let rgb = image::RgbImage::from_fn(image.width(), image.height(), |x, y| {
        let p = image[(x, y)];
        image::Rgb([p[0], p[1], p[2]])
    });
    let resized = image::imageops::resize(&rgb, spec.side, spec.side, FilterType::Triangle);
    let plane = (spec.side * spec.side) as usize;
    let mut input = vec![0.; plane * 3];
    for (i, p) in resized.pixels().enumerate() {
        for c in 0..3 {
            input[c * plane + i] = (f32::from(p[c]) / 255. - spec.mean[c]) / spec.std[c];
        }
    }
    input
}
pub(super) fn prompt(point: Point, size: [u32; 2], spec: Spec) -> [f32; 2] {
    std::array::from_fn(|axis| (point[axis] * f64::from(spec.side) / f64::from(size[axis])) as f32)
}

pub(super) fn decode(
    shape: &[i64],
    masks: &[f32],
    scores: &[f32],
    object_score: f32,
    point: Option<Point>,
    alpha: &GrayImage,
) -> Result<GrayImage> {
    if shape.len() != 5
        || shape[..3] != [1, 1, 3]
        || !(1..=1024).contains(&shape[3])
        || !(1..=1024).contains(&shape[4])
        || scores.len() != 3
        || masks.len() != (shape[3] * shape[4] * 3) as usize
        || scores.iter().chain(masks).any(|v| !v.is_finite())
    {
        return Err(failed(
            "validate object masks",
            "unexpected tensor dimensions or nonfinite predictions",
        ));
    }
    if object_score < 0. {
        return Ok(GrayImage::new(alpha.width(), alpha.height()));
    }
    let (w, h) = (shape[4] as u32, shape[3] as u32);
    let plane = (w * h) as usize;
    let choice = (0..3)
        .filter(|i| {
            point.is_none_or(|p| {
                bilinear(
                    &masks[i * plane..(i + 1) * plane],
                    w,
                    h,
                    p[0].floor() as u32,
                    p[1].floor() as u32,
                    alpha.width(),
                    alpha.height(),
                ) > 0.
            })
        })
        .max_by(|a, b| scores[*a].total_cmp(&scores[*b]));
    let Some(choice) = choice else {
        return Ok(GrayImage::new(alpha.width(), alpha.height()));
    };
    let logits = &masks[choice * plane..(choice + 1) * plane];
    Ok(GrayImage::from_fn(alpha.width(), alpha.height(), |x, y| {
        let logit = bilinear(logits, w, h, x, y, alpha.width(), alpha.height());
        Luma([(f32::from(alpha[(x, y)][0]) / (1. + (-logit).exp())).round() as u8])
    }))
}

// ONNX/PyTorch align_corners=false interpolation. Image crate's floating pixel
// resize clamps to [0,1], which would destroy the sign of segmentation logits.
fn bilinear(
    values: &[f32],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    out_width: u32,
    out_height: u32,
) -> f32 {
    let sx = ((x as f64 + 0.5) * f64::from(width) / f64::from(out_width) - 0.5)
        .clamp(0., f64::from(width - 1));
    let sy = ((y as f64 + 0.5) * f64::from(height) / f64::from(out_height) - 0.5)
        .clamp(0., f64::from(height - 1));
    let x0 = sx.floor() as usize;
    let y0 = sy.floor() as usize;
    let x1 = (x0 + 1).min(width as usize - 1);
    let y1 = (y0 + 1).min(height as usize - 1);
    let tx = (sx - x0 as f64) as f32;
    let ty = (sy - y0 as f64) as f32;
    let at = |x, y| values[y * width as usize + x];
    let top = at(x0, y0) * (1. - tx) + at(x1, y0) * tx;
    let bottom = at(x0, y1) * (1. - tx) + at(x1, y1) * tx;
    top * (1. - ty) + bottom * ty
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preprocessing_keeps_rgb_channels_and_scales_non_square_prompts() {
        let pixels = RgbaImage::from_pixel(2, 1, image::Rgba([255, 0, 128, 255]));
        let spec = Spec {
            side: 2,
            ..Spec::SAM3
        };
        let input = normalize(&pixels, spec);
        assert_eq!(&input[..4], &[1.; 4]);
        assert_eq!(&input[4..8], &[-1.; 4]);
        assert!(input[8..].iter().all(|v| (*v - 1. / 255.).abs() < 1e-6));
        assert_eq!(prompt([800., 200.], [1600, 400], Spec::SAM3), [504., 504.]);
    }

    #[test]
    fn click_chooses_best_scored_instance_containing_prompt() {
        let alpha = GrayImage::from_pixel(2, 1, Luma([255]));
        let masks = [-8., 8., 8., -8., 8., 8.];
        let result = decode(
            &[1, 1, 3, 1, 2],
            &masks,
            &[0.99, 0.9, 0.8],
            1.,
            Some([0., 0.]),
            &alpha,
        )
        .unwrap();
        assert!(result[(0, 0)][0] > 253);
        assert!(result[(1, 0)][0] < 2);
    }

    #[test]
    fn click_containment_uses_the_resized_mask_at_boundary_pixels() {
        let alpha = GrayImage::from_pixel(4, 1, Luma([255]));
        let masks = [-1., 8.].repeat(3);
        let result = decode(
            &[1, 1, 3, 1, 2],
            &masks,
            &[0.9; 3],
            1.,
            Some([1.5, 0.5]),
            &alpha,
        )
        .unwrap();
        assert!(result[(1, 0)][0] > 128);
    }

    #[test]
    fn box_mask_can_have_a_hole_and_preserves_source_transparency() {
        let alpha = GrayImage::from_vec(3, 1, vec![128, 255, 0]).unwrap();
        let masks = [8., -8., 8.].repeat(3);
        let result = decode(&[1, 1, 3, 1, 3], &masks, &[0.9, 0.8, 0.7], 1., None, &alpha).unwrap();
        assert_eq!(result.as_raw(), &[128, 0, 0]);
    }

    #[test]
    fn absent_object_is_empty_and_malformed_predictions_fail() {
        let alpha = GrayImage::from_pixel(1, 1, Luma([255]));
        let decode = |shape: &[i64], masks: &[f32], scores: &[f32], presence| {
            super::decode(shape, masks, scores, presence, None, &alpha)
        };
        assert_eq!(
            decode(&[1, 1, 3, 1, 1], &[8.; 3], &[0.9; 3], -1.)
                .unwrap()
                .as_raw(),
            &[0]
        );
        assert!(decode(&[1, 1, 3, 1, 1], &[f32::NAN; 3], &[0.9; 3], 1.).is_err());
        assert!(decode(&[1, 1, 3, 1, 1], &[8.; 3], &[f32::INFINITY; 3], 1.).is_err());
        assert!(decode(&[1, 1, 3, 1, 2], &[8.; 3], &[0.9; 3], 1.).is_err());
    }

    #[test]
    fn logits_keep_negative_background_and_confident_foreground_when_resized() {
        let alpha = GrayImage::from_pixel(8, 4, Luma([255]));
        let masks = [-8., 8., -8., 8.].repeat(3);
        let output = decode(
            &[1, 1, 3, 2, 2],
            &masks,
            &[0.9, 0.8, 0.7],
            1.,
            Some([6., 2.]),
            &alpha,
        )
        .unwrap();
        assert!(output[(0, 2)][0] < 2);
        assert!(output[(7, 2)][0] > 253);
        assert!(output[(3, 2)][0] < 128);
        assert!(output[(4, 2)][0] > 128);
    }
}
