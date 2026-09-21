use super::*;
use compositor::geometry::Point;

impl Tool {
    pub(super) fn has_brush_cursor(self) -> bool {
        matches!(
            self,
            Self::Brush
                | Self::Erase
                | Self::Clone
                | Self::Heal
                | Self::Blur
                | Self::Smudge
                | Self::Liquify
        )
    }
}

impl Editor {
    pub(super) fn brush_cursor(&self, zoom: f64, offset: Point) -> Result<Element> {
        let mut overlay = div().absolute().size_full();
        if !self.tools.tool.has_brush_cursor()
            || self.pending
            || self.modal.is_some()
            || self.space_pan
            || matches!(self.gesture, Some(Gesture::Sample))
            || matches!(
                self.canvas_cursor(zoom, offset),
                super::tool_cursor::Cursor::Image(super::cursor_art::Glyph::Eyedropper)
            )
        {
            return Ok(overlay);
        }
        let (center, hardness) = match self.gesture {
            Some(Gesture::BrushTip(drag)) => (
                Some(drag.anchor),
                (drag.adjustment == super::brush_tip::Adjustment::Hardness)
                    .then_some(self.tools.brush.hardness),
            ),
            _ => (self.canvas_pointer, None),
        };
        let Some(center) = center else {
            return Ok(overlay);
        };
        let point = [
            (center[0] - offset[0]) / zoom,
            (center[1] - offset[1]) / zoom,
        ];
        let diameter = (self.tools.brush.diameter * zoom).max(1.) as f32;
        if let Some(preview) = self.clone_preview_image(zoom, offset) {
            overlay = overlay.child(
                div()
                    .absolute()
                    .translate(
                        center[0] as f32 - diameter / 2.,
                        center[1] as f32 - diameter / 2.,
                    )
                    .size(diameter, diameter)
                    .rounded(diameter / 2.)
                    .overflow_hidden()
                    .opacity(self.tools.brush.opacity as f32)
                    .child(quickgui::img(preview).size_full()),
            );
        }
        let mut strokes = Vec::new();
        for (width, color) in [(2.5, Color::WHITE), (1., Color::BLACK)] {
            strokes.push((
                circle(center, diameter, width, false).map_err(cursor_error)?,
                color,
            ));
            if let Some(hardness) = hardness.filter(|hardness| *hardness > 0.) {
                strokes.push((
                    circle(center, diameter * hardness as f32, width, true)
                        .map_err(cursor_error)?,
                    color,
                ));
            }
        }
        if self.tools.tool == Tool::Clone
            && let Some(source) = self.clone_cursor_source(point)
        {
            let center = quickgui::Point::new(
                (offset[0] + source[0] * zoom) as f32,
                (offset[1] + source[1] * zoom) as f32,
            );
            for (width, color) in [(3., Color::WHITE), (1., Color::BLACK)] {
                let mut path =
                    quickgui::PathBuilder::stroke(width).with_style(quickgui::PathStyle::Stroke(
                        quickgui::StrokeOptions::default()
                            .with_line_width(width)
                            .with_line_cap(quickgui::LineCap::Round),
                    ));
                path.move_to(quickgui::Point::new(center.x - 7., center.y));
                path.line_to(quickgui::Point::new(center.x + 7., center.y));
                path.move_to(quickgui::Point::new(center.x, center.y - 7.));
                path.line_to(quickgui::Point::new(center.x, center.y + 7.));
                strokes.push((path.build().map_err(cursor_error)?, color));
            }
        }
        overlay = overlay.child(
            quickgui::canvas(move |_, painter| {
                for (path, color) in &strokes {
                    painter.paint_path(path, *color);
                }
            })
            .absolute()
            .size_full(),
        );
        Ok(overlay)
    }

    pub(super) fn clone_cursor_source(&self, point: Point) -> Option<Point> {
        if (self.tools.clone_aligned || matches!(self.gesture, Some(Gesture::Paint { .. })))
            && let Some(delta) = self.tools.clone_offset
        {
            Some([point[0] + delta[0], point[1] + delta[1]])
        } else {
            self.tools.clone_source
        }
    }
}

fn cursor_error(error: quickgui::PathError) -> compositor::Error {
    compositor::invalid(format!(
        "Could not draw the brush cursor: {error}. The document is preserved; reduce the brush size or zoom out and try again."
    ))
}

fn circle(
    center: Point,
    diameter: f32,
    width: f32,
    dashed: bool,
) -> std::result::Result<quickgui::Path, quickgui::PathError> {
    let mut path = quickgui::PathBuilder::stroke(width);
    if dashed {
        path = path.dash_array(&[4., 3.]);
    }
    let radius = diameter / 2.;
    let right = quickgui::Point::new(center[0] as f32 + radius, center[1] as f32);
    let left = quickgui::Point::new(center[0] as f32 - radius, center[1] as f32);
    let radii = quickgui::Size::new(radius, radius);
    path.move_to(right);
    path.arc_to(radii, 0., false, true, left);
    path.arc_to(radii, 0., false, true, right);
    path.close();
    path.build()
}

#[cfg(test)]
#[path = "brush_cursor_tests.rs"]
mod tests;
