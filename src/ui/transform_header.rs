//! Live transform fields preserve the original document until Apply or Cancel.
use super::*;
mod drag;
mod effects;
#[cfg(test)]
mod floating_tests;
mod history;
#[cfg(test)]
mod layer_tests;
use drag::Draft;
pub(super) use drag::HeaderDrag;
mod sampling;
mod view;
pub(super) use sampling::new as sampling_picker;

use compositor::{
    geometry::{Point, Transform},
    invalid,
};

enum TransformSource {
    Layers(Document),
    Floating,
}

pub(super) struct TransformEdit {
    source: TransformSource,
    current: Draft,
    bounds: Transform,
    pixels: Point,
    values: [String; 6],
    valid: bool,
}
fn values(bounds: Transform, pixels: Point) -> [String; 6] {
    [
        bounds.origin[0],
        bounds.origin[1],
        bounds.size[0],
        bounds.size[1],
        bounds.size[0] / pixels[0] * 100.,
        bounds.rotation,
    ]
    .map(format_value)
}
fn format_value(value: f64) -> String {
    if (value - value.round()).abs() < 0.005 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    }
}
impl Editor {
    fn can_edit_transform_numbers(&self) -> bool {
        !self.pending
            && self.gesture.is_none()
            && !self
                .transform_edit
                .as_ref()
                .is_some_and(|e| matches!(e.current, Draft::Perspective { .. }))
            && !self
                .pending_pixels
                .as_ref()
                .is_some_and(|edit| matches!(edit.placement, floating::Placement::Perspective(_)))
    }

    pub(super) fn start_toolbar_transform(&mut self) -> Result<()> {
        self.tools.tool = Tool::Move;
        self.begin_header_transform()
    }

    pub(super) fn finish_toolbar_transform(&mut self, apply: bool) -> Result<()> {
        self.finish_header_transform(apply)?;
        if apply {
            self.commit_pixels()
        } else {
            if self.pending_pixels.take().is_some() {
                self.session_mut().cancel();
            }
            Ok(())
        }
    }

    fn header_transform_bounds(&self) -> Option<Transform> {
        if let Some(edit) = &self.transform_edit {
            return Some(edit.current.placement().bounds());
        }
        if let Some(edit) = &self.pending_pixels {
            return Some(edit.placement.bounds());
        }
        let doc = self.current_document()?;
        compositor::transform::selection_bounds(doc, self.tools.mask_target)
    }
    fn header_transform_pixel_size(&self, bounds: Transform) -> Point {
        if let Some(edit) = &self.pending_pixels {
            return edit.pixel_size();
        }
        self.transform_pixel_size(bounds.size)
    }

    fn begin_header_transform(&mut self) -> Result<()> {
        if !self.can_edit_transform_numbers() {
            return Ok(());
        }
        if self.transform_edit.is_none() {
            let doc = &self.session().document;
            let source = if self.pending_pixels.is_some() {
                TransformSource::Floating
            } else {
                TransformSource::Layers(doc.clone())
            };
            let bounds = self
                .header_transform_bounds()
                .ok_or_else(|| invalid("Select a layer to transform."))?;
            let pixels = self.header_transform_pixel_size(bounds);
            if !matches!(source, TransformSource::Floating) {
                let label = self.transform_history_label(false);
                self.session_mut().begin(label)?;
            }
            self.transform_edit = Some(TransformEdit {
                source,
                current: Draft::Affine(bounds),
                bounds,
                pixels,
                values: values(bounds, pixels),
                valid: true,
            });
        }
        Ok(())
    }

    fn change_header_transform(&mut self, change: impl FnOnce(&mut Transform)) -> Result<()> {
        if !self.can_edit_transform_numbers() {
            return Ok(());
        }
        self.begin_header_transform()?;
        let Some(edit) = &mut self.transform_edit else {
            return Ok(());
        };
        let mut next = edit.current.placement().bounds();
        change(&mut next);
        edit.values = values(next, edit.pixels);
        self.preview_header_transform(next)
    }

    fn header_transform_input(&mut self, index: usize, input: &str) -> Result<()> {
        if !self.can_edit_transform_numbers() {
            return Ok(());
        }
        self.begin_header_transform()?;
        let previous_bounds = self.header_transform_bounds();
        let Some(edit) = &mut self.transform_edit else {
            return Ok(());
        };
        edit.values[index] = input.into();
        edit.valid = false;
        let parsed = edit
            .values
            .each_ref()
            .map(|v| v.parse::<f64>().ok().filter(|v| v.is_finite()));
        let [Some(x), Some(y), Some(w), Some(h), Some(scale), Some(angle)] = parsed else {
            return Ok(());
        };
        if w <= 0. || h <= 0. || scale <= 0. {
            return Ok(());
        }
        let mut next = Transform {
            origin: [x, y],
            size: [w, h],
            rotation: angle % 360.,
            ..edit.current.placement().bounds()
        };
        let previous = previous_bounds.unwrap_or(edit.bounds);
        if index == 4 {
            next.size = edit.pixels.map(|size| size * scale / 100.);
            next.origin = [x + (w - next.size[0]) / 2., y + (h - next.size[1]) / 2.];
            for (i, n) in next.origin.into_iter().chain(next.size).enumerate() {
                edit.values[i] = format_value(n);
            }
        } else if matches!(index, 2 | 3) {
            if self.tools.transform_ratio {
                let ratio = previous.size[0] / previous.size[1];
                if index == 2 {
                    next.size[1] = w / ratio;
                    edit.values[3] = format_value(next.size[1]);
                } else {
                    next.size[0] = h * ratio;
                    edit.values[2] = format_value(next.size[0]);
                }
            }
            edit.values[4] = format_value(next.size[0] / edit.pixels[0] * 100.);
        }
        self.preview_header_transform(next)
    }
    fn preview_header_transform(&mut self, next: Transform) -> Result<()> {
        let Some(edit) = &self.transform_edit else {
            return Ok(());
        };
        let preview = match &edit.source {
            TransformSource::Layers(original) => {
                let mut preview = original.clone();
                compositor::transform::apply(&mut preview, edit.bounds, next, self.tools.mask_target)?;
                preview
            }
            TransformSource::Floating => self.pending_pixels.as_ref()
                .ok_or_else(|| invalid("The floating selection is no longer available. Cancel the transform to return to the canvas."))?
                .preview(floating::Placement::Affine(next))?,
        };
        preview.validate()?;
        if matches!(edit.source, TransformSource::Floating)
            && let Some(pending) = &mut self.pending_pixels
        {
            pending.placement = floating::Placement::Affine(next);
        }
        if let Some(edit) = &mut self.transform_edit {
            edit.current = Draft::Affine(next);
            edit.valid = true;
        }
        self.session_mut().document = preview;
        Ok(())
    }
    pub(super) fn finish_header_transform(&mut self, apply: bool) -> Result<()> {
        let Some(edit) = &self.transform_edit else {
            return Ok(());
        };
        if apply && !edit.valid {
            return Err(invalid(
                "Enter valid transform values, or Cancel to restore the layer.",
            ));
        }
        if apply {
            // Numeric edits of floating pixels stay in the existing transaction.
            // A later canvas drag still replays the original lifted pixels.
            if !matches!(edit.source, TransformSource::Floating) {
                let perspective = matches!(edit.current, Draft::Perspective { .. });
                let label = self.transform_history_label(perspective);
                self.session_mut().commit_named(label)?;
            }
        } else {
            if matches!(edit.source, TransformSource::Floating) {
                self.pending_pixels = None;
            }
            self.session_mut().cancel();
        }
        self.transform_edit = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, Keystroke, WindowOptions};
    #[test]
    fn sampling_flips_and_numbers_share_one_cancellable_transform() {
        for apply in [false, true] {
            let mut editor = Editor::with_test_document();
            let mut document = Document::new(100, 50).unwrap();
            compositor::edits::fill(&mut document, [100, 80, 60, 255], false, false).unwrap();
            editor.tabs = vec![Session::new(document.clone(), None).into()];
            let (mut cx, view) = Application::new()
                .bind_keys(quickgui::select_key_bindings())
                .into_test_context(
                    WindowOptions::new("Transform controls").size(1800., 900.),
                    editor,
                )
                .unwrap();
            let window = view.window_handle();
            cx.click(window, "transform-sampling").unwrap();
            assert!(
                cx.read(view, |e| e.tools.transform_sampling.is_open())
                    .unwrap()
            );
            cx.simulate_keystrokes(window, "n enter").unwrap();
            cx.click(window, "transform-flip-h").unwrap();
            cx.click(window, "transform-flip-v").unwrap();
            cx.focus(window, "transform-value-0").unwrap();
            cx.simulate_keystroke(
                window,
                Keystroke::new(Key::Character("a".into()), Modifiers::CONTROL),
            )
            .unwrap();
            cx.simulate_input(window, "5").unwrap();
            cx.read(view, |e| {
                let transform = e.session().document.layers[0].transform;
                assert_eq!(transform.origin, [5., 0.]);
                assert_eq!(transform.sampling, compositor::geometry::Sampling::Nearest);
                assert!(transform.flip_x && transform.flip_y);
                assert!(e.session().undo_label().is_none());
            })
            .unwrap();
            cx.click(
                window,
                if apply {
                    "transform-apply"
                } else {
                    "transform-cancel"
                },
            )
            .unwrap();
            if apply {
                assert_eq!(
                    cx.read(view, |e| e.session().undo_label().map(str::to_owned))
                        .unwrap()
                        .as_deref(),
                    Some("Transform Layer")
                );
                cx.update(view, |e, cx| e.action(Action::Undo, cx)).unwrap();
                assert!(
                    cx.read(view, |e| e.session().undo_label().is_none())
                        .unwrap()
                );
            }
            assert_eq!(
                cx.read(view, |e| e.session().document.clone()).unwrap(),
                document
            );
        }
    }

    #[test]
    fn narrow_transform_bar_scrolls_fields_and_keeps_commit_buttons_visible() {
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Narrow transform").size(800., 594.),
                Editor::with_test_document(),
            )
            .unwrap();
        let window = view.window_handle();
        let apply = cx.element_bounds(window, "transform-apply").unwrap();
        let cancel = cx.element_bounds(window, "transform-cancel").unwrap();
        let viewport = cx
            .element_bounds(window, "transform-fields-scroll")
            .unwrap();
        let first = cx.element_bounds(window, "transform-value-0").unwrap();
        assert!(apply.x + apply.width <= 800. && cancel.x > viewport.x);
        assert!(viewport.width > 200. && viewport.x + viewport.width <= cancel.x);
        assert!(
            cx.simulate_retained_scroll(
                window,
                "transform-fields-scroll",
                quickgui::Vector::new(-1000., 0.)
            )
            .unwrap()
        );
        assert!(cx.element_bounds(window, "transform-value-0").unwrap().x < first.x);
        assert_eq!(cx.element_bounds(window, "transform-apply").unwrap(), apply);
        let flip = cx.element_bounds(window, "transform-flip-v").unwrap();
        assert!(flip.x + flip.width <= cancel.x);
    }

    #[test]
    fn live_transform_fields_preview_cancel_and_commit_as_one_edit() {
        let mut editor = Editor::with_test_document();
        let mut document = Document::new(100, 50).unwrap();
        compositor::edits::fill(&mut document, [100, 80, 60, 255], false, false).unwrap();
        editor.tabs = vec![Session::new(document.clone(), None).into()];
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Transform fields").size(1280., 900.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        cx.focus(window, "transform-value-2").unwrap();
        cx.simulate_keystroke(
            window,
            Keystroke::new(Key::Character("a".into()), Modifiers::CONTROL),
        )
        .unwrap();
        cx.simulate_input(window, "200").unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.layers[0].transform.size)
                .unwrap(),
            [200., 100.]
        );
        cx.click(window, "transform-cancel").unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            document
        );
        cx.update(view, |e, cx| {
            e.header_transform_input(4, "50").unwrap();
            e.header_transform_input(4, "150").unwrap();
            e.changed(cx);
        })
        .unwrap();
        cx.click(window, "transform-apply").unwrap();
        cx.read(view, |e| {
            assert_eq!(e.session().document.layers[0].transform.size, [150., 75.]);
            assert_eq!(
                e.session().document.layers[0].transform.origin,
                [-25., -12.5]
            );
            assert_eq!(e.session().undo_label(), Some("Transform Layer"));
        })
        .unwrap();
        cx.update(view, |e, _| e.session_mut().undo()).unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            document
        );
    }
    #[test]
    fn selected_pixel_fields_preserve_unselected_pixels_and_cancel_or_undo() {
        let mut editor = Editor::with_test_document();
        let mut doc = Document::new(8, 4).unwrap();
        compositor::edits::fill(&mut doc, [120, 80, 40, 255], false, false).unwrap();
        doc.selection = Some(compositor::selection::Selection::rectangle(
            8,
            4,
            [0., 0.],
            [2., 2.],
            false,
        ));
        editor.tabs = vec![Session::new(doc.clone(), None).into()];
        editor.begin_pixel_transform().unwrap();
        assert_eq!(editor.header_transform_bounds().unwrap().size, [2., 2.]);
        editor.header_transform_input(0, "4").unwrap();
        assert_eq!(
            editor.session().document.layers[0].transform,
            doc.layers[0].transform
        );
        let pixels = editor.session().document.layers[0].raster().unwrap();
        assert_eq!(pixels[(0, 0)][3], 0);
        assert_eq!(pixels[(3, 3)], image::Rgba([120, 80, 40, 255]));
        editor.finish_toolbar_transform(false).unwrap();
        assert_eq!(editor.session().document, doc);
        editor.begin_pixel_transform().unwrap();
        editor.header_transform_input(0, "4").unwrap();
        editor.finish_toolbar_transform(true).unwrap();
        assert_eq!(
            editor
                .session()
                .document
                .selection
                .as_ref()
                .unwrap()
                .bounds(),
            Some([4., 0., 6., 2.])
        );
        editor.session_mut().undo();
        assert_eq!(editor.session().document, doc);
    }
    #[test]
    fn invalid_transform_drafts_keep_pixels_and_cannot_be_committed() {
        let mut editor = Editor::with_test_document();
        editor.tabs = vec![Session::new(Document::new(8, 8).unwrap(), None).into()];
        compositor::edits::fill(
            &mut editor.session_mut().document,
            [100, 80, 60, 255],
            false,
            false,
        )
        .unwrap();
        let original = editor.session().document.clone();
        editor.header_transform_input(2, "-1").unwrap();
        assert!(editor.nudge(&Key::ArrowRight, Modifiers::empty()).is_err());
        assert!(editor.finish_header_transform(true).is_err());
        assert_eq!(editor.session().document, original);
        editor.finish_header_transform(false).unwrap();
        assert!(editor.transform_edit.is_none());
        assert!(editor.session().undo_label().is_none());
    }
}
