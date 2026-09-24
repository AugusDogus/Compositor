//! CropControls.swift's ratio menu and live frame dimensions.
use super::*;
use crate::ui::dropdown::Dropdown;
use quickgui::{PickerItem, SelectPopoverLayout, StateAccessor};

#[derive(Clone, Copy)]
pub(super) enum Ratio {
    Free,
    Original,
    Square,
    FourThirds,
    Widescreen,
    Portrait,
    Tall,
}
impl Ratio {
    fn label(self) -> &'static str {
        match self {
            Self::Free => "Free",
            Self::Original => "Original",
            Self::Square => "1:1",
            Self::FourThirds => "4:3",
            Self::Widescreen => "16:9",
            Self::Portrait => "3:4",
            Self::Tall => "9:16",
        }
    }
    fn value(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Original => "original",
            other => other.label(),
        }
    }
}
pub(super) fn new() -> Dropdown<Ratio> {
    let mut state = Dropdown::new(
        [
            Ratio::Free,
            Ratio::Original,
            Ratio::Square,
            Ratio::FourThirds,
            Ratio::Widescreen,
            Ratio::Portrait,
            Ratio::Tall,
        ]
        .map(|ratio| PickerItem::new(ratio.label(), ratio).id(ratio.value())),
    )
    .expect("Crop ratio options have unique static IDs")
    .with_layout(SelectPopoverLayout::new(170., 24.).trigger_height(24.));
    state.select_id("free");
    state
}
impl Editor {
    pub(super) fn current_crop_ratio(&self) -> Option<f64> {
        if matches!(
            self.tools.crop_picker.selected_value(),
            Some(Ratio::Original)
        ) {
            let doc = &self.session().document;
            Some(f64::from(doc.width) / f64::from(doc.height))
        } else {
            self.tools.crop_ratio
        }
    }

    pub(super) fn crop_header(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let label = self
            .tools
            .crop_picker
            .value_text()
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                self.tools
                    .crop_ratio
                    .map_or_else(|| "Free".into(), |ratio| format!("{ratio:.3}:1"))
            });
        let selector = self.tools.crop_picker.element_with(
            cx,
            "crop-ratio",
            "Crop ratio",
            StateAccessor::new(|this: &mut Self| &mut this.tools.crop_picker),
            Self::tool_header_control(label)
                .flex_1()
                .min_w(0.)
                .flex_row()
                .items_center()
                .child(div().flex_1())
                .child(Icon::PopupChevron.element(14.)),
            |this, ratio, cx| {
                if this.tools.tool != Tool::Crop
                    || this.modal.is_some()
                    || this.pending
                    || !this.has_document()
                {
                    return;
                }
                let result = this.apply_form(Action::CropSettings, vec![ratio.value().into()]);
                this.result(result, cx);
            },
        );
        let mut row = div().flex_row().items_center().gap(14.).flex_1().child(
            div()
                .flex_row()
                .items_center()
                .gap(8.)
                .w(170.)
                .flex_shrink_0()
                .child(text("Ratio").text_size(12.).line_height(15.))
                .child(selector),
        );
        if let Some(crop) = &self.tools.pending_crop {
            row = row.child(
                text(format!(
                    "{} × {} px",
                    crop.frame.size[0] as u32, crop.frame.size[1] as u32
                ))
                .text_size(12.)
                .line_height(15.)
                .font_features(
                    quickgui::FontFeatures::new().enable(quickgui::FontFeatureTag::TABULAR_NUMBERS),
                )
                .whitespace_nowrap()
                .flex_shrink_0(),
            );
        }
        row.child(div().flex_1()).child(self.crop_controls(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, WindowOptions};

    #[test]
    fn ratio_menu_previews_frame_and_cancel_preserves_pixels_and_history() {
        let mut e = Editor::with_test_document();
        e.tabs = vec![Session::new(Document::new(400, 300).unwrap(), None).into()];
        e.tools.tool = Tool::Crop;
        let original = e.session().document.clone();
        let (mut cx, view) = Application::new()
            .bind_keys(quickgui::select_key_bindings())
            .into_test_context(WindowOptions::new("Crop ratio").size(900., 600.), e)
            .unwrap();
        let window = view.window_handle();
        assert!(cx.click(window, "crop-apply").is_err());
        cx.click(window, "crop-ratio").unwrap();
        assert!(cx.read(view, |e| e.tools.crop_picker.is_open()).unwrap());
        cx.simulate_keystrokes(window, "down down down down enter")
            .unwrap();
        cx.read(view, |e| {
            assert_eq!(e.tools.crop_ratio, Some(16. / 9.));
            assert_eq!(e.tools.crop_picker.value_text().as_deref(), Some("16:9"));
            let crop = e.tools.pending_crop.as_ref().unwrap();
            assert_eq!(crop.frame.size, [400., 225.]);
            assert_eq!(crop.frame.origin, [0., 38.]);
            assert_eq!(e.session().document, original);
        })
        .unwrap();
        cx.click(window, "crop-cancel").unwrap();
        cx.read(view, |e| {
            assert!(e.tools.pending_crop.is_none());
            assert!(e.session().undo_label().is_none());
            assert_eq!(e.session().document, original);
        })
        .unwrap();
        cx.click(window, "crop-ratio").unwrap();
        assert!(cx.read(view, |e| e.tools.crop_picker.is_open()).unwrap());
        cx.simulate_keystrokes(window, "up up up enter").unwrap();
        assert_eq!(
            cx.read(view, |e| e.tools.crop_ratio).unwrap(),
            Some(4. / 3.)
        );
        cx.click(window, "crop-ratio").unwrap();
        cx.update(view, |e, cx| e.select_tool(Tool::Brush, cx))
            .unwrap();
        assert!(cx.read(view, |e| !e.tools.crop_picker.is_open()).unwrap());
        cx.update(view, |e, _| {
            e.tabs
                .push(Session::new(Document::new(200, 400).unwrap(), None).into());
            e.activate_tab(1);
            assert_eq!(e.current_crop_ratio(), None);
            e.tools.crop_picker.select_id("original");
            assert_eq!(e.current_crop_ratio(), Some(0.5));
        })
        .unwrap();
    }
}
