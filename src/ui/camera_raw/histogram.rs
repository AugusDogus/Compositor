use super::*;
#[derive(Clone, Copy, Default, PartialEq)]
pub(super) enum Scope {
    #[default]
    Histogram,
    Vectorscope,
}
impl Editor {
    pub(in crate::ui) fn camera_histogram(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let mut bins = [[0u32; 256]; 3];
        let mut scope = vec![0_f32; 64 * 64];
        if let Some(pixels) = self.camera_scope_pixels() {
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
        let mode = self.camera_raw.scope;
        let scope_peak = scope.iter().copied().fold(1_f32, f32::max);
        let histogram = quickgui::canvas(move |bounds, painter| {
            if mode == Scope::Vectorscope {
                let side = bounds.height.min(bounds.width);
                let left = (bounds.width - side) / 2.;
                for (i, value) in scope.iter().enumerate() {
                    if *value > 0. {
                        let alpha = ((*value / scope_peak).sqrt() * 255.).round() as u8;
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
                for (i, count) in bins[channel].iter().enumerate() {
                    let x = (i as f32 + 0.5) * bounds.width / 256.;
                    let height = (*count as f32 / maximum).sqrt() * bounds.height;
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
