//! Backdrop surfaces for spatial adjustment layers. Each surface captures the
//! stack immediately before its adjustment, including prior spatial adjustments.
use super::*;
use crate::{
    Result,
    adjustment::{ExtendedAdjustment, Kind},
    geometry::Transform,
};
use std::sync::Arc;

pub(super) struct Surface {
    pub image: Arc<RgbaImage>,
    pub transform: Transform,
}
pub(super) type Surfaces = HashMap<Uuid, Surface>;

// The same operation determines traversal, halo size and surface evaluation.
enum Spatial {
    Gaussian(f64),
    Motion { distance: f64, angle: f64 },
    ShadowsHighlights(crate::shadows_highlights::Settings),
}
impl Spatial {
    fn from(layer: &Layer) -> Option<Self> {
        match &layer.content {
            LayerContent::Adjustment(a) => match a.kind {
                Kind::GaussianBlur => Some(Self::Gaussian(a.blur_radius.unwrap_or(10.))),
                Kind::MotionBlur => Some(Self::Motion {
                    distance: a.motion_distance.unwrap_or(10.),
                    angle: a.motion_angle.unwrap_or(0.),
                }),
                _ => None,
            },
            LayerContent::ExtendedAdjustment(a) => match **a {
                ExtendedAdjustment::ShadowsHighlights(settings) if !settings.identity() => {
                    Some(Self::ShadowsHighlights(settings))
                }
                _ => None,
            },
            _ => None,
        }
    }
    fn margin(&self) -> f64 {
        match self {
            Self::Gaussian(radius) => radius * 3. + 2.,
            Self::Motion { distance, .. } => distance * 0.5 + 2.,
            Self::ShadowsHighlights(settings) => settings.radius() * 1.5 + 2.,
        }
    }
    fn apply(self, input: RgbaImage, spacing: f64, accelerated: bool) -> Result<RgbaImage> {
        match self {
            Self::Gaussian(radius) => {
                let radius = (radius / spacing) as f32;
                // Below this scale adjacent Gaussian weights cannot affect 8-bit output.
                if radius <= 0.1 {
                    Ok(input)
                } else if accelerated {
                    crate::filters::gaussian_rgba(&input, radius)
                } else {
                    Ok(crate::native_pixels::unpremultiply(image::imageops::blur(
                        &crate::native_pixels::premultiply(&input),
                        radius,
                    )))
                }
            }
            Self::Motion { distance, angle } => {
                let distance = distance / spacing;
                let gpu = if accelerated {
                    gpu::motion::blur(&input, distance, angle)?
                } else {
                    None
                };
                Ok(gpu.unwrap_or_else(|| crate::filters::motion::apply(&input, distance, angle)))
            }
            Self::ShadowsHighlights(settings) => {
                crate::shadows_highlights::apply(&input, settings, 1. / spacing, accelerated)
            }
        }
    }
}

pub(super) fn prepare(
    doc: &Document,
    size: [u32; 2],
    origin: Point,
    step: Point,
    accelerated: bool,
) -> Result<Surfaces> {
    fn ordered(doc: &Document, state: &RenderState, parent: Option<Uuid>, out: &mut Vec<Uuid>) {
        for layer in doc
            .layers
            .iter()
            .filter(|l| l.parent == parent && l.visible)
        {
            if state.stacked.contains(&layer.id) {
                continue;
            }
            if layer.is_group() {
                ordered(doc, state, Some(layer.id), out);
            } else if let Some(children) = state.stacks.get(&layer.id) {
                for index in children {
                    let child = &doc.layers[*index];
                    if Spatial::from(child).is_some() {
                        out.push(child.id);
                    }
                }
            } else if Spatial::from(layer).is_some() {
                out.push(layer.id);
            }
        }
    }
    let mut state = RenderState::new(doc);
    let mut ids = Vec::new();
    ordered(doc, &state, None, &mut ids);
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
    // Root adjustments can sample the combined canvas. Independent board
    // chains only need their largest halo, not the sum of neighboring boards.
    let mut margins: HashMap<Option<Uuid>, f64> = HashMap::new();
    for layer in ids.iter().filter_map(|id| doc.layer(*id)) {
        let margin = Spatial::from(layer).map_or(0., |op| op.margin());
        *margins
            .entry(crate::artboard::owner(doc, layer.id))
            .or_default() += margin;
    }
    let margin = margins.remove(&None).unwrap_or(0.) + margins.values().copied().fold(0., f64::max);
    let pad = [
        (margin / step[0]).ceil() as u32,
        (margin / step[1]).ceil() as u32,
    ];
    let width=size[0].checked_add(pad[0].saturating_mul(2)).ok_or_else(||crate::invalid("Adjustment preview exceeds the supported image size. Reduce the radius or distance, or zoom out."))?;
    let height=size[1].checked_add(pad[1].saturating_mul(2)).ok_or_else(||crate::invalid("Adjustment preview exceeds the supported image size. Reduce the radius or distance, or zoom out."))?;
    crate::document::validate_size(width, height)?;
    // Retained surfaces plus the input and blur scratch allocations share the
    // document's memory budget rather than multiplying it for each adjustment.
    let scratch = if ids
        .iter()
        .filter_map(|id| doc.layer(*id))
        .any(|layer| matches!(Spatial::from(layer), Some(Spatial::ShadowsHighlights(_))))
    {
        7
    } else {
        3
    };
    crate::document::validate_pixel_budget(
        u64::from(width) * u64::from(height) * (ids.len() as u64 + scratch),
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
        let Some(operation) = Spatial::from(layer) else {
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
        let image = operation.apply(input, spacing, accelerated)?;
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
    fn minimum_gaussian_document() -> Document {
        let mut doc = document(Kind::GaussianBlur);
        if let LayerContent::Adjustment(adjustment) = &mut doc.layers[1].content {
            adjustment.blur_radius = Some(0.1);
            adjustment.validate().unwrap();
        }
        doc
    }

    #[test]
    fn minimum_gaussian_radius_renders_at_half_zoom() {
        let doc = minimum_gaussian_document();
        let actual = region(&doc, 32, 24, [0., 0.], [2., 2.]).unwrap();
        let mut baseline = doc.clone();
        baseline.layers.pop();
        let expected = region(&baseline, 32, 24, [0., 0.], [2., 2.]).unwrap();
        for y in 0..24 {
            for x in 0..32 {
                assert_eq!(actual[(x, y)], expected[(x, y)], "({x},{y})");
            }
        }
    }

    #[test]
    #[ignore = "Requires a hardware Vulkan adapter"]
    fn gpu_minimum_gaussian_radius_renders_at_half_zoom() {
        let doc = minimum_gaussian_document();
        let expected = region(&doc, 32, 24, [0., 0.], [2., 2.]).unwrap();
        // Require a hardware engine so a fallback cannot conceal a GPU regression.
        super::super::gpu::initialize().unwrap();
        let actual = region_accelerated(
            &doc,
            32,
            24,
            [0., 0.],
            [2., 2.],
            &mut DownsampleCache::default(),
        )
        .unwrap();
        for (actual, expected) in actual.as_raw().iter().zip(expected.as_raw()) {
            assert!(
                actual.abs_diff(*expected) <= 1,
                "GPU {actual}, CPU {expected}"
            );
        }
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

#[cfg(test)]
mod shadows_tests;
