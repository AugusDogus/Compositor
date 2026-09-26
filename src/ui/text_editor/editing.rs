use super::*;
use std::ops::Range;

impl Draft {
    pub(in crate::ui) fn selected_color(&self, style: &Text) -> [f64; 3] {
        let byte = if self.selection.is_empty() {
            self.selection.start
        } else {
            style
                .content
                .get(self.selection.start..)
                .and_then(|s| s.chars().next())
                .map_or(self.selection.start, |ch| {
                    self.selection.start + ch.len_utf8()
                })
        };
        style.caret_color(byte)
    }

    pub(super) fn sync_color(&mut self) {
        let [r, g, b] = self
            .selected_color(&self.style)
            .map(|v| (v * 255.).round() as u8);
        self.color = format!("#{r:02X}{g:02X}{b:02X}");
    }

    fn remember(&mut self) {
        self.history.push(self.style.clone());
        self.redo.clear();
        let mut bytes = 0;
        let keep = self
            .history
            .iter()
            .rev()
            .take(100)
            .take_while(|t| {
                bytes += t.content.len()
                    + t.color_runs.as_ref().map_or(0, |r| {
                        r.len() * std::mem::size_of::<compositor::text::ColorRun>()
                    });
                bytes <= 8 * 1024 * 1024
            })
            .count();
        self.history.drain(..self.history.len() - keep);
    }

    pub(in crate::ui) fn set_color_field(&mut self, color: String) {
        self.color = color;
        if let Ok(style) = self.parsed()
            && style != self.style
        {
            self.remember();
            self.style = style;
        }
    }

    pub(super) fn set_selection(&mut self, selection: Range<usize>) {
        if selection == self.selection || self.style.content.get(selection.clone()).is_none() {
            return;
        }
        // A partially typed hex value must not become an unrelated color when the caret moves.
        if let Ok(style) = self.parsed() {
            self.style = style;
        }
        self.selection = selection;
        self.sync_color();
    }

    pub(super) fn replace_input(&mut self, value: &str, selection: Option<Range<usize>>) {
        if value == self.style.content {
            return;
        }
        let mut style = self.parsed().unwrap_or_else(|_| self.style.clone());
        let old = &style.content;
        let selected = self.selection.clone();
        let replacement = old
            .get(..selected.start)
            .zip(old.get(selected.end..))
            .and_then(|(prefix, suffix)| {
                value
                    .strip_prefix(prefix)?
                    .strip_suffix(suffix)
                    .map(|middle| (selected, middle))
            });
        let (range, inserted) = replacement.unwrap_or_else(|| {
            let prefix = old
                .chars()
                .zip(value.chars())
                .take_while(|(a, b)| a == b)
                .map(|(c, _)| c.len_utf8())
                .sum::<usize>();
            let suffix = old[prefix..]
                .chars()
                .rev()
                .zip(value[prefix..].chars().rev())
                .take_while(|(a, b)| a == b)
                .map(|(c, _)| c.len_utf8())
                .sum::<usize>();
            (
                prefix..old.len() - suffix,
                &value[prefix..value.len() - suffix],
            )
        });
        match style.replace_characters(range, inserted) {
            Ok(()) => {
                self.remember();
                self.style = style;
                self.selection = selection
                    .filter(|r| self.style.content.get(r.clone()).is_some())
                    .unwrap_or(value.len()..value.len());
                self.sync_color();
                self.error.clear();
            }
            Err(error) => self.error = error.to_string(),
        }
    }

    pub(super) fn undo_text(&mut self, redo: bool) {
        let replacement = if redo {
            self.redo.pop()
        } else {
            self.history.pop()
        };
        if let Some(style) = replacement {
            let current = std::mem::replace(&mut self.style, style);
            if redo {
                self.history.push(current);
            } else {
                self.redo.push(current);
            }
            self.selection = self.style.content.len()..self.style.content.len();
            self.sync_color();
            self.error.clear();
        }
    }
}

impl Editor {
    pub(in crate::ui) fn sync_text_selection(&mut self, cx: &EventContext) {
        if let Some(selection) = cx.text_input_selection("text-content")
            && let Some(Form::Text(draft)) = &mut self.modal
        {
            draft.set_selection(selection);
        }
    }
}
