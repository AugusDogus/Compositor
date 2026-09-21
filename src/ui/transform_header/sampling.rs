//! Source sampling menu, sharing the current transform transaction with numeric edits.
use super::*;
use crate::ui::dropdown::Dropdown;
use compositor::geometry::Sampling;
use quickgui::{PickerItem, SelectPopoverLayout, StateAccessor};

fn label(sampling: Sampling) -> &'static str {
    match sampling {
        Sampling::Nearest => "Nearest",
        Sampling::Smooth => "Smooth",
        Sampling::High => "High quality",
    }
}

pub(in crate::ui) fn new() -> Dropdown<Sampling> {
    let mut menu = Dropdown::new(
        [Sampling::Nearest, Sampling::Smooth, Sampling::High]
            .map(|value| PickerItem::new(label(value), value).id(label(value))),
    )
    .expect("Transform sampling IDs are unique")
    .with_layout(SelectPopoverLayout::new(150., 24.).trigger_height(24.));
    menu.select_id(label(Sampling::High));
    menu
}

impl Editor {
    pub(in crate::ui) fn sync_transform_sampling(&mut self) {
        if !self.has_document() {
            return;
        }
        let value = self
            .header_transform_bounds()
            .map_or(Sampling::High, |t| t.sampling);
        if self.tools.transform_sampling.selected_value() != Some(&value) {
            self.tools.transform_sampling.select_id(label(value));
        }
    }
    pub(super) fn transform_sampling_picker(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let value = self
            .header_transform_bounds()
            .map_or(Sampling::High, |t| t.sampling);
        let selector = self.tools.transform_sampling.element_with(
            cx,
            "transform-sampling",
            "Sampling",
            StateAccessor::new(|this: &mut Self| &mut this.tools.transform_sampling),
            Self::tool_header_control(label(value))
                .px(6.)
                .whitespace_nowrap()
                .flex_1()
                .min_w(0.)
                .flex_row()
                .items_center()
                .child(div().flex_1())
                .child(Icon::PopupChevron.element(14.).flex_shrink_0()),
            |this, value, cx| {
                if this.tools.tool != Tool::Move || this.modal.is_some() || !this.has_document() {
                    return;
                }
                let result = this.change_header_transform(|transform| transform.sampling = value);
                this.result(result, cx);
            },
        );
        div()
            .flex_row()
            .items_center()
            .gap(8.)
            .w(170.)
            .flex_shrink_0()
            .child(text("Sampling").text_size(12.).line_height(15.))
            .child(selector.disabled(
                !self.can_edit_transform_numbers() || self.header_transform_bounds().is_none(),
            ))
    }
}

#[cfg(test)]
#[path = "sampling_tests.rs"]
mod tests;
