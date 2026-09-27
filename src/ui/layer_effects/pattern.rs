//! PAT choices belong to the effects draft; selected tiles live in the project.
use super::*;
use compositor::{
    invalid,
    pattern::{Overlay, Pack, Pattern},
};
use quickgui::PathPromptOptions;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::ui) struct Request {
    session: Uuid,
    layer: Uuid,
    draft: Uuid,
}
#[derive(Clone)]
struct Entry {
    pattern: Pattern,
    thumbnail: Image,
}
#[derive(Clone)]
pub(super) struct Choices {
    token: Uuid,
    entries: Arc<Vec<Entry>>,
}
impl Default for Choices {
    fn default() -> Self {
        Self {
            token: Uuid::new_v4(),
            entries: Arc::new(Vec::new()),
        }
    }
}
pub(in crate::ui) struct Imported {
    entries: Arc<Vec<Entry>>,
}
impl Imported {
    pub(in crate::ui) fn read(path: &std::path::Path) -> Result<Self> {
        let pack = Pack::read(path)?;
        Self::from_patterns(pack.patterns())
    }
    fn from_patterns(patterns: &[Pattern]) -> Result<Self> {
        let entries = patterns
            .iter()
            .map(|pattern| {
                let thumb = image::imageops::thumbnail(pattern.pixels().as_ref(), 40, 40);
                let thumbnail = Image::from_rgba(thumb.width(), thumb.height(), thumb.into_raw())
                    .map_err(|e| {
                    invalid(format!(
                        "Could not display pattern {}: {e}. No patterns were imported.",
                        pattern.name()
                    ))
                })?;
                Ok(Entry {
                    pattern: pattern.clone(),
                    thumbnail,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            entries: Arc::new(entries),
        })
    }
}
impl Editor {
    fn pattern_request(&self) -> Option<Request> {
        let Some(Form::Effects(edit)) = &self.modal else {
            return None;
        };
        Some(Request {
            session: self.session().id,
            layer: edit.id,
            draft: edit.patterns.token,
        })
    }
    fn load_pattern_pack(&mut self, cx: &mut EventContext) {
        if self.pending {
            return;
        }
        let Some(request) = self.pattern_request() else {
            return;
        };
        let options = PathPromptOptions::new().title("Import Patterns").filters([
            super::super::file_dialogs::file_filter("Photoshop patterns", &["pat"]),
        ]);
        let operation = alerts::Operation::Import;
        match cx.prompt_for_paths(options) {
            Ok(response) => self.await_response(cx, operation, response, move |e, result, cx| {
                if e.pattern_request() != Some(request) {
                    return;
                }
                match result {
                    Ok(Some(paths)) => {
                        if let Some(path) = paths.into_iter().next() {
                            e.queue_file(super::super::file_jobs::FileJob::PatternPack {
                                request,
                                path,
                            });
                        }
                    }
                    Ok(None) => {}
                    Err(error) => e.show_error(
                        operation,
                        format!("Could not choose a PAT file: {error}. Retry Import PAT."),
                    ),
                }
                cx.invalidate();
            }),
            Err(error) => self.show_error(
                operation,
                format!("Could not open the pattern file dialog: {error}. Retry Import PAT."),
            ),
        }
    }
    pub(in crate::ui) fn receive_pattern_pack(
        &mut self,
        request: Request,
        imported: Imported,
    ) -> Result<()> {
        if self.pattern_request() != Some(request) {
            return Err(invalid(
                "The pattern settings closed before the import finished. Reopen Layer Effects and import the PAT file again. Existing layers are unchanged.",
            ));
        }
        if let Some(Form::Effects(edit)) = &mut self.modal {
            edit.patterns.entries = imported.entries;
            edit.kind = EffectKind::Pattern;
        }
        self.status = "Choose a pattern to preview it on this layer.".into();
        Ok(())
    }
    fn choose_pattern(&mut self, index: usize) {
        if self.pending {
            return;
        }
        self.change_effect(|edit| {
            let Some(entry) = edit.patterns.entries.get(index) else {
                return;
            };
            match &mut edit.effects.pattern_overlay {
                Some(overlay) => {
                    overlay.pattern = entry.pattern.clone();
                    overlay.settings.enabled = true;
                }
                None => {
                    edit.effects.pattern_overlay =
                        Some(Box::new(Overlay::new(entry.pattern.clone())))
                }
            }
        });
    }
    pub(super) fn pattern_controls(
        &self,
        cx: &mut ViewContext<'_, Self>,
        edit: &EffectsEditor,
    ) -> Element {
        let mut contents = div().flex_col().gap(8.).child(
            self.control("Import PAT…")
                .id("pattern-import")
                .disabled(self.pending)
                .on_click(cx.listener("pattern-import", |e, cx| e.load_pattern_pack(cx))),
        );
        if let Some(overlay) = &edit.effects.pattern_overlay {
            contents = contents.child(
                text(format!("Pattern: {}", overlay.pattern.name()))
                    .truncate()
                    .text_size(12.),
            );
        } else {
            contents =
                contents.child(text("Import a PAT file, then choose a pattern.").text_size(12.));
        }
        if !edit.patterns.entries.is_empty() {
            let mut choices = div().flex_col().gap(3.).max_h(160.).overflow_y_scroll();
            for (index, entry) in edit.patterns.entries.iter().enumerate() {
                let selected = edit
                    .effects
                    .pattern_overlay
                    .as_ref()
                    .is_some_and(|overlay| {
                        Arc::ptr_eq(overlay.pattern.pixels(), entry.pattern.pixels())
                    });
                choices = choices.child(
                    self.control("")
                        .h(44.)
                        .flex_shrink_0()
                        .gap(8.)
                        .id(format!("pattern-choice-{index}"))
                        .selected(selected)
                        .disabled(self.pending)
                        .child(
                            quickgui::img(entry.thumbnail.clone())
                                .size(40., 40.)
                                .flex_shrink_0(),
                        )
                        .child(text(entry.pattern.name()).truncate().min_w(0.))
                        .on_click(
                            cx.listener(format!("pattern-choice-{index}"), move |e, cx| {
                                e.choose_pattern(index);
                                e.changed(cx);
                            }),
                        ),
                );
            }
            contents = contents.child(choices);
        }
        contents
    }
}
#[cfg(test)]
mod tests;
