use super::*;
use compositor::brush::sampled::abr::Pack;

#[derive(Clone, Default)]
pub(super) enum Preview {
    #[default]
    Empty,
    Queued(usize),
    Running {
        index: usize,
        next: usize,
    },
    Ready {
        index: usize,
        image: Image,
    },
    Failed {
        index: usize,
        error: String,
    },
}
impl Preview {
    pub(super) fn request(&mut self, index: usize) {
        match self {
            Self::Running { next, .. } => *next = index,
            Self::Ready { index: shown, .. } if *shown == index => {}
            _ => *self = Self::Queued(index),
        }
    }
    fn take(&mut self) -> Option<usize> {
        let Self::Queued(index) = *self else {
            return None;
        };
        *self = Self::Running { index, next: index };
        Some(index)
    }
    fn finish(&mut self, completed: usize, result: Result<Image>) {
        let Self::Running { index, next } = *self else {
            return;
        };
        if completed != index {
            return;
        }
        *self = if next != index {
            Self::Queued(next)
        } else {
            match result {
                Ok(image) => Self::Ready { index, image },
                Err(error) => Self::Failed {
                    index,
                    error: error.to_string(),
                },
            }
        };
    }
}
fn render(pack: &Pack, index: usize) -> Result<Image> {
    let tip = pack.decode(&[index])?.pop().ok_or_else(|| {
        invalid("The ABR preview returned no tip. Choose another tip or reopen the pack.")
    })?;
    let alpha = image::DynamicImage::ImageLuma8(tip.pixels().clone())
        .thumbnail(120, 120)
        .into_luma8();
    let rgba = image::RgbaImage::from_fn(alpha.width(), alpha.height(), |x, y| {
        image::Rgba([230, 230, 230, alpha[(x, y)][0]])
    });
    Image::from_rgba(rgba.width(), rgba.height(), rgba.into_raw()).map_err(|e| {
        invalid(format!(
            "Could not display the brush preview: {e}. Retry preview."
        ))
    })
}
impl Editor {
    fn brush_import_mut(&mut self) -> Option<&mut import::Import> {
        match &mut self.modal {
            Some(Form::BrushTips(Draft {
                import: Some(import),
                ..
            })) => Some(import),
            _ => None,
        }
    }
    pub(in crate::ui) fn start_brush_preview(&mut self, cx: &ViewContext<'_, Self>) {
        let Some(import) = self.brush_import_mut() else {
            return;
        };
        let Some(index) = import.preview.take() else {
            return;
        };
        let pack = import.pack.clone();
        let work = pack.clone();
        let target = pack.clone();
        let launched = cx.spawn_background(move || render(&work, index), move |this, result, cx| {
            let result = result.map_err(|e| invalid(format!("Brush preview worker failed: {e}. Retry preview; no brushes were loaded."))).and_then(|r| r);
            this.finish_brush_preview(&target, index, result);
            cx.invalidate();
        });
        if let Err(error) = launched {
            self.finish_brush_preview(
                &pack,
                index,
                Err(invalid(format!(
                    "Could not start brush preview: {error}. Retry preview; no brushes were loaded."
                ))),
            );
        }
    }
    fn finish_brush_preview(&mut self, pack: &Arc<Pack>, index: usize, result: Result<Image>) {
        if let Some(import) = self
            .brush_import_mut()
            .filter(|import| Arc::ptr_eq(&import.pack, pack))
        {
            import.preview.finish(index, result);
        }
    }
    pub(super) fn brush_preview_view(
        &self,
        cx: &mut ViewContext<'_, Self>,
        preview: &Preview,
        pack: &Pack,
    ) -> Element {
        let panel = div()
            .w(128.)
            .min_w(0.)
            .flex_shrink_0()
            .flex_col()
            .max_h(300.)
            .overflow_y_scroll()
            .gap(8.)
            .child(text("Tip preview").text_size(12.));
        match preview {
            Preview::Empty => panel.child(
                text("Select a tip to inspect its shape.")
                    .wrap()
                    .text_size(12.),
            ),
            Preview::Queued(_) | Preview::Running { .. } => {
                panel.child(text("Loading preview…").wrap().text_size(12.))
            }
            Preview::Ready { index, image } => panel
                .child(text(pack.tips()[*index].name.clone()).wrap().text_size(12.))
                .child(
                    div()
                        .w(128.)
                        .h(128.)
                        .flex_row()
                        .items_center()
                        .justify_center()
                        .rounded(6.)
                        .bg(self.colors.neutral(28))
                        .child(
                            quickgui::img(image.clone())
                                .id("abr-tip-preview")
                                .w(image.width() as f32)
                                .h(image.height() as f32),
                        ),
                ),
            Preview::Failed { index, error } => {
                let index = *index;
                panel
                    .child(text(error.as_str()).wrap().text_size(12.))
                    .child(self.control("Retry preview").on_click(cx.listener(
                        "abr-preview-retry",
                        move |this, cx| {
                            if let Some(import) = this.brush_import_mut() {
                                import.preview.request(index);
                            }
                            cx.invalidate();
                        },
                    )))
            }
        }
    }
}

#[cfg(test)]
mod tests;
