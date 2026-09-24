//! Layout and summaries mirror CanvasSizeSheet.swift and ImageSizeSheet.swift.
use super::*;
use compositor::{canvas_size, invalid};

fn callout(value: impl Into<Arc<str>>) -> Element {
    text(value)
        .text_size(12.)
        .line_height(15.)
        .text_color(Color::rgb8(180, 180, 180))
        .wrap()
}
fn memory([width, height]: [u32; 2]) -> String {
    super::byte_count::Style::Memory.format(u64::from(width) * u64::from(height) * 4)
}
fn row(label: &'static str, input: Element, width: f32) -> Element {
    div()
        .flex_row()
        .items_center()
        .gap(10.)
        .child(text(label).text_size(13.).line_height(16.).w(width))
        .child(input)
}
impl Editor {
    pub(super) fn size_result(
        &self,
        action: Action,
        fields: &[(&str, String)],
    ) -> Result<[u32; 2]> {
        let number = |index: usize| {
            fields
                .get(index)
                .and_then(|f| f.1.parse::<f64>().ok())
                .filter(|v| v.is_finite())
                .ok_or_else(|| invalid("Enter valid, finite dimensions."))
        };
        let dimensions = [number(0)?, number(1)?];
        let doc = &self.session().document;
        match action {
            Action::CanvasSize => {
                let unit = match fields.get(4).map(|f| f.1.as_str()) {
                    Some("px") => canvas_size::Unit::Pixels,
                    Some("percent") => canvas_size::Unit::Percent,
                    Some("inches") => canvas_size::Unit::Inches,
                    Some("cm") => canvas_size::Unit::Centimeters,
                    _ => return Err(invalid("Choose a canvas unit.")),
                };
                canvas_size::dimensions(
                    doc,
                    dimensions,
                    unit,
                    fields.get(5).is_some_and(|f| f.1 == "1"),
                )
            }
            Action::ImageSize => {
                self.image_sizing
                    .dimensions(dimensions, [doc.width, doc.height], number(2)?)
            }
            _ => Err(invalid(
                "Open Canvas Size or Image Size to change dimensions.",
            )),
        }
    }
    fn size_dimensions(
        &self,
        cx: &mut ViewContext<'_, Self>,
        fields: &[(&str, String)],
        label_width: f32,
    ) -> Element {
        let mut controls = div().flex_col().gap(16.);
        for (index, label) in ["Width", "Height"].into_iter().enumerate() {
            controls = controls.child(row(
                label,
                self.size_number_input(cx, index, &fields[index].1, 3)
                    .flex_1()
                    .min_w(0.),
                label_width,
            ));
        }
        controls
    }
    pub(super) fn size_dialog_view(
        &self,
        cx: &mut ViewContext<'_, Self>,
        action: Action,
        fields: &[(&str, String)],
    ) -> Element {
        if matches!(action, Action::CanvasSize) {
            self.canvas_size_view(cx, fields)
        } else {
            self.image_size_view(cx, fields)
        }
    }
    fn canvas_size_view(
        &self,
        cx: &mut ViewContext<'_, Self>,
        fields: &[(&str, String)],
    ) -> Element {
        let doc = &self.session().document;
        let mut controls = div().flex_col().gap(16.).flex_shrink_0()
            .child(text(format!("Current: {} × {} pixels", doc.width, doc.height)).text_size(13.).line_height(16.))
            .child(callout(format!("{} uncompressed RGBA canvas", memory([doc.width, doc.height]))))
            .child(Self::divider())
            .child(row("Units", self.size_unit_picker(cx, Action::CanvasSize), 60.))
            .child(self.size_dimensions(cx, fields, 60.))
            .child(Self::check_control("Relative to current dimensions", fields[5].1 == "1").text_size(13.).line_height(16.)
                .on_click(cx.listener(self.size_field_id(5), |this, cx| {
                    let checked = matches!(&this.modal, Some(Form::Edit { fields, .. }) if fields[5].1 == "1");
                    this.update_form_field(5, if checked { "0" } else { "1" });
                    this.changed(cx);
                })))
            .child(self.dimension_controls(cx, Action::CanvasSize));
        controls = controls.child(match self.size_result(Action::CanvasSize, fields) {
            Ok(size) => callout(format!(
                "New: {} × {} pixels · {} uncompressed",
                size[0],
                size[1],
                memory(size)
            )),
            Err(_) => callout("Final dimensions must be 1–30,000 pixels per side.")
                .text_color(Color::rgb8(255, 159, 10)),
        });
        controls = controls
            .child(self.canvas_anchor_controls(cx, fields))
            .child(row("Canvas extension", self.size_fill_picker(cx), 115.));
        if fields[6].1.starts_with('#') {
            let well = match forms::parse_color(&fields[6].1) {
                Ok([r, g, b, _]) => {
                    Self::color_well(Color::rgb8(r, g, b), "Choose canvas extension color")
                }
                Err(_) => Self::control("Invalid color"),
            };
            controls = controls.child(
                div()
                    .flex_row()
                    .items_center()
                    .child(text("Extension color").text_size(13.).line_height(16.))
                    .child(div().flex_1())
                    .child(
                        well.on_click(cx.listener("canvas-extension-custom", |this, cx| {
                            this.open_canvas_extension_picker();
                            cx.invalidate();
                        })),
                    ),
            );
        }
        controls
    }
    fn image_size_view(
        &self,
        cx: &mut ViewContext<'_, Self>,
        fields: &[(&str, String)],
    ) -> Element {
        let doc = &self.session().document;
        let mut controls = div()
            .flex_col()
            .gap(18.)
            .flex_shrink_0()
            .child(
                text(format!("Current: {} × {} pixels", doc.width, doc.height))
                    .text_size(13.)
                    .line_height(16.)
                    .text_color(Color::rgb8(180, 180, 180)),
            )
            .child(row(
                "Units",
                self.size_unit_picker(cx, Action::ImageSize),
                75.,
            ))
            .child(self.size_dimensions(cx, fields, 75.).gap(18.))
            .child(self.dimension_controls(cx, Action::ImageSize))
            .child(
                div()
                    .flex_row()
                    .items_center()
                    .gap(8.)
                    .child(
                        text("Resolution")
                            .text_size(13.)
                            .line_height(16.)
                            .flex_shrink_0(),
                    )
                    .child(
                        self.size_number_input(cx, 2, &fields[2].1, 3)
                            .flex_1()
                            .min_w(0.),
                    )
                    .child(
                        text("pixels/inch")
                            .text_size(13.)
                            .line_height(16.)
                            .flex_shrink_0()
                            .text_color(Color::rgb8(180, 180, 180)),
                    ),
            )
            .child(self.image_resample_control(cx));
        if self.image_sizing.resamples() {
            controls = controls.child(row("Sampling", self.size_sampling_picker(cx), 75.))
                .child(callout("Resizes layer pixels and applies existing transforms. Undo restores the originals."));
        } else {
            controls = controls.child(callout(
                "Only print dimensions and resolution change. Pixels stay unchanged.",
            ));
        }
        controls.child(match self.size_result(Action::ImageSize, fields) {
            Ok(size) => callout(format!("Result: {} × {} pixels", size[0], size[1])),
            Err(_) => callout(
                "Use 1–30,000 pixels per side, up to 200 megapixels, and 1–9,600 pixels/inch.",
            )
            .text_color(Color::rgb8(255, 159, 10)),
        })
    }
    fn canvas_anchor_controls(
        &self,
        cx: &mut ViewContext<'_, Self>,
        fields: &[(&str, String)],
    ) -> Element {
        let mut grid = div().flex_col().gap(3.);
        let mut selected = "Center";
        for (vertical, labels) in [
            ("top", ["Top left", "Top center", "Top right"]),
            ("center", ["Middle left", "Center", "Middle right"]),
            ("bottom", ["Bottom left", "Bottom center", "Bottom right"]),
        ] {
            let mut row = div().flex_row().gap(3.);
            for (horizontal, label) in ["left", "center", "right"].into_iter().zip(labels) {
                let active = fields[2].1 == horizontal && fields[3].1 == vertical;
                if active {
                    selected = label;
                }
                row = row.child(
                    button()
                        .size(27., 27.)
                        .p(0.)
                        .flex_row()
                        .items_center()
                        .justify_center()
                        .rounded(13.5)
                        .selected(active)
                        .focus(super::controls::focus_outline)
                        .border(1., Color::rgb8(77, 77, 77))
                        .bg(if active {
                            Color::rgb8(65, 100, 143)
                        } else {
                            Color::rgb8(38, 38, 38)
                        })
                        .accessibility_label(label)
                        .accessibility_value(if active { "Selected" } else { "" })
                        .tooltip(label)
                        .child(if active {
                            div()
                                .size(9., 9.)
                                .rounded(5.)
                                .bg(Color::rgb8(210, 225, 245))
                        } else {
                            Icon::Circle.element(10.)
                        })
                        .on_click(cx.listener(
                            format!("anchor-{vertical}-{horizontal}"),
                            move |this, cx| {
                                this.update_form_field(2, horizontal);
                                this.update_form_field(3, vertical);
                                this.changed(cx);
                            },
                        )),
                );
            }
            grid = grid.child(row);
        }
        div().flex_row().gap(24.)
            .child(div().flex_col().gap(8.).flex_shrink_0().child(text("Anchor").text_size(13.).line_height(16.)).child(grid))
            .child(div().flex_col().gap(8.).mt(28.).flex_1().min_w(0.)
                .child(text(selected).text_size(12.).line_height(15.).font_bold())
                .child(callout("Keeps this point fixed. Artwork is not scaled; cropped content remains outside the canvas.")))
    }
}
