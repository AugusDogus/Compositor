use super::*;

impl Editor {
    pub(super) fn photo_filter_fields(
        &self,
        cx: &mut ViewContext<'_, Self>,
        action: Action,
        fields: &[(&'static str, String)],
    ) -> Element {
        let Some((_, chosen)) = fields.get(2) else {
            return div();
        };
        let mut rows = div().flex_col().gap(16.).flex_shrink_0();
        rows = rows.child(self.photo_filter_presets(cx, chosen));
        rows = rows.child(self.photo_filter_swatch(cx, chosen));
        if let Some((_, value)) = fields.first()
            && let Some(control) = self.parameter_control(cx, action, 0, value)
        {
            rows = rows.child(control);
        }
        rows.child(self.form_toggle(
            cx,
            1,
            "Preserve luminosity",
            fields.get(1).is_some_and(|(_, value)| value == "1"),
        ))
    }
    fn photo_filter_presets(&self, cx: &mut ViewContext<'_, Self>, chosen: &str) -> Element {
        let mut row = div().flex_row().gap(4.);
        for (label, color) in [
            ("Warming", "#EC8A00"),
            ("Cooling", "#007ECC"),
            ("Sepia", "#AC7A33"),
        ] {
            let button = Self::segment(label, chosen.eq_ignore_ascii_case(color))
                .h(26.)
                .px(8.)
                .text_size(13.)
                .line_height(16.)
                .on_click(
                    cx.listener(format!("photo-filter-{label}"), move |this, cx| {
                        this.update_form_field(2, color);
                        this.changed(cx);
                    }),
                );
            row = row.child(button);
        }
        row
    }
    fn photo_filter_swatch(&self, cx: &mut ViewContext<'_, Self>, chosen: &str) -> Element {
        let rgb = compositor::palette::parse_hex(chosen).unwrap_or([0, 0, 0, 255]);
        let swatch = super::palette_controls::swatch(
            Color::rgb8(rgb[0], rgb[1], rgb[2]),
            "Photo Filter color",
        )
        .on_click(cx.listener("photo-filter-color", |this, cx| {
            this.open_photo_filter_picker();
            this.changed(cx);
        }));
        div()
            .flex_row()
            .items_center()
            .gap(10.)
            .child(text("Filter color").text_size(13.).line_height(16.))
            .child(swatch)
    }
}

#[cfg(test)]
mod tests;
