use crate::{
    Result,
    document::Layer,
    geometry::{Point, Sampling, Transform},
    invalid, native_pixels, render,
};
use image::{GrayImage, Rgba, RgbaImage};
use std::{cell::RefCell, collections::HashMap, sync::Arc};

const SIDE: u32 = 128;
const CACHE_BYTES: usize = 64 * 1024 * 1024;

enum Input {
    Pixels(Arc<RgbaImage>, Transform),
    Mask(Arc<GrayImage>, Transform, u8),
}

struct Tile {
    pixels: RgbaImage,
    origin: Point,
    used: u64,
}

#[derive(Default)]
struct Cache {
    tiles: HashMap<(u32, u32), Tile>,
    bytes: usize,
    clock: u64,
}

impl Cache {
    fn insert(&mut self, key: (u32, u32), mut tile: Tile) {
        let bytes = tile.pixels.as_raw().len();
        while self.bytes + bytes > CACHE_BYTES {
            let Some(oldest) = self
                .tiles
                .iter()
                .min_by_key(|(_, tile)| tile.used)
                .map(|(key, _)| *key)
            else {
                break;
            };
            if let Some(removed) = self.tiles.remove(&oldest) {
                self.bytes -= removed.pixels.as_raw().len();
            }
        }
        self.clock = self.clock.saturating_add(1);
        tile.used = self.clock;
        if let Some(previous) = self.tiles.insert(key, tile) {
            self.bytes -= previous.pixels.as_raw().len();
        }
        self.bytes += bytes;
    }
}

/// Immutable stroke input blurred in bounded tiles, including a Gaussian halo
/// and interpolation border. The cache is local to one UI-thread stroke.
pub(super) struct Blur {
    input: Input,
    canvas: [u32; 2],
    sigma: f32,
    accelerated: bool,
    cache: RefCell<Cache>,
}

impl Blur {
    pub fn new(layer: &Layer, canvas: [u32; 2], diameter: f64, mask: bool) -> Result<Self> {
        let input = if mask {
            let mask = layer
                .mask
                .as_ref()
                .ok_or_else(|| invalid("Add a mask before blurring it."))?;
            Input::Mask(
                mask.pixels.clone(),
                mask.placement.unwrap_or(layer.transform),
                (mask.background() * 255.).round() as u8,
            )
        } else {
            Input::Pixels(
                layer
                    .raster()
                    .ok_or_else(|| invalid("Select a layer containing pixels before blurring it."))?
                    .clone(),
                layer.transform,
            )
        };
        Ok(Self {
            input,
            canvas,
            sigma: (diameter / 10.).clamp(1.5, 30.) as f32,
            accelerated: true,
            cache: RefCell::default(),
        })
    }

    fn sharp(&self, point: Point) -> Rgba<u8> {
        match &self.input {
            Input::Pixels(image, transform) => {
                // Screenshot imports and ordinary pixel layers already align with
                // the canvas. Avoid bilinear reconstruction of exact pixel centers.
                if transform.origin == [0., 0.]
                    && transform.size == [f64::from(image.width()), f64::from(image.height())]
                    && transform.rotation == 0.
                    && !transform.flip_x
                    && !transform.flip_y
                    && transform.warp.is_none()
                {
                    return image
                        .get_pixel_checked(point[0] as u32, point[1] as u32)
                        .copied()
                        .unwrap_or(Rgba([0; 4]));
                }
                Rgba(
                    render::pixel(image, transform.unit(point), transform.sampling)
                        .map(|v| (v * 255.).round() as u8),
                )
            }
            Input::Mask(pixels, transform, background) => {
                let unit = transform.unit(point);
                let level = if unit.iter().all(|v| (0. ..1.).contains(v)) {
                    pixels[(
                        (unit[0] * pixels.width() as f64) as u32,
                        (unit[1] * pixels.height() as f64) as u32,
                    )][0]
                } else {
                    *background
                };
                Rgba([level, level, level, 255])
            }
        }
    }

    fn tile(&self, key: (u32, u32)) -> Result<Tile> {
        let min = [
            (key.0 * SIDE).saturating_sub(1),
            (key.1 * SIDE).saturating_sub(1),
        ];
        let max = [
            ((key.0 + 1) * SIDE + 1).min(self.canvas[0]),
            ((key.1 + 1) * SIDE + 1).min(self.canvas[1]),
        ];
        self.region(min, max)
    }

    fn region(&self, min: [u32; 2], max: [u32; 2]) -> Result<Tile> {
        // image's Gaussian kernel radius is less than four sigma. Clip halos to
        // the canvas so its boundary handling matches a full-canvas blur.
        let halo = (self.sigma * 4.).ceil() as u32 + 2;
        let start = min.map(|v| v.saturating_sub(halo));
        let end = [
            max[0].saturating_add(halo).min(self.canvas[0]),
            max[1].saturating_add(halo).min(self.canvas[1]),
        ];
        let sharp = RgbaImage::from_fn(end[0] - start[0], end[1] - start[1], |x, y| {
            self.sharp([(start[0] + x) as f64 + 0.5, (start[1] + y) as f64 + 0.5])
        });
        let premultiplied = native_pixels::premultiply(&sharp);
        let blurred = if self.accelerated
            && let Some(pixels) = render::gpu_brush_blur(&premultiplied, self.sigma)?
        {
            pixels
        } else {
            image::imageops::blur(&premultiplied, self.sigma)
        };
        let pixels = native_pixels::unpremultiply(
            image::imageops::crop_imm(
                &blurred,
                min[0] - start[0],
                min[1] - start[1],
                max[0] - min[0],
                max[1] - min[1],
            )
            .to_image(),
        );
        Ok(Tile {
            pixels,
            origin: min.map(f64::from),
            used: 0,
        })
    }

    /// Blur adjacent missing tiles together, sharing their halo and one GPU
    /// readback. Bound temporary buffers independently from canvas dimensions.
    pub fn prepare(&self, bounds: [f64; 4]) -> Result<()> {
        let start = [bounds[0], bounds[1]].map(|v| v.max(0.).floor() as u32 / SIDE);
        let end = [bounds[2], bounds[3]].map(|v| (v.max(0.).ceil() as u32).div_ceil(SIDE));
        if end[0].saturating_sub(start[0]) as u64 * u64::from(end[1].saturating_sub(start[1])) > 64
        {
            return Ok(());
        }
        let mut cache = self.cache.borrow_mut();
        let mut missing = Vec::new();
        for y in start[1]..end[1] {
            for x in start[0]..end[0] {
                if x * SIDE < self.canvas[0]
                    && y * SIDE < self.canvas[1]
                    && !cache.tiles.contains_key(&(x, y))
                {
                    missing.push((x, y));
                }
            }
        }
        if missing.len() < 2 {
            return Ok(());
        }
        let min_key = [
            missing.iter().map(|key| key.0).min().unwrap_or(0),
            missing.iter().map(|key| key.1).min().unwrap_or(0),
        ];
        let max_key = [
            missing.iter().map(|key| key.0).max().unwrap_or(0),
            missing.iter().map(|key| key.1).max().unwrap_or(0),
        ];
        let min = min_key.map(|v| (v * SIDE).saturating_sub(1));
        let max = [
            ((max_key[0] + 1) * SIDE + 1).min(self.canvas[0]),
            ((max_key[1] + 1) * SIDE + 1).min(self.canvas[1]),
        ];
        if u64::from(max[0] - min[0]) * u64::from(max[1] - min[1]) > 1_048_576 {
            return Ok(());
        }
        let region = self.region(min, max)?;
        for key in missing {
            let left = (key.0 * SIDE).saturating_sub(1);
            let top = (key.1 * SIDE).saturating_sub(1);
            let right = ((key.0 + 1) * SIDE + 1).min(self.canvas[0]);
            let bottom = ((key.1 + 1) * SIDE + 1).min(self.canvas[1]);
            let pixels = image::imageops::crop_imm(
                &region.pixels,
                left - min[0],
                top - min[1],
                right - left,
                bottom - top,
            )
            .to_image();
            cache.insert(
                key,
                Tile {
                    pixels,
                    origin: [f64::from(left), f64::from(top)],
                    used: 0,
                },
            );
        }
        Ok(())
    }

    pub fn sample(&self, point: Point, sampling: Sampling) -> Result<[f64; 4]> {
        if point
            .iter()
            .zip(self.canvas)
            .any(|(p, size)| !(0. ..size as f64).contains(p))
        {
            return Ok([0.; 4]);
        }
        let key = (point[0] as u32 / SIDE, point[1] as u32 / SIDE);
        let mut cache = self.cache.borrow_mut();
        if !cache.tiles.contains_key(&key) {
            let tile = self.tile(key)?;
            cache.insert(key, tile);
        }
        cache.clock = cache.clock.saturating_add(1);
        let used = cache.clock;
        // The tile was either already cached or inserted directly above.
        let tile = cache
            .tiles
            .get_mut(&key)
            .expect("blur tile is cached before sampling");
        tile.used = used;
        Ok(render::pixel(
            &tile.pixels,
            [
                (point[0] - tile.origin[0]) / tile.pixels.width() as f64,
                (point[1] - tile.origin[1]) / tile.pixels.height() as f64,
            ],
            sampling,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{LayerContent, Mask};
    use image::{GrayImage, Luma};

    const EDGE_POINTS: [Point; 5] = [
        [0.1, 0.1],
        [127.2, 128.2],
        [128.1, 127.1],
        [255.4, 255.8],
        [269.9, 259.9],
    ];

    fn source_layer() -> Layer {
        let mut layer = Layer::blank("Source", 270, 260);
        layer.content =
            LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(270, 260, |x, y| {
                Rgba([
                    (x % 256) as u8,
                    (y % 256) as u8,
                    100,
                    if (x + y).is_multiple_of(3) { 100 } else { 255 },
                ])
            }))));
        layer.mask = Some(Mask {
            pixels: Arc::new(GrayImage::from_fn(17, 15, |x, y| {
                Luma([((x + y) * 7) as u8])
            })),
            placement: Some(Transform {
                origin: [100., 100.],
                ..Transform::new(70, 60)
            }),
            enabled: true,
            linked: true,
        });
        layer
    }

    #[test]
    fn tiled_blur_matches_full_snapshot_at_tile_and_canvas_edges() {
        let layer = source_layer();
        for mask in [false, true] {
            for diameter in [15., 97., 300.] {
                let mut source = Blur::new(&layer, [270, 260], diameter, mask).unwrap();
                source.accelerated = false;
                let sharp = RgbaImage::from_fn(270, 260, |x, y| {
                    source.sharp([x as f64 + 0.5, y as f64 + 0.5])
                });
                let full = native_pixels::unpremultiply(image::imageops::blur(
                    &native_pixels::premultiply(&sharp),
                    source.sigma,
                ));
                for batch in [false, true] {
                    source.cache.replace(Cache::default());
                    if batch {
                        source.prepare([0., 0., 270., 260.]).unwrap();
                    }
                    for p in EDGE_POINTS {
                        for sampling in [Sampling::Nearest, Sampling::High] {
                            let actual = source.sample(p, sampling).unwrap();
                            let expected =
                                render::pixel(&full, [p[0] / 270., p[1] / 260.], sampling);
                            for (a, b) in actual.into_iter().zip(expected) {
                                assert!(
                                    (a - b).abs() < 1e-10,
                                    "{p:?}, diameter {diameter}, mask {mask}: {a} vs {b}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    #[ignore = "Requires a hardware Vulkan adapter"]
    fn batched_gpu_blur_matches_lazy_tiles_for_transformed_pixels_and_masks() {
        let probe = RgbaImage::from_pixel(2, 2, Rgba([50, 60, 70, 255]));
        assert!(
            render::gpu_brush_blur(&probe, 1.5).unwrap().is_some(),
            "Hardware GPU required"
        );
        for transformed in [false, true] {
            let mut layer = source_layer();
            if transformed {
                layer.transform = Transform {
                    origin: [13., -7.],
                    rotation: 23.,
                    flip_x: true,
                    ..Transform::new(230, 210)
                };
            }
            for mask in [false, true] {
                for diameter in [15., 97., 300.] {
                    let lazy = Blur::new(&layer, [270, 260], diameter, mask).unwrap();
                    let batched = Blur::new(&layer, [270, 260], diameter, mask).unwrap();
                    batched.prepare([0., 0., 270., 260.]).unwrap();
                    assert_eq!(batched.cache.borrow().tiles.len(), 9);
                    for point in EDGE_POINTS {
                        for sampling in [Sampling::Nearest, Sampling::High] {
                            assert_eq!(
                                batched.sample(point, sampling).unwrap(),
                                lazy.sample(point, sampling).unwrap(),
                                "point {point:?}, diameter {diameter}, mask {mask}, transformed {transformed}"
                            );
                        }
                    }
                }
            }
        }
    }
}
