use super::*;

impl Editor {
    fn text_number(
        &self,
        cx: &mut ViewContext<'_, Self>,
        draft: &Draft,
        index: usize,
        label: &'static str,
    ) -> Element {
        div()
            .flex_col()
            .gap(5.)
            .flex_1()
            .min_w(0.)
            .child(text(label).text_size(12.))
            .child(
                Self::text_field(draft.numbers[index].clone())
                    .id(format!("text-number-{index}"))
                    .w_full()
                    .on_input(cx.input_listener(
                        format!("text-number-{index}"),
                        move |this, value, cx| {
                            if let Some(Form::Text(draft)) = &mut this.modal {
                                draft.numbers[index] = value.into();
                                draft.error.clear();
                            }
                            cx.invalidate();
                        },
                    )),
            )
    }

    fn text_font_controls(&self, cx: &mut ViewContext<'_, Self>, draft: &Draft) -> Element {
        let mut section = div().flex_col().gap(5.).child(text("Font").text_size(12.));
        section = section.child(
            div()
                .flex_row()
                .gap(6.)
                .child(
                    Self::text_field(draft.style.font_name.clone())
                        .id("text-font")
                        .flex_1()
                        .on_input(cx.input_listener("text-font", |this, value, cx| {
                            if let Some(Form::Text(draft)) = &mut this.modal {
                                draft.style.font_name = value.into();
                                draft.fonts_open = true;
                            }
                            cx.invalidate();
                        })),
                )
                .child(Self::segment("Browse", false).on_click(cx.listener(
                    "text-font-browse",
                    |this, cx| {
                        if let Some(Form::Text(draft)) = &mut this.modal {
                            draft.fonts_open = !draft.fonts_open;
                        }
                        cx.invalidate();
                    },
                ))),
        );
        if draft.fonts_open {
            let mut list = div()
                .flex_col()
                .max_h(130.)
                .overflow_y_scroll()
                .bg(Color::rgb8(28, 28, 28));
            if let Some(renderer) = &self.text_renderer {
                let query = draft.style.font_name.to_lowercase();
                // Exact family in the field means Browse displays all installed families.
                let exact = renderer
                    .font_names()
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(&query));
                for (i, family) in renderer
                    .font_names()
                    .iter()
                    .enumerate()
                    .filter(|(_, name)| exact || name.to_lowercase().contains(&query))
                {
                    let family = family.clone();
                    list = list.child(Self::control(family.clone()).w_full().on_click(
                        cx.listener(format!("text-font-choice-{i}"), move |this, cx| {
                            if let Some(Form::Text(draft)) = &mut this.modal {
                                draft.style.font_name = family.clone();
                                draft.fonts_open = false;
                            }
                            cx.invalidate();
                        }),
                    ));
                }
            }
            section = section.child(list);
        }
        if self.text_renderer.as_ref().is_some_and(|renderer| {
            !renderer
                .font_names()
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&draft.style.font_name))
        }) {
            section = section.child(text("This font is unavailable. Edited text will use Inter Variable until you choose an installed font.").text_size(11.).text_color(Color::rgb8(210,180,115)).wrap());
        }
        section
    }

    fn text_preview_view(&mut self, draft: &Draft) -> Element {
        let style = match draft.parsed() {
            Ok(style) => style,
            Err(error) => return text(error.to_string()).text_size(12.).wrap(),
        };
        if self
            .text_preview
            .as_ref()
            .is_none_or(|preview| preview.style != style)
        {
            let mut small = style.clone();
            let scale = (72. / style.font_size)
                .min(
                    style
                        .box_size
                        .map_or(1., |size| 1024. / size[0].max(size[1])),
                )
                .min(1.);
            small.font_size = (style.font_size * scale).max(1.);
            small.tracking *= scale;
            small.leading *= scale;
            small.box_size = small
                .box_size
                .map(|size| size.map(|v| (v * scale).max(16.)));
            let image = self
                .text_renderer
                .get_or_insert_with(TextRenderer::default)
                .render(&small)
                .map_err(|error| error.to_string())
                .and_then(|pixels| {
                    let scale = (490. / f64::from(pixels.width()))
                        .min(96. / f64::from(pixels.height()))
                        .min(1.);
                    let width = (f64::from(pixels.width()) * scale).round().max(1.) as u32;
                    let height = (f64::from(pixels.height()) * scale).round().max(1.) as u32;
                    let thumbnail = image::imageops::thumbnail(&pixels, width, height);
                    Image::from_rgba(thumbnail.width(), thumbnail.height(), thumbnail.into_raw())
                        .map_err(|error| format!("Text preview could not be displayed: {error}"))
                });
            self.text_preview = Some(Preview { style, image });
        }
        let preview = match &self.text_preview {
            Some(Preview {
                image: Ok(image), ..
            }) => quickgui::img(image)
                .w(image.width() as f32)
                .h(image.height() as f32)
                .flex_shrink_0(),
            Some(Preview {
                image: Err(error), ..
            }) => text(error.clone())
                .text_size(12.)
                .text_color(Color::rgb8(160, 30, 30))
                .wrap(),
            None => div(),
        };
        div()
            .id("text-preview")
            .h(115.)
            .w_full()
            .flex_row()
            .items_center()
            .justify_center()
            .p(8.)
            .bg(Color::rgb8(215, 215, 215))
            .rounded(5.)
            .overflow_hidden()
            .child(preview)
    }

    fn text_style_controls(&self, cx: &mut ViewContext<'_, Self>, draft: &Draft) -> Element {
        let mut controls = div()
            .flex_col()
            .gap(12.)
            .child(self.text_font_controls(cx, draft));
        controls = controls.child(
            div()
                .flex_row()
                .gap(12.)
                .child(self.text_number(cx, draft, 0, "Size (px)"))
                .child(self.text_number(cx, draft, 1, "Tracking (px)"))
                .child(self.text_number(cx, draft, 2, "Leading (0 = auto)")),
        );
        let mut alignment = div()
            .flex_row()
            .items_center()
            .gap(6.)
            .child(text("Alignment").text_size(12.));
        for (value, label) in [
            (Alignment::Left, "Left"),
            (Alignment::Center, "Center"),
            (Alignment::Right, "Right"),
        ] {
            alignment = alignment.child(
                Self::segment(label, draft.style.alignment == value).on_click(cx.listener(
                    format!("text-align-{label}"),
                    move |this, cx| {
                        if let Some(Form::Text(draft)) = &mut this.modal {
                            draft.style.alignment = value;
                        }
                        cx.invalidate();
                    },
                )),
            );
        }
        let rgb = draft.parsed().unwrap_or_else(|_| draft.style.clone());
        let swatch = button()
            .size(24., 24.)
            .bg(Color::rgb8(
                (rgb.red * 255.).round() as u8,
                (rgb.green * 255.).round() as u8,
                (rgb.blue * 255.).round() as u8,
            ))
            .border(1., Color::rgb8(100, 100, 100))
            .rounded(4.)
            .accessibility_label("Text color")
            .on_click(cx.listener("text-color-picker", |this, cx| {
                this.open_text_color_picker();
                cx.invalidate();
            }));
        controls = controls.child(
            alignment.child(div().flex_1()).child(swatch).child(
                Self::text_field(draft.color.clone())
                    .id("text-color")
                    .w(100.)
                    .flex_shrink_0()
                    .on_input(cx.input_listener("text-color", |this, value, cx| {
                        if let Some(Form::Text(draft)) = &mut this.modal {
                            draft.color = value.into();
                        }
                        cx.invalidate();
                    })),
            ),
        );
        controls.child(
            Self::check_control("Fixed paragraph box", draft.style.box_size.is_some()).on_click(
                cx.listener("text-box", |this, cx| {
                    if let Some(Form::Text(draft)) = &mut this.modal {
                        draft.style.box_size = if draft.style.box_size.is_some() {
                            None
                        } else {
                            Some([400., 200.])
                        };
                    }
                    cx.invalidate();
                }),
            ),
        )
    }

    pub(in crate::ui) fn text_editor_view(
        &mut self,
        cx: &mut ViewContext<'_, Self>,
        draft: &Draft,
    ) -> Element {
        let dialog = quickgui::Dialog::new("editor-dialog", true)
            .initial_focus("text-content")
            .restore_focus_to("workspace")
            .dismiss_on_backdrop(false);
        let mut contents = div()
            .flex_col()
            .gap(14.)
            .p(22.)
            .bg(Color::rgb8(43, 43, 43))
            .w(560_f32.min((cx.size().width - 40.).max(320.)))
            .rounded(10.)
            .max_h((cx.size().height - 60.).max(300.))
            .overflow_y_scroll();
        contents = contents.child(
            text(if draft.layer.is_some() {
                "Edit Text"
            } else {
                "New Text Layer"
            })
            .text_size(17.)
            .font_semibold(),
        );
        contents = contents.child(
            quickgui::text_area(draft.style.content.clone())
                .id("text-content")
                .w_full()
                .h(160.)
                .text_size(16.)
                .focus(controls::focus_outline)
                .bg(Color::rgb8(26, 26, 26))
                .text_input_padding(10.)
                .on_input(cx.input_listener("text-content", |this, value, cx| {
                    if let Some(Form::Text(draft)) = &mut this.modal {
                        draft.style.content = value.into();
                        draft.error.clear();
                    }
                    cx.invalidate();
                }))
                .on_key_down(cx.key_down_listener("text-content", |this, event, cx| {
                    this.text_key(&event.key, event.modifiers, cx);
                })),
        );
        contents = contents.child(self.text_style_controls(cx, draft));
        if draft.style.box_size.is_some() {
            contents = contents.child(
                div()
                    .flex_row()
                    .gap(12.)
                    .child(self.text_number(cx, draft, 3, "Box width (px)"))
                    .child(self.text_number(cx, draft, 4, "Box height (px)")),
            );
        }
        contents = contents.child(self.text_preview_view(draft));
        if !draft.error.is_empty() {
            contents = contents.child(
                text(draft.error.clone())
                    .text_color(Color::rgb8(255, 130, 120))
                    .wrap(),
            );
        }
        contents = contents.child(
            div()
                .flex_row()
                .justify_end()
                .gap(8.)
                .child(
                    Self::segment("Cancel", false)
                        .on_click(cx.listener("text-cancel", |this, cx| this.cancel_form(cx))),
                )
                .child(
                    Self::segment("Apply", true)
                        .on_click(cx.listener("text-apply", |this, cx| this.text_submit(cx))),
                ),
        );
        self.mount_form(cx, dialog, contents, 560., "Text", None)
    }
}
