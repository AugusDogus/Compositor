//! The original overlapping swatches with small swap and reset controls in their corners.
use super::*;

pub(super) enum ForegroundStyle {
    Brush,
    Shape,
}

impl Editor {
    pub(super) fn color_well(color: Color, label: &'static str) -> Element {
        button()
            .size(44., 24.)
            .flex_shrink_0()
            .rounded(5.)
            .bg(color)
            .border(1., Color::rgb8(120, 120, 120))
            .focus(super::controls::focus_outline)
            .accessibility_label(label)
    }

    pub(super) fn header_foreground(
        &self,
        cx: &mut ViewContext<'_, Self>,
        style: ForegroundStyle,
    ) -> Element {
        let [r, g, b, _] = self.tools.brush.color;
        let color = Color::rgb8(r, g, b);
        let (label, width, swatch) = match style {
            ForegroundStyle::Brush => (
                "Color",
                34.,
                div().size_full().rounded(4.).bg(Color::BLACK).p(1.).child(
                    div()
                        .size_full()
                        .rounded(3.)
                        .border(1., Color::WHITE)
                        .bg(color),
                ),
            ),
            ForegroundStyle::Shape => (
                "Fill",
                36.,
                div()
                    .size_full()
                    .rounded(3.)
                    .bg(color)
                    .border(1., Color::rgba8(0, 0, 0, 128)),
            ),
        };
        div()
            .flex_row()
            .items_center()
            .gap(6.)
            .flex_shrink_0()
            .child(text(label).text_size(12.).line_height(15.))
            .child(
                button()
                    .size(width, 18.)
                    .p(0.)
                    .rounded(4.)
                    .bg(Color::TRANSPARENT)
                    .accessibility_label("Foreground color")
                    .tooltip(match style {
                        ForegroundStyle::Shape => {
                            "Shapes fill with the foreground color; click to change it"
                        }
                        ForegroundStyle::Brush => "Foreground color",
                    })
                    .child(swatch)
                    .on_click(cx.listener("header-foreground", |this, cx| {
                        this.action(Action::Color, cx)
                    })),
            )
    }

    pub(super) fn palette_controls(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let [foreground, background] = self
            .palette_colors(self.tools.mask_target)
            .map(|[r, g, b, _]| Color::rgb8(r, g, b));
        div()
            .w(36.)
            .h(36.)
            .flex_shrink_0()
            .relative()
            .child(
                swatch(background, "Background color")
                    .absolute()
                    .left(12.)
                    .top(12.)
                    .on_click(cx.listener("palette-background", |this, cx| {
                        this.action(Action::Color, cx);
                        if let Some(Form::Color(picker)) = &mut this.modal {
                            picker.select_background();
                        } else if let Some(Form::MaskColor(target)) = &mut this.modal {
                            *target = palette::MaskSwatch::Background;
                        }
                    })),
            )
            .child(
                swatch(foreground, "Foreground color")
                    .absolute()
                    .left(0.)
                    .top(0.)
                    .on_click(cx.listener(320_u64, |this, cx| this.action(Action::Color, cx))),
            )
            .child(
                button()
                    .absolute()
                    .left(27.)
                    .top(-3.)
                    .size(12., 12.)
                    .p(0.)
                    .items_center()
                    .justify_center()
                    .accessibility_label("Swap colors")
                    .tooltip("Swap foreground and background (X)")
                    .child(Icon::ArrowLeftRight.element(9.).rotate_degrees(45.))
                    .on_click(cx.listener("palette-swap", |this, cx| {
                        let result = this.change_palette(true);
                        this.operation_result(alerts::Operation::Paint, result, cx);
                    })),
            )
            .child(
                button()
                    .absolute()
                    .left(-1.)
                    .top(27.)
                    .size(12., 12.)
                    .p(0.)
                    .items_center()
                    .justify_center()
                    .accessibility_label("Default colors")
                    .tooltip("Default colors (D)")
                    .child(Icon::RotateCcw.element(7.5))
                    .on_click(cx.listener("palette-reset", |this, cx| {
                        let result = this.change_palette(false);
                        this.operation_result(alerts::Operation::Paint, result, cx);
                    })),
            )
    }
}

pub(super) fn swatch(color: Color, label: impl Into<Arc<str>>) -> Element {
    let label = label.into();
    button()
        .size(24., 24.)
        .p(1.)
        .rounded(6.)
        .bg(Color::BLACK)
        .accessibility_label(label.clone())
        .tooltip(label)
        .child(
            div()
                .size_full()
                .rounded(5.)
                .border(1.5, Color::WHITE)
                .bg(color),
        )
}
