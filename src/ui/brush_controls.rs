use super::dropdown::Dropdown;
use super::scalar_controls::Scalar;
use super::*;
use compositor::filters::Healing;
use quickgui::{PickerItem, SelectPopoverLayout};

pub(super) fn mask_picker() -> Dropdown<bool> {
    let mut menu = Dropdown::new([
        PickerItem::new("Black · Hide", false).id("black"),
        PickerItem::new("White · Reveal", true).id("white"),
    ])
    .expect("Mask paint IDs are unique")
    .with_layout(SelectPopoverLayout::new(180., 24.).trigger_height(24.));
    menu.select_id("black");
    menu
}

impl Editor {
    pub(super) fn brush_header(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let sampled = matches!(self.tools.tool, Tool::Brush | Tool::Erase)
            && matches!(
                self.tools.brush_shape,
                compositor::brush::sampled::Shape::Sampled(_)
            );
        let opacity_label = match self.tools.tool {
            Tool::Dodge | Tool::Burn => "Exposure",
            Tool::Blur | Tool::Smudge | Tool::Liquify | Tool::Sponge => "Strength",
            _ => "Opacity",
        };
        let mut row = div()
            .flex_row()
            .items_center()
            .gap(12.)
            .flex_shrink_0()
            .child(self.scrub_label(
                cx,
                "brush-size-label",
                Scalar::BrushSize,
                (1., 2000.),
                1.,
                text("Size").text_size(12.).line_height(15.),
            ))
            .child(
                self.scrub_unit_suffix(
                    self.brush_value(cx, "brush-size", Scalar::BrushSize, (1., 2000.))
                        .w(48.),
                    "px",
                    cx,
                    "scrub-unit-brushsize",
                    Scalar::BrushSize,
                    (1., 2000.),
                ),
            );
        if !sampled {
            row = row
                .child(self.scrub_label(
                    cx,
                    "brush-hardness-label",
                    Scalar::BrushHardness,
                    (0., 100.),
                    1.,
                    text("Hardness").text_size(12.).line_height(15.),
                ))
                .child(self.scalar_slider(
                    cx,
                    "brush-hardness-slider",
                    "Hardness",
                    Scalar::BrushHardness,
                    (0., 100.),
                    100.,
                ))
                .child(
                    self.scrub_unit_suffix(
                        self.brush_value(cx, "brush-hardness", Scalar::BrushHardness, (0., 100.))
                            .w(42.),
                        "%",
                        cx,
                        "scrub-unit-brushhardness",
                        Scalar::BrushHardness,
                        (0., 100.),
                    ),
                );
        }
        row = row
            .child(self.scrub_label(
                cx,
                "brush-opacity-label",
                Scalar::BrushOpacity,
                (1., 100.),
                1.,
                text(opacity_label).text_size(12.).line_height(15.),
            ))
            .child(
                self.scalar_slider(
                    cx,
                    "brush-opacity-slider",
                    opacity_label,
                    Scalar::BrushOpacity,
                    (1., 100.),
                    100.,
                )
                .tooltip("Press 1–9 for 10–90%, 0 for 100%"),
            )
            .child(
                self.scrub_unit_suffix(
                    self.brush_value(cx, "brush-opacity", Scalar::BrushOpacity, (1., 100.))
                        .tooltip("Press 1–9 for 10–90%, 0 for 100%")
                        .w(42.),
                    "%",
                    cx,
                    "scrub-unit-brushopacity",
                    Scalar::BrushOpacity,
                    (1., 100.),
                ),
            );
        if matches!(self.tools.tool, Tool::Brush | Tool::Erase) {
            let label = match &self.tools.brush_shape {
                compositor::brush::sampled::Shape::Round => "Round tip",
                compositor::brush::sampled::Shape::Sampled(_) => "Sampled tip",
            };
            row = row.child(Self::control(label).on_click(cx.listener(
                "brush-tip-picker",
                |this, cx| {
                    this.open_brush_tips();
                    cx.invalidate();
                },
            )));
            row = row
                .child(self.scrub_label(cx,"brush-smoothing-label",Scalar::BrushSmoothing,(0.,100.),1.,text("Smoothing").text_size(12.).line_height(15.)))
                .child(self.scalar_slider(
                    cx,
                    "brush-smoothing-slider",
                    "Smoothing",
                    Scalar::BrushSmoothing,
                    (0., 100.),
                    80.,
                ).tooltip("The brush trails the pointer by this many screen points; 0 follows exactly. Catches up on release."))
                .child(self.brush_value(cx, "brush-smoothing", Scalar::BrushSmoothing, (0., 100.))
                    .accessibility_label("Smoothing")
                    .w(42.));
        }
        if matches!(
            self.tools.tool,
            Tool::Brush
                | Tool::Erase
                | Tool::Clone
                | Tool::Blur
                | Tool::Heal
                | Tool::Dodge
                | Tool::Burn
                | Tool::Sponge
        ) {
            row = row
                .child(Self::segment("Pressure", self.tools.pen_pressure)
                    .id("pen-pressure")
                    .tooltip("Use tablet pressure for brush size. Mouse strokes keep the configured size.")
                    .on_click(cx.listener("pen-pressure", |this, cx| { this.tools.pen_pressure = !this.tools.pen_pressure; cx.invalidate(); })))
                .child(Self::segment("Tilt", self.tools.pen_tilt)
                    .id("pen-tilt")
                    .tooltip("Use tablet tilt for an elliptical brush tip.")
                    .on_click(cx.listener("pen-tilt", |this, cx| { this.tools.pen_tilt = !this.tools.pen_tilt; cx.invalidate(); })));
        }
        if self.tools.mask_target {
            row = row.child(self.mask_paint_picker(cx));
        } else if !matches!(
            self.tools.tool,
            Tool::Clone
                | Tool::Blur
                | Tool::Smudge
                | Tool::Liquify
                | Tool::Dodge
                | Tool::Burn
                | Tool::Sponge
        ) {
            row = row.child(self.header_foreground(cx, palette_controls::ForegroundStyle::Brush));
        }
        row
    }

    fn mask_paint_picker(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let label = if self.tools.mask_paint_white {
            "White · Reveal"
        } else {
            "Black · Hide"
        };
        let selector = self.tools.mask_paint_picker.element(
            cx,
            "mask-paint",
            "Paint",
            |this| &mut this.tools.mask_paint_picker,
            Self::tool_header_control(label)
                .flex_1()
                .min_w(0.)
                .flex_row()
                .items_center()
                .child(div().flex_1())
                .child(Icon::PopupChevron.element(14.)),
            |this, white, cx| {
                this.tools.mask_paint_white = white;
                cx.invalidate();
            },
        );
        div()
            .flex_row()
            .items_center()
            .gap(8.)
            .w(180.)
            .flex_shrink_0()
            .child(text("Paint").text_size(12.).line_height(15.))
            .child(selector)
    }
    pub(super) fn segment(label: &'static str, active: bool) -> Element {
        button()
            .flex_row()
            .items_center()
            .justify_center()
            .whitespace_nowrap()
            .text_size(12.)
            .line_height(15.)
            .focus(super::controls::focus_outline)
            .selected(active)
            .h(24.)
            .px(9.)
            .rounded(5.)
            .bg(if active {
                Color::rgb8(76, 76, 76)
            } else {
                Color::TRANSPARENT
            })
            .child(text(label))
    }
    pub(super) fn brush_mode_controls(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let mut controls = div().flex_row().items_center().gap(12.).flex_shrink_0();
        if self.tools.tool == Tool::Wand {
            controls = controls
                .child(div().flex_row().items_center().gap(6.).flex_shrink_0()
                    .child(self.scrub_label(cx,"wand-tolerance-label",Scalar::WandTolerance,(0.,255.),1.,text("Tolerance").text_size(12.).line_height(15.)))
                    .child(self.brush_value(cx, "wand-tolerance", Scalar::WandTolerance, (0., 255.))
                        .text_right())
                    .tooltip("How far each color channel (0–255) can differ from the clicked color and still be selected"))
                .child(self.wand_sample_picker(cx));
        }

        if self.tools.tool == Tool::Clone {
            controls = controls.child(
                Self::check_control("Aligned", self.tools.clone_aligned)
                    .tooltip("Keep the source moving with the brush between strokes; off starts every stroke at the source point").on_click(cx.listener(
                    "clone-alignment",
                    |this, cx| {
                        this.tools.clone_aligned = !this.tools.clone_aligned;
                        this.tools.clone_offset = None;
                        cx.invalidate();
                    },
                )),
            );
        }
        if matches!(self.tools.tool, Tool::Clone | Tool::Wand | Tool::Object) {
            let clone = self.tools.tool == Tool::Clone;
            let object = self.tools.tool == Tool::Object;
            let all = if clone {
                self.tools.clone_sample_all
            } else if object {
                self.tools.object_sample_all
            } else {
                self.tools.wand_sample_all
            };
            let mut modes = div()
                .flex_row()
                .p(2.)
                .gap(2.)
                .rounded(6.)
                .bg(Color::rgb8(29, 29, 29));
            for (value, label) in [(false, "This Layer"), (true, "All Layers")] {
                modes = modes.child(Self::segment(label, value == all).on_click(cx.listener(
                    format!("sample-layers-{value}"),
                    move |this, cx| {
                        if clone {
                            this.tools.clone_sample_all = value;
                        } else if object {
                            this.tools.object_sample_all = value;
                        } else {
                            this.tools.wand_sample_all = value;
                        }
                        cx.invalidate();
                    },
                )));
            }
            controls = controls.child(modes.tooltip(if clone {
                "Copy from the active layer only, or from every visible layer as shown"
            } else {
                "Read colors from the active layer only, or from every visible layer as shown"
            }));
        }
        if self.tools.tool == Tool::Wand {
            controls = controls.child(
                Self::check_control("Contiguous", self.tools.wand_contiguous)
                    .tooltip("Select only similar pixels connected to the one you click; off selects them everywhere").on_click(
                    cx.listener("wand-contiguous", |this, cx| {
                        this.tools.wand_contiguous = !this.tools.wand_contiguous;
                        cx.invalidate();
                    }),
                ),
            );
        }
        if self.tools.tool == Tool::Heal {
            let mut modes = div()
                .flex_row()
                .p(2.)
                .gap(2.)
                .rounded(6.)
                .bg(Color::rgb8(29, 29, 29))
                .flex_1()
                .min_w(0.);
            for (mode, label) in [
                (Healing::ContentAware, "Content-Aware"),
                (Healing::CreateTexture, "Create Texture"),
                (Healing::Proximity, "Proximity Match"),
            ] {
                modes = modes.child(
                    Self::segment(label, self.tools.healing == mode)
                        .flex_1()
                        .px(2.)
                        .on_click(cx.listener(format!("healing-{label}"), move |this, cx| {
                            this.tools.healing = mode;
                            cx.invalidate();
                        })),
                );
            }
            controls = controls.child(
                div()
                    .id("healing-type")
                    .accessibility_label("Type")
                    .flex_row()
                    .items_center()
                    .gap(8.)
                    .w(330.)
                    .flex_shrink_0()
                    .child(text("Type").text_size(12.).line_height(15.))
                    .child(modes),
            );
        }
        controls
    }
}
