use crate::{
    Result,
    document::{Layer, Mask},
    geometry::{Point, Sampling, Transform},
    invalid, native_pixels, render,
};
use image::{Rgba, RgbaImage};
use std::{cell::RefCell, collections::HashMap, sync::Arc};

const SIDE: u32 = 128;
const CACHE_BYTES: usize = 64 * 1024 * 1024;

enum Input {
    Pixels(Arc<RgbaImage>, Transform),
    Mask(Mask, Transform, u8),
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

/// Immutable stroke input blurred in bounded tiles, including a Gaussian halo
/// and interpolation border. The cache is local to one UI-thread stroke.
pub(super) struct Blur {
    input: Input,
    canvas: [u32; 2],
    sigma: f32,
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
                mask.clone(),
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
            cache: RefCell::default(),
        })
    }

    fn sharp(&self, point: Point) -> Rgba<u8> {
        match &self.input {
            Input::Pixels(image, transform) => Rgba(
                render::pixel(image, transform.unit(point), transform.sampling)
                    .map(|v| (v * 255.).round() as u8),
            ),
            Input::Mask(mask, transform, background) => {
                let unit = transform.unit(point);
                let level = if unit.iter().all(|v| (0. ..1.).contains(v)) {
                    mask.pixels[(
                        (unit[0] * mask.pixels.width() as f64) as u32,
                        (unit[1] * mask.pixels.height() as f64) as u32,
                    )][0]
                } else {
                    *background
                };
                Rgba([level, level, level, 255])
            }
        }
    }

    fn tile(&self, key: (u32, u32)) -> Tile {
        let min = [
            (key.0 * SIDE).saturating_sub(1),
            (key.1 * SIDE).saturating_sub(1),
        ];
        let max = [
            ((key.0 + 1) * SIDE + 1).min(self.canvas[0]),
            ((key.1 + 1) * SIDE + 1).min(self.canvas[1]),
        ];
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
        let blurred = image::imageops::blur(&native_pixels::premultiply(&sharp), self.sigma);
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
        Tile {
            pixels,
            origin: min.map(f64::from),
            used: 0,
        }
    }

    pub fn sample(&self, point: Point, sampling: Sampling) -> [f64; 4] {
        if point
            .iter()
            .zip(self.canvas)
            .any(|(p, size)| !(0. ..size as f64).contains(p))
        {
            return [0.; 4];
        }
        let key = (point[0] as u32 / SIDE, point[1] as u32 / SIDE);
        let mut cache = self.cache.borrow_mut();
        if !cache.tiles.contains_key(&key) {
            let tile = self.tile(key);
            let bytes = tile.pixels.as_raw().len();
            while cache.bytes + bytes > CACHE_BYTES {
                let Some(key) = cache
                    .tiles
                    .iter()
                    .min_by_key(|(_, tile)| tile.used)
                    .map(|(key, _)| *key)
                else {
                    break;
                };
                if let Some(tile) = cache.tiles.remove(&key) {
                    cache.bytes -= tile.pixels.as_raw().len();
                }
            }
            cache.bytes += bytes;
            cache.tiles.insert(key, tile);
        }
        cache.clock = cache.clock.saturating_add(1);
        let used = cache.clock;
        // The tile was either already cached or inserted directly above.
        let tile = cache
            .tiles
            .get_mut(&key)
            .expect("blur tile is cached before sampling");
        tile.used = used;
        render::pixel(
            &tile.pixels,
            [
                (point[0] - tile.origin[0]) / tile.pixels.width() as f64,
                (point[1] - tile.origin[1]) / tile.pixels.height() as f64,
            ],
            sampling,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::LayerContent;
    use image::{GrayImage, Luma};

    #[test]
    fn tiled_blur_matches_full_snapshot_at_tile_and_canvas_edges() {
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
        for mask in [false, true] {
            for diameter in [15., 97., 300.] {
                let source = Blur::new(&layer, [270, 260], diameter, mask).unwrap();
                let sharp = RgbaImage::from_fn(270, 260, |x, y| {
                    source.sharp([x as f64 + 0.5, y as f64 + 0.5])
                });
                let full = native_pixels::unpremultiply(image::imageops::blur(
                    &native_pixels::premultiply(&sharp),
                    source.sigma,
                ));
                for p in [
                    [0.1, 0.1],
                    [127.2, 128.2],
                    [128.1, 127.1],
                    [255.4, 255.8],
                    [269.9, 259.9],
                ] {
                    for sampling in [Sampling::Nearest, Sampling::High] {
                        let actual = source.sample(p, sampling);
                        let expected = render::pixel(&full, [p[0] / 270., p[1] / 260.], sampling);
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
