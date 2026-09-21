//! MagicWand.swift's point, 3 by 3 and 5 by 5 sampling choices.
use super::dropdown::Dropdown;
use super::*;
use quickgui::{PickerItem, SelectPopoverLayout};

fn label(radius: usize) -> &'static str {
    match radius {
        1 => "3 by 3 Average",
        2 => "5 by 5 Average",
        _ => "Point Sample",
    }
}
pub(super) fn new() -> Dropdown<usize> {
    let mut menu = Dropdown::new(
        (0..=2).map(|radius| PickerItem::new(label(radius), radius).id(label(radius))),
    )
    .expect("Wand sampling IDs are unique")
    .with_layout(SelectPopoverLayout::new(155., 24.).trigger_height(24.));
    menu.select_id(label(0));
    menu
}
impl Editor {
    pub(super) fn wand_sample_picker(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        self.tools.wand_picker.element(
            cx,
            "wand-sample",
            "Sample Size",
            |this| &mut this.tools.wand_picker,
            Self::tool_header_control(label(self.tools.wand_radius))
                .w(155.)
                .tooltip("Match the clicked pixel, or the average of the pixels around it")
                .flex_row()
                .items_center()
                .child(div().flex_1())
                .child(Icon::PopupChevron.element(14.)),
            |this, radius, cx| {
                this.tools.wand_radius = radius;
                cx.invalidate();
            },
        )
    }
}
