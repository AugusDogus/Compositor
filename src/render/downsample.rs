use crate::{
    document::{Document, LayerContent},
    geometry::{Point, Sampling, Transform},
    native_pixels,
};
use image::{
    GrayImage, RgbaImage,
    imageops::{FilterType, resize},
};
use std::{
    collections::VecDeque,
    sync::{Arc, Weak},
};

fn reduced_size(t: Transform, source: (u32, u32), step: Point) -> (u32, u32) {
    let (sin, cos) = t.rotation.to_radians().sin_cos();
    let width = t.size[0] * (cos / step[0]).hypot(sin / step[1]);
    let height = t.size[1] * (sin / step[0]).hypot(cos / step[1]);
    (
        (width.ceil().max(1.) as u32).min(source.0),
        (height.ceil().max(1.) as u32).min(source.1),
    )
}

enum Entry {
    Color {
        source: Weak<RgbaImage>,
        image: Arc<RgbaImage>,
    },
    Mask {
        source: Weak<GrayImage>,
        image: Arc<GrayImage>,
    },
}
impl Entry {
    fn bytes(&self) -> usize {
        match self {
            Self::Color { image, .. } => image.len(),
            Self::Mask { image, .. } => image.len(),
        }
    }
    fn alive(&self) -> bool {
        match self {
            Self::Color { source, .. } => source.strong_count() > 0,
            Self::Mask { source, .. } => source.strong_count() > 0,
        }
    }
}

/// A bounded LRU of immutable reductions. Weak source references do not retain
/// obsolete full-resolution assets after edits or closing a project.
pub struct DownsampleCache {
    entries: VecDeque<Entry>,
    limit: usize,
}
impl Default for DownsampleCache {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
            limit: 64 * 1024 * 1024,
        }
    }
}
impl DownsampleCache {
    fn insert(&mut self, entry: Entry) {
        if entry.bytes() > self.limit {
            return;
        }
        let mut bytes = self.entries.iter().map(Entry::bytes).sum::<usize>() + entry.bytes();
        while bytes > self.limit {
            if let Some(old) = self.entries.pop_front() {
                bytes -= old.bytes();
            } else {
                break;
            }
        }
        self.entries.push_back(entry);
    }

    fn cached_color(
        &mut self,
        pixels: &Arc<RgbaImage>,
        size: (u32, u32),
    ) -> Option<Arc<RgbaImage>> {
        let source = Arc::downgrade(pixels);
        if let Some(index) = self.entries.iter().position(|entry| matches!(entry, Entry::Color { source: key, image } if key.ptr_eq(&source) && image.dimensions() == size))
            && let Some(entry) = self.entries.remove(index)
        {
            let image = match &entry { Entry::Color { image, .. } => Some(image.clone()), _ => None };
            self.entries.push_back(entry);
            if let Some(image) = image { return Some(image); }
        }
        None
    }

    fn color(&mut self, pixels: &Arc<RgbaImage>, size: (u32, u32)) -> Arc<RgbaImage> {
        if let Some(image) = self.cached_color(pixels, size) {
            return image;
        }
        let source = Arc::downgrade(pixels);
        let image = Arc::new(native_pixels::unpremultiply(resize(
            &native_pixels::premultiply(pixels),
            size.0,
            size.1,
            FilterType::Lanczos3,
        )));
        self.insert(Entry::Color {
            source,
            image: image.clone(),
        });
        image
    }

    fn cached_mask(&mut self, pixels: &Arc<GrayImage>, size: (u32, u32)) -> Option<Arc<GrayImage>> {
        let source = Arc::downgrade(pixels);
        if let Some(index) = self.entries.iter().position(|entry| matches!(entry, Entry::Mask { source: key, image } if key.ptr_eq(&source) && image.dimensions() == size))
            && let Some(entry) = self.entries.remove(index)
        {
            let image = match &entry { Entry::Mask { image, .. } => Some(image.clone()), _ => None };
            self.entries.push_back(entry);
            if let Some(image) = image { return Some(image); }
        }
        None
    }

    fn mask(&mut self, pixels: &Arc<GrayImage>, size: (u32, u32)) -> Arc<GrayImage> {
        if let Some(image) = self.cached_mask(pixels, size) {
            return image;
        }
        let source = Arc::downgrade(pixels);
        let image = Arc::new(resize(
            pixels.as_ref(),
            size.0,
            size.1,
            FilterType::Lanczos3,
        ));
        self.insert(Entry::Mask {
            source,
            image: image.clone(),
        });
        image
    }

    pub(super) fn prepare_accelerated(
        &mut self,
        doc: &Document,
        step: Point,
    ) -> crate::Result<Document> {
        self.entries.retain(Entry::alive);
        let mut prepared = doc.clone();
        for layer in &mut prepared.layers {
            if layer.transform.sampling == Sampling::High
                && let LayerContent::Raster(Some(pixels)) = &mut layer.content
            {
                let size = reduced_size(layer.transform, pixels.dimensions(), step);
                if size != pixels.dimensions() {
                    if let Some(cached) = self.cached_color(pixels, size) {
                        *pixels = cached;
                    } else if let Some(reduced) = super::gpu::resize::color(pixels, size)? {
                        let reduced = Arc::new(reduced);
                        self.insert(Entry::Color {
                            source: Arc::downgrade(pixels),
                            image: reduced.clone(),
                        });
                        *pixels = reduced;
                    }
                }
            }
            if let Some(mask) = &mut layer.mask {
                let t = mask.placement.unwrap_or(layer.transform);
                let size = reduced_size(t, mask.pixels.dimensions(), step);
                if t.sampling == Sampling::High && size != mask.pixels.dimensions() {
                    if let Some(cached) = self.cached_mask(&mask.pixels, size) {
                        mask.pixels = cached;
                    } else if let Some(reduced) = super::gpu::resize::mask(&mask.pixels, size)? {
                        let reduced = Arc::new(reduced);
                        self.insert(Entry::Mask {
                            source: Arc::downgrade(&mask.pixels),
                            image: reduced.clone(),
                        });
                        mask.pixels = reduced;
                    }
                }
            }
        }
        // Unsupported sizes and unavailable hardware use the same reference reducer.
        Ok(self.prepare(&prepared, step))
    }

    pub(super) fn prepare(&mut self, doc: &Document, step: Point) -> Document {
        self.entries.retain(Entry::alive);
        let mut prepared = doc.clone();
        for layer in &mut prepared.layers {
            if layer.transform.sampling == Sampling::High
                && let LayerContent::Raster(Some(pixels)) = &mut layer.content
            {
                let reduced = reduced_size(layer.transform, pixels.dimensions(), step);
                if reduced != pixels.dimensions() {
                    *pixels = self.color(pixels, reduced);
                }
            }
            if let Some(mask) = &mut layer.mask {
                let t = mask.placement.unwrap_or(layer.transform);
                if t.sampling == Sampling::High {
                    let reduced = reduced_size(t, mask.pixels.dimensions(), step);
                    if reduced != mask.pixels.dimensions() {
                        mask.pixels = self.mask(&mask.pixels, reduced);
                    }
                }
            }
        }
        prepared
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Mask;
    use image::{Luma, Rgba};

    #[test]
    fn reuse_tracks_pixel_identity_and_size_while_mask_and_color_stay_distinct() {
        let mut doc = Document::new(80, 80).unwrap();
        doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
            80,
            80,
            Rgba([90, 120, 200, 128]),
        ))));
        doc.layers[0].mask = Some(Mask {
            pixels: Arc::new(GrayImage::from_pixel(80, 80, Luma([128]))),
            enabled: true,
            linked: true,
            placement: None,
        });
        let mut cache = DownsampleCache::default();
        let first = cache.prepare(&doc, [2.; 2]);
        doc.layers[0].opacity = 0.5;
        doc.layers[0].transform.origin = [10., 20.];
        let moved = cache.prepare(&doc, [2.; 2]);
        assert!(Arc::ptr_eq(
            first.layers[0].raster().unwrap(),
            moved.layers[0].raster().unwrap()
        ));
        assert!(Arc::ptr_eq(
            &first.layers[0].mask.as_ref().unwrap().pixels,
            &moved.layers[0].mask.as_ref().unwrap().pixels
        ));
        assert_eq!(cache.entries.len(), 2);
        let smaller = cache.prepare(&doc, [4., 2.]);
        assert_eq!(smaller.layers[0].raster().unwrap().dimensions(), (20, 40));
        assert!(!Arc::ptr_eq(
            first.layers[0].raster().unwrap(),
            smaller.layers[0].raster().unwrap()
        ));
        doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
            80,
            80,
            Rgba([255, 0, 0, 255]),
        ))));
        let edited = cache.prepare(&doc, [2.; 2]);
        assert_eq!(
            edited.layers[0].raster().unwrap()[(10, 10)],
            Rgba([255, 0, 0, 255])
        );
        assert_eq!(cache.entries.len(), 3, "obsolete source entries are pruned");
    }

    #[test]
    fn cache_budget_evicts_old_reductions_without_retaining_source_pixels() {
        let mut cache = DownsampleCache {
            entries: VecDeque::new(),
            limit: 400,
        };
        let source = Arc::new(RgbaImage::new(40, 40));
        let first = cache.color(&source, (10, 10));
        assert_eq!(Arc::strong_count(&source), 1);
        cache.color(&source, (8, 8));
        assert_eq!(cache.entries.len(), 1);
        assert!(!Arc::ptr_eq(&first, &cache.color(&source, (10, 10))));
        cache.color(&source, (20, 20));
        assert!(cache.entries.iter().map(Entry::bytes).sum::<usize>() <= 400);
    }
}
