use std::{
    collections::{HashMap, HashSet},
    mem,
    ops::Range,
};

use bytemuck::{Pod, Zeroable};
use wgpu::{
    BindGroup, BufferAddress, ColorTargetState, Device, Extent3d, FilterMode, FragmentState,
    MultisampleState, Origin3d, PipelineCompilationOptions, PrimitiveState, PrimitiveTopology,
    Queue, RenderPipeline, RenderPipelineDescriptor, Sampler, ShaderStages, TexelCopyBufferLayout,
    TexelCopyTextureInfo, TextureAspect, TextureDimension, TextureFormat, TextureUsages,
    TextureViewDescriptor, VertexAttribute, VertexBufferLayout, VertexFormat, VertexState,
    VertexStepMode, util::DeviceExt,
};

/// Split a 4x5 color matrix into four coefficient rows plus one offset row.
fn color_matrix_rows(matrix: crate::ColorMatrix) -> [[f32; 4]; 5] {
    let values = matrix.as_array();
    [
        [values[0], values[1], values[2], values[3]],
        [values[5], values[6], values[7], values[8]],
        [values[10], values[11], values[12], values[13]],
        [values[15], values[16], values[17], values[18]],
        [values[4], values[9], values[14], values[19]],
    ]
}

use crate::{Image, Rect, Scene, image::ImageId, scene::PrimitiveRef};

/// Maximum decoded image pixels retained by one renderer on the GPU.
pub const MAX_GPU_IMAGE_CACHE_BYTES: u64 = 128 * 1024 * 1024;
/// Maximum distinct image textures retained by one renderer.
pub const MAX_GPU_IMAGE_CACHE_ENTRIES: usize = 256;

const INITIAL_IMAGE_CAPACITY: usize = 64;
const BUFFERED_FRAMES: usize = 3;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ImagePrepareStats {
    pub images: usize,
    pub uploads: usize,
    pub draw_calls: usize,
    pub cache_bytes: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ViewUniform {
    viewport: [f32; 2],
    scale: f32,
    _padding: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ImageInstance {
    rect: [f32; 4],
    uv: [f32; 4],
    clip: [f32; 4],
    mask: [f32; 4],
    /// Corner radius, whether a color filter is present, opacity, unused.
    radius_filtered_opacity_padding: [f32; 4],
    /// The 4x5 color matrix, split into five rows of four plus one offset column.
    color_matrix: [[f32; 4]; 5],
}

struct CachedImage {
    _texture: wgpu::Texture,
    _view: wgpu::TextureView,
    bind_group: BindGroup,
    bytes: u64,
    last_used_frame: u64,
}

struct ImageBatch {
    order: u32,
    image: ImageId,
    instances: Range<u32>,
}

#[derive(Clone, Copy)]
struct OrderedImage {
    order: u32,
    image: ImageId,
    instance: ImageInstance,
}

pub(crate) struct ImageRenderer {
    pipeline: RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    view_bind_group: BindGroup,
    image_bind_group_layout: wgpu::BindGroupLayout,
    sampler: Sampler,
    instance_buffers: Vec<wgpu::Buffer>,
    instance_capacities: Vec<usize>,
    active_buffer: usize,
    pub(crate) uploads: crate::renderer::upload::BufferUploads,
    instances: Vec<ImageInstance>,
    pending: Vec<OrderedImage>,
    batches: Vec<ImageBatch>,
    layer_batches: Vec<Range<usize>>,
    admitted_images: Vec<Image>,
    admitted_ids: HashSet<ImageId>,
    cache: HashMap<ImageId, CachedImage>,
    resident_bytes: u64,
    frame: u64,
}

impl ImageRenderer {
    pub(crate) fn new(device: &Device, format: TextureFormat) -> Self {
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quickgui image view uniform"),
            contents: bytemuck::bytes_of(&ViewUniform::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let view_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("quickgui image view bind group layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
        let view_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("quickgui image view bind group"),
            layout: &view_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });
        let image_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("quickgui image texture bind group layout"),
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
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("quickgui image sampler"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..Default::default()
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("quickgui image pipeline layout"),
            bind_group_layouts: &[
                Some(&view_bind_group_layout),
                Some(&image_bind_group_layout),
            ],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!("image.wgsl"));
        let attributes = [
            VertexAttribute {
                format: VertexFormat::Float32x4,
                offset: 0,
                shader_location: 0,
            },
            VertexAttribute {
                format: VertexFormat::Float32x4,
                offset: 16,
                shader_location: 1,
            },
            VertexAttribute {
                format: VertexFormat::Float32x4,
                offset: 32,
                shader_location: 2,
            },
            VertexAttribute {
                format: VertexFormat::Float32x4,
                offset: 48,
                shader_location: 3,
            },
            VertexAttribute {
                format: VertexFormat::Float32x4,
                offset: 64,
                shader_location: 4,
            },
            VertexAttribute {
                format: VertexFormat::Float32x4,
                offset: 80,
                shader_location: 5,
            },
            VertexAttribute {
                format: VertexFormat::Float32x4,
                offset: 96,
                shader_location: 6,
            },
            VertexAttribute {
                format: VertexFormat::Float32x4,
                offset: 112,
                shader_location: 7,
            },
            VertexAttribute {
                format: VertexFormat::Float32x4,
                offset: 128,
                shader_location: 8,
            },
            VertexAttribute {
                format: VertexFormat::Float32x4,
                offset: 144,
                shader_location: 9,
            },
        ];
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("quickgui image pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: PipelineCompilationOptions::default(),
                buffers: &[Some(VertexBufferLayout {
                    array_stride: mem::size_of::<ImageInstance>() as BufferAddress,
                    step_mode: VertexStepMode::Instance,
                    attributes: &attributes,
                })],
            },
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: PipelineCompilationOptions::default(),
                targets: &[Some(ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        Self {
            pipeline,
            uniform_buffer,
            view_bind_group,
            image_bind_group_layout,
            sampler,
            instance_buffers: (0..BUFFERED_FRAMES)
                .map(|_| create_instance_buffer(device, INITIAL_IMAGE_CAPACITY))
                .collect(),
            instance_capacities: vec![INITIAL_IMAGE_CAPACITY; BUFFERED_FRAMES],
            active_buffer: 0,
            uploads: crate::renderer::upload::BufferUploads::default(),
            instances: Vec::with_capacity(INITIAL_IMAGE_CAPACITY),
            pending: Vec::with_capacity(INITIAL_IMAGE_CAPACITY),
            batches: Vec::with_capacity(16),
            layer_batches: Vec::with_capacity(4),
            admitted_images: Vec::with_capacity(16),
            admitted_ids: HashSet::with_capacity(16),
            cache: HashMap::with_capacity(32),
            resident_bytes: 0,
            frame: 0,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
        scene: &Scene,
        viewport: Rect,
        physical_width: u32,
        physical_height: u32,
        scale: f32,
    ) -> ImagePrepareStats {
        self.uploads.begin_frame();
        self.frame = self.frame.wrapping_add(1);
        self.instances.clear();
        self.pending.clear();
        self.batches.clear();
        self.layer_batches.clear();
        self.admitted_images.clear();
        self.admitted_ids.clear();

        let mut admitted_bytes = 0_u64;
        for layer in scene.paint_layers() {
            for primitive in layer.images() {
                let id = primitive.image.id();
                if self.admitted_ids.contains(&id)
                    || !visible_image(primitive.destination, primitive.clip, viewport)
                {
                    continue;
                }
                let image_bytes = primitive.image.byte_len() as u64;
                if self.admitted_images.len() == MAX_GPU_IMAGE_CACHE_ENTRIES
                    || admitted_bytes.saturating_add(image_bytes) > MAX_GPU_IMAGE_CACHE_BYTES
                {
                    continue;
                }
                admitted_bytes += image_bytes;
                self.admitted_ids.insert(id);
                self.admitted_images.push(primitive.image.clone());
            }
        }
        let mut uploads = 0;
        for index in 0..self.admitted_images.len() {
            let image = self.admitted_images[index].clone();
            let id = image.id();
            if let Some(cached) = self.cache.get_mut(&id) {
                cached.last_used_frame = self.frame;
                continue;
            }
            self.evict_until_fits(image.byte_len() as u64);
            let cached = upload_image(
                device,
                queue,
                &self.image_bind_group_layout,
                &self.sampler,
                &image,
                self.frame,
            );
            self.resident_bytes += cached.bytes;
            self.cache.insert(id, cached);
            uploads += 1;
        }

        for layer in scene.paint_layers() {
            let batch_start = self.batches.len();
            self.pending.clear();
            for item in layer.paint() {
                let PrimitiveRef::Image(index) = item.primitive else {
                    continue;
                };
                let primitive = &layer.images()[index];
                if !self.admitted_ids.contains(&primitive.image.id()) {
                    continue;
                }
                let clip = primitive.clip.unwrap_or(viewport);
                let Some(clip) = clip.intersection(viewport) else {
                    continue;
                };
                if !primitive.destination.intersects(clip) {
                    continue;
                }
                self.pending.push(OrderedImage {
                    order: item.order,
                    image: primitive.image.id(),
                    instance: ImageInstance {
                        rect: [
                            primitive.destination.x,
                            primitive.destination.y,
                            primitive.destination.width,
                            primitive.destination.height,
                        ],
                        uv: [
                            primitive.source_uv.x,
                            primitive.source_uv.y,
                            primitive.source_uv.width,
                            primitive.source_uv.height,
                        ],
                        clip: [clip.x, clip.y, clip.right(), clip.bottom()],
                        mask: [
                            primitive.mask.x,
                            primitive.mask.y,
                            primitive.mask.width,
                            primitive.mask.height,
                        ],
                        radius_filtered_opacity_padding: [
                            primitive.radius,
                            f32::from(!primitive.color_matrix.is_identity()),
                            primitive.opacity,
                            0.0,
                        ],
                        color_matrix: color_matrix_rows(primitive.color_matrix),
                    },
                });
            }
            self.pending
                .sort_unstable_by_key(|image| (image.order, image.image));
            for image in &self.pending {
                let index = self.instances.len() as u32;
                self.instances.push(image.instance);
                let extends_last = self.batches.len() > batch_start
                    && self.batches.last().is_some_and(|batch| {
                        batch.order == image.order && batch.image == image.image
                    });
                if extends_last {
                    self.batches
                        .last_mut()
                        .expect("the previous image batch exists")
                        .instances
                        .end = index + 1;
                } else {
                    self.batches.push(ImageBatch {
                        order: image.order,
                        image: image.image,
                        instances: index..index + 1,
                    });
                }
            }
            self.layer_batches.push(batch_start..self.batches.len());
        }

        self.uploads.write(
            0,
            queue,
            &self.uniform_buffer,
            bytemuck::bytes_of(&ViewUniform {
                viewport: [physical_width as f32, physical_height as f32],
                scale,
                _padding: 0.0,
            }),
        );
        self.active_buffer = (self.active_buffer + 1) % BUFFERED_FRAMES;
        let required = self.instances.len().max(1);
        if required > self.instance_capacities[self.active_buffer] {
            let capacity = required.next_power_of_two();
            self.instance_buffers[self.active_buffer] = create_instance_buffer(device, capacity);
            self.instance_capacities[self.active_buffer] = capacity;
            self.uploads.reset(1 + self.active_buffer);
        }
        if !self.instances.is_empty() {
            self.uploads.write(
                1 + self.active_buffer,
                queue,
                &self.instance_buffers[self.active_buffer],
                bytemuck::cast_slice(&self.instances),
            );
        }

        debug_assert!(self.cache.len() <= MAX_GPU_IMAGE_CACHE_ENTRIES);
        debug_assert!(self.resident_bytes <= MAX_GPU_IMAGE_CACHE_BYTES);
        ImagePrepareStats {
            images: self.instances.len(),
            uploads,
            draw_calls: self.batches.len(),
            cache_bytes: self.resident_bytes,
        }
    }

    pub(crate) fn render_order<'pass>(
        &'pass self,
        pass: &mut wgpu::RenderPass<'pass>,
        layer: usize,
        order: u32,
    ) {
        let Some(layer_batches) = self.layer_batches.get(layer) else {
            return;
        };
        let mut batches = self.batches[layer_batches.clone()]
            .iter()
            .filter(|batch| batch.order == order);
        let Some(first) = batches.next() else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.view_bind_group, &[]);
        pass.set_vertex_buffer(0, self.instance_buffers[self.active_buffer].slice(..));
        for batch in std::iter::once(first).chain(batches) {
            let cached = self
                .cache
                .get(&batch.image)
                .expect("visible image texture is resident");
            pass.set_bind_group(1, &cached.bind_group, &[]);
            pass.draw(0..6, batch.instances.clone());
        }
    }

    fn evict_until_fits(&mut self, incoming_bytes: u64) {
        while self.cache.len() >= MAX_GPU_IMAGE_CACHE_ENTRIES
            || self.resident_bytes + incoming_bytes > MAX_GPU_IMAGE_CACHE_BYTES
        {
            let Some(id) = self
                .cache
                .iter()
                .filter(|(id, _)| !self.admitted_ids.contains(id))
                .min_by_key(|(_, cached)| cached.last_used_frame)
                .map(|(id, _)| *id)
            else {
                debug_assert!(false, "admitted image set must fit the cache limits");
                break;
            };
            if let Some(removed) = self.cache.remove(&id) {
                self.resident_bytes = self.resident_bytes.saturating_sub(removed.bytes);
            }
        }
    }
}

fn visible_image(destination: Rect, clip: Option<Rect>, viewport: Rect) -> bool {
    clip.unwrap_or(viewport)
        .intersection(viewport)
        .is_some_and(|clip| destination.intersects(clip))
}

#[cfg(test)]
fn admit_visible(images: impl Iterator<Item = (ImageId, u64)>) -> Vec<ImageId> {
    let mut admitted = Vec::new();
    let mut seen = HashSet::new();
    let mut bytes = 0_u64;
    for (id, image_bytes) in images {
        if !seen.insert(id) {
            continue;
        }
        if admitted.len() == MAX_GPU_IMAGE_CACHE_ENTRIES
            || bytes.saturating_add(image_bytes) > MAX_GPU_IMAGE_CACHE_BYTES
        {
            continue;
        }
        bytes += image_bytes;
        admitted.push(id);
    }
    admitted
}

fn upload_image(
    device: &Device,
    queue: &Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &Sampler,
    image: &Image,
    frame: u64,
) -> CachedImage {
    let size = Extent3d {
        width: image.width(),
        height: image.height(),
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("quickgui cached image"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8UnormSrgb,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: Origin3d::ZERO,
            aspect: TextureAspect::All,
        },
        image.rgba(),
        TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(image.width() * 4),
            rows_per_image: Some(image.height()),
        },
        size,
    );
    let view = texture.create_view(&TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("quickgui cached image bind group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    CachedImage {
        _texture: texture,
        _view: view,
        bind_group,
        bytes: image.byte_len() as u64,
        last_used_frame: frame,
    }
}

fn create_instance_buffer(device: &Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("quickgui image instance buffer"),
        size: (capacity * mem::size_of::<ImageInstance>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admission_is_source_ordered_unique_and_byte_bounded() {
        let first = Image::from_rgba(1, 1, [0; 4].as_slice()).unwrap();
        let second = Image::from_rgba(1, 1, [0; 4].as_slice()).unwrap();
        let third = Image::from_rgba(1, 1, [0; 4].as_slice()).unwrap();
        let almost_full = MAX_GPU_IMAGE_CACHE_BYTES - 4;
        let admitted = admit_visible(
            [
                (first.id(), almost_full),
                (first.id(), almost_full),
                (second.id(), 8),
                (third.id(), 4),
            ]
            .into_iter(),
        );
        assert_eq!(admitted, vec![first.id(), third.id()]);
    }

    #[test]
    fn visibility_respects_both_clip_and_viewport() {
        let viewport = Rect::new(0.0, 0.0, 100.0, 100.0);
        assert!(visible_image(
            Rect::new(80.0, 80.0, 40.0, 40.0),
            None,
            viewport
        ));
        assert!(!visible_image(
            Rect::new(80.0, 80.0, 40.0, 40.0),
            Some(Rect::new(0.0, 0.0, 50.0, 50.0)),
            viewport
        ));
    }
}
