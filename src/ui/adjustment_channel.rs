//! Keyboard-accessible range and channel menus shared by the adjustment sheets.
use super::*;
use compositor::adjustment::ColorRange;
use quickgui::{Popover, PopoverKind, PopoverMenu, PopoverMenuItem};

#[derive(Clone, Copy, PartialEq)]
struct Choose(usize);

impl Editor {
    pub(super) fn toggle_adjustment_channel(&mut self, cx: &mut EventContext) {
        let Some(edit) = &mut self.adjustment_edit else {
            return;
        };
        if edit.channel_popup {
            edit.channel_popup = false;
            cx.invalidate();
            return;
        }
        let (labels, selected): (&[&str], String) = match edit.settings.kind {
            Kind::HueSaturation => (
                &[
                    "Master", "Reds", "Yellows", "Greens", "Cyans", "Blues", "Magentas",
                ],
                format!(
                    "{:?}",
                    edit.settings
                        .hsv_settings
                        .as_ref()
                        .map_or(ColorRange::Master, |s| s.range)
                ),
            ),
            Kind::Levels => (
                &["RGB", "Red", "Green", "Blue"],
                format!("{:?}", edit.settings.levels.channel),
            ),
            Kind::Curves => (
                &["RGB", "Red", "Green", "Blue"],
                format!("{:?}", edit.settings.curves.channel),
            ),
            _ => return,
        };
        self.adjustment_channel_menu =
            PopoverMenu::new(labels.iter().enumerate().map(|(index, &label)| {
                PopoverMenuItem::radio(
                    format!("adjustment-channel-{index}"),
                    label,
                    "adjustment-channels",
                    selected == label,
                    Choose(index),
                )
                .close_on_activate(false)
            }))
            .expect("Adjustment channels have unique static IDs");
        if let Some(index) = labels.iter().position(|&label| label == selected) {
            self.adjustment_channel_menu.highlight(index);
        }
        edit.channel_popup = true;
        cx.focus(quickgui::FocusHandle::new("adjustment-channel-items"));
        cx.invalidate();
    }

    pub(super) fn adjustment_channel_popup(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let Some(edit) = &self.adjustment_edit else {
            return div();
        };
        if !edit.channel_popup {
            return div();
        }
        let width = if edit.settings.kind == Kind::HueSaturation {
            160.
        } else {
            126.
        };
        let content = self.adjustment_channel_menu.element(
            cx,
            "adjustment-channel-items",
            |this| &mut this.adjustment_channel_menu,
            div().w(width).flex_col().p(5.).gap(1.),
            |item, state| super::menus::style::choice(item.label().clone(), state),
            |this, cx| this.close_adjustment_channel(cx),
        );
        Popover::new("adjustment-channel", "adjustment-channel-popup", true)
            .kind(PopoverKind::Menu)
            .side(quickgui::AnchorSide::Bottom)
            .align(quickgui::AnchorAlign::Start)
            .initial_focus("adjustment-channel-items")
            .surface_with(
                super::menus::style::surface()
                    .child(content)
                    .on_action(cx.action_listener(
                        "adjustment-channel-popup",
                        |this, choice: &Choose, cx| {
                            let result = this.choose_adjustment_channel(choice.0);
                            this.result(result, cx);
                        },
                    ))
                    .on_dismiss(cx.dismiss_listener("adjustment-channel-popup", |this, cx| {
                        this.close_adjustment_channel(cx)
                    })),
            )
    }

    fn close_adjustment_channel(&mut self, cx: &mut EventContext) {
        if let Some(edit) = &mut self.adjustment_edit {
            edit.channel_popup = false;
        }
        cx.focus(quickgui::FocusHandle::new("adjustment-channel"));
        cx.invalidate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, Keystroke, WindowOptions};

    #[test]
    fn keyboard_channel_changes_commit_only_on_selection_and_escape_keeps_the_sheet() {
        let mut editor = Editor::with_test_document();
        compositor::edits::fill(
            &mut editor.session_mut().document,
            [128, 128, 128, 255],
            false,
            false,
        )
        .unwrap();
        editor.open_pixel_adjustment(Kind::Curves).unwrap();
        let (mut cx, view) = Application::new()
            .bind_keys(quickgui::popover_menu_key_bindings())
            .into_test_context(WindowOptions::new("Channels").size(1280., 900.), editor)
            .unwrap();
        let window = view.window_handle();
        cx.click(window, "adjustment-channel").unwrap();
        for key in [Key::ArrowDown, Key::Enter] {
            cx.simulate_keystroke(window, Keystroke::new(key, Modifiers::empty()))
                .unwrap();
        }
        cx.read(view, |e| {
            let edit = e.adjustment_edit.as_ref().unwrap();
            assert_eq!(
                edit.settings.curves.channel,
                compositor::adjustment::Channel::Red
            );
            assert!(!edit.channel_popup);
        })
        .unwrap();
        cx.click(window, "adjustment-channel").unwrap();
        for key in [Key::ArrowDown, Key::Escape] {
            cx.simulate_keystroke(window, Keystroke::new(key, Modifiers::empty()))
                .unwrap();
        }
        cx.read(view, |e| {
            let edit = e.adjustment_edit.as_ref().unwrap();
            assert_eq!(
                edit.settings.curves.channel,
                compositor::adjustment::Channel::Red
            );
            assert!(!edit.channel_popup);
            assert!(e.modal.is_some());
        })
        .unwrap();
        assert_eq!(
            cx.focused(window).unwrap(),
            Some(quickgui::ElementId::from("adjustment-channel"))
        );
    }
}
