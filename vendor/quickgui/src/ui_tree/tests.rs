use super::*;
use crate::element::InputConstraints;
use crate::{
    AnimatedImageFrame, Animation, AnimationExt, AnimationRepeat, FocusHandle, Image, IntoElement,
    PathBuilder, SpringAnimation, Transition, button, container_query, div, form, img, text,
    text_input,
};

thread_local! {
    static RECORDED_TEXT_WIDTHS: std::cell::RefCell<Option<Vec<Option<f32>>>> =
        const { std::cell::RefCell::new(None) };
    static RECORDED_TEXT_IDS: std::cell::RefCell<Option<Vec<TextId>>> =
        const { std::cell::RefCell::new(None) };
}

struct TestTextLayout;

impl TextLayoutEngine for TestTextLayout {
    fn measure_text(
        &mut self,
        id: TextId,
        content: &Arc<str>,
        style: &TextStyle,
        max_width: Option<f32>,
        _scale_factor: f32,
    ) -> Size {
        RECORDED_TEXT_IDS.with(|recording| {
            if let Some(ids) = recording.borrow_mut().as_mut() {
                ids.push(id);
            }
        });
        RECORDED_TEXT_WIDTHS.with(|recording| {
            if let Some(widths) = recording.borrow_mut().as_mut() {
                widths.push(max_width);
            }
        });
        let natural_width = content.chars().count() as f32 * style.font_size * 0.5;
        Size::new(
            max_width.map_or(natural_width, |width| natural_width.min(width)),
            style.line_height,
        )
    }

    fn measure_styled_text(
        &mut self,
        id: TextId,
        content: &Arc<str>,
        style: &TextStyle,
        _highlights: &Arc<[TextHighlight]>,
        max_width: Option<f32>,
        scale_factor: f32,
    ) -> Size {
        self.measure_text(id, content, style, max_width, scale_factor)
    }

    fn text_geometry(
        &mut self,
        _id: TextId,
        _content: &Arc<str>,
        _style: &TextStyle,
        _highlights: Option<&Arc<[TextHighlight]>>,
        _width: f32,
        _scale_factor: f32,
        _visible_y: std::ops::Range<f32>,
    ) -> crate::renderer::StyledTextGeometry {
        crate::renderer::StyledTextGeometry {
            backgrounds: Vec::new(),
            decorations: Vec::new(),
        }
    }

    fn text_caret_position_with_highlights(
        &mut self,
        _id: TextId,
        _content: &Arc<str>,
        _style: &TextStyle,
        _highlights: Option<&Arc<[TextHighlight]>>,
        _width: f32,
        _scale_factor: f32,
        _index: usize,
    ) -> Point {
        Point::ZERO
    }

    fn text_index_for_point_with_highlights(
        &mut self,
        _id: TextId,
        _content: &Arc<str>,
        _style: &TextStyle,
        _highlights: Option<&Arc<[TextHighlight]>>,
        _width: f32,
        _scale_factor: f32,
        _point: Point,
    ) -> usize {
        0
    }

    fn text_selection_rects_with_highlights(
        &mut self,
        _id: TextId,
        _content: &Arc<str>,
        _style: &TextStyle,
        _highlights: Option<&Arc<[TextHighlight]>>,
        _width: f32,
        _scale_factor: f32,
        _visible_y: std::ops::Range<f32>,
        _start: usize,
        _end: usize,
    ) -> Vec<Rect> {
        Vec::new()
    }
}

fn playback_animation(repeat: AnimationRepeat) -> AnimatedImage {
    AnimatedImage::with_repeat(
        [
            AnimatedImageFrame::new(
                Image::from_rgba(1, 1, vec![1, 0, 0, 255]).unwrap(),
                Duration::from_millis(40),
            ),
            AnimatedImageFrame::new(
                Image::from_rgba(1, 1, vec![2, 0, 0, 255]).unwrap(),
                Duration::from_millis(60),
            ),
        ],
        repeat,
    )
    .unwrap()
}

fn assign_runtime_ids(element: &mut Element) {
    if let Some(id) = element.explicit_id {
        element.runtime_id = id;
    }
    for child in &mut element.children {
        assign_runtime_ids(child);
    }
}

mod direction_sticky_snap;
mod drag_selection;
mod focus_accessibility;
mod layout_invalidation;
mod layout_motion;
mod pointer_scroll;
mod state_styles;
mod text_services;
