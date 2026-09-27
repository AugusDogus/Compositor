use super::*;
use compositor::{
    export_sizes::{Batch, Fit, Format},
    invalid,
};
use quickgui::PathPromptOptions;

fn batch(fields: &[(&str, String)]) -> Result<Batch> {
    let value = |index: usize| {
        fields.get(index).map(|(_, value)| value.as_str())
        .ok_or_else(|| invalid("Export settings are incomplete. Close the sheet and choose Export Sizes again."))
    };
    let fit = match value(1)? {
        "fit" => Fit::Contain,
        "fill" => Fit::Cover,
        _ => return Err(invalid("Choose Fit or Fill for export framing.")),
    };
    let format = match value(2)? {
        "png" => Format::Png,
        "jpeg" => Format::Jpeg {
            quality: value(3)?
                .trim()
                .parse::<u8>()
                .map_err(|_| invalid("JPEG quality must be a whole number from 0 to 100."))?,
        },
        _ => return Err(invalid("Choose PNG or JPEG for export.")),
    };
    Batch::parse(value(0)?, fit, format)
}

impl Editor {
    pub(super) fn open_export_sizes(&mut self) {
        self.retain_tool_panel();
        let document = self.session().committed_document();
        self.modal = Some(Form::Edit {
            title: "Export Sizes",
            action: Action::ExportSizes,
            fields: vec![
                ("Sizes", format!("{}x{}", document.width, document.height)),
                ("Framing", "fit".into()),
                ("Format", "png".into()),
                ("Quality", "85".into()),
            ],
            error: String::new(),
        });
    }

    pub(super) fn export_sizes_fields(
        &self,
        cx: &mut ViewContext<'_, Self>,
        fields: &[(&'static str, String)],
    ) -> Element {
        let mut contents = div().flex_col().gap(14.);
        if let Some((_, sizes)) = fields.first() {
            contents = contents
                .child(text("Sizes in pixels, separated by commas").text_size(13.))
                .child(
                    self.form_input_with_id(cx, 0, sizes, self.size_field_id(0))
                        .w_full(),
                );
        }
        let mut presets = div().flex_row().gap(6.);
        for (label, sizes) in [
            ("Social", "1080x1080, 1080x1350, 1080x1920"),
            ("Video", "1280x720, 1920x1080, 3840x2160"),
            ("Print", "2480x3508, 2550x3300"),
        ] {
            presets = presets.child(Self::control(label).on_click(cx.listener(
                format!("export-sizes-{label}"),
                move |this, cx| {
                    this.update_form_field(0, sizes);
                    this.changed(cx);
                },
            )));
        }
        contents = contents.child(presets);
        if let Some((_, fit)) = fields.get(1) {
            contents =
                contents.child(self.field_choices(cx, 1, fit, &[("Fit", "fit"), ("Fill", "fill")]));
        }
        if let Some((_, format)) = fields.get(2) {
            contents = contents.child(self.field_choices(
                cx,
                2,
                format,
                &[("PNG", "png"), ("JPEG", "jpeg")],
            ));
            if format == "jpeg"
                && let Some((_, quality)) = fields.get(3)
            {
                contents = contents.child(
                    div()
                        .flex_row()
                        .items_center()
                        .gap(12.)
                        .child(text("Quality (0–100)").text_size(13.))
                        .child(
                            self.form_input_with_id(cx, 3, quality, self.size_field_id(3))
                                .w(80.),
                        ),
                );
            }
        }
        contents.child(text("Fit keeps the whole canvas with padding. Fill crops centrally. PNG preserves transparency; JPEG uses white. A new export folder is created in your chosen destination. Your project stays unchanged.")
            .text_size(12.).line_height(16.).wrap())
    }

    pub(super) fn finish_export_sizes(&mut self, cx: &mut EventContext) {
        let Some(Form::Edit {
            fields,
            action: Action::ExportSizes,
            ..
        }) = &self.modal
        else {
            return;
        };
        let batch = match batch(fields) {
            Ok(batch) => batch,
            Err(failure) => {
                if let Some(Form::Edit { error, .. }) = &mut self.modal {
                    *error = failure.to_string();
                }
                self.changed(cx);
                return;
            }
        };
        let document = self.session().committed_document().clone();
        let title = self.session().title();
        let operation = alerts::Operation::ExportSizes;
        let options = PathPromptOptions::new()
            .files(false)
            .directories(true)
            .title("Choose Export Destination");
        match cx.prompt_for_paths(options) {
            Ok(response) => self.await_response(cx, operation, response, move |this, result, _| match result {
                Ok(Some(paths)) => {
                    if let Some(parent) = paths.into_iter().next() {
                        this.finish_form();
                        this.queue_file(super::file_jobs::FileJob::ExportSizes { document, batch, parent, title });
                    }
                }
                Ok(None) => this.status = "Export cancelled. Your project is unchanged.".into(),
                Err(error) => this.show_error(operation, format!("Could not choose an export destination: {error}. Choose Export again to retry.")),
            }),
            Err(error) => self.show_error(operation, format!("Could not open the export destination dialog: {error}. Choose Export again to retry.")),
        }
    }
}

#[cfg(test)]
mod tests;
