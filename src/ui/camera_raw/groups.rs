use super::*;
use crate::ui::dropdown::Dropdown;
use quickgui::{PickerItem, SelectPopoverLayout};
pub(super) fn menu() -> Dropdown<Group> {
    let mut menu = Dropdown::new(
        Group::ALL.map(|group| PickerItem::new(group.name(), group).id(group.name())),
    )
    .expect("Camera Raw group names are unique")
    .with_layout(SelectPopoverLayout::new(220., 26.).trigger_height(28.));
    menu.select_id(Group::Light.name());
    menu
}
impl Editor {
    pub(super) fn camera_group_picker(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let trigger = Self::control(self.camera_raw.group.name())
            .w_full()
            .flex_row()
            .items_center()
            .child(div().flex_1())
            .child(Icon::PopupChevron.element(14.));
        self.camera_raw.groups.element(
            cx,
            "camera-group",
            "Camera Raw group",
            |this| &mut this.camera_raw.groups,
            trigger,
            |this, group, cx| {
                this.camera_change(|edit| {
                    edit.group = group;
                    edit.tool = super::pointer::Tool::None;
                    edit.drag = None;
                });
                this.camera_raw
                    .groups
                    .select_id(this.camera_raw.group.name());
                this.changed(cx);
            },
        )
    }
}
