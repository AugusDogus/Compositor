//! Layer-panel availability and shared layer transactions.
use super::*;

impl Action {
    pub(super) fn requires_layer_edit(self) -> bool {
        matches!(
            self,
            Self::AddLayer
                | Self::Duplicate
                | Self::DeleteLayer
                | Self::Group
                | Self::Ungroup
                | Self::MoveOutOfGroup
                | Self::Merge
                | Self::Raise
                | Self::Lower
                | Self::Rename
                | Self::Transform
                | Self::AddMask
                | Self::HideMask
                | Self::LinkMask
                | Self::ToggleMask
                | Self::DeleteMask
                | Self::Clip
                | Self::FlipX
                | Self::FlipY
                | Self::FlipCanvasX
                | Self::FlipCanvasY
                | Self::Blend
                | Self::Adjustment(_)
                | Self::EditAdjustment
        )
    }
}

impl Editor {
    pub(super) fn toggle_clipping(&mut self, target: uuid::Uuid) -> Result<()> {
        let label = match compositor::clipping::change(&self.session().document, target) {
            Some(compositor::clipping::Change::Create(_)) => "Create Clipping Mask",
            Some(compositor::clipping::Change::Release) => "Release Clipping Mask",
            None => {
                return Err(compositor::invalid(
                    "This layer cannot form a clipping mask. Place it directly above a pixel layer or an existing clipping stack. The document is unchanged.",
                ));
            }
        };
        self.session_mut()
            .edit(label, |doc| compositor::clipping::toggle(doc, target))
    }

    pub(super) fn toggle_layer_visibility(&mut self, target: uuid::Uuid) -> Result<()> {
        let layer = self.session().document.layer(target).ok_or_else(|| {
            compositor::invalid(
                "The layer was removed. No visibility changed. Choose an existing layer.",
            )
        })?;
        let visible = !layer.visible;
        self.session_mut().edit(if visible { "Show Layer" } else { "Hide Layer" }, |doc| {
            let layer = doc.layers.iter_mut().find(|layer| layer.id == target).ok_or_else(|| {
                compositor::invalid("The layer was removed. No visibility changed. Choose an existing layer.")
            })?;
            layer.visible = visible;
            Ok(())
        })
    }

    pub(super) fn can_duplicate_layer(&self) -> bool {
        if !self.can_edit_layers() {
            return false;
        }
        let doc = &self.session().document;
        doc.active_layer().is_some() && (doc.selection.is_none() || self.can_copy_pixels())
    }

    pub(super) fn can_receive_tab_layers(&self, index: usize) -> bool {
        self.tabs.get(index).is_some_and(|tab| {
            if tab.session().is_none() {
                true
            } else if index == self.current {
                self.can_edit_layers()
            } else {
                // Other blocking edits cannot be parked when switching projects.
                tab.parked_tools.pending_crop.is_none()
            }
        })
    }

    pub(super) fn can_edit_layers(&self) -> bool {
        self.develop.is_none()
            && self.layout_drag.is_none()
            && self.psd_conversion.is_none()
            && self.has_document()
            && !self.pending
            && self.gesture.is_none()
            && self.rename.is_none()
            && self.transform_edit.is_none()
            && self.pending_pixels.is_none()
            && self.tools.pending_crop.is_none()
            && self.pending_gradient.is_none()
            // Blend is an anchored preview menu, not a blocking edit sheet.
            && matches!(self.modal, None | Some(Form::Blend))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compositor::geometry::Transform;
    use quickgui::{Application, WindowOptions};

    #[test]
    fn pressing_the_current_layer_preserves_its_transform_until_selection_changes() {
        let mut editor = Editor::with_test_document();
        let mut doc = Document::new(20, 20).unwrap();
        compositor::edits::fill(&mut doc, [50, 120, 200, 255], false, false).unwrap();
        let first = doc.active.unwrap();
        doc.add(compositor::document::Layer::blank("Second", 20, 20))
            .unwrap();
        doc.select(first, false);
        editor.tabs = vec![Session::new(doc, None).into()];
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Layer selection").size(1500., 900.),
                editor,
            )
            .unwrap();
        cx.update(view, |e, cx| {
            e.start_toolbar_transform().unwrap();
            e.changed(cx);
        })
        .unwrap();
        for id in [
            quickgui::ElementId::from(1001_u64),
            format!("layer-thumbnail-{first}-false").into(),
        ] {
            cx.simulate_mouse_down(
                view.window_handle(),
                id,
                quickgui::MouseDownEvent {
                    button: quickgui::MouseButton::Left,
                    position: quickgui::Point::new(1395., 282.),
                    modifiers: Modifiers::empty(),
                    click_count: 1,
                    first_mouse: false,
                },
            )
            .unwrap();
            cx.click(view.window_handle(), 1001_u64).unwrap();
            cx.read(view, |e| {
                assert!(
                    e.transform_edit.is_some(),
                    "Pressing the active target committed its draft"
                );
                assert_eq!(e.session().document.active, Some(first));
            })
            .unwrap();
        }
        cx.simulate_mouse_down(
            view.window_handle(),
            1004_u64,
            quickgui::MouseDownEvent {
                button: quickgui::MouseButton::Left,
                position: quickgui::Point::new(1395., 334.),
                modifiers: Modifiers::empty(),
                click_count: 1,
                first_mouse: false,
            },
        )
        .unwrap();
        cx.read(view, |e| {
            assert!(e.transform_edit.is_none());
            assert_ne!(e.session().document.active, Some(first));
        })
        .unwrap();
    }

    #[test]
    fn pending_edits_disable_layer_controls_and_shortcuts_without_committing() {
        for kind in 0..4 {
            let mut editor = Editor::with_test_document();
            let mut doc = Document::new(20, 20).unwrap();
            compositor::edits::fill(&mut doc, [50, 120, 200, 255], false, false).unwrap();
            if kind == 3 {
                doc.selection = Some(compositor::selection::Selection::rectangle(
                    20,
                    20,
                    [2., 2.],
                    [10., 10.],
                    false,
                ));
            }
            editor.tabs = vec![Session::new(doc, None).into()];
            let (mut cx, view) = Application::new()
                .into_test_context(
                    WindowOptions::new("Pending edits").size(1500., 900.),
                    editor,
                )
                .unwrap();
            let window = view.window_handle();
            cx.update(view, |e, cx| {
                match kind {
                    0 => e.start_toolbar_transform().unwrap(),
                    1 => e.begin_gradient([0., 0.]).unwrap(),
                    2 => {
                        e.tools.pending_crop = Some(crop::CropPreview {
                            frame: Transform::new(10, 10),
                            guides: [None; 2],
                        })
                    }
                    _ => e.begin_pixel_transform().unwrap(),
                }
                e.changed(cx);
            })
            .unwrap();
            let before = cx.read(view, |e| e.session().document.clone()).unwrap();
            for id in [
                quickgui::ElementId::from(510_u64),
                511_u64.into(),
                513_u64.into(),
                500_u64.into(),
                "layer-add-mask".into(),
                "layer-adjustment-menu".into(),
            ] {
                assert!(
                    matches!(
                        cx.click(window, id),
                        Err(quickgui::TestAppError::NotClickable { .. })
                    ),
                    "Control {id:?} enabled during pending edit {kind}"
                );
            }
            let active = before.active.unwrap();
            cx.simulate_context_menu(
                window,
                format!("layer-row-{active}"),
                quickgui::Point::new(1400., 280.),
                Modifiers::empty(),
            )
            .unwrap();
            cx.update(view, |e, cx| {
                e.action(Action::AddLayer, cx);
                e.action(Action::Duplicate, cx);
                e.action(Action::DeleteLayer, cx);
                e.step_blend(true).unwrap();
                e.opacity_digit(5, std::time::Instant::now()).unwrap();
            })
            .unwrap();
            cx.read(view, |e| {
                assert_eq!(e.session().document, before);
                assert!(e.session().undo_label().is_none());
            })
            .unwrap();
        }
    }
    #[test]
    fn selecting_layers_under_hue_and_filter_survives_preview_refresh_and_cancel() {
        for filter in [false, true] {
            for thumbnail in [false, true] {
                let mut e = Editor::with_test_document();
                let mut doc = Document::new(8, 8).unwrap();
                compositor::edits::fill(&mut doc, [80, 120, 160, 255], false, false).unwrap();
                let original = doc.active.unwrap();
                compositor::layer_ops::add_blank(&mut doc).unwrap();
                compositor::edits::fill(&mut doc, [200, 40, 30, 255], false, false).unwrap();
                let selected = doc.active.unwrap();
                doc.select(original, false);
                e.tabs = vec![Session::new(doc, None).into()];
                if filter {
                    e.open_filter(compositor::filters::Filter::Gaussian { radius: 1. })
                        .unwrap();
                } else {
                    e.open_pixel_adjustment(Kind::HueSaturation).unwrap();
                }
                let (mut cx, view) = Application::new()
                    .into_test_context(
                        WindowOptions::new("Selection beneath panel").size(1500., 900.),
                        e,
                    )
                    .unwrap();
                let target: quickgui::ElementId = if thumbnail {
                    format!("layer-thumbnail-{selected}-false").into()
                } else {
                    1004_u64.into()
                };
                cx.simulate_mouse_down(
                    view.window_handle(),
                    target,
                    quickgui::MouseDownEvent {
                        button: quickgui::MouseButton::Left,
                        position: quickgui::Point::new(1395., 282.),
                        modifiers: Modifiers::empty(),
                        click_count: 1,
                        first_mouse: false,
                    },
                )
                .unwrap();
                cx.update(view, |e, _| {
                    assert_eq!(e.session().document.active, Some(selected));
                    e.refresh_adjustment_document();
                    e.refresh_filter_document();
                    assert_eq!(e.session().document.active, Some(selected));
                    assert!(e.modal.is_some());
                    e.cancel_adjustment();
                    e.cancel_filter();
                    assert_eq!(e.session().document.active, Some(selected));
                    assert_eq!(e.session().document.selected, [selected].into());
                    assert!(e.session().undo_label().is_none());
                })
                .unwrap();
            }
        }
    }
}
