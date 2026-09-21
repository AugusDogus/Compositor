use super::*;
use compositor::{background::Quality, invalid};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Mode {
    Basic,
    Advanced,
}

impl Editor {
    pub(super) fn background_values(&self, values: &[String]) -> Result<Quality> {
        if self.background_mode == Mode::Basic {
            return Ok(Quality::Basic);
        }
        let number = |index: usize| {
            values
                .get(index)
                .and_then(|v| v.trim().parse::<f64>().ok())
                .ok_or_else(|| invalid("Enter a number for each background refinement setting."))
        };
        let quality = Quality::Advanced {
            refine_edges: number(0)?,
            contrast: number(1)?,
            shift_edge: number(2)?,
        };
        quality.validate()?;
        Ok(quality)
    }

    pub(super) fn background_controls(
        &self,
        cx: &mut ViewContext<'_, Self>,
        action: Action,
    ) -> Element {
        if !matches!(action, Action::RemoveBackground) {
            return div();
        }
        let mut choices = div().flex_row().gap(2.).p(2.).rounded(6.)
            .bg(Color::rgb8(29, 29, 29))
            .accessibility_label("Quality")
            .tooltip("Basic is quick; Advanced refines the mask against the layer's own detail, for hair and fur");
        for (id, label, mode) in [
            ("background-basic", "Basic", Mode::Basic),
            ("background-advanced", "Advanced", Mode::Advanced),
        ] {
            choices = choices.child(
                Self::segment(label, self.background_mode == mode)
                    .text_size(13.)
                    .line_height(16.)
                    .flex_1()
                    .justify_center()
                    .on_click(cx.listener(id, move |this, cx| {
                        this.background_mode = mode;
                        this.refresh_filter();
                        this.changed(cx);
                    })),
            );
        }
        div().flex_col().gap(16.)
            .child(text("Hide the background behind a layer mask, keeping the foreground subjects. The pixels stay, so the background can be painted back at any time.").text_size(13.).line_height(16.).wrap())
            .child(choices)
    }
}
