//! Placement and title bars for Swift's three FloatingPanelController instances.
use super::*;
use quickgui::{Dialog, LayoutBoundsHandle, MouseButton, PointerPhase};

#[derive(Clone, Copy)]
pub(super) enum PanelKind {
    Levels,
    HueSaturation,
    Filter,
}

#[derive(Default)]
struct PanelPlacement {
    dragged: Option<[f32; 2]>,
    bounds: LayoutBoundsHandle,
}

impl PanelPlacement {
    fn position(&self) -> Option<[f32; 2]> {
        self.dragged
            .or_else(|| self.bounds.bounds().map(|bounds| [bounds.x, bounds.y]))
    }
}

#[derive(Default)]
pub(super) struct PanelPositions {
    levels: PanelPlacement,
    hue_saturation: PanelPlacement,
    filter: PanelPlacement,
}

impl PanelPositions {
    fn placement_mut(&mut self, kind: PanelKind) -> &mut PanelPlacement {
        match kind {
            PanelKind::Levels => &mut self.levels,
            PanelKind::HueSaturation => &mut self.hue_saturation,
            PanelKind::Filter => &mut self.filter,
        }
    }
    fn placement(&self, kind: PanelKind) -> &PanelPlacement {
        match kind {
            PanelKind::Levels => &self.levels,
            PanelKind::HueSaturation => &self.hue_saturation,
            PanelKind::Filter => &self.filter,
        }
    }
    pub(super) fn position(&self, kind: PanelKind) -> Option<[f32; 2]> {
        self.placement(kind).position()
    }
}

impl Editor {
    pub(super) fn mount_form(
        &mut self,
        cx: &mut ViewContext<'_, Self>,
        dialog: Dialog,
        mut contents: Element,
        width: f32,
        title: &'static str,
        panel_kind: Option<PanelKind>,
    ) -> Element {
        let available = cx.size();
        // Swift sizes the hosted content; the Linux frame adds a one-point border per side.
        let width = width + if panel_kind.is_some() { 2. } else { 0. };
        let panel_max_height = (available.height - 146.).max(200.);
        let dismiss = cx.dismiss_listener(dialog.popover_id(), |this, cx| {
            this.cancel_form(cx);
        });
        let root = if panel_kind.is_some() {
            // Only the panel blocks input. The rest of the editor and its menus remain reachable.
            div().id(dialog.root_id()).absolute().inset_0().size_full()
        } else {
            dialog.root()
        }
        .flex_row()
        .items_center()
        .justify_center();
        if let Some(kind) = panel_kind {
            contents = div()
                .report_bounds(self.panel_positions.placement(kind).bounds.clone())
                .w(width)
                .flex_shrink_0()
                .max_h(panel_max_height)
                .flex_col()
                .bg(Color::rgb8(45, 45, 45))
                .border(1., Color::rgb8(90, 90, 90))
                .rounded(8.)
                .shadow(super::surfaces::panel_shadow())
                .child(self.floating_panel_title(cx, dialog, kind, title))
                .child(contents.w_full().min_h(0.).rounded_b(7.));
        }
        let mut popup =
            dialog
                .popup_with(contents)
                .on_dismiss(dismiss)
                .on_key_down(
                    cx.key_down_listener(dialog.popover_id(), |this, event, cx| {
                        this.form_key(&event.key, event.modifiers, cx);
                    }),
                );
        if let Some(kind) = panel_kind {
            popup = popup.accessibility_modal(false).block_pointer();
            let mut positioner = div().absolute().flex_row();
            if let Some([x, y]) = self.panel_positions.position(kind) {
                // Measure against the window's available height, independently of the saved
                // position. Otherwise an offscreen position shrinks the panel and that clipped
                // measurement prevents it from recovering its full height on the next frame.
                let height = self
                    .panel_positions
                    .placement(kind)
                    .bounds
                    .bounds()
                    .map_or(panel_max_height, |bounds| {
                        bounds.height.min(panel_max_height)
                    });
                let top = y.clamp(0., (available.height - height - 12.).max(0.));
                positioner = positioner
                    .left(x.clamp(0., (available.width - width).max(0.)))
                    .top(top)
                    .w(width)
                    .h(panel_max_height)
                    .items_start();
            } else {
                // The canvas excludes the tool rail, layers, top chrome, and status bar.
                positioner = positioner
                    .left(56.)
                    .top(116.)
                    .w((available.width - 56. - self.panel_layout.width).max(1.))
                    .h((available.height - 146.).max(1.))
                    .items_center()
                    .justify_center();
            }
            popup = positioner.child(popup);
        }

        let root = if panel_kind.is_some() {
            root
        } else {
            root.child(dialog.backdrop().bg(Color::TRANSPARENT))
        };
        let root = root.child(popup);
        if panel_kind.is_some() {
            self.panel_activation
                .present(root.child(self.adjustment_channel_popup(cx)))
        } else {
            root
        }
    }

    pub(super) fn floating_panel_kind(&self) -> Option<PanelKind> {
        self.modal
            .as_ref()
            .and_then(|form| self.form_panel_kind(form))
    }

    pub(super) fn form_panel_kind(&self, form: &Form) -> Option<PanelKind> {
        if !matches!(
            form,
            Form::Edit {
                action: Action::EditAdjustment | Action::CameraRaw | Action::Filter(_) | Action::RemoveBackground,
                ..
            }
        ) {
            return None;
        }
        if let Some(edit) = &self.adjustment_edit {
            Some(match edit.settings.kind {
                Kind::Levels => PanelKind::Levels,
                Kind::HueSaturation => PanelKind::HueSaturation,
                _ => PanelKind::Filter,
            })
        } else {
            self.filter_edit.as_ref().map(|_| PanelKind::Filter)
        }
    }

    pub(super) fn floating_panel_title(
        &self,
        cx: &mut ViewContext<'_, Self>,
        dialog: Dialog,
        kind: PanelKind,
        title: &'static str,
    ) -> Element {
        dialog
            .title_with(
                div()
                    .h(28.)
                    .flex_shrink_0()
                    .px(8.)
                    .flex_row()
                    .items_center()
                    .bg(Color::rgb8(52, 52, 52))
                    .rounded_t(7.)
                    .cursor(quickgui::CursorStyle::OpenHand)
                    .child(
                        Icon::X
                            .button("Close panel")
                            .tab_index(-1)
                            .size(20., 20.)
                            .on_click(cx.listener("floating-panel-close", |this, cx| {
                                this.size_menus.close(cx);
                                this.cancel_adjustment();
                                this.cancel_filter();
                                this.modal = None;
                                cx.focus(quickgui::FocusHandle::new("workspace"));
                                this.changed(cx);
                            })),
                    )
                    .child(div().flex_1())
                    .child(text(title).text_size(12.).font_semibold())
                    .child(div().flex_1())
                    .child(div().w(20.)),
            )
            .on_pointer(
                cx.pointer_listener(dialog.title_id(), move |this, event, cx| {
                    if event.button != MouseButton::Left {
                        return;
                    }
                    let position = &mut this.panel_positions.placement_mut(kind).dragged;
                    match event.phase {
                        PointerPhase::Down => {
                            // The title begins immediately inside the panel's one-pixel border.
                            *position = Some([
                                event.position.x - event.local_position.x - 1.,
                                event.position.y - event.local_position.y - 1.,
                            ]);
                        }
                        PointerPhase::Move => {
                            if let Some(position) = position {
                                position[0] = (position[0] + event.delta.x).max(0.);
                                position[1] = (position[1] + event.delta.y).max(0.);
                                cx.invalidate();
                            }
                        }
                        _ => {}
                    }
                }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, Point, WindowOptions};

    #[test]
    fn tool_panels_center_on_the_canvas_and_remember_separate_drag_positions() {
        let mut editor = Editor::with_test_document();
        editor.tabs[0].set_document(Document::new(20, 20).unwrap(), None);
        compositor::edits::fill(
            &mut editor.session_mut().document,
            [50, 120, 200, 255],
            false,
            false,
        )
        .unwrap();
        let original = editor.session().document.clone();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Panel placement").size(1500., 1000.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        let dialog = Dialog::new("editor-dialog", true);
        let mut moved = Vec::new();
        for (index, kind) in [Kind::Levels, Kind::HueSaturation, Kind::Exposure]
            .into_iter()
            .enumerate()
        {
            cx.update(view, |e, cx| e.action(Action::AdjustPixels(kind), cx))
                .unwrap();
            let popup = cx.element_bounds(window, dialog.popover_id()).unwrap();
            let canvas = cx.element_bounds(window, "canvas").unwrap();
            assert!((popup.x + popup.width / 2. - canvas.x - canvas.width / 2.).abs() < 1.);
            assert!(
                (popup.y + popup.height / 2. - canvas.y - canvas.height / 2.).abs() < 1.,
                "index={index}, popup={popup:?}, canvas={canvas:?}"
            );
            let title = cx.element_bounds(window, dialog.title_id()).unwrap();
            let content_width = match kind {
                Kind::Levels => 440.,
                Kind::HueSaturation => 460.,
                Kind::Exposure => 380.,
                _ => unreachable!(),
            };
            assert_eq!(
                title.width, content_width,
                "Panel border consumed content width"
            );
            let from = Point::new(title.x + title.width / 2., title.y + 10.);
            let delta = [(index + 1) as f32 * 20., 15.];
            cx.simulate_pointer_drag(
                window,
                dialog.title_id(),
                from,
                Point::new(from.x + delta[0], from.y + delta[1]),
            )
            .unwrap();
            let after = cx.element_bounds(window, dialog.popover_id()).unwrap();
            assert!((after.x - popup.x - delta[0]).abs() < 1.);
            assert!((after.y - popup.y - delta[1]).abs() < 1.);
            moved.push([after.x, after.y]);
            cx.click(window, "floating-panel-close").unwrap();
            cx.read(view, |e| {
                assert!(e.modal.is_none());
                assert_eq!(e.session().document, original);
                assert!(e.session().undo_label().is_none());
            })
            .unwrap();
        }
        for (kind, position) in [Kind::Levels, Kind::HueSaturation, Kind::Curves]
            .into_iter()
            .zip(moved)
        {
            cx.update(view, |e, cx| e.action(Action::AdjustPixels(kind), cx))
                .unwrap();
            let popup = cx.element_bounds(window, dialog.popover_id()).unwrap();
            assert!((popup.x - position[0]).abs() < 1.);
            assert!((popup.y - position[1]).abs() < 1.);
            cx.click(window, "floating-panel-close").unwrap();
        }
        cx.update(view, |e, cx| e.action(Action::CanvasSize, cx))
            .unwrap();
        let popup = cx.element_bounds(window, dialog.popover_id()).unwrap();
        assert!((popup.x + popup.width / 2. - 750.).abs() < 1.);
        assert!(cx.element_bounds(window, "floating-panel-close").is_err());
    }
    #[test]
    fn first_panel_position_survives_reopen_resize_and_filter_content_changes() {
        let mut editor = Editor::with_test_document();
        editor.tabs[0].set_document(Document::new(20, 20).unwrap(), None);
        compositor::edits::fill(
            &mut editor.session_mut().document,
            [50, 120, 200, 255],
            false,
            false,
        )
        .unwrap();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Remember first placement").size(1500., 1000.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        let dialog = Dialog::new("editor-dialog", true);
        cx.update(view, |e, cx| {
            e.action(Action::AdjustPixels(Kind::Exposure), cx)
        })
        .unwrap();
        let first = cx.element_bounds(window, dialog.popover_id()).unwrap();
        cx.simulate_window_resize(window, quickgui::Size::new(1600., 1100.))
            .unwrap();
        let resized = cx.element_bounds(window, dialog.popover_id()).unwrap();
        assert_eq!([first.x, first.y], [resized.x, resized.y]);
        cx.click(window, "floating-panel-close").unwrap();
        cx.update(view, |e, cx| {
            e.action(Action::AdjustPixels(Kind::Curves), cx)
        })
        .unwrap();
        let reopened = cx.element_bounds(window, dialog.popover_id()).unwrap();
        assert_eq!([first.x, first.y], [reopened.x, reopened.y]);
        assert_ne!(first.height, reopened.height);
    }

    #[test]
    fn resizing_and_dragging_keep_the_full_panel_visible_when_it_fits() {
        for kind in [Kind::Levels, Kind::HueSaturation, Kind::Exposure] {
            let mut editor = Editor::with_test_document();
            editor.tabs[0].set_document(Document::new(20, 20).unwrap(), None);
            compositor::edits::fill(
                &mut editor.session_mut().document,
                [50, 120, 200, 255],
                false,
                false,
            )
            .unwrap();
            let original = editor.session().document.clone();
            let (mut cx, view) = Application::new()
                .font(crate::UI_FONT)
                .into_test_context(
                    WindowOptions::new("Panel resize").size(3840., 2160.),
                    editor,
                )
                .unwrap();
            let window = view.window_handle();
            let dialog = Dialog::new("editor-dialog", true);
            cx.update(view, |e, cx| e.action(Action::AdjustPixels(kind), cx))
                .unwrap();
            let initial = cx.element_bounds(window, dialog.popover_id()).unwrap();
            for (width, height) in [(1500., 900.), (800., 520.), (1500., 900.)] {
                cx.simulate_window_resize(window, quickgui::Size::new(width, height))
                    .unwrap();
                let popup = cx.element_bounds(window, dialog.popover_id()).unwrap();
                assert!(popup.x >= 0. && popup.y >= 0.);
                assert!(popup.right() <= width && popup.bottom() <= height);
                if height == 900. {
                    assert!(
                        (popup.height - initial.height).abs() < 1.,
                        "{kind:?} shrank despite available room: {initial:?} -> {popup:?}"
                    );
                }
            }
            let title = cx.element_bounds(window, dialog.title_id()).unwrap();
            let from = Point::new(title.x + title.width / 2., title.y + 10.);
            cx.simulate_pointer_drag(window, dialog.title_id(), from, Point::new(1490., 890.))
                .unwrap();
            let moved = cx.element_bounds(window, dialog.popover_id()).unwrap();
            assert!(moved.right() <= 1500. && moved.bottom() <= 900.);
            assert!((moved.height - initial.height).abs() < 1.);
            cx.click(window, "floating-panel-close").unwrap();
            cx.read(view, |e| {
                assert!(e.modal.is_none());
                assert_eq!(e.session().document, original);
                assert!(e.session().undo_label().is_none());
            })
            .unwrap();
        }
    }

    #[test]
    fn panel_allows_menu_invert_and_view_commands_without_dismissing_the_preview() {
        let mut editor = Editor::with_test_document();
        editor.tabs[0].set_document(Document::new(20, 20).unwrap(), None);
        compositor::edits::fill(
            &mut editor.session_mut().document,
            [50, 120, 200, 255],
            false,
            false,
        )
        .unwrap();
        let original = editor.session().document.clone();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Non-modal menus").size(1500., 1000.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        cx.update(view, |e, cx| {
            e.action(Action::AdjustPixels(Kind::Levels), cx)
        })
        .unwrap();
        cx.click(
            window,
            quickgui::Menubar::new("application-menu").item_id(4),
        )
        .unwrap();
        let invert = cx.read(view, |e| e.menus.command_id("Invert")).unwrap();
        cx.click(window, invert).unwrap();
        let inverted = cx
            .read(view, |e| {
                assert!(e.adjustment_edit.is_some());
                assert_eq!(e.session().undo_label(), Some("Invert"));
                assert_ne!(e.session().document, original);
                e.session().document.clone()
            })
            .unwrap();
        cx.simulate_keystrokes(window, "ctrl-1").unwrap();
        cx.read(view, |e| {
            assert_eq!(e.session().zoom, 1.);
            assert!(e.adjustment_edit.is_some());
            assert_eq!(e.session().document, inverted);
        })
        .unwrap();
        cx.simulate_keystrokes(window, "alt-e").unwrap();
        assert!(cx.element_bounds(window, "application-menu-popup").is_ok());
        cx.simulate_keystrokes(window, "escape").unwrap();
        assert!(cx.read(view, |e| e.adjustment_edit.is_some()).unwrap());
        cx.click(window, "floating-panel-close").unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            inverted
        );
        cx.simulate_keystrokes(window, "ctrl-z").unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            original
        );
    }

    #[test]
    fn levels_blocks_project_commands_and_tool_changes_while_hue_keeps_its_preview_when_switching_tools()
     {
        let mut editor = Editor::with_test_document();
        editor.tabs[0].set_document(Document::new(20, 20).unwrap(), None);
        compositor::edits::fill(
            &mut editor.session_mut().document,
            [50, 120, 200, 255],
            false,
            false,
        )
        .unwrap();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Panel command boundaries").size(1500., 1000.),
                editor,
            )
            .unwrap();
        cx.update(view, |e, cx| {
            e.action(Action::AdjustPixels(Kind::Levels), cx);
            let tool = e.tools.tool;
            let doc = e.session().document.clone();
            for action in [
                Action::Save,
                Action::CanvasSize,
                Action::ImageSize,
                Action::Import,
                Action::FlipCanvasX,
            ] {
                assert!(!e.action_available(action));
                e.action(action, cx);
                assert_eq!(e.session().document, doc);
                assert!(e.adjustment_edit.is_some());
            }
            e.select_tool(Tool::Brush, cx);
            assert_eq!(e.tools.tool, tool);
            e.cancel_adjustment();
            e.modal = None;
            e.action(Action::AdjustPixels(Kind::HueSaturation), cx);
            if let Some(Form::Edit { fields, .. }) = &mut e.modal {
                fields[0].1 = "90".into();
            }
            e.preview_adjustment().unwrap();
            let preview = e.session().document.clone();
            e.select_tool(Tool::Brush, cx);
            assert_eq!(e.tools.tool, Tool::Brush);
            assert!(e.adjustment_edit.is_some());
            assert!(e.session().has_pending_edit());
            assert_eq!(e.session().document, preview);
            e.cancel_adjustment();
            assert_eq!(e.session().document, doc);
        })
        .unwrap();
    }

    #[test]
    fn panel_title_and_close_remain_visible_at_the_minimum_window_size() {
        let mut editor = Editor::with_test_document();
        editor.tabs[0].set_document(Document::new(20, 20).unwrap(), None);
        compositor::edits::fill(
            &mut editor.session_mut().document,
            [50, 120, 200, 255],
            false,
            false,
        )
        .unwrap();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Small tool panel").size(800., 520.),
                editor,
            )
            .unwrap();
        for kind in [Kind::Levels, Kind::HueSaturation] {
            cx.update(view, |e, cx| e.action(Action::AdjustPixels(kind), cx))
                .unwrap();
            let popup = cx
                .element_bounds(
                    view.window_handle(),
                    Dialog::new("editor-dialog", true).popover_id(),
                )
                .unwrap();
            let close = cx
                .element_bounds(view.window_handle(), "floating-panel-close")
                .unwrap();
            assert!(popup.x >= 0. && popup.y >= 0.);
            assert!(
                popup.x + popup.width <= 800. && popup.y + popup.height <= 520.,
                "Panel outside window: {popup:?}"
            );
            assert!(close.y >= popup.y && close.y + close.height <= popup.y + popup.height);
            cx.click(view.window_handle(), "floating-panel-close")
                .unwrap();
            assert!(cx.read(view, |e| e.modal.is_none()).unwrap());
        }
    }
}
