//! Type tool creation, editing and font controls.
use super::*;
use compositor::{
    geometry::Point,
    invalid,
    text::{Alignment, Text, TextRenderer},
};
use uuid::Uuid;

pub(super) struct Preview {
    style: Text,
    image: std::result::Result<Image, String>,
}

#[derive(Clone)]
pub(super) struct Draft {
    session: Uuid,
    layer: Option<Uuid>,
    origin: Point,
    pub style: Text,
    numbers: [String; 5],
    pub(super) color: String,
    fonts_open: bool,
    error: String,
}
impl Draft {
    pub(super) fn parsed(&self) -> Result<Text> {
        let mut style = self.style.clone();
        let number = |i: usize| {
            self.numbers[i].parse::<f64>().map_err(|_| {
                invalid("Enter valid numbers for text size, spacing and box dimensions.")
            })
        };
        style.font_size = number(0)?;
        style.tracking = number(1)?;
        style.leading = number(2)?;
        if style.box_size.is_some() {
            style.box_size = Some([number(3)?, number(4)?]);
        }
        let hex = self.color.trim().trim_start_matches('#');
        if hex.len() != 6 || !hex.is_ascii() {
            return Err(invalid(
                "Enter the text color as six hexadecimal digits, such as #3366CC.",
            ));
        }
        let rgb = u32::from_str_radix(hex, 16).map_err(|_| {
            invalid("Enter the text color as six hexadecimal digits, such as #3366CC.")
        })?;
        let original = format!(
            "{:02X}{:02X}{:02X}",
            (style.red * 255.).round() as u8,
            (style.green * 255.).round() as u8,
            (style.blue * 255.).round() as u8
        );
        if !hex.eq_ignore_ascii_case(&original) {
            style.red = f64::from((rgb >> 16) & 255) / 255.;
            style.green = f64::from((rgb >> 8) & 255) / 255.;
            style.blue = f64::from(rgb & 255) / 255.;
        }
        style.validate()?;
        Ok(style)
    }
}

impl Editor {
    pub(super) fn begin_text(&mut self, start: Point, end: Point, force_new: bool) -> Result<()> {
        if !self.can_edit_layers() {
            return Ok(());
        }
        let box_size = if (end[0] - start[0]).hypot(end[1] - start[1]) * self.session().zoom > 4. {
            Some([
                (end[0] - start[0]).abs().max(16.),
                (end[1] - start[1]).abs().max(16.),
            ])
        } else {
            None
        };
        let target = if force_new || box_size.is_some() {
            None
        } else {
            self.session()
                .document
                .layers
                .iter()
                .rev()
                .find(|layer| {
                    layer.text.is_some()
                        && self.session().document.layer_is_visible(layer.id)
                        && layer
                            .transform
                            .unit(start)
                            .iter()
                            .all(|v| (0. ..=1.).contains(v))
                })
                .cloned()
        };
        let mut style = target
            .as_ref()
            .and_then(|layer| layer.text.clone())
            .unwrap_or_else(|| {
                let mut style = self.text_defaults.clone();
                style.content.clear();
                style.box_size = box_size;
                let [r, g, b, _] = self.tools.brush.color;
                style.red = f64::from(r) / 255.;
                style.green = f64::from(g) / 255.;
                style.blue = f64::from(b) / 255.;
                style
            });
        style.validate()?;
        let mut origin = target
            .as_ref()
            .map_or([start[0].min(end[0]), start[1].min(end[1])], |l| {
                l.transform.origin
            });
        if target.is_none() && box_size.is_none() {
            let baseline = self
                .text_renderer
                .get_or_insert_with(TextRenderer::default)
                .first_baseline(&style)?;
            origin = [start[0] - 12., start[1] - baseline];
        }
        if let Some(layer) = &target {
            self.session_mut().document.select(layer.id, false);
        }
        let size = style.box_size.unwrap_or([400., 200.]);
        let draft = Draft {
            session: self.session().id,
            layer: target.map(|l| l.id),
            origin,
            numbers: [
                style.font_size.to_string(),
                style.tracking.to_string(),
                style.leading.to_string(),
                size[0].to_string(),
                size[1].to_string(),
            ],
            color: format!(
                "#{:02X}{:02X}{:02X}",
                (style.red * 255.).round() as u8,
                (style.green * 255.).round() as u8,
                (style.blue * 255.).round() as u8
            ),
            style: std::mem::take(&mut style),
            fonts_open: false,
            error: String::new(),
        };
        self.text_renderer.get_or_insert_with(TextRenderer::default);
        self.tools.mask_target = false;
        self.tools.tool = Tool::Text;
        self.modal = Some(Form::Text(Box::new(draft)));
        Ok(())
    }

    pub(super) fn edit_text_at(&mut self, point: Point) -> Result<bool> {
        if !self.can_edit_layers() {
            return Ok(false);
        }
        let doc = &self.session().document;
        let target = doc
            .layers
            .iter()
            .rev()
            .find(|layer| {
                layer.text.is_some()
                    && doc.layer_is_visible(layer.id)
                    && layer
                        .transform
                        .unit(point)
                        .iter()
                        .all(|v| (0. ..=1.).contains(v))
            })
            .map(|layer| layer.id);
        let Some(id) = target else {
            return Ok(false);
        };
        self.finish_pending_edits()?;
        self.session_mut().select_layer(id, false);
        self.edit_active_text()?;
        Ok(true)
    }

    pub(super) fn edit_active_text(&mut self) -> Result<()> {
        let layer = self
            .session()
            .document
            .active_layer()
            .ok_or_else(|| invalid("Select an editable text layer."))?;
        if layer.text.is_none() {
            return Err(invalid("Select an editable text layer."));
        }
        // At the layer's center, use direct layer identity even if another text layer overlaps it.
        let id = layer.id;
        let origin = layer.transform.origin;
        let style = layer
            .text
            .clone()
            .ok_or_else(|| invalid("Select an editable text layer."))?;
        self.begin_text(origin, origin, true)?;
        if let Some(Form::Text(draft)) = &mut self.modal {
            draft.layer = Some(id);
            draft.origin = origin;
            draft.numbers = [
                style.font_size.to_string(),
                style.tracking.to_string(),
                style.leading.to_string(),
                style.box_size.unwrap_or([400., 200.])[0].to_string(),
                style.box_size.unwrap_or([400., 200.])[1].to_string(),
            ];
            draft.color = format!(
                "#{:02X}{:02X}{:02X}",
                (style.red * 255.).round() as u8,
                (style.green * 255.).round() as u8,
                (style.blue * 255.).round() as u8
            );
            draft.style = style;
        }
        Ok(())
    }

    fn apply_text(&mut self) -> Result<()> {
        let Some(Form::Text(draft)) = &self.modal else {
            return Ok(());
        };
        let draft = draft.clone();
        if draft.session != self.session().id {
            return Err(invalid(
                "The text's project changed. Cancel and reopen the text editor.",
            ));
        }
        let style = draft.parsed()?;
        if draft.layer.is_none() && style.content.trim().is_empty() {
            self.modal = None;
            return Ok(());
        }
        if draft
            .layer
            .and_then(|id| self.session().document.layer(id))
            .and_then(|layer| layer.text.as_ref())
            == Some(&style)
        {
            self.modal = None;
            return Ok(());
        }
        let pixels = self
            .text_renderer
            .get_or_insert_with(TextRenderer::default)
            .render(&style)?;
        let defaults = style.clone();
        self.session_mut().edit(
            if draft.layer.is_some() {
                "Edit Text"
            } else {
                "New Text Layer"
            },
            |doc| {
                if let Some(id) = draft.layer {
                    let layer = doc
                        .layers
                        .iter_mut()
                        .find(|l| l.id == id)
                        .ok_or_else(|| invalid("The text layer was removed."))?;
                    compositor::text::update_layer(layer, style, pixels)
                } else {
                    doc.add(compositor::text::new_layer(style, pixels, draft.origin)?)
                }
            },
        )?;
        self.text_defaults = defaults;
        self.modal = None;
        Ok(())
    }

    fn text_key(&mut self, key: &Key, modifiers: Modifiers, cx: &mut EventContext) {
        let Some((key, modifiers)) = self.keymap.translate(key, modifiers, true) else {
            cx.prevent_default();
            return;
        };
        if key == Key::Enter && modifiers == Modifiers::CONTROL {
            cx.prevent_default();
            self.text_submit(cx);
            return;
        }
        if (modifiers - Modifiers::SHIFT) == Modifiers::ALT
            && matches!(
                key,
                Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown
            )
        {
            cx.prevent_default();
            if let Some(Form::Text(draft)) = &mut self.modal
                && let Ok(style) = draft.parsed()
            {
                let step = if modifiers.contains(Modifiers::SHIFT) {
                    10.
                } else {
                    1.
                };
                if matches!(key, Key::ArrowLeft | Key::ArrowRight) {
                    draft.numbers[1] = (style.tracking
                        + if key == Key::ArrowLeft { -step } else { step })
                    .clamp(-100., 1000.)
                    .to_string();
                } else {
                    draft.numbers[2] = (style.line_height()
                        + if key == Key::ArrowUp { -step } else { step })
                    .clamp(0., 5000.)
                    .to_string();
                }
            }
            cx.invalidate();
        } else {
            cx.propagate();
        }
    }

    fn text_submit(&mut self, cx: &mut EventContext) {
        match self.apply_text() {
            Ok(()) => cx.focus(quickgui::FocusHandle::new("workspace")),
            Err(error) => {
                if let Some(Form::Text(draft)) = &mut self.modal {
                    draft.error = error.to_string();
                }
            }
        }
        self.changed(cx);
    }

    pub(super) fn text_header(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let editable = self.has_document()
            && self
                .session()
                .document
                .active_layer()
                .is_some_and(|l| l.text.is_some());
        div()
            .flex_row()
            .items_center()
            .gap(12.)
            .child(text("Click for point text · Drag for paragraph text").text_size(12.))
            .child(
                Self::segment("Edit selected text", false)
                    .disabled(!editable)
                    .on_click(cx.listener("edit-active-text", |this, cx| {
                        let result = this.edit_active_text();
                        this.result(result, cx);
                    })),
            )
    }
}

mod view;

#[cfg(test)]
mod tests;
