use super::data::EdgeBackgrounds;

use crate::{Canvas, Color, Rect};

/// Physical and logical dimensions of one terminal grid cell.
///
/// Ghostty receives physical pixel dimensions, so text, cursors, and synthesized glyphs must all
/// derive their logical geometry from those same rounded values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct CellMetrics {
    pub(super) logical_width: f32,
    pub(super) logical_height: f32,
    pub(super) physical_width: u32,
    pub(super) physical_height: u32,
    pub(super) scale_factor: f32,
}

impl CellMetrics {
    pub(super) fn new(
        font_size: f32,
        line_height: f32,
        cell_width_ratio: f32,
        scale_factor: f32,
    ) -> Self {
        let scale_factor = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        };
        let physical_width = ((font_size * cell_width_ratio * scale_factor)
            .round()
            .max(1.0)) as u32;
        let physical_height = ((line_height * scale_factor).round().max(1.0)) as u32;
        Self {
            logical_width: physical_width as f32 / scale_factor,
            logical_height: physical_height as f32 / scale_factor,
            physical_width,
            physical_height,
            scale_factor,
        }
    }
}

/// Extend terminal edge-cell backgrounds through the visual grid padding.
///
/// The grid itself remains inset, so shell text keeps its breathing room. Full-screen TUIs usually
/// paint every edge cell, causing their surface background to reach the terminal's inner border.
pub(super) fn paint_padding_extension(
    canvas: &mut Canvas<'_>,
    edges: &EdgeBackgrounds,
    metrics: CellMetrics,
    padding_top: f32,
    padding_left: f32,
) {
    for_each_padding_rect(
        canvas.bounds(),
        edges,
        metrics,
        padding_top,
        padding_left,
        |rect, color| canvas.fill_rect(rect, color),
    );
}

fn for_each_padding_rect(
    bounds: Rect,
    edges: &EdgeBackgrounds,
    metrics: CellMetrics,
    padding_top: f32,
    padding_left: f32,
    mut paint: impl FnMut(Rect, Color),
) {
    if bounds.is_empty() || edges.top.is_empty() || edges.left.is_empty() {
        return;
    }

    let grid_left = padding_left.clamp(0.0, bounds.width);
    let grid_top = padding_top.clamp(0.0, bounds.height);
    let grid_right =
        (grid_left + edges.top.len() as f32 * metrics.logical_width).clamp(grid_left, bounds.width);
    let grid_bottom = (grid_top + edges.left.len() as f32 * metrics.logical_height)
        .clamp(grid_top, bounds.height);

    paint_horizontal_runs(
        &edges.top,
        grid_left,
        0.0,
        metrics.logical_width,
        grid_right,
        grid_top,
        &mut paint,
    );
    paint_horizontal_runs(
        &edges.bottom,
        grid_left,
        grid_bottom,
        metrics.logical_width,
        grid_right,
        bounds.height - grid_bottom,
        &mut paint,
    );
    paint_vertical_runs(
        &edges.left,
        0.0,
        grid_top,
        grid_left,
        metrics.logical_height,
        grid_bottom,
        &mut paint,
    );
    paint_vertical_runs(
        &edges.right,
        grid_right,
        grid_top,
        bounds.width - grid_right,
        metrics.logical_height,
        grid_bottom,
        &mut paint,
    );

    let corners = [
        (
            Rect::new(0.0, 0.0, grid_left, grid_top),
            edges.top.first().copied().flatten(),
        ),
        (
            Rect::new(grid_right, 0.0, bounds.width - grid_right, grid_top),
            edges.top.last().copied().flatten(),
        ),
        (
            Rect::new(0.0, grid_bottom, grid_left, bounds.height - grid_bottom),
            edges.bottom.first().copied().flatten(),
        ),
        (
            Rect::new(
                grid_right,
                grid_bottom,
                bounds.width - grid_right,
                bounds.height - grid_bottom,
            ),
            edges.bottom.last().copied().flatten(),
        ),
    ];
    for (rect, color) in corners {
        if !rect.is_empty()
            && let Some(color) = color
        {
            paint(rect, color);
        }
    }
}

fn paint_horizontal_runs(
    colors: &[Option<Color>],
    origin_x: f32,
    y: f32,
    cell_width: f32,
    right: f32,
    height: f32,
    paint: &mut impl FnMut(Rect, Color),
) {
    if height <= 0.0 || right <= origin_x {
        return;
    }
    for_each_color_run(colors, |range, color| {
        let left = (origin_x + range.start as f32 * cell_width).min(right);
        let run_right = (origin_x + range.end as f32 * cell_width).min(right);
        if run_right > left {
            paint(Rect::new(left, y, run_right - left, height), color);
        }
    });
}

fn paint_vertical_runs(
    colors: &[Option<Color>],
    x: f32,
    origin_y: f32,
    width: f32,
    cell_height: f32,
    bottom: f32,
    paint: &mut impl FnMut(Rect, Color),
) {
    if width <= 0.0 || bottom <= origin_y {
        return;
    }
    for_each_color_run(colors, |range, color| {
        let top = (origin_y + range.start as f32 * cell_height).min(bottom);
        let run_bottom = (origin_y + range.end as f32 * cell_height).min(bottom);
        if run_bottom > top {
            paint(Rect::new(x, top, width, run_bottom - top), color);
        }
    });
}

fn for_each_color_run(
    colors: &[Option<Color>],
    mut paint: impl FnMut(std::ops::Range<usize>, Color),
) {
    let mut start = 0;
    while start < colors.len() {
        let color = colors[start];
        let mut end = start + 1;
        while end < colors.len() && colors[end] == color {
            end += 1;
        }
        if let Some(color) = color {
            paint(start..end, color);
        }
        start = end;
    }
}

/// Paint one Unicode Block Elements glyph using Ghostty's cell-fraction rules.
///
/// These characters intentionally bypass font rasterization: a full block must cover every pixel
/// in its terminal cell, and complementary halves overlap their shared pixel when a cell dimension
/// is odd. This is what keeps terminal art continuous across columns and rows.
pub(super) fn paint_block(
    canvas: &mut Canvas<'_>,
    column: u16,
    row: u16,
    character: char,
    metrics: CellMetrics,
    color: Color,
) {
    if color.a <= 0.0 {
        return;
    }
    let origin_x = u32::from(column).saturating_mul(metrics.physical_width);
    let origin_y = u32::from(row).saturating_mul(metrics.physical_height);
    for_each_block_rect(
        character,
        metrics.physical_width,
        metrics.physical_height,
        |rect| {
            let color = color.with_alpha(color.a * f32::from(rect.alpha) / 255.0);
            canvas.fill_rect(
                Rect::new(
                    (origin_x + rect.x) as f32 / metrics.scale_factor,
                    (origin_y + rect.y) as f32 / metrics.scale_factor,
                    rect.width as f32 / metrics.scale_factor,
                    rect.height as f32 / metrics.scale_factor,
                ),
                color,
            );
        },
    );
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BlockRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    alpha: u8,
}

fn for_each_block_rect(
    character: char,
    cell_width: u32,
    cell_height: u32,
    mut paint: impl FnMut(BlockRect),
) {
    let mut rect = |x, y, width, height, alpha| {
        if width > 0 && height > 0 && alpha > 0 {
            paint(BlockRect {
                x,
                y,
                width,
                height,
                alpha,
            });
        }
    };
    let codepoint = character as u32;
    match codepoint {
        0x2580 => {
            let height = rounded_fraction(cell_height, 1, 2);
            rect(0, 0, cell_width, height, 255);
        }
        0x2581..=0x2587 => {
            let height = rounded_fraction(cell_height, codepoint - 0x2580, 8);
            rect(
                0,
                cell_height.saturating_sub(height),
                cell_width,
                height,
                255,
            );
        }
        0x2588 => rect(0, 0, cell_width, cell_height, 255),
        0x2589..=0x258f => {
            let width = rounded_fraction(cell_width, 0x2590 - codepoint, 8);
            rect(0, 0, width, cell_height, 255);
        }
        0x2590 => {
            let width = rounded_fraction(cell_width, 1, 2);
            rect(cell_width.saturating_sub(width), 0, width, cell_height, 255);
        }
        0x2591..=0x2593 => rect(
            0,
            0,
            cell_width,
            cell_height,
            ((codepoint - 0x2590) * 0x40) as u8,
        ),
        0x2594 => {
            let height = rounded_fraction(cell_height, 1, 8);
            rect(0, 0, cell_width, height, 255);
        }
        0x2595 => {
            let width = rounded_fraction(cell_width, 1, 8);
            rect(cell_width.saturating_sub(width), 0, width, cell_height, 255);
        }
        0x2596..=0x259f => {
            let quadrants = match codepoint {
                0x2596 => 0b0100,
                0x2597 => 0b1000,
                0x2598 => 0b0001,
                0x2599 => 0b1101,
                0x259a => 0b1001,
                0x259b => 0b0111,
                0x259c => 0b1011,
                0x259d => 0b0010,
                0x259e => 0b0110,
                0x259f => 0b1110,
                _ => unreachable!(),
            };
            let half_width_max = rounded_fraction(cell_width, 1, 2);
            let half_height_max = rounded_fraction(cell_height, 1, 2);
            let half_width_min = cell_width.saturating_sub(half_width_max);
            let half_height_min = cell_height.saturating_sub(half_height_max);
            if quadrants & 0b0001 != 0 {
                rect(0, 0, half_width_max, half_height_max, 255);
            }
            if quadrants & 0b0010 != 0 {
                rect(half_width_min, 0, half_width_max, half_height_max, 255);
            }
            if quadrants & 0b0100 != 0 {
                rect(0, half_height_min, half_width_max, half_height_max, 255);
            }
            if quadrants & 0b1000 != 0 {
                rect(
                    half_width_min,
                    half_height_min,
                    half_width_max,
                    half_height_max,
                    255,
                );
            }
        }
        _ => {}
    }
}

fn rounded_fraction(size: u32, numerator: u32, denominator: u32) -> u32 {
    debug_assert!(denominator > 0);
    ((u64::from(size) * u64::from(numerator) + u64::from(denominator / 2)) / u64::from(denominator))
        as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_metrics_snap_to_device_pixels() {
        assert_eq!(
            CellMetrics::new(14.0, 20.5, 0.6, 2.0),
            CellMetrics {
                logical_width: 8.5,
                logical_height: 20.5,
                physical_width: 17,
                physical_height: 41,
                scale_factor: 2.0,
            }
        );
    }

    #[test]
    fn default_edge_backgrounds_do_not_add_padding_quads() {
        let edges = EdgeBackgrounds::empty(2, 2);
        let mut rects = Vec::new();
        for_each_padding_rect(
            Rect::new(0.0, 0.0, 40.0, 40.0),
            &edges,
            CellMetrics {
                logical_width: 10.0,
                logical_height: 10.0,
                physical_width: 10,
                physical_height: 10,
                scale_factor: 1.0,
            },
            8.0,
            8.0,
            |rect, color| rects.push((rect, color)),
        );

        assert!(rects.is_empty());
    }

    #[test]
    fn edge_backgrounds_extend_to_every_padding_edge() {
        let surface = Color::rgb8(9, 105, 218);
        let edges = EdgeBackgrounds::solid(2, 2, surface);
        let mut rects = Vec::new();
        for_each_padding_rect(
            Rect::new(0.0, 0.0, 40.0, 40.0),
            &edges,
            CellMetrics {
                logical_width: 10.0,
                logical_height: 10.0,
                physical_width: 10,
                physical_height: 10,
                scale_factor: 1.0,
            },
            8.0,
            8.0,
            |rect, color| rects.push((rect, color)),
        );

        assert_eq!(
            rects,
            [
                (Rect::new(8.0, 0.0, 20.0, 8.0), surface),
                (Rect::new(8.0, 28.0, 20.0, 12.0), surface),
                (Rect::new(0.0, 8.0, 8.0, 20.0), surface),
                (Rect::new(28.0, 8.0, 12.0, 20.0), surface),
                (Rect::new(0.0, 0.0, 8.0, 8.0), surface),
                (Rect::new(28.0, 0.0, 12.0, 8.0), surface),
                (Rect::new(0.0, 28.0, 8.0, 12.0), surface),
                (Rect::new(28.0, 28.0, 12.0, 12.0), surface),
            ]
        );
    }

    #[test]
    fn block_geometry_matches_ghostty_pixel_rounding() {
        let rects = |character| {
            let mut rects = Vec::new();
            for_each_block_rect(character, 17, 41, |rect| rects.push(rect));
            rects
        };

        assert_eq!(
            rects('█'),
            [BlockRect {
                x: 0,
                y: 0,
                width: 17,
                height: 41,
                alpha: 255,
            }]
        );
        assert_eq!(
            rects('▀'),
            [BlockRect {
                x: 0,
                y: 0,
                width: 17,
                height: 21,
                alpha: 255,
            }]
        );
        assert_eq!(
            rects('▄'),
            [BlockRect {
                x: 0,
                y: 20,
                width: 17,
                height: 21,
                alpha: 255,
            }]
        );
        assert_eq!(
            rects('▌'),
            [BlockRect {
                x: 0,
                y: 0,
                width: 9,
                height: 41,
                alpha: 255,
            }]
        );
        assert_eq!(
            rects('▐'),
            [BlockRect {
                x: 8,
                y: 0,
                width: 9,
                height: 41,
                alpha: 255,
            }]
        );
        assert_eq!(rects('░')[0].alpha, 0x40);
        assert_eq!(rects('▒')[0].alpha, 0x80);
        assert_eq!(rects('▓')[0].alpha, 0xc0);
        assert_eq!(rects('a'), []);
    }
}
