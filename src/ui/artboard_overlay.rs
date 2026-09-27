use super::*;
use compositor::{
    document::LayerContent,
    geometry::{Point, Transform},
    invalid,
};
pub(super) fn label(frame: Transform, zoom: f64, offset: Point) -> quickgui::Rect {
    quickgui::Rect::new(
        (frame.origin[0] * zoom + offset[0]) as f32,
        (frame.origin[1] * zoom + offset[1] - 24.) as f32,
        (frame.size[0] * zoom).clamp(30., 180.) as f32,
        22.,
    )
}
impl Editor {
    pub(super) fn artboard_overlay(
        &self,
        zoom: f64,
        offset: Point,
        size: [f32; 2],
    ) -> Result<Element> {
        let mut overlay = div()
            .absolute()
            .size_full()
            .accessibility_hidden(true)
            .overflow_hidden();
        let mut segments = Vec::new();
        for layer in self
            .session()
            .document
            .layers
            .iter()
            .filter(|l| l.visible && matches!(l.content, LayerContent::Artboard(_)))
        {
            let rect = label(layer.transform, zoom, offset);
            if rect.x + rect.width >= 0.
                && rect.y + rect.height >= 0.
                && rect.x <= size[0]
                && rect.y <= size[1]
            {
                overlay = overlay.child(
                    text(layer.name.clone())
                        .absolute()
                        .left(rect.x)
                        .top(rect.y)
                        .w(rect.width)
                        .h(rect.height)
                        .px(4.)
                        .text_size(11.)
                        .line_height(20.)
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .bg(self.colors.neutral(38))
                        .text_color(if self.session().document.selected.contains(&layer.id) {
                            self.colors.accent()
                        } else {
                            self.colors.neutral(200)
                        }),
                );
            }
            let points = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]].map(|p| {
                let p = layer.transform.point(p);
                [p[0] * zoom + offset[0], p[1] * zoom + offset[1]]
            });
            for i in 0..4 {
                let a = points[i];
                let b = points[(i + 1) % 4];
                if let Some((s, e)) = super::selection_paths::clip_segment(a, b, size) {
                    let at = |t: f64| {
                        quickgui::Point::new(
                            (a[0] + (b[0] - a[0]) * t) as f32,
                            (a[1] + (b[1] - a[1]) * t) as f32,
                        )
                    };
                    segments.push([at(s), at(e)]);
                }
            }
        }
        // Each edge uses two commands. Bound each mesh independently of the
        // document's 10,000-layer limit.
        let paths: Result<Vec<_>> = segments
            .chunks(8192)
            .map(|chunk| {
                let mut outline = quickgui::PathBuilder::stroke(1.);
                for &[start, end] in chunk {
                    outline.move_to(start);
                    outline.line_to(end);
                }
                outline.build().map_err(|error| invalid(format!(
                "Artboard frames could not be drawn: {error}. Saved artboards are unchanged."
            )))
            })
            .collect();
        let paths = paths?;
        Ok(overlay.child(
            quickgui::canvas(move |_, painter| {
                for path in &paths {
                    painter.paint_path(path, Color::rgb8(100, 105, 115));
                }
            })
            .absolute()
            .size_full(),
        ))
    }
}
