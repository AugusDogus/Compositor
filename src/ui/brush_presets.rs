//! Loaded brush tips are bounded session resources; the chosen shape belongs to each project.
mod import;
mod library;
use super::*;
use compositor::brush::sampled::{Sampled, Shape, Tip};
use compositor::invalid;
use quickgui::PathPromptOptions;

const MAX_TIPS: usize = compositor::brush::sampled::abr::MAX_IMPORT_TIPS;
const MAX_TOTAL_PIXELS: usize = compositor::brush::sampled::abr::MAX_IMPORT_PIXELS;

#[derive(Default)]
pub(super) struct Library {
    tips: Vec<Entry>,
}
struct Entry {
    brush: Sampled,
    thumbnail: Image,
}
impl Entry {
    fn label(&self) -> String {
        self.brush.hose().map_or_else(
            || self.brush.name().to_string(),
            |hose| format!("{} · {} cells", hose.name(), hose.cells().len()),
        )
    }
}
#[derive(Clone)]
pub(super) struct Draft {
    import: Option<import::Import>,
    spacing: String,
    error: String,
}

impl Editor {
    pub(super) fn open_brush_tips(&mut self) {
        let spacing = match &self.tools.brush_shape {
            Shape::Round => 25.,
            Shape::Sampled(brush) => brush.spacing() * 100.,
        };
        self.modal = Some(Form::BrushTips(Draft {
            import: None,
            spacing: format!("{spacing:.1}"),
            error: String::new(),
        }));
    }

    fn load_brush_tip(&mut self, cx: &mut EventContext) {
        let options = PathPromptOptions::new().title("Load Brush Tips").filters([
            super::file_dialogs::file_filter("Brush tips (GBR, GIH, ABR)", &["gbr", "gih", "abr"]),
        ]);
        let operation = alerts::Operation::Import;
        match cx.prompt_for_paths(options) {
            Ok(response) => {
                self.await_response(cx, operation, response, move |this, result, cx| {
                    match result {
                        Ok(Some(paths)) => {
                            if let Some(path) = paths.into_iter().next() {
                                this.queue_file(super::file_jobs::FileJob::BrushTip(path));
                            }
                        }
                        Ok(None) => {
                            this.status =
                                "Brush import cancelled. The selected tip is unchanged.".into()
                        }
                        Err(error) => this.show_error(
                            operation,
                            format!("Could not choose a brush: {error}. Retry Load Brushes."),
                        ),
                    }
                    cx.invalidate();
                })
            }
            Err(error) => self.show_error(
                operation,
                format!("Could not open the brush file dialog: {error}. Retry Load Brushes."),
            ),
        }
    }

    fn brush_spacing_input(&mut self, value: &str) {
        let Some(Form::BrushTips(draft)) = &mut self.modal else {
            return;
        };
        draft.spacing = value.into();
        let Some(percent) = value
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| (1. ..=1000.).contains(v))
        else {
            draft.error = "Spacing must be 1 to 1000%.".into();
            return;
        };
        if let Shape::Sampled(brush) = &mut self.tools.brush_shape
            && let Err(error) = brush.set_spacing(percent / 100.)
        {
            draft.error = error.to_string();
            return;
        }
        draft.error.clear();
    }

    pub(super) fn brush_tips_view(&self, cx: &mut ViewContext<'_, Self>, draft: &Draft) -> Element {
        if let Some(import) = &draft.import {
            self.brush_import_view(cx, import, &draft.error)
        } else {
            self.loaded_brush_tips_view(cx, draft)
        }
    }
    fn loaded_brush_tips_view(&self, cx: &mut ViewContext<'_, Self>, draft: &Draft) -> Element {
        let mut choices = div()
            .flex_col()
            .gap(4.)
            .max_h(300.)
            .overflow_y_scroll()
            .child(
                Self::control("Round tip")
                    .selected(matches!(self.tools.brush_shape, Shape::Round))
                    .selected_style(|s| s.bg(Color::rgb8(65, 107, 158)))
                    .on_click(cx.listener("brush-tip-round", |this, cx| {
                        this.tools.brush_shape = Shape::Round;
                        this.open_brush_tips();
                        cx.invalidate();
                    })),
            );
        for (index, entry) in self.brush_presets.tips.iter().enumerate() {
            let selected = matches!(&self.tools.brush_shape, Shape::Sampled(brush) if brush.same_source(&entry.brush));
            let (width, height) = entry.brush.tip().pixels().dimensions();
            let scale = 40. / width.max(height) as f32;
            choices = choices.child(
                Self::control("")
                    .h(48.)
                    .flex_shrink_0()
                    .rounded(6.)
                    .gap(8.)
                    .selected(selected)
                    .selected_style(|s| s.bg(Color::rgb8(65, 107, 158)))
                    .child(
                        div()
                            .w(40.)
                            .h(40.)
                            .flex_shrink_0()
                            .flex_row()
                            .items_center()
                            .justify_center()
                            .child(
                                quickgui::img(entry.thumbnail.clone())
                                    .id(format!("brush-tip-thumbnail-{index}"))
                                    .w(width as f32 * scale)
                                    .h(height as f32 * scale),
                            ),
                    )
                    .child(text(entry.label()).truncate().min_w(0.).flex_1())
                    .on_click(cx.listener(format!("brush-tip-{index}"), move |this, cx| {
                        if let Some(entry) = this.brush_presets.tips.get(index) {
                            this.tools.brush_shape = Shape::Sampled(entry.brush.clone());
                            this.open_brush_tips();
                            cx.invalidate();
                        }
                    })),
            );
        }
        let sampled = matches!(self.tools.brush_shape, Shape::Sampled(_));
        div().flex_col().gap(12.).child(choices)
            .child(Self::control("Load Brushes…").on_click(cx.listener("brush-tip-load", |this, cx| this.load_brush_tip(cx))))
            .child(text("GBR, GIH and ABR brushes use the foreground color. Embedded colors are not used. Brushes stay loaded until Compositor closes.").wrap().text_size(12.))
            .child(div().flex_row().items_center().gap(8.).child(text("Spacing (%)"))
                .child(Self::text_field(draft.spacing.clone()).id("brush-tip-spacing").w(90.).disabled(!sampled)
                    .on_input(cx.input_listener("brush-tip-spacing", |this, value, cx| { this.brush_spacing_input(value); cx.invalidate(); }))))
            .child(Self::control("Unload selected tip").disabled(!sampled).on_click(cx.listener("brush-tip-unload", |this, cx| { this.unload_brush_tip(); cx.invalidate(); })))
            .child(text(draft.error.clone()).text_size(12.).wrap())
            .child(Self::control("Close").on_click(cx.listener("brush-tips-close", |this, cx| this.cancel_form(cx))))
    }
}

#[cfg(test)]
mod tests;
