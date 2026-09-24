use super::*;
#[derive(Clone, Copy, Default, PartialEq)]
pub(super) enum Scope {
    #[default]
    Histogram,
    Vectorscope,
}

struct Data {
    bins: [[u32; 256]; 3],
    scope: Vec<f32>,
    maximum: f32,
    scope_peak: f32,
}
impl Data {
    fn from_pixels(pixels: Option<&image::RgbaImage>) -> Self {
        let mut bins = [[0u32; 256]; 3];
        let mut scope = vec![0_f32; 64 * 64];
        if let Some(pixels) = pixels {
            let step = (pixels.len() / 4 / 65_536).max(1);
            for pixel in pixels.pixels().step_by(step) {
                if pixel[3] > 0 {
                    for c in 0..3 {
                        bins[c][usize::from(pixel[c])] += 1;
                    }
                    let rgb = [pixel[0], pixel[1], pixel[2]].map(|v| f64::from(v) / 255.);
                    let max = rgb.into_iter().fold(0_f64, f64::max);
                    let min = rgb.into_iter().fold(1_f64, f64::min);
                    if max > 0. && max - min > 1e-4 {
                        let hue = compositor::camera_raw::sampling::hsl(rgb)[0].to_radians();
                        let saturation = (max - min) / max;
                        let x = ((0.5 + hue.cos() * saturation * 0.48) * 64.) as usize;
                        let y = ((0.5 + hue.sin() * saturation * 0.48) * 64.) as usize;
                        scope[y.min(63) * 64 + x.min(63)] += f32::from(pixel[3]) / 255.;
                    }
                }
            }
        }
        let maximum = bins.iter().flatten().copied().max().unwrap_or(1).max(1) as f32;
        let scope_peak = scope.iter().copied().fold(1_f32, f32::max);
        Self {
            bins,
            scope,
            maximum,
            scope_peak,
        }
    }
}
/// Holding the source Arc makes identity checks safe even when a new image has
/// the same dimensions. Copy-on-write edits cannot silently mutate cached pixels.
pub(super) struct Cache {
    pixels: Option<Arc<image::RgbaImage>>,
    data: Arc<Data>,
}
impl Default for Cache {
    fn default() -> Self {
        Self {
            pixels: None,
            data: Arc::new(Data::from_pixels(None)),
        }
    }
}
impl Cache {
    fn data(&mut self, pixels: Option<&Arc<image::RgbaImage>>) -> Arc<Data> {
        let unchanged = match (self.pixels.as_ref(), pixels) {
            (Some(old), Some(new)) => Arc::ptr_eq(old, new),
            (None, None) => true,
            _ => false,
        };
        if !unchanged {
            self.data = Arc::new(Data::from_pixels(pixels.map(Arc::as_ref)));
            self.pixels = pixels.cloned();
        }
        Arc::clone(&self.data)
    }
}
impl Editor {
    pub(in crate::ui) fn camera_histogram(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let data = self
            .camera_raw
            .scopes
            .borrow_mut()
            .data(self.camera_scope_pixels());
        let mode = self.camera_raw.scope;
        let histogram = quickgui::canvas(move |bounds, painter| {
            if mode == Scope::Vectorscope {
                let side = bounds.height.min(bounds.width);
                let left = (bounds.width - side) / 2.;
                for (i, value) in data.scope.iter().enumerate() {
                    if *value > 0. {
                        let alpha = ((*value / data.scope_peak).sqrt() * 255.).round() as u8;
                        painter.fill_rect(
                            quickgui::Rect::new(
                                left + (i % 64) as f32 * side / 64.,
                                (i / 64) as f32 * side / 64.,
                                side / 64. + 0.5,
                                side / 64. + 0.5,
                            ),
                            Color::rgba8(160, 220, 180, alpha),
                        );
                    }
                }
                return;
            }
            for (channel, color) in [
                Color::rgba8(230, 70, 70, 170),
                Color::rgba8(80, 210, 100, 170),
                Color::rgba8(80, 130, 245, 170),
            ]
            .into_iter()
            .enumerate()
            {
                let mut path = quickgui::PathBuilder::stroke((bounds.width / 256.).max(1.));
                for (i, count) in data.bins[channel].iter().enumerate() {
                    let x = (i as f32 + 0.5) * bounds.width / 256.;
                    let height = (*count as f32 / data.maximum).sqrt() * bounds.height;
                    path.move_to(quickgui::Point::new(x, bounds.height));
                    path.line_to(quickgui::Point::new(x, bounds.height - height));
                }
                if let Ok(path) = path.build() {
                    painter.paint_path(path, color);
                }
            }
        })
        .id("camera-histogram")
        .h(72.)
        .w_full()
        .bg(Color::rgb8(20, 20, 20));
        let readout = self.camera_raw.readout.map_or_else(
            || "Move over the image to inspect RGB".to_owned(),
            |rgb| {
                format!(
                    "R {}   G {}   B {}",
                    (rgb[0] * 255.).round() as u8,
                    (rgb[1] * 255.).round() as u8,
                    (rgb[2] * 255.).round() as u8
                )
            },
        );
        div()
            .flex_col()
            .gap(4.)
            .child(
                div()
                    .flex_row()
                    .gap(8.)
                    .child(
                        Self::check_control("Histogram", mode == Scope::Histogram).on_click(
                            cx.listener("camera-scope-histogram", |this, cx| {
                                this.camera_raw.scope = Scope::Histogram;
                                cx.invalidate();
                            }),
                        ),
                    )
                    .child(
                        Self::check_control("Vectorscope", mode == Scope::Vectorscope).on_click(
                            cx.listener("camera-scope-vector", |this, cx| {
                                this.camera_raw.scope = Scope::Vectorscope;
                                cx.invalidate();
                            }),
                        ),
                    ),
            )
            .child(histogram)
            .child(text(readout).text_size(11.).line_height(14.))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn scopes_reuse_pixels_and_invalidate_on_replacement_or_copy_on_write() {
        let mut cache = Cache::default();
        let mut pixels = Arc::new(RgbaImage::from_pixel(2, 2, Rgba([255, 0, 0, 255])));
        let first = cache.data(Some(&pixels));
        assert_eq!(first.bins[0][255], 4);
        assert_eq!(first.scope.iter().sum::<f32>(), 4.);
        assert!(Arc::ptr_eq(&first, &cache.data(Some(&pixels))));
        Arc::make_mut(&mut pixels).put_pixel(0, 0, Rgba([0, 255, 0, 128]));
        let changed = cache.data(Some(&pixels));
        assert!(!Arc::ptr_eq(&first, &changed));
        assert_eq!(changed.bins[0][255], 3);
        assert_eq!(changed.bins[1][255], 1);
        assert!((changed.scope.iter().sum::<f32>() - (3. + 128. / 255.)).abs() < 1e-5);
        let replacement = Arc::new(RgbaImage::from_pixel(2, 2, Rgba([0, 0, 255, 255])));
        let replaced = cache.data(Some(&replacement));
        assert_eq!(replaced.bins[2][255], 4);
        assert!(!Arc::ptr_eq(&changed, &replaced));
        assert!(Arc::ptr_eq(&replaced, &cache.data(Some(&replacement))));
        let cleared = cache.data(None);
        assert_eq!(cleared.bins.iter().flatten().sum::<u32>(), 0);
        assert_eq!(cleared.scope.iter().sum::<f32>(), 0.);
        assert!(Arc::ptr_eq(&cleared, &cache.data(None)));
    }
}
