use crate::{Result, geometry::Transform, invalid, render};
use image::RgbaImage;
use std::{collections::HashMap, sync::Arc};

const SIDE: usize = 128;

struct Tile {
    pixels: Vec<[f32; 4]>,
    touched: Vec<bool>,
}

/// Untouched pixels come from the immutable source. Only edited tiles allocate
/// float pixels and touch coverage, regardless of the canvas dimensions.
pub(super) struct Plane {
    source: Arc<RgbaImage>,
    transform: Transform,
    size: [usize; 2],
    tiles: HashMap<(usize, usize), Tile>,
}

impl Plane {
    pub fn new(source: Arc<RgbaImage>, transform: Transform, size: [usize; 2]) -> Self {
        Self {
            source,
            transform,
            size,
            tiles: HashMap::new(),
        }
    }

    fn original(source: &RgbaImage, transform: Transform, x: usize, y: usize) -> [f32; 4] {
        let p = render::pixel(
            source,
            transform.unit([x as f64 + 0.5, y as f64 + 0.5]),
            transform.sampling,
        );
        [
            (p[0] * p[3]) as f32,
            (p[1] * p[3]) as f32,
            (p[2] * p[3]) as f32,
            p[3] as f32,
        ]
    }

    pub fn get(&self, x: usize, y: usize) -> [f32; 4] {
        self.tiles.get(&(x / SIDE, y / SIDE)).map_or_else(
            || Self::original(&self.source, self.transform, x, y),
            |tile| tile.pixels[(y % SIDE) * SIDE + x % SIDE],
        )
    }

    pub fn set(&mut self, x: usize, y: usize, pixel: [f32; 4]) -> Result<()> {
        let key = (x / SIDE, y / SIDE);
        if !self.tiles.contains_key(&key)
            && (self.tiles.len() as u64 + 1) * (SIDE * SIDE) as u64 > crate::document::MAX_SURFACE_PIXELS
        {
            return Err(invalid(
                "This warp stroke exceeds 200 million working pixels. Use a shorter stroke or smaller brush. Cancel the stroke to preserve the original layer.",
            ));
        }
        let tile = self.tiles.entry(key).or_insert_with(|| Tile {
            pixels: (0..SIDE * SIDE)
                .map(|i| {
                    Self::original(
                        &self.source,
                        self.transform,
                        key.0 * SIDE + i % SIDE,
                        key.1 * SIDE + i / SIDE,
                    )
                })
                .collect(),
            touched: vec![false; SIDE * SIDE],
        });
        let index = (y % SIDE) * SIDE + x % SIDE;
        tile.pixels[index] = pixel;
        tile.touched[index] = true;
        Ok(())
    }

    pub fn touched(&self, x: usize, y: usize) -> bool {
        self.tiles
            .get(&(x / SIDE, y / SIDE))
            .is_some_and(|tile| tile.touched[(y % SIDE) * SIDE + x % SIDE])
    }

    pub fn sample(&self, x: f64, y: f64) -> [f32; 4] {
        interpolate(self.size[0], self.size[1], x, y, |x, y| self.get(x, y))
    }
}

pub(super) fn interpolate(
    w: usize,
    h: usize,
    x: f64,
    y: f64,
    pixel: impl Fn(usize, usize) -> [f32; 4],
) -> [f32; 4] {
    let (ix, iy) = (x.floor() as usize, y.floor() as usize);
    let (jx, jy) = ((ix + 1).min(w - 1), (iy + 1).min(h - 1));
    let (fx, fy) = ((x - ix as f64) as f32, (y - iy as f64) as f32);
    let [a, b, c, d] = [pixel(ix, iy), pixel(jx, iy), pixel(ix, jy), pixel(jx, jy)];
    std::array::from_fn(|k| {
        let top = a[k] * (1. - fx) + b[k] * fx;
        let bottom = c[k] * (1. - fx) + d[k] * fx;
        top * (1. - fy) + bottom * fy
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sparse_working_pixels_keep_unedited_source_and_interpolate_across_tiles() {
        let image = Arc::new(RgbaImage::from_pixel(1, 1, image::Rgba([120, 60, 30, 128])));
        let mut transform = Transform::new(30_000, 30_000);
        transform.sampling = crate::geometry::Sampling::Nearest;
        let mut plane = Plane::new(image, transform, [30_000, 30_000]);
        let original = plane.get(128, 128);
        plane.set(127, 128, [1.; 4]).unwrap();
        assert_eq!(plane.get(128, 128), original);
        assert!(!plane.touched(128, 128));
        assert!(plane.touched(127, 128));
        assert_eq!(
            plane.sample(127.5, 128.),
            original.map(|value| (value + 1.) * 0.5)
        );
        assert_eq!(plane.tiles.len(), 1);
        plane.set(128, 128, [0.; 4]).unwrap();
        assert_eq!(plane.sample(127.5, 128.), [0.5; 4]);
        assert_eq!(plane.tiles.len(), 2);
    }
}
