//! Image and Filter command eligibility follows EditorSession.canAdjustColors.
use super::*;

impl Editor {
    pub(super) fn can_edit_pixels(&self) -> bool {
        if !self.can_edit_layers() {
            return false;
        }
        let doc = &self.session().document;
        doc.selected.len() == 1
            && doc.selection.as_ref().is_none_or(|s| s.bounds().is_some())
            && doc.active_layer().is_some_and(|layer| {
                doc.layer_is_visible(layer.id)
                    && if self.tools.mask_target {
                        layer.mask.as_ref().is_some_and(|mask| mask.enabled)
                    } else {
                        matches!(layer.content, compositor::document::LayerContent::Raster(_))
                    }
            })
    }

    pub(super) fn can_adjust_colors(&self) -> bool {
        if !self.has_document()
            || self.pending
            || self.gesture.is_some()
            || self.pending_pixels.is_some()
            || self.rename.is_some()
            || self.adjustment_edit.is_some()
            || self.filter_edit.is_some()
            || self.tools.mask_target
            || !matches!(
                self.modal,
                None | Some(Form::Blend | Form::Color(_) | Form::MaskColor(_))
            )
        {
            return false;
        }
        let doc = &self.session().document;
        doc.selected.len() == 1
            && doc.selection.as_ref().is_none_or(|s| s.bounds().is_some())
            && doc
                .active_layer()
                .is_some_and(|layer| layer.raster().is_some() && doc.layer_is_visible(layer.id))
    }

    pub(super) fn can_content_aware_fill(&self) -> bool {
        self.can_adjust_colors() && self.session().document.selection.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compositor::{document::Layer, filters::Filter, selection::Selection};
    use quickgui::{Application, WindowOptions};

    fn editor() -> Editor {
        let mut e = Editor::with_test_document();
        let mut doc = Document::new(20, 20).unwrap();
        compositor::edits::fill(&mut doc, [50, 120, 200, 255], false, false).unwrap();
        e.tabs = vec![Session::new(doc, None).into()];
        e
    }

    #[test]
    fn brush_and_gradient_presses_reject_ineligible_targets_without_starting_history() {
        for tool in [
            Tool::Brush,
            Tool::Erase,
            Tool::Clone,
            Tool::Blur,
            Tool::Heal,
            Tool::Smudge,
            Tool::Liquify,
            Tool::Gradient,
        ] {
            for state in 0..5 {
                let mut e = editor();
                e.tools.tool = tool;
                e.tools.clone_source = Some([1., 1.]);
                e.session_mut().fit = false;
                e.session_mut().zoom = 1.;
                match state {
                    0 => e.session_mut().document.layers[0].visible = false,
                    1 => {
                        let doc = &mut e.session_mut().document;
                        let active = doc.active.unwrap();
                        let mut parent = Layer::blank("Hidden folder", 20, 20);
                        parent.content = compositor::document::LayerContent::Group;
                        parent.visible = false;
                        doc.layers[0].parent = Some(parent.id);
                        doc.add(parent).unwrap();
                        doc.select(active, false);
                    }
                    2 => {
                        let doc = &mut e.session_mut().document;
                        let active = doc.active.unwrap();
                        doc.add(Layer::blank("Second", 20, 20)).unwrap();
                        doc.select(active, true);
                    }
                    3 => {
                        e.session_mut().document.selection =
                            Some(Selection::rectangle(20, 20, [2., 2.], [2., 2.], false))
                    }
                    _ => {
                        e.tools.mask_target = true;
                        e.session_mut().document.layers[0].mask =
                            Some(compositor::document::Mask {
                                pixels: std::sync::Arc::new(image::GrayImage::from_pixel(
                                    20,
                                    20,
                                    image::Luma([255]),
                                )),
                                enabled: false,
                                linked: true,
                                placement: None,
                            });
                    }
                }
                let before = e.session().document.clone();
                let point = quickgui::Point::new(10., 10.);
                let mut event = quickgui::PointerEvent {
                    phase: quickgui::PointerPhase::Down,
                    position: point,
                    origin: point,
                    local_position: point,
                    local_origin: point,
                    delta: quickgui::Vector::ZERO,
                    button: quickgui::MouseButton::Left,
                    modifiers: Modifiers::empty(),
                    size: quickgui::Size::new(20., 20.),
                };
                for phase in [
                    quickgui::PointerPhase::Down,
                    quickgui::PointerPhase::Move,
                    quickgui::PointerPhase::Up,
                ] {
                    event.phase = phase;
                    e.pointer(&event).unwrap();
                    assert_eq!(
                        e.session().document,
                        before,
                        "{tool:?}, state {state}, {phase:?}"
                    );
                    assert!(e.gesture.is_none(), "{tool:?}, state {state}, {phase:?}");
                    assert!(
                        !e.session().has_pending_edit(),
                        "{tool:?}, state {state}, {phase:?}"
                    );
                    assert!(e.session().undo_label().is_none());
                    assert!(e.pending_gradient.is_none());
                }
            }
        }
    }

    #[test]
    fn unavailable_image_commands_preserve_the_document_and_existing_dialog() {
        for state in 0..7 {
            let mut e = editor();
            match state {
                0 => e.session_mut().document.layers[0].visible = false,
                1 => {
                    let doc = &mut e.session_mut().document;
                    let active = doc.active.unwrap();
                    let mut parent = Layer::blank("Hidden folder", 20, 20);
                    parent.content = compositor::document::LayerContent::Group;
                    parent.visible = false;
                    let parent_id = parent.id;
                    doc.add(parent).unwrap();
                    doc.layers[0].parent = Some(parent_id);
                    doc.select(active, false);
                }
                2 => {
                    let doc = &mut e.session_mut().document;
                    let active = doc.active.unwrap();
                    doc.add(Layer::blank("Second", 20, 20)).unwrap();
                    doc.select(active, true);
                }
                3 => {
                    e.session_mut().document.selection =
                        Some(Selection::rectangle(20, 20, [2., 2.], [2., 2.], false))
                }
                4 => {
                    e.session_mut().document.selection =
                        Some(Selection::rectangle(20, 20, [2., 2.], [10., 10.], false));
                    e.begin_pixel_transform().unwrap();
                }
                5 => e.open_pixel_adjustment(Kind::Levels).unwrap(),
                _ => e.open_filter(Filter::Gaussian { radius: 1. }).unwrap(),
            }
            let before = e.session().document.clone();
            let (mut cx, view) = Application::new()
                .into_test_context(
                    WindowOptions::new("Image command eligibility").size(1500., 900.),
                    e,
                )
                .unwrap();
            cx.update(view, |e, cx| {
                for action in [
                    Action::AdjustPixels(Kind::HueSaturation),
                    Action::AdjustPixels(Kind::Curves),
                    Action::Filter(Filter::Gaussian { radius: 2. }),
                    Action::RemoveBackground,
                ] {
                    e.action(action, cx);
                    assert_eq!(e.session().document, before, "State {state}");
                    assert!(e.session().undo_label().is_none(), "State {state}");
                    match state {
                        4 => assert!(e.pending_pixels.is_some()),
                        5 => assert_eq!(
                            e.adjustment_edit.as_ref().unwrap().settings.kind,
                            Kind::Levels
                        ),
                        6 => assert!(matches!(
                            e.modal,
                            Some(Form::Edit {
                                action: Action::Filter(Filter::Gaussian { radius: 1. }),
                                ..
                            })
                        )),
                        _ => assert!(
                            e.modal.is_none(),
                            "State {state} opened an unavailable editor"
                        ),
                    }
                }
            })
            .unwrap();
        }
    }

    #[test]
    fn content_aware_fill_requires_a_nonempty_selection() {
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Content-aware fill eligibility").size(1500., 900.),
                editor(),
            )
            .unwrap();
        cx.update(view, |e, cx| {
            e.action(Action::Filter(Filter::ContentFill), cx);
            assert!(e.filter_edit.is_none());
            e.session_mut().document.selection =
                Some(Selection::rectangle(20, 20, [2., 2.], [10., 10.], false));
            e.action(Action::Filter(Filter::ContentFill), cx);
            assert!(e.filter_edit.is_some());
        })
        .unwrap();
    }

    #[test]
    fn image_commands_commit_a_pending_gradient_and_keep_it_when_the_dialog_is_cancelled() {
        for action in [
            Action::AdjustPixels(Kind::Levels),
            Action::Filter(Filter::Gaussian { radius: 1. }),
            Action::RemoveBackground,
        ] {
            let mut e = editor();
            e.begin_gradient([0., 0.]).unwrap();
            e.move_gradient([19., 19.], gradient::Endpoint::End, false)
                .unwrap();
            let gradient = e.session().document.clone();
            let (mut cx, view) = Application::new()
                .into_test_context(
                    WindowOptions::new("Gradient to image command").size(1500., 900.),
                    e,
                )
                .unwrap();
            cx.update(view, |e, cx| {
                e.action(action, cx);
                assert!(e.pending_gradient.is_none());
                assert!(e.adjustment_edit.is_some() || e.filter_edit.is_some());
                e.cancel_adjustment();
                e.cancel_filter();
                assert_eq!(e.session().document, gradient);
                assert_eq!(e.session().undo_label(), Some("Gradient"));
            })
            .unwrap();
        }
    }
    #[test]
    fn adjustments_commit_numeric_transforms_before_opening() {
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Transform to adjustment").size(1500., 900.),
                editor(),
            )
            .unwrap();
        cx.focus(view.window_handle(), "transform-value-0").unwrap();
        cx.simulate_keystrokes(view.window_handle(), "ctrl-a")
            .unwrap();
        cx.simulate_input(view.window_handle(), "5").unwrap();
        cx.update(view, |e, cx| {
            assert!(e.transform_edit.is_some());
            e.action(Action::AdjustPixels(Kind::HueSaturation), cx);
            assert!(e.transform_edit.is_none());
            assert!(e.adjustment_edit.is_some());
            e.cancel_adjustment();
            assert_eq!(e.session().document.layers[0].transform.origin[0], 5.);
            assert_eq!(e.session().undo_label(), Some("Transform Layer"));
        })
        .unwrap();
    }

    #[test]
    fn hue_keeps_crop_and_polygon_drafts_while_levels_and_filters_cancel_them() {
        for action in [
            Action::AdjustPixels(Kind::HueSaturation),
            Action::AdjustPixels(Kind::Levels),
            Action::Filter(Filter::Gaussian { radius: 1. }),
            Action::RemoveBackground,
        ] {
            for crop_draft in [true, false] {
                let mut e = editor();
                if crop_draft {
                    e.tools.pending_crop = Some(crop::CropPreview {
                        frame: compositor::geometry::Transform::new(10, 10),
                        guides: [None; 2],
                    });
                } else {
                    e.polygon_click([1., 1.], 1., compositor::selection::SelectionMode::Replace)
                        .unwrap();
                }
                let (mut cx, view) = Application::new()
                    .into_test_context(
                        WindowOptions::new("Image command drafts").size(1500., 900.),
                        e,
                    )
                    .unwrap();
                cx.update(view, |e, cx| {
                    e.action(action, cx);
                    let retained = matches!(action, Action::AdjustPixels(Kind::HueSaturation));
                    assert!(e.adjustment_edit.is_some() || e.filter_edit.is_some());
                    assert_eq!(e.tools.pending_crop.is_some(), crop_draft && retained);
                    assert_eq!(e.tools.polygon.is_some(), !crop_draft && retained);
                    assert!(e.session().undo_label().is_none());
                })
                .unwrap();
            }
        }
    }
}
