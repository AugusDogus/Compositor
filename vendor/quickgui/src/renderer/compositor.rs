//! Compositing layers: offscreen group textures, subtree filters, backdrops, and blend modes.
//!
//! Everything an element can declare that cannot be expressed as one more instanced primitive —
//! a non-translational transform, a subtree blur or drop shadow, a backdrop effect, a non-normal
//! blend mode — makes that element a *compositing group*. [`Scene`] records the group, keeps its
//! descendants' paint layers inside it, and places one composite primitive in the parent layer at
//! the group's own cross-primitive paint order.
//!
//! This module owns the pass sequencing that turns those records into GPU work, and both the
//! window renderer (`gpu.rs`) and the headless visual-test renderer (`offscreen.rs`) drive the
//! same code, so a screenshot test exercises the production path.
//!
//! ## Why group textures are window-sized
//!
//! Text is prepared by Glyphon at absolute window coordinates and its vertex buffers are built
//! before any pass begins; the instanced shape, image, SVG, path, and custom-shader renderers
//! likewise bake one window-sized projection into a shared uniform. Allocating a group texture at
//! the group's own bounds would need every one of those renderers to learn a per-layer origin.
//! Allocating at the window's physical size instead lets a group be rendered with the unmodified
//! per-frame preparation — text included, so text rotates with its parent — at the cost of memory,
//! which [`MAX_LAYER_TEXTURE_BYTES`] bounds with honest degradation once the bound is reached.
//! A backdrop-only group with opaque group compositing paints its foreground directly into its
//! parent after filtering the destination. It needs only the shared capture and blur scratch,
//! avoiding one full-window content texture per menu without changing primitive preparation.

use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

use wgpu::{
    BindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, ColorTargetState,
    ColorWrites, CommandEncoder, Device, Extent3d, FragmentState, LoadOp, MultisampleState,
    Operations, PipelineCompilationOptions, PrimitiveState, Queue, RenderPass,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor,
    ShaderStages, Texture, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
    TextureView, TextureViewDescriptor, VertexState,
};

use crate::{
    BlendMode, Color as UiColor, ColorMatrix, Corners, MAX_LAYER_TEXTURE_BYTES, Rect, Scene,
    ScenePlane, Transform2D,
    custom_shader_renderer::CustomShaderRenderer,
    image_renderer::ImageRenderer,
    path_renderer::PathRenderer,
    scene::{BLUR_MARGIN_SIGMAS, PaintGroup, PaintLayer, PrimitiveRef},
    svg_renderer::SvgRenderer,
};

use super::{RendererError, ShapeRenderer, TextSystem};

/// Largest number of Gaussian taps evaluated per direction and per axis.
///
/// A wider support is approximated by striding the taps, so the cost of the largest accepted
/// [`MAX_BLUR_RADIUS`](crate::MAX_BLUR_RADIUS) stays bounded at any scale factor.
const MAX_BLUR_TAPS: f32 = 48.0;

/// Composite draws recorded per frame: a backdrop, a drop shadow, and the group itself for each
/// group, plus headroom.
const MAX_COMPOSITE_DRAWS: usize = 4 * crate::MAX_LAYERS_PER_FRAME;
/// Blur passes recorded per frame: two axes each for the group blur and the backdrop blur.
const MAX_BLUR_DRAWS: usize = 4 * crate::MAX_LAYERS_PER_FRAME;

const COMPOSITE_UNIFORM_STRIDE: u64 = 256;
const BLUR_UNIFORM_STRIDE: u64 = 256;
const MAX_CACHED_LAYER_COMMAND_BYTES: usize = 32 * 1024 * 1024;

/// Per-frame compositing telemetry surfaced through [`RenderStats`](crate::RenderStats).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CompositeStats {
    /// Groups composited from their own offscreen texture this frame.
    pub layers: usize,
    /// Offscreen group passes recorded this frame.
    pub layer_passes: usize,
    /// Group textures whose unchanged content was reused without recording a group pass.
    pub reused_layers: usize,
    /// Separable blur passes recorded this frame.
    pub blur_passes: usize,
    /// Bytes of offscreen texture retained by this window after eviction.
    pub layer_texture_bytes: u64,
    /// Declared layer effects painted without their effect because a bound was reached.
    pub skipped_layer_effects: usize,
}

/// The immutable per-frame description of the target being rendered.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CompositeFrame {
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub format: TextureFormat,
    /// Whether the target texture can be copied from, which destination-reading effects need.
    pub target_copyable: bool,
    /// Restrict rendering to one plane, as the window renderer does around a native view.
    pub plane: Option<ScenePlane>,
}

/// Borrowed references to every primitive renderer that participates in a pass.
pub(crate) struct SceneRenderers<'a> {
    pub shapes: &'a ShapeRenderer,
    pub path: Option<&'a PathRenderer>,
    pub custom_shader: Option<&'a CustomShaderRenderer>,
    pub image: Option<&'a ImageRenderer>,
    pub svg: Option<&'a SvgRenderer>,
    pub text: &'a TextSystem,
}

impl<'a> SceneRenderers<'a> {
    fn draw(
        &'a self,
        pass: &mut RenderPass<'a>,
        layer: usize,
        order: u32,
    ) -> Result<(), RendererError> {
        self.shapes.render_order(pass, layer, order);
        if let Some(path) = self.path {
            path.render_order(pass, layer, order);
        }
        if let Some(custom_shader) = self.custom_shader {
            custom_shader.render_order(pass, layer, order);
        }
        if let Some(image) = self.image {
            image.render_order(pass, layer, order);
        }
        if let Some(svg) = self.svg {
            svg.render_order(pass, layer, order);
        }
        self.text.render_order(pass, layer, order)
    }
}

/// One recorded unit of work inside a target's pass sequence.
#[derive(Clone, Copy, Debug)]
enum Step {
    /// Draw every primitive of one paint layer at one cross-primitive order.
    Draw {
        layer: usize,
        order: u32,
        clip: Option<Rect>,
    },
    /// End the current pass, copy the target, and blur the copy. Destination-reading effects
    /// (backdrop filters and every blend mode but `normal` and `screen`) need this first.
    Capture { blur: Option<usize> },
    /// Draw one composite quad.
    Composite { draw: usize, clip: Option<Rect> },
}

impl Step {
    /// Direct backdrop foreground still obeys the clip formerly applied by its composite quad.
    /// Intersect existing clips so nested direct groups cannot escape an ancestor's bounds.
    fn clipped(mut self, bounds: Rect) -> Self {
        match &mut self {
            Self::Draw { clip, .. } | Self::Composite { clip, .. } => {
                *clip = Some(match *clip {
                    Some(previous) => previous.intersection(bounds).unwrap_or(Rect::ZERO),
                    None => bounds,
                });
            }
            Self::Capture { .. } => {}
        }
        self
    }
}

/// Identity of one retained offscreen texture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LayerTextureKey {
    /// The rendered content of a group.
    Content(u16),
    /// The blurred content of a group.
    Blurred(u16),
    /// Shared scratch: 0 the blur ping-pong, 1 the destination copy, 2 the blurred copy.
    Scratch(u8),
}

struct LayerTexture {
    key: LayerTextureKey,
    width: u32,
    height: u32,
    format: TextureFormat,
    texture: Arc<Texture>,
    view: Arc<TextureView>,
    last_used: u64,
    generation: u64,
}

impl LayerTexture {
    fn bytes(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height) * 4
    }
}

/// A bounded, least-recently-used pool of window-sized offscreen textures.
struct LayerTextureCache {
    textures: Vec<LayerTexture>,
    frame: u64,
    bytes: u64,
    generation: u64,
    byte_limit: u64,
}

impl Default for LayerTextureCache {
    fn default() -> Self {
        Self {
            textures: Vec::new(),
            frame: 0,
            bytes: 0,
            generation: 0,
            byte_limit: MAX_LAYER_TEXTURE_BYTES,
        }
    }
}

impl LayerTextureCache {
    fn generation(&self, key: LayerTextureKey) -> u64 {
        self.textures
            .iter()
            .find(|texture| texture.key == key)
            .map_or(0, |texture| texture.generation)
    }
    fn begin_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }

    /// Reuse or allocate the texture for `key`, or return `None` when the bound forbids it.
    fn acquire(
        &mut self,
        device: &Device,
        key: LayerTextureKey,
        width: u32,
        height: u32,
        format: TextureFormat,
    ) -> Option<(Arc<Texture>, Arc<TextureView>)> {
        let frame = self.frame;
        if let Some(existing) = self.textures.iter_mut().find(|texture| {
            texture.key == key
                && texture.width == width
                && texture.height == height
                && texture.format == format
        }) {
            existing.last_used = frame;
            return Some((existing.texture.clone(), existing.view.clone()));
        }
        // Repurpose a same-shaped texture this frame has not claimed before allocating.
        if let Some(reusable) = self.textures.iter_mut().find(|texture| {
            texture.last_used != frame
                && texture.width == width
                && texture.height == height
                && texture.format == format
        }) {
            self.generation = self.generation.wrapping_add(1);
            reusable.generation = self.generation;
            reusable.key = key;
            reusable.last_used = frame;
            return Some((reusable.texture.clone(), reusable.view.clone()));
        }

        let bytes = u64::from(width) * u64::from(height) * 4;
        if bytes > self.byte_limit {
            return None;
        }
        while self.bytes + bytes > self.byte_limit {
            let victim = self
                .textures
                .iter()
                .enumerate()
                .filter(|(_, texture)| texture.last_used != frame)
                .min_by_key(|(_, texture)| texture.last_used)
                .map(|(index, _)| index)?;
            self.bytes -= self.textures[victim].bytes();
            self.textures.remove(victim);
        }

        let texture = Arc::new(device.create_texture(&TextureDescriptor {
            label: Some("quickgui compositing layer"),
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_SRC
                | TextureUsages::COPY_DST,
            view_formats: &[],
        }));
        let view = Arc::new(texture.create_view(&TextureViewDescriptor::default()));
        self.bytes += bytes;
        self.generation = self.generation.wrapping_add(1);
        self.textures.push(LayerTexture {
            key,
            width,
            height,
            format,
            texture: texture.clone(),
            view: view.clone(),
            last_used: frame,
            generation: self.generation,
        });
        Some((texture, view))
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CompositeUniform {
    inverse: [f32; 4],
    inverse_translation: [f32; 2],
    source_size: [f32; 2],
    quad: [f32; 4],
    clip: [f32; 4],
    mask: [f32; 4],
    corners: [f32; 4],
    matrix_r: [f32; 4],
    matrix_g: [f32; 4],
    matrix_b: [f32; 4],
    matrix_a: [f32; 4],
    matrix_offset: [f32; 4],
    tint: [f32; 4],
    params: [f32; 4],
    viewport: [f32; 2],
    _padding: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BlurUniform {
    params: [f32; 4],
    source: [f32; 4],
    viewport: [f32; 2],
    _padding: [f32; 2],
}

/// How a composite draw combines with what is already in the target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CompositeBlend {
    /// Premultiplied source-over.
    Over,
    /// Exact separable `screen`, which needs no destination copy.
    Screen,
    /// The shader already produced the final color from a captured destination.
    Replace,
}

impl CompositeBlend {
    fn index(self) -> usize {
        match self {
            Self::Over => 0,
            Self::Screen => 1,
            Self::Replace => 2,
        }
    }

    fn state(self) -> BlendState {
        match self {
            Self::Over => BlendState::PREMULTIPLIED_ALPHA_BLENDING,
            // `screen` is exactly Cs + Cb(1 - Cs) in premultiplied form.
            Self::Screen => BlendState {
                color: BlendComponent {
                    src_factor: BlendFactor::One,
                    dst_factor: BlendFactor::OneMinusSrc,
                    operation: BlendOperation::Add,
                },
                alpha: BlendComponent {
                    src_factor: BlendFactor::One,
                    dst_factor: BlendFactor::OneMinusSrcAlpha,
                    operation: BlendOperation::Add,
                },
            },
            Self::Replace => BlendState::REPLACE,
        }
    }
}

/// One recorded composite draw.
struct CompositeDraw {
    bind_group: BindGroup,
    offset: u32,
    blend: CompositeBlend,
}

/// One recorded separable blur pass.
struct BlurDraw {
    bind_group: BindGroup,
    offset: u32,
    target: Arc<TextureView>,
}

/// The textures one group claimed for this frame.
struct GroupTextures {
    /// The group's own content texture; absent for direct backdrops or dropped effects.
    content: Option<Arc<TextureView>>,
    content_texture: Option<Arc<Texture>>,
    /// The blurred copy of that content, when a subtree blur or drop shadow was declared.
    blurred: Option<Arc<TextureView>>,
    /// The blur ping-pong scratch shared with every other group.
    scratch: Option<Arc<TextureView>>,
}

/// A group's finished plan for this frame.
struct GroupPlan {
    id: u16,
    content: Option<Arc<TextureView>>,
    content_texture: Option<Arc<Texture>>,
    /// Blur passes replayed straight after the group's own pass.
    blurs: Vec<usize>,
    /// Steps replayed into the group's own pass.
    steps: Vec<Step>,
    /// Steps the parent replays where the group's composite belongs.
    composite: Vec<Step>,
    /// Whether the effect was dropped and the subtree paints directly into its parent.
    inlined: bool,
}

#[derive(Clone)]
struct GroupContent {
    layers: Vec<Arc<PaintLayer>>,
    children: Vec<(PaintGroup, GroupContent)>,
}

impl GroupContent {
    fn allocated_bytes(&self) -> usize {
        self.layers.capacity() * size_of::<Arc<PaintLayer>>()
            + self
                .layers
                .iter()
                .map(|layer| layer.allocated_bytes())
                .sum::<usize>()
            + self.children.capacity() * size_of::<(PaintGroup, GroupContent)>()
            + self
                .children
                .iter()
                .map(|(group, content)| {
                    group.layers.capacity() * size_of::<usize>() + content.allocated_bytes()
                })
                .sum::<usize>()
    }

    fn of(scene: &Scene, group: &PaintGroup) -> Self {
        Self {
            layers: group
                .layers
                .iter()
                .map(|index| scene.paint_layers()[*index].clone())
                .collect(),
            children: scene
                .groups()
                .iter()
                .filter(|child| child.parent == group.id)
                .map(|child| (child.clone(), Self::of(scene, child)))
                .collect(),
        }
    }

    fn matches(&self, other: &Self) -> bool {
        self.layers.len() == other.layers.len()
            && self.children.len() == other.children.len()
            && self
                .layers
                .iter()
                .zip(&other.layers)
                .all(|(a, b)| a.same_commands(b))
            && self
                .children
                .iter()
                .zip(&other.children)
                .all(|((a, ac), (b, bc))| {
                    a.id == b.id
                        && a.bounds == b.bounds
                        && a.backdrop_bounds == b.backdrop_bounds
                        && a.clip == b.clip
                        && a.effects == b.effects
                        && a.opacity == b.opacity
                        && ac.matches(bc)
                })
    }
}

struct CachedGroup {
    content: GroupContent,
    texture: Weak<Texture>,
    blurred: Option<Weak<TextureView>>,
    scale: f32,
    sigma: f32,
    descendant_effects: Vec<(u16, bool)>,
    content_generation: u64,
    blur_generation: u64,
}

struct GroupReuse {
    content: GroupContent,
    pixels: bool,
    blur: bool,
    sigma: f32,
    descendant_effects: Vec<(u16, bool)>,
}

/// Immutable device objects, created lazily on the first frame that declares a layer effect.
struct CompositePipelines {
    format: TextureFormat,
    composite_layout: wgpu::BindGroupLayout,
    blur_layout: wgpu::BindGroupLayout,
    composite: [RenderPipeline; 3],
    blur: RenderPipeline,
    sampler: wgpu::Sampler,
    composite_uniforms: wgpu::Buffer,
    blur_uniforms: wgpu::Buffer,
}

/// Owns every compositing resource for one window or headless target.
#[derive(Default)]
pub(crate) struct Compositor {
    pipelines: Option<CompositePipelines>,
    cache: LayerTextureCache,
    content_cache: HashMap<u16, CachedGroup>,
}

impl Compositor {
    /// Whether this compositor has ever allocated anything. A window that never declares a layer
    /// effect keeps this `true` and pays nothing per frame.
    #[cfg(test)]
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn is_idle(&self) -> bool {
        self.pipelines.is_none() && self.cache.textures.is_empty()
    }

    #[cfg(test)]
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn retained_bytes(&self) -> u64 {
        self.cache.bytes
    }

    fn ensure_pipelines(&mut self, device: &Device, format: TextureFormat) {
        if self
            .pipelines
            .as_ref()
            .is_none_or(|pipelines| pipelines.format != format)
        {
            self.pipelines = Some(CompositePipelines::new(device, format));
        }
    }

    /// Render one scene into `target_view`, compositing every declared layer effect.
    ///
    /// The zero-group path is exactly the historical single-pass loop: no texture allocated, no
    /// pipeline compiled, no extra pass recorded.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_scene(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        scene: &Scene,
        renderers: &SceneRenderers<'_>,
        target_view: &TextureView,
        target_texture: Option<&Texture>,
        clear: Option<UiColor>,
        frame: CompositeFrame,
    ) -> Result<CompositeStats, RendererError> {
        let plane = frame.plane;
        let root_layers: Vec<usize> = scene
            .paint_layers()
            .iter()
            .enumerate()
            .filter(|(_, layer)| {
                layer.key().group == 0 && plane.is_none_or(|plane| layer.key().plane == plane)
            })
            .map(|(index, _)| index)
            .collect();
        let groups: Vec<&PaintGroup> = scene
            .groups()
            .iter()
            .filter(|group| plane.is_none_or(|plane| group.plane == plane))
            .collect();

        let mut stats = CompositeStats {
            skipped_layer_effects: scene.skipped_groups(),
            layer_texture_bytes: self.cache.bytes,
            ..Default::default()
        };

        if groups.is_empty() {
            self.content_cache
                .retain(|id, _| scene.groups().iter().any(|group| group.id == *id));
            let mut pass = begin_pass(encoder, target_view, clear, "quickgui scene pass");
            for layer in root_layers {
                for order in 0..=scene.paint_layers()[layer].max_order() {
                    renderers.draw(&mut pass, layer, order)?;
                }
            }
            return Ok(stats);
        }

        // Phase 1: claim textures, deepest group first, under the retained-byte bound. The
        // separable blur's ping-pong is shared by every group, because each pair of axis passes
        // completes before the next begins.
        self.cache.begin_frame();
        let blur_scratch = if groups.iter().any(|group| {
            group.effects.blur > 0.0
                || group.effects.backdrop_blur > 0.0
                || group
                    .effects
                    .drop_shadow
                    .is_some_and(|shadow| shadow.sigma() > 0.0)
        }) {
            self.scratch(device, 0, frame).map(|(_, view)| view)
        } else {
            None
        };
        let mut textures: Vec<GroupTextures> = Vec::with_capacity(groups.len());
        for group in groups.iter().rev() {
            textures.push(self.claim_textures(device, scene, group, blur_scratch.as_ref(), frame));
        }
        let capture = self.scratch(device, 1, frame);
        let capture_blur = if groups.iter().any(|group| group.effects.backdrop_blur > 0.0) {
            self.scratch(device, 2, frame)
        } else {
            None
        };
        stats.layer_texture_bytes = self.cache.bytes;

        self.content_cache
            .retain(|id, _| scene.groups().iter().any(|group| group.id == *id));
        let mut reuse: Vec<_> = groups
            .iter()
            .rev()
            .zip(&textures)
            .map(|(group, claimed)| {
                let content = GroupContent::of(scene, group);
                let previous = self.content_cache.get(&group.id);
                let pixels = previous.is_some_and(|previous| {
                    previous.scale == frame.scale
                        && previous.content_generation
                            == self.cache.generation(LayerTextureKey::Content(group.id))
                        && previous
                            .texture
                            .upgrade()
                            .zip(claimed.content_texture.as_ref())
                            .is_some_and(|(old, current)| Arc::ptr_eq(&old, current))
                        && previous.content.matches(&content)
                });
                let sigma = group.effects.blur.max(
                    group
                        .effects
                        .drop_shadow
                        .map_or(0.0, |shadow| shadow.sigma()),
                );
                let blur = pixels
                    && previous.is_some_and(|previous| {
                        previous.sigma == sigma
                            && previous.blur_generation
                                == self.cache.generation(LayerTextureKey::Blurred(group.id))
                            && previous
                                .blurred
                                .as_ref()
                                .and_then(Weak::upgrade)
                                .zip(claimed.blurred.as_ref())
                                .is_some_and(|(old, current)| Arc::ptr_eq(&old, current))
                    });
                GroupReuse {
                    content,
                    pixels,
                    blur,
                    sigma,
                    descendant_effects: Vec::new(),
                }
            })
            .collect();

        // Phase 2: build every draw and the ordered step lists.
        self.ensure_pipelines(device, frame.format);
        let pipelines = self
            .pipelines
            .as_ref()
            .expect("the compositor pipelines were just created");
        let mut composite_draws: Vec<CompositeDraw> = Vec::new();
        let mut blur_draws: Vec<BlurDraw> = Vec::new();
        let mut composite_uniforms: Vec<CompositeUniform> = Vec::new();
        let mut blur_uniforms: Vec<BlurUniform> = Vec::new();
        let mut plans: Vec<GroupPlan> = Vec::new();

        for ((group, claimed), reuse) in groups.iter().rev().zip(textures.iter()).zip(&mut reuse) {
            let plan = plan_group(
                device,
                pipelines,
                scene,
                group,
                claimed,
                frame,
                capture.as_ref().map(|(_, view)| view),
                capture_blur.as_ref().map(|(_, view)| view),
                blur_scratch.as_ref(),
                &plans,
                &mut composite_draws,
                &mut blur_draws,
                &mut composite_uniforms,
                &mut blur_uniforms,
            );
            // Texture pressure or per-frame draw limits can inline a child that previously had
            // an effect (and vice versa), even when its declaration is unchanged.
            reuse.descendant_effects = plans.iter().map(|plan| (plan.id, plan.inlined)).collect();
            reuse.pixels &= self
                .content_cache
                .get(&group.id)
                .is_some_and(|previous| previous.descendant_effects == reuse.descendant_effects);
            reuse.blur &= reuse.pixels;
            if plan.inlined {
                stats.skipped_layer_effects += 1;
            } else if plan.content.is_some() {
                stats.layers += 1;
                if reuse.pixels {
                    stats.reused_layers += 1;
                } else {
                    stats.layer_passes += 1;
                }
            }
            plans.push(plan);
        }
        plans.reverse();
        let root_steps = build_steps(scene, &root_layers, &plans);
        stats.blur_passes = blur_draws.len()
            - plans
                .iter()
                .rev()
                .zip(&reuse)
                .filter(|(_, reuse)| reuse.blur)
                .map(|(plan, _)| plan.blurs.len())
                .sum::<usize>();

        // Base and overlay planes can be encoded before one queue submission. Queue writes are
        // executed before that submission, so each plane needs distinct uniform addresses.
        let overlay = u64::from(frame.plane == Some(ScenePlane::Overlay));
        let composite_offset = overlay * COMPOSITE_UNIFORM_STRIDE * MAX_COMPOSITE_DRAWS as u64;
        let blur_offset = overlay * BLUR_UNIFORM_STRIDE * MAX_BLUR_DRAWS as u64;
        for draw in &mut composite_draws {
            draw.offset += composite_offset as u32;
        }
        for draw in &mut blur_draws {
            draw.offset += blur_offset as u32;
        }
        queue.write_buffer(
            &pipelines.composite_uniforms,
            composite_offset,
            &pack_uniforms(&composite_uniforms, COMPOSITE_UNIFORM_STRIDE),
        );
        queue.write_buffer(
            &pipelines.blur_uniforms,
            blur_offset,
            &pack_uniforms(&blur_uniforms, BLUR_UNIFORM_STRIDE),
        );

        // Phase 3: record group passes deepest first, then the target's own pass.
        for (plan, reuse) in plans.iter().rev().zip(&reuse) {
            let Some(content) = plan.content.clone() else {
                continue;
            };
            if !reuse.pixels {
                record_target(
                    encoder,
                    renderers,
                    &plan.steps,
                    &composite_draws,
                    &blur_draws,
                    pipelines,
                    &content,
                    plan.content_texture.as_deref(),
                    capture.as_ref().map(|(texture, _)| texture.as_ref()),
                    Some(UiColor::TRANSPARENT),
                    frame,
                    "quickgui layer pass",
                )?;
            }
            for blur in plan.blurs.iter().filter(|_| !reuse.blur) {
                let draw = &blur_draws[*blur];
                let mut pass = begin_pass(encoder, &draw.target, None, "quickgui layer blur");
                pass.set_pipeline(&pipelines.blur);
                pass.set_bind_group(0, &draw.bind_group, &[draw.offset]);
                pass.draw(0..6, 0..1);
            }
        }

        record_target(
            encoder,
            renderers,
            &root_steps,
            &composite_draws,
            &blur_draws,
            pipelines,
            target_view,
            target_texture,
            capture.as_ref().map(|(texture, _)| texture.as_ref()),
            clear,
            frame,
            "quickgui scene pass",
        )?;

        for (((group, claimed), reuse), plan) in groups
            .iter()
            .rev()
            .zip(&textures)
            .zip(reuse)
            .zip(plans.iter().rev())
        {
            self.content_cache.remove(&group.id);
            let retained_bytes: usize = self
                .content_cache
                .values()
                .map(|entry| entry.content.allocated_bytes())
                .sum();
            if !plan.inlined
                && retained_bytes + reuse.content.allocated_bytes()
                    <= MAX_CACHED_LAYER_COMMAND_BYTES
                && let Some(texture) = &claimed.content_texture
            {
                self.content_cache.insert(
                    group.id,
                    CachedGroup {
                        content: reuse.content,
                        texture: Arc::downgrade(texture),
                        blurred: claimed.blurred.as_ref().map(Arc::downgrade),
                        scale: frame.scale,
                        sigma: reuse.sigma,
                        descendant_effects: reuse.descendant_effects,
                        content_generation: self
                            .cache
                            .generation(LayerTextureKey::Content(group.id)),
                        blur_generation: self.cache.generation(LayerTextureKey::Blurred(group.id)),
                    },
                );
            } else {
                self.content_cache.remove(&group.id);
            }
        }

        Ok(stats)
    }

    fn scratch(
        &mut self,
        device: &Device,
        slot: u8,
        frame: CompositeFrame,
    ) -> Option<(Arc<Texture>, Arc<TextureView>)> {
        self.cache.acquire(
            device,
            LayerTextureKey::Scratch(slot),
            frame.width,
            frame.height,
            frame.format,
        )
    }

    fn claim_textures(
        &mut self,
        device: &Device,
        scene: &Scene,
        group: &PaintGroup,
        blur_scratch: Option<&Arc<TextureView>>,
        frame: CompositeFrame,
    ) -> GroupTextures {
        let dropped = GroupTextures {
            content: None,
            content_texture: None,
            blurred: None,
            scratch: None,
        };
        if physical_transform(group.effects.transform, frame.scale)
            .inverse()
            .is_none()
        {
            return dropped;
        }
        if group.effects.reads_destination() && !frame.target_copyable {
            // This surface cannot be read back, so the effect cannot be honored.
            return dropped;
        }
        if can_paint_foreground_directly(scene, group) {
            // Foreground primitives need no separate texture when only the destination changes.
            return dropped;
        }
        let Some((content_texture, content)) = self.cache.acquire(
            device,
            LayerTextureKey::Content(group.id),
            frame.width,
            frame.height,
            frame.format,
        ) else {
            return dropped;
        };
        let needs_blur =
            group.effects.blur > 0.0 || group.effects.drop_shadow.is_some_and(|s| s.sigma() > 0.0);
        if !needs_blur {
            return GroupTextures {
                content: Some(content),
                content_texture: Some(content_texture),
                blurred: None,
                scratch: None,
            };
        }
        let scratch = blur_scratch.cloned();
        let blurred = self
            .cache
            .acquire(
                device,
                LayerTextureKey::Blurred(group.id),
                frame.width,
                frame.height,
                frame.format,
            )
            .map(|(_, view)| view);
        match (scratch, blurred) {
            (Some(scratch), Some(blurred)) => GroupTextures {
                content: Some(content),
                content_texture: Some(content_texture),
                blurred: Some(blurred),
                scratch: Some(scratch),
            },
            // Without room for the convolution the group would paint unblurred, which is a
            // different picture rather than a cheaper one. Drop the effect instead.
            _ => dropped,
        }
    }
}

/// These groups can filter the parent and then draw their foreground into it unchanged.
/// Opacity and foreground effects still require isolation. So do children that filter or blend
/// with their destination: they must see this group's pixels, not the parent's backdrop.
fn can_paint_foreground_directly(scene: &Scene, group: &PaintGroup) -> bool {
    let effects = group.effects;
    let child_reads_parent = scene.groups().iter().any(|child| {
        child.parent == group.id
            && (child.effects.has_backdrop() || child.effects.blend != BlendMode::Normal)
    });
    effects.has_backdrop()
        && effects.transform.is_identity()
        && effects.blur == 0.0
        && effects.drop_shadow.is_none()
        && effects.color_matrix.is_identity()
        && effects.blend == BlendMode::Normal
        && group.opacity == 1.0
        && !child_reads_parent
}

#[allow(clippy::too_many_arguments)]
fn plan_group(
    device: &Device,
    pipelines: &CompositePipelines,
    scene: &Scene,
    group: &PaintGroup,
    claimed: &GroupTextures,
    frame: CompositeFrame,
    capture: Option<&Arc<TextureView>>,
    capture_blur: Option<&Arc<TextureView>>,
    blur_scratch: Option<&Arc<TextureView>>,
    planned: &[GroupPlan],
    composite_draws: &mut Vec<CompositeDraw>,
    blur_draws: &mut Vec<BlurDraw>,
    composite_uniforms: &mut Vec<CompositeUniform>,
    blur_uniforms: &mut Vec<BlurUniform>,
) -> GroupPlan {
    let effects = group.effects;
    let scale = frame.scale;
    let transform = physical_transform(effects.transform, scale);
    let Some(inverse) = transform.inverse() else {
        // A collapsed transform has no drawable area. Inlining its original
        // primitives would make deliberately hidden content visible again.
        return GroupPlan {
            id: group.id,
            content: None,
            content_texture: None,
            blurs: Vec::new(),
            steps: Vec::new(),
            composite: Vec::new(),
            inlined: false,
        };
    };
    let dropped = |steps: Vec<Step>| GroupPlan {
        id: group.id,
        content: None,
        content_texture: None,
        blurs: Vec::new(),
        steps: Vec::new(),
        composite: steps,
        inlined: true,
    };
    let inline_steps = || build_steps(scene, &group.layers, planned);
    let direct = can_paint_foreground_directly(scene, group);
    let content = claimed.content.clone();
    if content.is_none() && !direct {
        return dropped(inline_steps());
    }
    if composite_draws.len() + 3 > MAX_COMPOSITE_DRAWS || blur_draws.len() + 4 > MAX_BLUR_DRAWS {
        return dropped(inline_steps());
    }
    let reads_destination = effects.reads_destination();
    if reads_destination && (!frame.target_copyable || capture.is_none()) {
        return dropped(inline_steps());
    }
    if effects.backdrop_blur > 0.0 && (capture_blur.is_none() || blur_scratch.is_none()) {
        return dropped(inline_steps());
    }

    let source_size = [frame.width as f32, frame.height as f32];
    let clip = physical_rect(group.clip, scale, frame);
    let matrix = effects.color_matrix;

    let mut blurs = Vec::new();
    if let (Some(content), Some(blurred), Some(scratch)) =
        (&content, &claimed.blurred, &claimed.scratch)
    {
        // One pair of passes serves both the subtree blur and the drop shadow; the wider of the
        // two standard deviations wins.
        let sigma = effects
            .blur
            .max(effects.drop_shadow.map_or(0.0, |shadow| shadow.sigma()))
            * scale;
        blurs.push(push_blur(
            device,
            pipelines,
            blur_draws,
            blur_uniforms,
            content,
            scratch.clone(),
            [1.0, 0.0],
            sigma,
            frame,
        ));
        blurs.push(push_blur(
            device,
            pipelines,
            blur_draws,
            blur_uniforms,
            scratch,
            blurred.clone(),
            [0.0, 1.0],
            sigma,
            frame,
        ));
    }

    let mut composite = Vec::new();
    if reads_destination {
        let capture = capture.expect("a destination-reading group holds a capture texture");
        let blur = if effects.backdrop_blur > 0.0 {
            let target = capture_blur.expect("a blurred backdrop holds its own texture");
            let scratch = blur_scratch
                .cloned()
                .expect("a blurred backdrop holds the shared ping-pong");
            // Horizontal into the ping-pong, vertical into the blurred capture.
            let horizontal = push_blur(
                device,
                pipelines,
                blur_draws,
                blur_uniforms,
                capture,
                scratch.clone(),
                [1.0, 0.0],
                effects.backdrop_blur * scale,
                frame,
            );
            let _ = push_blur(
                device,
                pipelines,
                blur_draws,
                blur_uniforms,
                &scratch,
                target.clone(),
                [0.0, 1.0],
                effects.backdrop_blur * scale,
                frame,
            );
            Some(horizontal)
        } else {
            None
        };
        composite.push(Step::Capture { blur });

        if effects.has_backdrop() {
            let source = if effects.backdrop_blur > 0.0 {
                capture_blur.expect("a blurred backdrop holds its own texture")
            } else {
                capture
            };
            // The backdrop is filtered in the element's own coordinate system, so it travels
            // through the same transform and is masked by the element's rounded rectangle.
            // Painted bounds can include shadows and overflowing children. Only the original
            // border box receives the backdrop, retaining its geometry even at viewport edges.
            let bounds = group.backdrop_bounds;
            let mask = Rect::new(
                bounds.x * scale,
                bounds.y * scale,
                bounds.width * scale,
                bounds.height * scale,
            );
            let quad = transform.transform_rect(mask);
            composite.push(push_composite(
                device,
                pipelines,
                composite_draws,
                composite_uniforms,
                CompositeParameters {
                    source,
                    destination: capture,
                    inverse,
                    source_size,
                    quad,
                    clip,
                    mask: Some((mask, effects.backdrop_corners, scale)),
                    matrix: effects.backdrop_matrix,
                    tint: None,
                    opacity: group.opacity,
                    blend: BlendMode::Normal,
                    frame,
                },
            ));
        }
    }

    if direct {
        composite.extend(inline_steps().into_iter().map(|step| step.clipped(clip)));
        return GroupPlan {
            id: group.id,
            content: None,
            content_texture: None,
            blurs: Vec::new(),
            steps: Vec::new(),
            composite,
            inlined: false,
        };
    }
    let content = content.expect("foreground effects require a claimed content texture");
    let composited = claimed.blurred.as_ref().filter(|_| effects.blur > 0.0);
    // A drop shadow with no blur is the subtree's own silhouette, offset and tinted.
    let shadow_source = claimed.blurred.as_ref().unwrap_or(&content);
    if let Some(shadow) = effects.drop_shadow {
        let source = shadow_source;
        let offset = Transform2D::translate(shadow.offset.x * scale, shadow.offset.y * scale);
        let shadow_transform = offset.then(transform);
        if let Some(shadow_inverse) = shadow_transform.inverse() {
            let quad =
                shadow_transform.transform_rect(physical_rect(group.bounds_box(), scale, frame));
            composite.push(push_composite(
                device,
                pipelines,
                composite_draws,
                composite_uniforms,
                CompositeParameters {
                    source,
                    destination: capture.unwrap_or(&content),
                    inverse: shadow_inverse,
                    source_size,
                    quad,
                    clip,
                    mask: None,
                    matrix: ColorMatrix::IDENTITY,
                    tint: Some(shadow.color),
                    opacity: group.opacity,
                    blend: BlendMode::Normal,
                    frame,
                },
            ));
        }
    }

    let source = composited.unwrap_or(&content);
    let quad = transform.transform_rect(physical_rect(group.bounds_box(), scale, frame));
    composite.push(push_composite(
        device,
        pipelines,
        composite_draws,
        composite_uniforms,
        CompositeParameters {
            source,
            destination: capture.unwrap_or(&content),
            inverse,
            source_size,
            quad,
            clip,
            mask: None,
            matrix,
            tint: None,
            opacity: group.opacity,
            blend: effects.blend,
            frame,
        },
    ));

    GroupPlan {
        id: group.id,
        content: Some(content),
        content_texture: claimed.content_texture.clone(),
        blurs,
        steps: build_steps(scene, &group.layers, planned),
        composite,
        inlined: false,
    }
}

struct CompositeParameters<'a> {
    source: &'a TextureView,
    destination: &'a TextureView,
    inverse: Transform2D,
    source_size: [f32; 2],
    quad: Rect,
    clip: Rect,
    mask: Option<(Rect, Corners, f32)>,
    matrix: ColorMatrix,
    tint: Option<UiColor>,
    opacity: f32,
    blend: BlendMode,
    frame: CompositeFrame,
}

fn push_composite(
    device: &Device,
    pipelines: &CompositePipelines,
    draws: &mut Vec<CompositeDraw>,
    uniforms: &mut Vec<CompositeUniform>,
    parameters: CompositeParameters<'_>,
) -> Step {
    let matrix = parameters.matrix.as_array();
    let (mask, corners) = match parameters.mask {
        Some((rect, corners, scale)) => (
            [rect.x, rect.y, rect.width, rect.height],
            [
                corners.top_left * scale,
                corners.top_right * scale,
                corners.bottom_right * scale,
                corners.bottom_left * scale,
            ],
        ),
        None => ([0.0; 4], [0.0; 4]),
    };
    let tint = match parameters.tint {
        Some(color) => [
            color.r * color.a,
            color.g * color.a,
            color.b * color.a,
            color.a,
        ],
        None => [0.0, 0.0, 0.0, -1.0],
    };
    let reads_destination = parameters.blend.reads_destination();
    let blend = if reads_destination {
        CompositeBlend::Replace
    } else if parameters.blend == BlendMode::Screen {
        CompositeBlend::Screen
    } else {
        CompositeBlend::Over
    };
    let offset = (uniforms.len() as u64 * COMPOSITE_UNIFORM_STRIDE) as u32;
    uniforms.push(CompositeUniform {
        inverse: [
            parameters.inverse.a,
            parameters.inverse.b,
            parameters.inverse.c,
            parameters.inverse.d,
        ],
        inverse_translation: [parameters.inverse.tx, parameters.inverse.ty],
        source_size: parameters.source_size,
        quad: [
            parameters.quad.x,
            parameters.quad.y,
            parameters.quad.width,
            parameters.quad.height,
        ],
        clip: [
            parameters.clip.x,
            parameters.clip.y,
            parameters.clip.right(),
            parameters.clip.bottom(),
        ],
        mask,
        corners,
        matrix_r: [matrix[0], matrix[1], matrix[2], matrix[3]],
        matrix_g: [matrix[5], matrix[6], matrix[7], matrix[8]],
        matrix_b: [matrix[10], matrix[11], matrix[12], matrix[13]],
        matrix_a: [matrix[15], matrix[16], matrix[17], matrix[18]],
        matrix_offset: [matrix[4], matrix[9], matrix[14], matrix[19]],
        tint,
        params: [
            parameters.opacity.clamp(0.0, 1.0),
            parameters.blend.code() as f32,
            if reads_destination { 1.0 } else { 0.0 },
            if parameters.matrix.is_identity() {
                0.0
            } else {
                1.0
            },
        ],
        viewport: [
            parameters.frame.width as f32,
            parameters.frame.height as f32,
        ],
        _padding: [0.0; 2],
    });
    let bind_group =
        pipelines.composite_bind_group(device, parameters.source, parameters.destination);
    let draw = draws.len();
    draws.push(CompositeDraw {
        bind_group,
        offset,
        blend,
    });
    Step::Composite { draw, clip: None }
}

#[allow(clippy::too_many_arguments)]
fn push_blur(
    device: &Device,
    pipelines: &CompositePipelines,
    draws: &mut Vec<BlurDraw>,
    uniforms: &mut Vec<BlurUniform>,
    source: &TextureView,
    target: Arc<TextureView>,
    direction: [f32; 2],
    sigma: f32,
    frame: CompositeFrame,
) -> usize {
    let sigma = sigma.max(1.0e-3);
    let support = (sigma * BLUR_MARGIN_SIGMAS).ceil().max(1.0);
    let step = (support / MAX_BLUR_TAPS).ceil().max(1.0);
    let taps = (support / step).ceil().min(MAX_BLUR_TAPS);
    let offset = (uniforms.len() as u64 * BLUR_UNIFORM_STRIDE) as u32;
    uniforms.push(BlurUniform {
        params: [direction[0], direction[1], taps, step],
        source: [
            1.0 / (sigma * sigma),
            frame.width as f32,
            frame.height as f32,
            0.0,
        ],
        viewport: [frame.width as f32, frame.height as f32],
        _padding: [0.0; 2],
    });
    let bind_group = pipelines.blur_bind_group(device, source);
    let index = draws.len();
    draws.push(BlurDraw {
        bind_group,
        offset,
        target,
    });
    index
}

/// Flatten one target's layers into the ordered step list its passes replay.
fn build_steps(scene: &Scene, layers: &[usize], planned: &[GroupPlan]) -> Vec<Step> {
    let mut steps = Vec::new();
    for layer_index in layers {
        let layer = &scene.paint_layers()[*layer_index];
        if layer.paint().is_empty() {
            continue;
        }
        for order in 0..=layer.max_order() {
            steps.push(Step::Draw {
                layer: *layer_index,
                order,
                clip: None,
            });
            for item in layer.paint() {
                if item.order != order {
                    continue;
                }
                let PrimitiveRef::Group(slot) = item.primitive else {
                    continue;
                };
                let Some(reference) = layer.groups().get(slot) else {
                    continue;
                };
                if let Some(plan) = planned.iter().find(|plan| plan.id == reference.group) {
                    steps.extend_from_slice(&plan.composite);
                }
            }
        }
    }
    steps
}

fn physical_transform(transform: Transform2D, scale: f32) -> Transform2D {
    Transform2D::new(
        transform.a,
        transform.b,
        transform.c,
        transform.d,
        transform.tx * scale,
        transform.ty * scale,
    )
}

fn physical_rect(rect: Rect, scale: f32, frame: CompositeFrame) -> Rect {
    let left = (rect.x * scale).max(0.0);
    let top = (rect.y * scale).max(0.0);
    let right = (rect.right() * scale).min(frame.width as f32);
    let bottom = (rect.bottom() * scale).min(frame.height as f32);
    Rect::new(left, top, (right - left).max(0.0), (bottom - top).max(0.0))
}

fn begin_pass<'encoder>(
    encoder: &'encoder mut CommandEncoder,
    view: &TextureView,
    clear: Option<UiColor>,
    label: &'static str,
) -> RenderPass<'encoder> {
    encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: match clear {
                    Some(color) => {
                        let [r, g, b, a] = color.premultiplied_srgba();
                        LoadOp::Clear(wgpu::Color {
                            r: f64::from(r),
                            g: f64::from(g),
                            b: f64::from(b),
                            a: f64::from(a),
                        })
                    }
                    None => LoadOp::Load,
                },
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

/// Replay one target's steps, splitting the pass wherever a step needs the destination copied.
#[allow(clippy::too_many_arguments)]
fn record_target<'a>(
    encoder: &mut CommandEncoder,
    renderers: &'a SceneRenderers<'a>,
    steps: &[Step],
    composite_draws: &'a [CompositeDraw],
    blur_draws: &'a [BlurDraw],
    pipelines: &'a CompositePipelines,
    target_view: &TextureView,
    target_texture: Option<&Texture>,
    capture_texture: Option<&Texture>,
    clear: Option<UiColor>,
    frame: CompositeFrame,
    label: &'static str,
) -> Result<(), RendererError> {
    let mut load = clear;
    let mut index = 0;
    let mut opened = false;
    while index <= steps.len() {
        let mut end = index;
        while end < steps.len() && !matches!(steps[end], Step::Capture { .. }) {
            end += 1;
        }
        if index < end || !opened {
            let mut pass = begin_pass(encoder, target_view, load, label);
            for step in &steps[index..end] {
                let clip = match *step {
                    Step::Draw { clip, .. } | Step::Composite { clip, .. } => clip,
                    Step::Capture { .. } => unreachable!("captures end the segment"),
                };
                if !set_step_clip(&mut pass, clip, frame) {
                    continue;
                }
                match *step {
                    Step::Draw { layer, order, .. } => renderers.draw(&mut pass, layer, order)?,
                    Step::Composite { draw, .. } => {
                        let draw = &composite_draws[draw];
                        pass.set_pipeline(&pipelines.composite[draw.blend.index()]);
                        pass.set_bind_group(0, &draw.bind_group, &[draw.offset]);
                        pass.draw(0..6, 0..1);
                    }
                    Step::Capture { .. } => unreachable!("captures end the segment"),
                }
            }
            load = None;
            opened = true;
        }
        if end >= steps.len() {
            break;
        }
        let Step::Capture { blur } = steps[end] else {
            unreachable!("the segment ended on a capture")
        };
        if let (Some(target), Some(capture)) = (target_texture, capture_texture) {
            copy_target(encoder, target, capture, frame);
        }
        if let Some(blur) = blur {
            for draw in &blur_draws[blur..(blur + 2).min(blur_draws.len())] {
                let mut pass = begin_pass(encoder, &draw.target, None, "quickgui backdrop blur");
                pass.set_pipeline(&pipelines.blur);
                pass.set_bind_group(0, &draw.bind_group, &[draw.offset]);
                pass.draw(0..6, 0..1);
            }
        }
        index = end + 1;
    }
    Ok(())
}

fn set_step_clip(pass: &mut RenderPass<'_>, clip: Option<Rect>, frame: CompositeFrame) -> bool {
    let Some(clip) = clip else {
        pass.set_scissor_rect(0, 0, frame.width, frame.height);
        return true;
    };
    if clip.is_empty() {
        return false;
    }
    // Match composite.wgsl's inclusive comparison at pixel centers, including fractional edges.
    let left = (clip.x - 0.5).ceil().clamp(0., frame.width as f32) as u32;
    let top = (clip.y - 0.5).ceil().clamp(0., frame.height as f32) as u32;
    let right = (clip.right() + 0.5).floor().clamp(0., frame.width as f32) as u32;
    let bottom = (clip.bottom() + 0.5).floor().clamp(0., frame.height as f32) as u32;
    if right <= left || bottom <= top {
        return false;
    }
    pass.set_scissor_rect(left, top, right - left, bottom - top);
    true
}

/// Copy the whole target into the shared capture texture so a destination-reading effect can
/// sample what is already painted behind it.
fn copy_target(
    encoder: &mut CommandEncoder,
    target: &Texture,
    capture: &Texture,
    frame: CompositeFrame,
) {
    encoder.copy_texture_to_texture(
        wgpu::TexelCopyTextureInfo {
            texture: target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyTextureInfo {
            texture: capture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        Extent3d {
            width: frame.width,
            height: frame.height,
            depth_or_array_layers: 1,
        },
    );
}

fn pack_uniforms<T: bytemuck::Pod>(values: &[T], stride: u64) -> Vec<u8> {
    let stride = stride as usize;
    let mut packed = vec![0_u8; stride * values.len().max(1)];
    for (index, value) in values.iter().enumerate() {
        let bytes = bytemuck::bytes_of(value);
        packed[index * stride..index * stride + bytes.len()].copy_from_slice(bytes);
    }
    packed
}

impl CompositePipelines {
    fn new(device: &Device, format: TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("quickgui composite shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../composite.wgsl").into()),
        });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("quickgui composite bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<CompositeUniform>() as u64,
                        ),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let blur_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("quickgui blur bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<BlurUniform>() as u64
                        ),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let composite_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("quickgui composite pipeline layout"),
                bind_group_layouts: &[Some(&composite_layout)],
                immediate_size: 0,
            });
        let blur_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("quickgui blur pipeline layout"),
            bind_group_layouts: &[Some(&blur_layout)],
            immediate_size: 0,
        });
        let make_composite = |blend: CompositeBlend| {
            device.create_render_pipeline(&RenderPipelineDescriptor {
                label: Some("quickgui composite pipeline"),
                layout: Some(&composite_pipeline_layout),
                vertex: VertexState {
                    module: &module,
                    entry_point: Some("composite_vertex"),
                    buffers: &[],
                    compilation_options: PipelineCompilationOptions::default(),
                },
                fragment: Some(FragmentState {
                    module: &module,
                    entry_point: Some("composite_fragment"),
                    targets: &[Some(ColorTargetState {
                        format,
                        blend: Some(blend.state()),
                        write_mask: ColorWrites::ALL,
                    })],
                    compilation_options: PipelineCompilationOptions::default(),
                }),
                primitive: PrimitiveState::default(),
                depth_stencil: None,
                multisample: MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let composite = [
            make_composite(CompositeBlend::Over),
            make_composite(CompositeBlend::Screen),
            make_composite(CompositeBlend::Replace),
        ];
        let blur = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("quickgui blur pipeline"),
            layout: Some(&blur_pipeline_layout),
            vertex: VertexState {
                module: &module,
                entry_point: Some("blur_vertex"),
                buffers: &[],
                compilation_options: PipelineCompilationOptions::default(),
            },
            fragment: Some(FragmentState {
                module: &module,
                entry_point: Some("blur_fragment"),
                targets: &[Some(ColorTargetState {
                    format,
                    blend: Some(BlendState::REPLACE),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: PipelineCompilationOptions::default(),
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("quickgui composite sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let composite_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("quickgui composite uniforms"),
            size: 2 * COMPOSITE_UNIFORM_STRIDE * MAX_COMPOSITE_DRAWS as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let blur_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("quickgui blur uniforms"),
            size: 2 * BLUR_UNIFORM_STRIDE * MAX_BLUR_DRAWS as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            format,
            composite_layout,
            blur_layout,
            composite,
            blur,
            sampler,
            composite_uniforms,
            blur_uniforms,
        }
    }

    fn composite_bind_group(
        &self,
        device: &Device,
        source: &TextureView,
        destination: &TextureView,
    ) -> BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("quickgui composite bind group"),
            layout: &self.composite_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.composite_uniforms,
                        offset: 0,
                        size: wgpu::BufferSize::new(std::mem::size_of::<CompositeUniform>() as u64),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(destination),
                },
            ],
        })
    }

    fn blur_bind_group(&self, device: &Device, source: &TextureView) -> BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("quickgui blur bind group"),
            layout: &self.blur_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.blur_uniforms,
                        offset: 0,
                        size: wgpu::BufferSize::new(std::mem::size_of::<BlurUniform>() as u64),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(source),
                },
            ],
        })
    }
}

/// The untransformed box a group's composite covers, grown by its effect margin.
impl PaintGroup {
    pub(crate) fn bounds_box(&self) -> Rect {
        let margin = self.effects.margin();
        Rect::new(
            self.bounds.x - margin,
            self.bounds.y - margin,
            self.bounds.width + margin * 2.0,
            self.bounds.height + margin * 2.0,
        )
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests;

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod cache_tests {
    use super::*;
    use crate::renderer::OffscreenRenderer;
    use crate::{Assets, PerformanceProfile};

    #[test]
    fn texture_budget_and_generations_survive_repurposing() {
        let fonts = crate::renderer::create_shared_font_system(&Assets::default(), &[]).unwrap();
        let renderer =
            pollster::block_on(OffscreenRenderer::new(PerformanceProfile::Balanced, fonts))
                .unwrap();
        let (device, _) = renderer.gpu();
        let mut cache = LayerTextureCache {
            byte_limit: 16 * 16 * 4,
            ..Default::default()
        };
        let format = TextureFormat::Rgba8Unorm;
        cache.begin_frame();
        let (original, _) = cache
            .acquire(device, LayerTextureKey::Content(1), 16, 16, format)
            .unwrap();
        let generation = cache.generation(LayerTextureKey::Content(1));
        assert!(
            cache
                .acquire(device, LayerTextureKey::Content(2), 16, 16, format)
                .is_none()
        );
        assert!(cache.bytes <= cache.byte_limit);
        cache.begin_frame();
        let (scratch, _) = cache
            .acquire(device, LayerTextureKey::Scratch(0), 16, 16, format)
            .unwrap();
        assert!(Arc::ptr_eq(&original, &scratch));
        assert_ne!(generation, cache.generation(LayerTextureKey::Scratch(0)));
        cache.begin_frame();
        let (reclaimed, _) = cache
            .acquire(device, LayerTextureKey::Content(1), 16, 16, format)
            .unwrap();
        assert!(Arc::ptr_eq(&original, &reclaimed));
        assert_ne!(
            generation,
            cache.generation(LayerTextureKey::Content(1)),
            "pointer identity cannot validate reused pixels"
        );
        cache.begin_frame();
        let (resized, _) = cache
            .acquire(device, LayerTextureKey::Content(1), 8, 8, format)
            .unwrap();
        assert!(!Arc::ptr_eq(&original, &resized));
        assert_eq!(cache.bytes, 8 * 8 * 4);
        assert!(
            cache
                .acquire(device, LayerTextureKey::Content(3), 32, 32, format)
                .is_none()
        );
    }
}
