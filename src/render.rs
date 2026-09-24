mod downsample;
pub(crate) mod gpu;
mod spatial;
pub use downsample::DownsampleCache;

use crate::{
    document::{Document, Layer, LayerContent},
    geometry::{Point, Sampling},
};
use image::{Rgba, RgbaImage};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
};
use uuid::Uuid;

pub fn pixel(image: &RgbaImage, unit: Point, sampling: Sampling) -> [f64; 4] {
    if unit.iter().any(|v| !(0. ..1.).contains(v)) {
        return [0.; 4];
    }
    if sampling == Sampling::Nearest {
        return image[(
            (unit[0] * image.width() as f64) as u32,
            (unit[1] * image.height() as f64) as u32,
        )]
            .0
            .map(|v| v as f64 / 255.);
    }
    let x = unit[0] * image.width() as f64 - 0.5;
    let y = unit[1] * image.height() as f64 - 0.5;
    let x0 = x.floor();
    let y0 = y.floor();
    let mut result = [0.; 4];
    // Interpolate premultiplied colors to avoid dark fringes along transparency.
    for (dx, dy, weight) in [
        (0., 0., (1. - x + x0) * (1. - y + y0)),
        (1., 0., (x - x0) * (1. - y + y0)),
        (0., 1., (1. - x + x0) * (y - y0)),
        (1., 1., (x - x0) * (y - y0)),
    ] {
        let p = image[(
            (x0 + dx).clamp(0., image.width() as f64 - 1.) as u32,
            (y0 + dy).clamp(0., image.height() as f64 - 1.) as u32,
        )]
            .0
            .map(|v| v as f64 / 255.);
        for i in 0..3 {
            result[i] += p[i] * p[3] * weight;
        }
        result[3] += p[3] * weight;
    }
    if result[3] > 0. {
        for i in 0..3 {
            result[i] /= result[3];
        }
    }
    result
}

fn mask_alpha(layer: &Layer, point: Point, backgrounds: &HashMap<Uuid, f64>) -> f64 {
    let Some(mask) = &layer.mask else {
        return 1.;
    };
    if !mask.enabled {
        return 1.;
    }
    let t = mask.placement.unwrap_or(layer.transform);
    let unit = t.unit(point);
    if unit.iter().any(|v| !(0. ..1.).contains(v)) {
        return if mask.placement.is_some() {
            backgrounds.get(&layer.id).copied().unwrap_or(0.)
        } else {
            0.
        };
    }
    mask_pixel(&mask.pixels, unit, t.sampling)
}

/// Sample coverage inside a nonempty mask. Callers define their own outside tone.
pub(crate) fn mask_pixel(pixels: &image::GrayImage, unit: Point, sampling: Sampling) -> f64 {
    if sampling == Sampling::Nearest {
        return pixels[(
            (unit[0] * pixels.width() as f64) as u32,
            (unit[1] * pixels.height() as f64) as u32,
        )][0] as f64
            / 255.;
    }
    let x = (unit[0] * pixels.width() as f64 - 0.5).clamp(0., (pixels.width() - 1) as f64);
    let y = (unit[1] * pixels.height() as f64 - 0.5).clamp(0., (pixels.height() - 1) as f64);
    let (ix, iy) = (x.floor() as u32, y.floor() as u32);
    let (jx, jy) = (
        (ix + 1).min(pixels.width() - 1),
        (iy + 1).min(pixels.height() - 1),
    );
    let (fx, fy) = (x - ix as f64, y - iy as f64);
    let top = pixels[(ix, iy)][0] as f64 * (1. - fx) + pixels[(jx, iy)][0] as f64 * fx;
    let bottom = pixels[(ix, jy)][0] as f64 * (1. - fx) + pixels[(jx, jy)][0] as f64 * fx;
    (top * (1. - fy) + bottom * fy) / 255.
}

fn coverage(
    doc: &Document,
    layer: &Layer,
    point: Point,
    depth: usize,
    backgrounds: &HashMap<Uuid, f64>,
) -> f64 {
    if depth > 256 {
        return 0.;
    }
    let alpha = match &layer.content {
        LayerContent::Raster(Some(image)) => {
            pixel(image, layer.transform.unit(point), layer.transform.sampling)[3]
        }
        LayerContent::Raster(None) => 0.,
        _ => 1.,
    };
    let upstream = layer
        .clip_source
        .and_then(|id| doc.layer(id))
        .map_or(1., |source| {
            coverage(doc, source, point, depth + 1, backgrounds)
        });
    alpha * layer.opacity * mask_alpha(layer, point, backgrounds) * upstream
}

/// Bake only the upstream clipping coverage into a layer's source pixel grid.
/// Its own raster mask, opacity, and blend mode remain editable.
pub(crate) fn clipped_pixels(doc: &Document, layer: &Layer) -> crate::Result<Option<RgbaImage>> {
    let (Some(source), Some(pixels)) = (layer.clip_source, layer.raster()) else {
        return Ok(None);
    };
    let source = doc.layer(source).ok_or_else(|| crate::invalid("The clipping source is missing. Reopen the project before copying or deleting its layers."))?;
    let backgrounds = doc
        .layers
        .iter()
        .filter_map(|l| l.mask.as_ref().map(|m| (l.id, m.background())))
        .collect();
    let (w, h) = pixels.dimensions();
    Ok(Some(RgbaImage::from_fn(w, h, |x, y| {
        let point = layer
            .transform
            .point([(x as f64 + 0.5) / w as f64, (y as f64 + 0.5) / h as f64]);
        let mut pixel = pixels[(x, y)];
        pixel[3] =
            (f64::from(pixel[3]) * coverage(doc, source, point, 0, &backgrounds)).round() as u8;
        pixel
    })))
}

struct RenderState {
    backgrounds: HashMap<Uuid, f64>,
    stacks: HashMap<Uuid, Vec<usize>>,
    stacked: HashSet<Uuid>,
    surfaces: spatial::Surfaces,
    before: Option<Uuid>,
}

impl RenderState {
    fn new(doc: &Document) -> Self {
        fn visit(doc: &Document, parent: Option<Uuid>, out: &mut Vec<usize>) {
            for (index, layer) in doc
                .layers
                .iter()
                .enumerate()
                .filter(|(_, l)| l.parent == parent && l.visible)
            {
                if layer.is_group() {
                    visit(doc, Some(layer.id), out);
                } else {
                    out.push(index);
                }
            }
        }
        let mut state = Self {
            backgrounds: doc
                .layers
                .iter()
                .filter_map(|l| l.mask.as_ref().map(|m| (l.id, m.background())))
                .collect(),
            stacks: HashMap::new(),
            stacked: HashSet::new(),
            surfaces: HashMap::new(),
            before: None,
        };
        let mut ordered = Vec::new();
        visit(doc, None, &mut ordered);
        for (position, index) in ordered.iter().enumerate() {
            let base = &doc.layers[*index];
            if base.clip_source.is_some() || matches!(base.content, LayerContent::Adjustment(_)) {
                continue;
            }
            let children: Vec<_> = ordered[position + 1..]
                .iter()
                .copied()
                .take_while(|i| {
                    let child = &doc.layers[*i];
                    child.clip_source == Some(base.id) && child.parent == base.parent
                })
                .collect();
            if !children.is_empty() {
                state
                    .stacked
                    .extend(children.iter().map(|i| doc.layers[*i].id));
                state.stacks.insert(base.id, children);
            }
        }
        state
    }
}

fn adjust(layer: &Layer, point: Point, opacity: f64, out: &mut [f64; 4], state: &RenderState) {
    if let LayerContent::Adjustment(adjustment) = &layer.content {
        let adjusted = state.surfaces.get(&layer.id).map_or_else(
            || adjustment.apply(*out, point),
            |surface| {
                pixel(
                    &surface.image,
                    surface.transform.unit(point),
                    Sampling::Smooth,
                )
            },
        );
        // Blend RGB at full coverage and retain the backdrop's alpha, as on macOS.
        let blended = layer.blend.composite(
            [out[0], out[1], out[2], 1.],
            [adjusted[0], adjusted[1], adjusted[2], 1.],
        );
        for i in 0..3 {
            out[i] += (blended[i] - out[i]) * opacity;
        }
    }
}

fn own_pixel(layer: &Layer, point: Point, backgrounds: &HashMap<Uuid, f64>) -> [f64; 4] {
    let mut color = layer.raster().map_or([0.; 4], |image| {
        pixel(image, layer.transform.unit(point), layer.transform.sampling)
    });
    color[3] *= layer.opacity * mask_alpha(layer, point, backgrounds);
    color
}

#[derive(Clone, Copy)]
struct InheritedCoverage {
    mask: f64,
    opacity: f64,
}

fn paint_children(
    doc: &Document,
    parent: Option<Uuid>,
    point: Point,
    inherited: InheritedCoverage,
    out: &mut [f64; 4],
    depth: usize,
    state: &RenderState,
) -> bool {
    if depth > 64 {
        return false;
    }
    for layer in doc
        .layers
        .iter()
        .filter(|l| l.parent == parent && l.visible)
    {
        if state.stacked.contains(&layer.id) {
            continue;
        }
        if state.before == Some(layer.id) {
            return true;
        }
        if layer.is_group() {
            if paint_children(
                doc,
                Some(layer.id),
                point,
                InheritedCoverage {
                    mask: inherited.mask * mask_alpha(layer, point, &state.backgrounds),
                    opacity: inherited.opacity * layer.opacity,
                },
                out,
                depth + 1,
                state,
            ) {
                return true;
            }
        } else if let Some(children) = state.stacks.get(&layer.id) {
            let mut group = own_pixel(layer, point, &state.backgrounds);
            let alpha = group[3];
            group[3] = 1.;
            for index in children {
                let child = &doc.layers[*index];
                if state.before == Some(child.id) {
                    *out = group;
                    out[3] = alpha;
                    return true;
                }
                if matches!(child.content, LayerContent::Adjustment(_)) {
                    adjust(
                        child,
                        point,
                        child.opacity
                            * inherited.opacity
                            * mask_alpha(child, point, &state.backgrounds),
                        &mut group,
                        state,
                    );
                } else {
                    let mut top = own_pixel(child, point, &state.backgrounds);
                    top[3] *= inherited.opacity;
                    group = child.blend.composite(group, top);
                }
            }
            // The stack shares its base's coverage. Source-over of separately clipped children
            // would thicken translucent edges and let adjustments affect unrelated lower layers.
            group[3] = alpha * inherited.mask * inherited.opacity;
            *out = layer.blend.composite(*out, group);
        } else if matches!(layer.content, LayerContent::Adjustment(_)) {
            if layer.clip_source.is_none() {
                adjust(
                    layer,
                    point,
                    layer.opacity
                        * mask_alpha(layer, point, &state.backgrounds)
                        * inherited.mask
                        * inherited.opacity,
                    out,
                    state,
                );
            }
        } else if let Some(image) = layer.raster() {
            let mut top = pixel(image, layer.transform.unit(point), layer.transform.sampling);
            top[3] = coverage(doc, layer, point, 0, &state.backgrounds)
                * inherited.mask
                * inherited.opacity;
            *out = layer.blend.composite(*out, top);
        }
    }
    false
}

/// A reusable composite sampler. Mask edge tones are computed once for a batch of pixels.
pub struct Sampler<'a> {
    document: Cow<'a, Document>,
    state: RenderState,
}

impl Sampler<'static> {
    pub fn new(document: &Document) -> crate::Result<Self> {
        Self::for_region(document, [document.width, document.height], [0., 0.])
    }

    fn for_region(document: &Document, size: [u32; 2], origin: Point) -> crate::Result<Self> {
        let doc = DownsampleCache::default()
            .prepare(&crate::effects::prepare(document, false)?, [1., 1.]);
        let surfaces = spatial::prepare(&doc, size, origin, [1., 1.], false)?;
        let mut sampler = Self::from_document(Cow::Owned(doc));
        sampler.state.surfaces = surfaces;
        Ok(sampler)
    }
}

impl<'a> Sampler<'a> {
    fn from_document(document: Cow<'a, Document>) -> Self {
        let state = RenderState::new(&document);
        Self { document, state }
    }
    pub fn sample(&self, point: Point) -> [f64; 4] {
        let mut out = [0.; 4];
        paint_children(
            &self.document,
            None,
            point,
            InheritedCoverage {
                mask: 1.,
                opacity: 1.,
            },
            &mut out,
            0,
            &self.state,
        );
        out
    }
}

pub fn sample(doc: &Document, point: Point) -> crate::Result<[f64; 4]> {
    // Spatial surfaces use pixel centers on the document grid. Retain both
    // neighboring centers for bilinear sampling; prepare adds each blur's halo.
    let origin = point.map(|v| (v - 0.5).floor());
    Ok(Sampler::for_region(doc, [2, 2], origin)?.sample(point))
}

#[cfg(test)]
mod sampling_tests;

pub fn region(
    doc: &Document,
    width: u32,
    height: u32,
    origin: Point,
    step: Point,
) -> crate::Result<RgbaImage> {
    region_cached(
        doc,
        width,
        height,
        origin,
        step,
        &mut DownsampleCache::default(),
    )
}

pub fn region_cached(
    doc: &Document,
    width: u32,
    height: u32,
    origin: Point,
    step: Point,
    cache: &mut DownsampleCache,
) -> crate::Result<RgbaImage> {
    let effects = crate::effects::prepare(doc, false)?;
    let prepared = cache.prepare(&effects, step);
    let mut sampler = Sampler::from_document(Cow::Borrowed(&prepared));
    sampler.state.surfaces = spatial::prepare(&prepared, [width, height], origin, step, false)?;
    Ok(RgbaImage::from_fn(width, height, |x, y| {
        let point = [
            origin[0] + (x as f64 + 0.5) * step[0],
            origin[1] + (y as f64 + 0.5) * step[1],
        ];
        Rgba(
            sampler
                .sample(point)
                .map(|v| (v.clamp(0., 1.) * 255.).round() as u8),
        )
    }))
}

/// Compile the viewport and resize pipelines before the first large preview.
pub(crate) fn gpu_effects(
    image: &RgbaImage,
    effects: &crate::effects::LayerEffects,
) -> crate::Result<Option<RgbaImage>> {
    gpu::effects::render(image, effects)
}

pub fn initialize_gpu() -> crate::Result<()> {
    gpu::initialize()
}

/// Interactive compositing with hardware acceleration and bounded CPU fallback.
/// Dispatch failures preserve the current preview and are reported to the editor.
pub fn region_accelerated(
    doc: &Document,
    width: u32,
    height: u32,
    origin: Point,
    step: Point,
    cache: &mut DownsampleCache,
) -> crate::Result<RgbaImage> {
    let effects = crate::effects::prepare(doc, true)?;
    let prepared = cache.prepare_accelerated(&effects, step)?;
    let surfaces = spatial::prepare(&prepared, [width, height], origin, step, true)?;
    if let Some(image) =
        gpu::render_surfaces(&prepared, [width, height], origin, step, &surfaces, None)?
    {
        return Ok(image);
    }
    let mut sampler = Sampler::from_document(Cow::Borrowed(&prepared));
    sampler.state.surfaces = surfaces;
    Ok(RgbaImage::from_fn(width, height, |x, y| {
        let point = [
            origin[0] + (f64::from(x) + 0.5) * step[0],
            origin[1] + (f64::from(y) + 0.5) * step[1],
        ];
        Rgba(
            sampler
                .sample(point)
                .map(|v| (v.clamp(0., 1.) * 255.).round() as u8),
        )
    }))
}

pub fn render(doc: &Document, width: u32, height: u32) -> crate::Result<RgbaImage> {
    region(
        doc,
        width,
        height,
        [0., 0.],
        [
            doc.width as f64 / width as f64,
            doc.height as f64 / height as f64,
        ],
    )
}

pub fn below(doc: &Document, id: Uuid) -> crate::Result<RgbaImage> {
    render(&below_source(doc, id), doc.width, doc.height)
}

pub fn sample_below(doc: &Document, id: Uuid, point: Point) -> crate::Result<[f64; 4]> {
    sample(&below_source(doc, id), point)
}

fn below_source(doc: &Document, id: Uuid) -> Document {
    fn visit(
        doc: &Document,
        parent: Option<Uuid>,
        target: Uuid,
        under: &mut std::collections::HashSet<Uuid>,
    ) -> bool {
        for layer in doc.layers.iter().filter(|l| l.parent == parent) {
            if layer.id == target {
                return true;
            }
            under.insert(layer.id);
            if layer.is_group() && visit(doc, Some(layer.id), target, under) {
                return true;
            }
        }
        false
    }
    let mut under = std::collections::HashSet::new();
    visit(doc, None, id, &mut under);
    let mut source = doc.clone();
    for layer in &mut source.layers {
        if !layer.is_group() && !under.contains(&layer.id) {
            layer.visible = false;
        }
    }
    source
}

pub(crate) fn gpu_coverage_blur(
    image: &image::GrayImage,
    sigma: f32,
) -> crate::Result<Option<image::GrayImage>> {
    gpu::gaussian::blur(image, sigma)
}

pub(crate) fn gpu_camera_geometry(
    image: &image::RgbaImage,
    matrix: [f32; 9],
) -> crate::Result<Option<image::RgbaImage>> {
    gpu::camera_geometry::warp(image, matrix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Mask;
    use image::{GrayImage, Luma};
    use std::sync::Arc;

    #[test]
    fn folder_opacity_multiplies_nested_children_without_changing_their_values() {
        let mut doc = Document::new(1, 1).unwrap();
        doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
            1,
            1,
            Rgba([255, 0, 0, 255]),
        ))));
        doc.layers[0].opacity = 0.5;
        let mut outer = Layer::blank("Outer", 1, 1);
        outer.content = LayerContent::Group;
        outer.opacity = 0.5;
        let mut inner = outer.clone();
        inner.id = Uuid::new_v4();
        inner.name = "Inner".into();
        inner.parent = Some(outer.id);
        doc.layers[0].parent = Some(inner.id);
        doc.layers.extend([inner, outer]);
        doc.validate().unwrap();
        assert_eq!(render(&doc, 1, 1).unwrap()[(0, 0)], Rgba([255, 0, 0, 32]));
        assert_eq!(doc.layers[0].opacity, 0.5);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("folders.comp");
        crate::project::save(&doc, &path).unwrap();
        let reopened = crate::project::load(&path).unwrap();
        assert_eq!(
            render(&reopened, 1, 1).unwrap(),
            render(&doc, 1, 1).unwrap()
        );
    }

    #[test]
    fn clipping_stack_keeps_base_alpha_instead_of_thickening_edges() {
        let mut doc = Document::new(1, 1).unwrap();
        doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
            1,
            1,
            Rgba([0, 255, 0, 128]),
        ))));
        let mut child = Layer::blank("Clipped", 1, 1);
        child.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
            1,
            1,
            Rgba([255, 0, 0, 255]),
        ))));
        child.clip_source = Some(doc.layers[0].id);
        doc.add(child).unwrap();
        assert_eq!(render(&doc, 1, 1).unwrap()[(0, 0)], Rgba([255, 0, 0, 128]));
        let mut group = Layer::blank("Folder", 1, 1);
        group.content = LayerContent::Group;
        group.mask = Some(Mask {
            pixels: Arc::new(GrayImage::from_pixel(1, 1, Luma([128]))),
            enabled: true,
            linked: true,
            placement: None,
        });
        for layer in &mut doc.layers {
            layer.parent = Some(group.id);
        }
        doc.add(group).unwrap();
        assert_eq!(render(&doc, 1, 1).unwrap()[(0, 0)], Rgba([255, 0, 0, 64]));
    }

    #[test]
    fn clipped_adjustment_changes_only_base_colors_over_another_layer() {
        let mut doc = Document::new(2, 1).unwrap();
        doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
            2,
            1,
            Rgba([0, 0, 255, 255]),
        ))));
        let mut base = Layer::blank("Green", 2, 1);
        base.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(2, 1, |x, _| {
            Rgba([0, 255, 0, if x == 0 { 128 } else { 0 }])
        }))));
        let mut adjustment = Layer::blank("Invert", 2, 1);
        let mut settings = crate::adjustment::Adjustment::new(crate::adjustment::Kind::Curves);
        settings.curves.channels[0] = vec![
            crate::adjustment::CurvePoint { x: 0., y: 255. },
            crate::adjustment::CurvePoint { x: 255., y: 0. },
        ];
        adjustment.content = LayerContent::Adjustment(Box::new(settings));
        adjustment.clip_source = Some(base.id);
        doc.add(base).unwrap();
        doc.add(adjustment).unwrap();
        let result = render(&doc, 2, 1).unwrap();
        assert_eq!(result[(0, 0)], Rgba([128, 0, 255, 255]));
        assert_eq!(result[(1, 0)], Rgba([0, 0, 255, 255]));
        assert_eq!(
            *crate::clipboard::copy(&doc, true, false).unwrap().pixels,
            result
        );
    }

    #[test]
    fn high_quality_downsampling_keeps_original_assets_and_averages_detail() {
        let mut doc = Document::new(1, 1).unwrap();
        let source = Arc::new(RgbaImage::from_fn(16, 16, |x, y| {
            let c = if (x + y) % 2 == 0 { 255 } else { 0 };
            Rgba([c, c, c, 255])
        }));
        doc.layers[0].content = LayerContent::Raster(Some(source.clone()));
        let pixel = render(&doc, 1, 1).unwrap()[(0, 0)];
        assert!((126..=129).contains(&pixel[0]));
        assert!(Arc::ptr_eq(doc.layers[0].raster().unwrap(), &source));
        assert_eq!(
            crate::clipboard::copy(&doc, true, false).unwrap().pixels[(0, 0)],
            pixel
        );
    }

    #[test]
    fn visible_region_matches_full_canvas_pixels_at_high_zoom() {
        let mut doc = Document::new(4000, 2).unwrap();
        doc.layers[0].content =
            LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(4000, 2, |x, _| {
                Rgba([(x % 256) as u8, 0, 0, 255])
            }))));
        let image = region(&doc, 3, 1, [2500., 1.], [1., 1.]).unwrap();
        assert_eq!(image[(0, 0)], Rgba([196, 0, 0, 255]));
        assert_eq!(image[(1, 0)], Rgba([197, 0, 0, 255]));
        assert_eq!(image[(2, 0)], Rgba([198, 0, 0, 255]));
    }

    #[test]
    fn adjustment_blend_changes_color_without_thickening_alpha() {
        let mut doc = Document::new(1, 1).unwrap();
        doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
            1,
            1,
            Rgba([128, 128, 128, 128]),
        ))));
        let mut adjustment = Layer::blank("Multiply", 1, 1);
        adjustment.content = LayerContent::Adjustment(Box::new(
            crate::adjustment::Adjustment::new(crate::adjustment::Kind::Exposure),
        ));
        adjustment.blend = crate::blend::Blend::Multiply;
        adjustment.opacity = 0.5;
        doc.add(adjustment).unwrap();
        assert_eq!(render(&doc, 1, 1).unwrap()[(0, 0)], Rgba([96, 96, 96, 128]));
    }

    #[test]
    fn clipping_source_visibility_does_not_change_coverage() {
        let mut doc = Document::new(1, 1).unwrap();
        doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
            1,
            1,
            Rgba([0, 255, 0, 128]),
        ))));
        doc.layers[0].visible = false;
        let mut top = Layer::blank("Top", 1, 1);
        top.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
            1,
            1,
            Rgba([255, 0, 0, 255]),
        ))));
        top.clip_source = Some(doc.layers[0].id);
        doc.add(top).unwrap();
        assert_eq!(render(&doc, 1, 1).unwrap()[(0, 0)], Rgba([255, 0, 0, 128]));
    }
    #[test]
    fn group_masks_multiply_child_coverage() {
        let mut doc = Document::new(1, 1).unwrap();
        let mut group = Layer::blank("Folder", 1, 1);
        group.content = LayerContent::Group;
        group.mask = Some(Mask {
            pixels: Arc::new(GrayImage::from_pixel(1, 1, Luma([128]))),
            enabled: true,
            linked: true,
            placement: None,
        });
        doc.layers[0].parent = Some(group.id);
        doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
            1,
            1,
            Rgba([255, 0, 0, 255]),
        ))));
        doc.add(group).unwrap();
        assert_eq!(render(&doc, 1, 1).unwrap()[(0, 0)], Rgba([255, 0, 0, 128]));
    }
}
