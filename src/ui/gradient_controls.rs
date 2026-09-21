//! GradientControls.swift: inline settings and preview-only commit actions.
use super::dropdown::Dropdown;
use super::*;
use compositor::gradient::{Shape, Style};
use quickgui::{PickerItem, SelectPopoverLayout};

fn label(style: Style) -> &'static str {
    match style {
        Style::ForegroundToBackground => "Foreground to Background",
        Style::ForegroundToTransparent => "Foreground to Transparent",
    }
}
pub(super) fn new() -> Dropdown<Style> {
    let mut menu = Dropdown::new(
        [
            Style::ForegroundToBackground,
            Style::ForegroundToTransparent,
        ]
        .map(|style| PickerItem::new(label(style), style).id(label(style))),
    )
    .expect("Gradient style IDs are unique")
    .with_layout(SelectPopoverLayout::new(205., 24.).trigger_height(24.));
    menu.select_id(label(Style::ForegroundToTransparent));
    menu
}

impl Editor {
    pub(super) fn sync_gradient_picker(&mut self) {
        self.tools
            .gradient_picker
            .select_id(label(self.tools.gradient.style));
    }
    fn gradient_style_picker(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        self.tools.gradient_picker.element(
            cx,
            "gradient-style",
            "Colors",
            |this| &mut this.tools.gradient_picker,
            Self::tool_header_control(label(self.tools.gradient.style))
                .w(205.)
                .flex_row()
                .items_center()
                .child(div().flex_1())
                .child(Icon::PopupChevron.element(14.)),
            |this, style, cx| {
                this.tools.gradient.style = style;
                let result = this.refresh_gradient();
                this.operation_result(alerts::Operation::Paint, result, cx);
            },
        )
    }
    pub(super) fn gradient_controls(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let edit = self.pending_gradient.is_some();
        let mut shapes = div()
            .flex_row()
            .p(2.)
            .gap(2.)
            .rounded(6.)
            .bg(Color::rgb8(29, 29, 29));
        for (shape, label) in [(Shape::Linear, "Linear"), (Shape::Radial, "Radial")] {
            shapes = shapes.child(
                Self::segment(label, self.tools.gradient.shape == shape)
                    .tooltip("Linear runs along the line; Radial spreads out from the start point")
                    .on_click(
                        cx.listener(format!("gradient-shape-{label}"), move |this, cx| {
                            this.tools.gradient.shape = shape;
                            let result = this.refresh_gradient();
                            this.operation_result(alerts::Operation::Paint, result, cx);
                        }),
                    ),
            );
        }
        let colors = self.gradient_preview_colors();
        let swatch = div()
            .id("gradient-swatch")
            .w(56.)
            .h(18.)
            .flex_shrink_0()
            .relative()
            .rounded(3.)
            .overflow_hidden()
            .bg(Color::WHITE)
            .child(
                quickgui::canvas(|_, painter| {
                    for y in 0..5 {
                        for x in 0..14 {
                            if (x + y) % 2 == 0 {
                                painter.fill_rect(
                                    quickgui::Rect::new(x as f32 * 4., y as f32 * 4., 4., 4.),
                                    Color::rgb8(128, 128, 128).with_alpha(0.45),
                                );
                            }
                        }
                    }
                })
                .absolute()
                .size_full(),
            )
            .child(div().absolute().size_full().bg_linear_gradient(
                quickgui::GradientDirection::ToRight,
                colors.map(|[r, g, b, a]| Color::rgba8(r, g, b, a)),
            ))
            .child(
                div()
                    .absolute()
                    .size_full()
                    .rounded(3.)
                    .border(1., Color::rgba8(0, 0, 0, 128)),
            )
            .accessibility_hidden(true);
        let mut row = div()
            .flex_row()
            .items_center()
            .gap(12.)
            .flex_1()
            .child(shapes)
            .child(swatch)
            .child(self.gradient_style_picker(cx))
            .child(
                Self::check_control("Reverse", self.tools.gradient.reversed).on_click(cx.listener(
                    "gradient-reverse",
                    |this, cx| {
                        this.tools.gradient.reversed = !this.tools.gradient.reversed;
                        let result = this.refresh_gradient();
                        this.operation_result(alerts::Operation::Paint, result, cx);
                    },
                )),
            )
            .child(text("Opacity").text_size(12.).line_height(15.))
            .child(
                self.scalar_slider(
                    cx,
                    "gradient-opacity-slider",
                    "Opacity",
                    super::scalar_controls::Scalar::GradientOpacity,
                    (1., 100.),
                    100.,
                )
                .tooltip("Press 1–9 for 10–90%, 0 for 100%"),
            )
            .child(Self::unit_suffix(
                self.brush_value(
                    cx,
                    "gradient-opacity",
                    super::scalar_controls::Scalar::GradientOpacity,
                    (1., 100.),
                )
                .w(42.)
                .tooltip("Press 1–9 for 10–90%, 0 for 100%"),
                "%",
            ))
            .child(div().flex_1());
        if self.tools.mask_target {
            row = row.child(
                text("Mask")
                    .text_size(12.)
                    .line_height(15.)
                    .text_color(Color::rgb8(160, 160, 160)),
            );
        }
        if edit {
            row = row
                .child(Self::tool_header_control("Cancel").on_click(cx.listener(
                    "gradient-cancel",
                    |this, cx| {
                        this.pending_gradient = None;
                        this.session_mut().cancel();
                        this.changed(cx);
                    },
                )))
                .child(
                    Self::tool_header_control("Apply")
                        .disabled(!edit)
                        .on_click(cx.listener("gradient-apply", |this, cx| {
                            let result = this.commit_gradient();
                            this.operation_result(alerts::Operation::Paint, result, cx);
                        })),
                );
        }
        row
    }
}
