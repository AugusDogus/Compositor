use super::*;
use compositor::brush::sampled::abr::{self, Pack};
use std::collections::BTreeSet;

#[derive(Clone)]
pub(super) struct Import {
    pub(super) pack: Arc<Pack>,
    pub(super) preview: preview::Preview,
    selected: BTreeSet<usize>,
}
impl Editor {
    pub(in crate::ui) fn open_brush_pack(&mut self, pack: Pack) {
        self.modal = Some(Form::BrushTips(Draft {
            spacing: String::new(),
            error: String::new(),
            import: Some(Import {
                pack: Arc::new(pack),
                preview: preview::Preview::default(),
                selected: BTreeSet::new(),
            }),
        }));
    }
    fn toggle_abr_tip(&mut self, index: usize) {
        let Some(Form::BrushTips(Draft {
            import: Some(import),
            error,
            ..
        })) = &mut self.modal
        else {
            return;
        };
        import.preview.request(index);
        if import.selected.remove(&index) {
            error.clear();
            return;
        }
        let Some(tip) = import
            .pack
            .tips()
            .get(index)
            .filter(|t| t.unavailable().is_none())
        else {
            return;
        };
        let pixels: usize = import
            .selected
            .iter()
            .map(|i| import.pack.tips()[*i].pixels())
            .sum();
        if self.brush_presets.tips.len() + import.selected.len() >= MAX_TIPS
            || self.brush_presets.pixels() + pixels + tip.pixels() > MAX_TOTAL_PIXELS
        {
            *error = "The selection would exceed the library limit (32 tips or 16 million pixels). Select fewer tips, or go Back and unload a tip first.".into();
            return;
        }
        import.selected.insert(index);
        error.clear();
    }
    pub(super) fn import_abr_selection(&mut self) {
        let Some(Form::BrushTips(Draft {
            import: Some(import),
            ..
        })) = &self.modal
        else {
            return;
        };
        if import.selected.is_empty() {
            return;
        }
        self.queue_file(super::super::file_jobs::FileJob::BrushTips {
            pack: import.pack.clone(),
            selected: import.selected.iter().copied().collect(),
        });
    }
    pub(super) fn brush_import_view(
        &self,
        cx: &mut ViewContext<'_, Self>,
        import: &Import,
        error: &str,
    ) -> Element {
        let mut rows = div()
            .flex_col()
            .gap(4.)
            .max_h(300.)
            .overflow_y_scroll()
            .min_w(0.)
            .flex_1();
        for (index, info) in import.pack.tips().iter().enumerate() {
            let label = format!("{} · {} × {}", info.name, info.size[0], info.size[1]);
            rows = rows.child(
                Self::control("")
                    .h(32.)
                    .flex_shrink_0()
                    .rounded(6.)
                    .selected(import.selected.contains(&index))
                    .selected_style(|s| s.bg(Color::rgb8(65, 107, 158)))
                    .child(text(label).truncate().min_w(0.))
                    .disabled(info.unavailable().is_some())
                    .tooltip(info.unavailable().unwrap_or("Select this sampled tip"))
                    .on_click(cx.listener(format!("abr-tip-{index}"), move |this, cx| {
                        this.toggle_abr_tip(index);
                        cx.invalidate();
                    })),
            );
        }
        div()
            .flex_col()
            .gap(12.)
            .child(text(import.pack.report()).wrap().text_size(12.))
            .child(text(abr::LIMITATIONS).wrap().text_size(12.))
            .child(
                div()
                    .flex_row()
                    .gap(12.)
                    .child(rows)
                    .child(self.brush_preview_view(cx, &import.preview, &import.pack)),
            )
            .child(
                text(format!(
                    "{} selected · {} of 32 library slots used",
                    import.selected.len(),
                    self.brush_presets.tips.len()
                ))
                .text_size(12.),
            )
            .child(text(error).wrap().text_size(12.))
            .child(
                div()
                    .flex_row()
                    .gap(8.)
                    .child(
                        Self::control("Back").on_click(cx.listener("abr-back", |this, cx| {
                            this.open_brush_tips();
                            cx.invalidate();
                        })),
                    )
                    .child(
                        Self::control("Import selected tips")
                            .disabled(import.selected.is_empty() || self.pending)
                            .on_click(cx.listener("abr-import", |this, cx| {
                                this.import_abr_selection();
                                cx.invalidate();
                            })),
                    ),
            )
    }
}
