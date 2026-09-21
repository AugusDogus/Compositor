use super::*;
use wgpu::{CommandEncoder, TextureView};

/// Re-premultiplies a transparent frame on its way to the window surface.
///
/// QuickGUI blends in linear light. On an sRGB-encoded surface every partially covered pixel
/// therefore ends the frame as `encode(a * c)`: the premultiplied *linear* colour, encoded. Core
/// Animation, DWM, and Wayland compositors treat a surface as premultiplied in its own encoding,
/// `a * encode(c)`, and the gap between the two forms is a light fringe on every anti-aliased
/// edge over a translucent backdrop. A transparent frame therefore renders into an intermediate
/// texture and is rewritten pixel by pixel into the expected form by one full-screen pass. Opaque
/// frames never allocate the intermediate or record the pass.
pub(super) struct TransparentPresent {
    pipeline: RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// One intermediate per surface the window presents: the base surface and the overlay.
    targets: [Option<PresentTarget>; 2],
}

/// The intermediate a transparent frame renders into before presentation.
pub(super) struct PresentTarget {
    pub texture: wgpu::Texture,
    pub view: TextureView,
    width: u32,
    height: u32,
}

impl TransparentPresent {
    pub(super) fn new(device: &Device, format: TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("quickgui present shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../present.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("quickgui present bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("quickgui present pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("quickgui present pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &module,
                entry_point: Some("present_vertex"),
                buffers: &[],
                compilation_options: PipelineCompilationOptions::default(),
            },
            fragment: Some(FragmentState {
                module: &module,
                entry_point: Some("present_fragment"),
                targets: &[Some(ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: PipelineCompilationOptions::default(),
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        // Pixels map one to one, so nearest sampling reads each source texel exactly.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("quickgui present sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        Self {
            pipeline,
            layout,
            sampler,
            targets: [None, None],
        }
    }

    /// The intermediate for `slot`, recreated only when the surface size changes.
    ///
    /// The returned handles are the frame's render target; they stay valid until the next call
    /// with a different size.
    pub(super) fn target(
        &mut self,
        device: &Device,
        format: TextureFormat,
        slot: usize,
        width: u32,
        height: u32,
    ) -> (wgpu::Texture, TextureView) {
        let width = width.max(1);
        let height = height.max(1);
        let fresh = self.targets[slot]
            .as_ref()
            .is_none_or(|target| target.width != width || target.height != height);
        if fresh {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("quickgui present intermediate"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: TextureUsages::RENDER_ATTACHMENT
                    | TextureUsages::TEXTURE_BINDING
                    | TextureUsages::COPY_SRC
                    | TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&TextureViewDescriptor::default());
            self.targets[slot] = Some(PresentTarget {
                texture,
                view,
                width,
                height,
            });
        }
        let target = self.targets[slot]
            .as_ref()
            .expect("the present intermediate was just created");
        (target.texture.clone(), target.view.clone())
    }

    /// Drop the intermediates, for a window that is no longer transparent.
    pub(super) fn release(&mut self) {
        self.targets = [None, None];
    }

    /// Record the pass that rewrites `slot`'s intermediate into `surface`.
    pub(super) fn present(
        &self,
        device: &Device,
        encoder: &mut CommandEncoder,
        slot: usize,
        surface: &TextureView,
    ) {
        let Some(target) = self.targets[slot].as_ref() else {
            return;
        };
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("quickgui present bind group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&target.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("quickgui present pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: surface,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// Whether frames presented to `config` need the re-premultiplying pass.
pub(super) fn needs_present_pass(config: &SurfaceConfiguration) -> bool {
    config.alpha_mode != CompositeAlphaMode::Opaque && config.format.is_srgb()
}
