//! Screen-space gradient guides from TransformOverlay.swift.
use super::*;
use quickgui::{CustomShader, Rect, ShaderParameters};

pub(super) fn shader() -> Result<CustomShader> {
    CustomShader::new(include_str!("gradient_overlay.wgsl"))
        .map_err(|error| compositor::invalid(format!("Could not prepare gradient guides: {error}")))
}

impl Editor {
    pub(super) fn gradient_overlay(
        &self,
        zoom: f64,
        offset: compositor::geometry::Point,
        backing_scale: f64,
    ) -> Element {
        let mut overlay = div().absolute().size_full().accessibility_hidden(true);
        let Some(edit) = &self.pending_gradient else {
            return overlay;
        };
        if (edit.end[0] - edit.start[0]).hypot(edit.end[1] - edit.start[1]) < 0.5 {
            return overlay;
        }
        let points = [edit.start, edit.end].map(|point| {
            std::array::from_fn::<_, 2, _>(|axis| (offset[axis] + point[axis] * zoom) as f32)
        });
        let radial = self.tools.gradient.shape == compositor::gradient::Shape::Radial;
        let shader = self.gradient_overlay_shader.clone();
        overlay = overlay.child(
            // Shade only the visible surface, even when a zoomed radial guide
            // would exceed the path tessellator's coordinate or dash limits.
            quickgui::canvas(move |bounds, painter| {
                painter.paint_shader(
                    Rect::new(0., 0., bounds.width, bounds.height),
                    &shader,
                    ShaderParameters::new()
                        .vector(0, [points[0][0], points[0][1], points[1][0], points[1][1]])
                        .vector(
                            1,
                            [if radial { 1. } else { 0. }, backing_scale as f32, 0., 0.],
                        ),
                );
            })
            .absolute()
            .size_full(),
        );
        for (point, [r, g, b, a]) in points.into_iter().zip(self.gradient_preview_colors()) {
            // A centered 1-point stroke around the source's 12-point disk
            // occupies 13 points; its 7-point color center is inset by 3.
            overlay = overlay.child(
                div()
                    .absolute()
                    .size(13., 13.)
                    .rounded(6.5)
                    .bg(Color::WHITE)
                    .border(1., Color::BLACK)
                    .translate(point[0] - 6.5, point[1] - 6.5)
                    .child(
                        div()
                            .absolute()
                            .left(2.)
                            .top(2.)
                            .size(7., 7.)
                            .rounded(3.5)
                            .bg(Color::rgb8(191, 191, 191))
                            .child(div().size_full().rounded(3.5).bg(Color::rgba8(r, g, b, a))),
                    ),
            );
        }
        overlay
    }
}
