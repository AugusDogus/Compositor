//! Loaded brush tips are bounded session resources; the chosen shape belongs to each project.
use super::*;
use compositor::brush::sampled::{Sampled, Shape, Tip};
use compositor::invalid;
use quickgui::PathPromptOptions;

const MAX_TIPS: usize = 32;
const MAX_TOTAL_PIXELS: usize = 16 * 1024 * 1024;

#[derive(Default)]
pub(super) struct Library {
    tips: Vec<Entry>,
}
struct Entry {
    tip: Arc<Tip>,
    thumbnail: Image,
}
#[derive(Clone)]
pub(super) struct Draft {
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
            spacing: format!("{spacing:.1}"),
            error: String::new(),
        }));
    }

    pub(super) fn install_brush_tip(&mut self, tip: Tip) -> Result<()> {
        let existing = self
            .brush_presets
            .tips
            .iter()
            .find(|entry| *entry.tip == tip)
            .map(|entry| entry.tip.clone());
        let tip = if let Some(tip) = existing {
            tip
        } else {
            let pixels: usize = self
                .brush_presets
                .tips
                .iter()
                .map(|entry| entry.tip.pixels().len())
                .sum();
            if self.brush_presets.tips.len() >= MAX_TIPS
                || pixels + tip.pixels().len() > MAX_TOTAL_PIXELS
            {
                return Err(invalid(
                    "The session brush library is full (32 tips or 16 million tip pixels). Existing tips and edits are unchanged. Restart Compositor to load a different set.",
                ));
            }
            let thumb = image::DynamicImage::ImageLuma8(tip.pixels().clone())
                .thumbnail(40, 40)
                .into_luma8();
            let rgba = image::RgbaImage::from_fn(thumb.width(), thumb.height(), |x, y| {
                image::Rgba([230, 230, 230, thumb[(x, y)][0]])
            });
            let thumbnail = Image::from_rgba(rgba.width(), rgba.height(), rgba.into_raw())
                .map_err(|error| invalid(format!("Could not display the imported brush tip: {error}. Existing tips are unchanged.")))?;
            let tip = Arc::new(tip);
            self.brush_presets.tips.push(Entry {
                tip: tip.clone(),
                thumbnail,
            });
            tip
        };
        self.tools.brush_shape = Shape::Sampled(Sampled::new(tip));
        self.open_brush_tips();
        self.status = "Brush tip loaded for this session. Imported colors are not used; paint uses the foreground color.".into();
        Ok(())
    }

    fn load_brush_tip(&mut self, cx: &mut EventContext) {
        let options = PathPromptOptions::new()
            .title("Load GBR Brush Tip")
            .filters([super::file_dialogs::file_filter(
                "GIMP brush (GBR v2)",
                &["gbr"],
            )]);
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
                            format!("Could not choose a brush: {error}. Retry Load GBR."),
                        ),
                    }
                    cx.invalidate();
                })
            }
            Err(error) => self.show_error(
                operation,
                format!("Could not open the brush file dialog: {error}. Retry Load GBR."),
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
            let selected = matches!(&self.tools.brush_shape, Shape::Sampled(brush) if Arc::ptr_eq(brush.tip(), &entry.tip));
            let (width, height) = entry.tip.pixels().dimensions();
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
                    .child(text(entry.tip.name()).truncate().min_w(0.).flex_1())
                    .on_click(cx.listener(format!("brush-tip-{index}"), move |this, cx| {
                        if let Some(entry) = this.brush_presets.tips.get(index) {
                            this.tools.brush_shape =
                                Shape::Sampled(Sampled::new(entry.tip.clone()));
                            this.open_brush_tips();
                            cx.invalidate();
                        }
                    })),
            );
        }
        let sampled = matches!(self.tools.brush_shape, Shape::Sampled(_));
        div().flex_col().gap(12.).child(choices)
            .child(Self::control("Load GBR…").on_click(cx.listener("brush-tip-load", |this, cx| this.load_brush_tip(cx))))
            .child(text("GBR v2 tip shapes use the foreground color. Embedded RGB colors are not used. Tips stay loaded until Compositor closes.").wrap().text_size(12.))
            .child(div().flex_row().items_center().gap(8.).child(text("Spacing (%)"))
                .child(Self::text_field(draft.spacing.clone()).id("brush-tip-spacing").w(90.).disabled(!sampled)
                    .on_input(cx.input_listener("brush-tip-spacing", |this, value, cx| { this.brush_spacing_input(value); cx.invalidate(); }))))
            .child(text(draft.error.clone()).text_size(12.).wrap())
            .child(Self::control("Close").on_click(cx.listener("brush-tips-close", |this, cx| this.cancel_form(cx))))
    }
}

#[cfg(test)]
mod tests;
