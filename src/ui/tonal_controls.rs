use super::*;
use compositor::brush::tonal::{Range, Tonal};

impl Tool {
    pub(super) fn is_tonal(self) -> bool {
        matches!(self, Self::Dodge | Self::Burn | Self::Sponge)
    }
}

impl Editor {
    pub(super) fn can_paint_tonal(&self) -> bool {
        !self.tools.mask_target
            && self.can_edit_pixels()
            && self
                .session()
                .document
                .active_layer()
                .is_some_and(|layer| layer.raster().is_some())
    }

    pub(super) fn tonal_operation(&self) -> Tonal {
        match self.tools.tool {
            Tool::Dodge => Tonal::Dodge(self.tools.tonal_range),
            Tool::Burn => Tonal::Burn(self.tools.tonal_range),
            _ if self.tools.sponge_saturate => Tonal::Saturate,
            _ => Tonal::Desaturate,
        }
    }

    pub(super) fn tonal_controls(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let mut row = div().flex_row().items_center().gap(2.).flex_shrink_0();
        if self.tools.tool == Tool::Sponge {
            for (saturate, label) in [(false, "Desaturate"), (true, "Saturate")] {
                row = row.child(
                    Self::segment(label, self.tools.sponge_saturate == saturate).on_click(
                        cx.listener(format!("sponge-{label}"), move |this, cx| {
                            this.tools.sponge_saturate = saturate;
                            cx.invalidate();
                        }),
                    ),
                );
            }
        } else {
            row = row.child(text("Range").text_size(12.).line_height(15.).mr(6.));
            for (range, label) in [
                (Range::All, "All"),
                (Range::Shadows, "Shadows"),
                (Range::Midtones, "Midtones"),
                (Range::Highlights, "Highlights"),
            ] {
                row = row.child(
                    Self::segment(label, self.tools.tonal_range == range).on_click(cx.listener(
                        format!("tonal-range-{label}"),
                        move |this, cx| {
                            this.tools.tonal_range = range;
                            cx.invalidate();
                        },
                    )),
                );
            }
        }
        row
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, WindowOptions};

    #[test]
    fn tonal_toolbar_controls_choose_operations_and_remember_brush_settings() {
        let mut editor = Editor::with_test_document();
        compositor::edits::fill(
            &mut editor.session_mut().document,
            [120, 80, 160, 255],
            false,
            false,
        )
        .unwrap();
        let original = editor.session().document.clone();
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Tonal brushes").size(1600., 1000.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        cx.click(window, "tonal-tool").unwrap();
        assert_eq!(cx.read(view, |e| e.tools.tool).unwrap(), Tool::Dodge);
        assert_eq!(cx.read(view, |e| e.tools.brush.opacity).unwrap(), 0.5);
        cx.click(window, "tonal-range-Highlights").unwrap();
        assert_eq!(
            cx.read(view, |e| e.tonal_operation()).unwrap(),
            Tonal::Dodge(Range::Highlights)
        );
        cx.click(window, "tool-mode-Burn").unwrap();
        assert_eq!(
            cx.read(view, |e| e.tonal_operation()).unwrap(),
            Tonal::Burn(Range::Highlights)
        );
        cx.click(window, "tool-mode-Sponge").unwrap();
        cx.click(window, "sponge-Saturate").unwrap();
        assert_eq!(
            cx.read(view, |e| e.tonal_operation()).unwrap(),
            Tonal::Saturate
        );
        cx.simulate_keystrokes(window, "3 v").unwrap();
        cx.click(window, "tonal-tool").unwrap();
        cx.read(view, |e| {
            assert_eq!(e.tools.tool, Tool::Sponge);
            assert_eq!(e.tools.brush.opacity, 0.3);
            assert_eq!(e.session().document, original);
        })
        .unwrap();
        for id in [
            "tool-mode-Dodge",
            "tool-mode-Burn",
            "tool-mode-Sponge",
            "sponge-Saturate",
            "sponge-Desaturate",
        ] {
            let bounds = cx.element_bounds(window, id).unwrap();
            assert!(bounds.height <= 28., "{id}: {bounds:?}");
            assert!(bounds.width > 30., "{id}: {bounds:?}");
        }
    }
}
