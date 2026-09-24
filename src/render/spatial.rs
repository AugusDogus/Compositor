//! Backdrop surfaces for spatial adjustment layers. Each surface captures the
//! stack immediately before its adjustment, including prior spatial adjustments.
use super::*;
use crate::{Result, adjustment::Kind, geometry::Transform};
use std::sync::Arc;

pub(super) struct Surface {
    pub image: Arc<RgbaImage>,
    pub transform: Transform,
}
pub(super) type Surfaces = HashMap<Uuid, Surface>;

pub(super) fn prepare(
    doc: &Document,
    size: [u32; 2],
    origin: Point,
    step: Point,
    accelerated: bool,
) -> Result<Surfaces> {
    fn ordered(doc: &Document, parent: Option<Uuid>, out: &mut Vec<Uuid>) {
        for layer in doc
            .layers
            .iter()
            .filter(|l| l.parent == parent && l.visible)
        {
            if layer.is_group() {
                ordered(doc, Some(layer.id), out);
            } else if matches!(&layer.content,LayerContent::Adjustment(a) if matches!(a.kind,Kind::GaussianBlur|Kind::MotionBlur))
            {
                out.push(layer.id);
            }
        }
    }
    let mut ids = Vec::new();
    ordered(doc, None, &mut ids);
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    // Build at document resolution when zoomed in, and uniform preview resolution
    // when zoomed out. This keeps blur radius isotropic for non-square view sampling.
    let spacing = step[0].min(step[1]).max(1.);
    let aligned = origin.map(|v| (v / spacing).floor() * spacing);
    let size = [
        ((origin[0] + f64::from(size[0]) * step[0] - aligned[0]) / spacing).ceil() as u32,
        ((origin[1] + f64::from(size[1]) * step[1] - aligned[1]) / spacing).ceil() as u32,
    ];
    let origin = aligned;
    let step = [spacing; 2];
    let mut state = RenderState::new(doc);
    // Accumulate halos so chained filters also have complete input at viewport edges.
    let margin: f64 = ids
        .iter()
        .filter_map(|id| doc.layer(*id))
        .map(|l| match &l.content {
            LayerContent::Adjustment(a) if a.kind == Kind::GaussianBlur => {
                a.blur_radius.unwrap_or(10.) * 3. + 2.
            }
            LayerContent::Adjustment(a) => a.motion_distance.unwrap_or(10.) * 0.5 + 2.,
            _ => 0.,
        })
        .sum();
    let pad = [
        (margin / step[0]).ceil() as u32,
        (margin / step[1]).ceil() as u32,
    ];
    let width=size[0].checked_add(pad[0].saturating_mul(2)).ok_or_else(||crate::invalid("Blur preview exceeds the supported image size. Reduce the blur radius or zoom out."))?;
    let height=size[1].checked_add(pad[1].saturating_mul(2)).ok_or_else(||crate::invalid("Blur preview exceeds the supported image size. Reduce the blur radius or zoom out."))?;
    crate::document::validate_size(width, height)?;
    // Retained surfaces plus the input and blur scratch allocations share the
    // document's memory budget rather than multiplying it for each adjustment.
    crate::document::validate_pixel_budget(
        u64::from(width) * u64::from(height) * (ids.len() as u64 + 3),
    )?;
    let origin = [
        origin[0] - f64::from(pad[0]) * step[0],
        origin[1] - f64::from(pad[1]) * step[1],
    ];
    let transform = Transform {
        origin,
        size: [f64::from(width) * step[0], f64::from(height) * step[1]],
        sampling: Sampling::Smooth,
        ..Transform::new(width, height)
    };
    for id in ids {
        let Some(layer) = doc.layer(id) else { continue };
        let LayerContent::Adjustment(a) = &layer.content else {
            continue;
        };
        // Disconnected clipping layers do not contribute to the renderer.
        if layer.clip_source.is_some() && !state.stacked.contains(&id) {
            continue;
        }
        state.before = Some(id);
        let gpu_input = if accelerated {
            gpu::render_surfaces(
                doc,
                [width, height],
                origin,
                step,
                &state.surfaces,
                Some(id),
            )?
        } else {
            None
        };
        let input = gpu_input.unwrap_or_else(|| {
            RgbaImage::from_fn(width, height, |x, y| {
                let point = [
                    origin[0] + (f64::from(x) + 0.5) * step[0],
                    origin[1] + (f64::from(y) + 0.5) * step[1],
                ];
                let mut pixel = [0.; 4];
                paint_children(
                    doc,
                    None,
                    point,
                    InheritedCoverage {
                        mask: 1.,
                        opacity: 1.,
                    },
                    &mut pixel,
                    0,
                    &state,
                );
                Rgba(pixel.map(|v| (v.clamp(0., 1.) * 255.).round() as u8))
            })
        });
        let image = match a.kind {
            Kind::GaussianBlur => {
                let radius = (a.blur_radius.unwrap_or(10.) / step[0]) as f32;
                if accelerated {
                    crate::filters::gaussian_rgba(&input, radius)?
                } else {
                    crate::native_pixels::unpremultiply(image::imageops::blur(
                        &crate::native_pixels::premultiply(&input),
                        radius,
                    ))
                }
            }
            Kind::MotionBlur => {
                let distance = a.motion_distance.unwrap_or(10.) / step[0];
                let angle = a.motion_angle.unwrap_or(0.);
                let gpu = if accelerated {
                    gpu::motion::blur(&input, distance, angle)?
                } else {
                    None
                };
                gpu.unwrap_or_else(|| motion(&input, distance, angle))
            }
            _ => unreachable!("spatial adjustment list contains only blur layers"),
        };
        state.surfaces.insert(
            id,
            Surface {
                image: Arc::new(image),
                transform,
            },
        );
    }
    Ok(state.surfaces)
}

fn motion(source: &RgbaImage, distance: f64, angle: f64) -> RgbaImage {
    let (sin, cos) = (-angle).to_radians().sin_cos();
    let steps = distance.ceil().max(1.) as usize + 1;
    let (w, h) = source.dimensions();
    RgbaImage::from_fn(w, h, |x, y| {
        let mut sum = [0.; 4];
        for i in 0..steps {
            let t = (i as f64 / (steps - 1) as f64 - 0.5) * distance;
            let p = pixel(
                source,
                [
                    (f64::from(x) + 0.5 + t * cos) / f64::from(w),
                    (f64::from(y) + 0.5 + t * sin) / f64::from(h),
                ],
                Sampling::Smooth,
            );
            for k in 0..3 {
                sum[k] += p[k] * p[3];
            }
            sum[3] += p[3];
        }
        if sum[3] > 0. {
            for k in 0..3 {
                sum[k] /= sum[3];
            }
        }
        sum[3] /= steps as f64;
        Rgba(sum.map(|v| (v.clamp(0., 1.) * 255.).round() as u8))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        adjustment::Adjustment,
        document::{Layer, LayerContent, Mask},
    };
    use image::{GrayImage, Luma};

    fn document(kind: Kind) -> Document {
        let mut doc = Document::new(64, 48).unwrap();
        doc.layers[0].content =
            LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(64, 48, |x, y| {
                Rgba([
                    if x < 32 { 255 } else { 0 },
                    if y < 24 { 255 } else { 0 },
                    0,
                    if x < 48 { 200 } else { 0 },
                ])
            }))));
        let mut layer = Layer::blank("Blur", 64, 48);
        let mut a = Adjustment::new(kind);
        a.blur_radius = Some(3.);
        a.motion_distance = Some(9.);
        a.motion_angle = Some(35.);
        layer.content = LayerContent::Adjustment(Box::new(a));
        doc.add(layer).unwrap();
        doc
    }
    #[test]
    fn blur_adjustment_blends_backdrop_preserves_alpha_mask_and_saved_parameters() {
        for kind in [Kind::GaussianBlur, Kind::MotionBlur] {
            let mut doc = document(kind);
            let original = doc.layers[0].raster().unwrap().clone();
            doc.layers[1].mask = Some(Mask {
                pixels: Arc::new(GrayImage::from_fn(64, 48, |_, y| {
                    Luma([if y < 24 { 255 } else { 0 }])
                })),
                enabled: true,
                linked: true,
                placement: None,
            });
            let output = region(&doc, 64, 48, [0., 0.], [1., 1.]).unwrap();
            assert!(
                output[(31, 12)][0] < 255 && output[(32, 12)][0] > 0,
                "{kind:?}"
            );
            assert_eq!(output[(31, 36)], original[(31, 36)]);
            for (a, b) in output.pixels().zip(original.pixels()) {
                assert_eq!(a[3], b[3]);
            }
            doc.layers[1].opacity = 0.;
            let disabled = region(&doc, 64, 48, [0., 0.], [1., 1.]).unwrap();
            let mut baseline = doc.clone();
            baseline.layers.pop();
            assert_eq!(
                disabled,
                region(&baseline, 64, 48, [0., 0.], [1., 1.]).unwrap()
            );
            let json = serde_json::to_string(match &doc.layers[1].content {
                LayerContent::Adjustment(a) => a,
                _ => unreachable!(),
            })
            .unwrap();
            let decoded: Adjustment = serde_json::from_str(&json).unwrap();
            decoded.validate().unwrap();
            assert_eq!(decoded.kind, kind);
        }
    }
    #[test]
    fn chained_blurs_and_clipped_stacks_match_partial_region_rendering() {
        let mut doc = document(Kind::GaussianBlur);
        doc.layers[1].clip_source = Some(doc.layers[0].id);
        let mut second = doc.layers[1].clone();
        second.id = Uuid::new_v4();
        if let LayerContent::Adjustment(a) = &mut second.content {
            a.kind = Kind::MotionBlur;
        }
        doc.add(second).unwrap();
        let full = region(&doc, 64, 48, [0., 0.], [1., 1.]).unwrap();
        let part = region(&doc, 12, 12, [26., 17.], [1., 1.]).unwrap();
        for y in 0..12 {
            for x in 0..12 {
                assert_eq!(part[(x, y)], full[(x + 26, y + 17)]);
            }
        }
        assert!(full[(32, 20)][0] > 0);
        assert_eq!(full[(55, 20)][3], 0);
    }
    #[test]
    #[ignore = "Requires a hardware Vulkan adapter"]
    fn gpu_blur_adjustments_match_reference_with_clipping_and_chains() {
        for kind in [Kind::GaussianBlur, Kind::MotionBlur] {
            let mut doc = document(kind);
            doc.layers[1].clip_source = Some(doc.layers[0].id);
            let mut second = doc.layers[1].clone();
            second.id = Uuid::new_v4();
            second.opacity = 0.6;
            doc.add(second).unwrap();
            let cpu = prepare(&doc, [64, 48], [0., 0.], [1., 1.], false).unwrap();
            let gpu = prepare(&doc, [64, 48], [0., 0.], [1., 1.], true).unwrap();
            let mut engine = super::super::gpu::Engine::new().unwrap();
            let a = super::super::gpu::scene::Scene::compile_surfaces(
                &doc,
                [0., 0.],
                [1., 1.],
                &gpu,
                None,
            )
            .unwrap();
            let actual = engine.render(&a, [64, 48], [0., 0.], [1., 1.]).unwrap();
            let mut sampler = Sampler::from_document(Cow::Borrowed(&doc));
            sampler.state.surfaces = cpu;
            for y in 0..48 {
                for x in 0..64 {
                    let expected = sampler
                        .sample([f64::from(x) + 0.5, f64::from(y) + 0.5])
                        .map(|v| (v * 255.).round() as u8);
                    for (a, b) in actual[(x, y)].0.into_iter().zip(expected) {
                        assert!(a.abs_diff(b) <= 4, "{kind:?} ({x},{y}):{a} vs {b}");
                    }
                }
            }
        }
    }
}
