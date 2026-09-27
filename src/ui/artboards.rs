//! Artboard frames share the layer hierarchy; settings edit the selected owner.
use super::*;
use compositor::{artboard, document::LayerContent, geometry::Transform, invalid};
use uuid::Uuid;
#[derive(Clone, Copy)]
pub(super) enum Target {
    None,
    Board(Uuid),
    Mixed,
}
impl Editor {
    pub(super) fn artboard_target(&self) -> Target {
        let Some(doc) = self.current_document().filter(|_| !self.tools.mask_target) else {
            return Target::None;
        };
        let Some(board) = doc
            .layers
            .iter()
            .find(|l| l.is_artboard() && doc.selected.contains(&l.id))
        else {
            return Target::None;
        };
        if doc
            .selected
            .iter()
            .all(|id| artboard::owner(doc, *id) == Some(board.id))
        {
            Target::Board(board.id)
        } else {
            Target::Mixed
        }
    }
    pub(super) fn transforms_artboard(&self) -> bool {
        !matches!(self.artboard_target(), Target::None)
    }
    pub(super) fn edit_selected_artboard(&mut self) -> Result<()> {
        let Target::Board(id) = self.artboard_target() else {
            return Err(invalid(
                "Select one artboard and its contents to edit its frame.",
            ));
        };
        self.open_artboard(Some(id))
    }
    pub(super) fn artboard_header(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        self.tool_header_shell()
            .child(self.tool_header_control("Artboard Settings…").on_click(
                cx.listener("artboard-settings", |e, cx| {
                    e.action(Action::ArtboardSettings, cx)
                }),
            ))
            .child(text("Drag a label or frame to move. Drag handles to resize.").text_size(12.))
    }
    pub(super) fn active_artboard(&self) -> Option<Uuid> {
        match self.artboard_target() {
            Target::Board(id) => Some(id),
            Target::None | Target::Mixed => None,
        }
    }

    pub(super) fn open_artboard(&mut self, id: Option<Uuid>) -> Result<()> {
        let doc = &self.session().document;
        let (name, frame, background) = if let Some(id) = id {
            let layer = doc
                .layer(id)
                .ok_or_else(|| invalid("The artboard was removed. Select another artboard."))?;
            let LayerContent::Artboard(board) = &layer.content else {
                return Err(invalid("Select an artboard to edit its frame."));
            };
            (layer.name.clone(), layer.transform, board.background)
        } else {
            (
                format!(
                    "Artboard {}",
                    doc.layers
                        .iter()
                        .filter(|l| matches!(l.content, LayerContent::Artboard(_)))
                        .count()
                        + 1
                ),
                Transform::new(doc.width, doc.height),
                [0; 4],
            )
        };
        self.modal = Some(Form::Edit {
            title: if id.is_some() {
                "Artboard Settings"
            } else {
                "New Artboard"
            },
            action: id.map_or(Action::NewArtboard, Action::EditArtboard),
            fields: vec![
                ("Name", name),
                ("X", frame.origin[0].to_string()),
                ("Y", frame.origin[1].to_string()),
                ("Width", frame.size[0].to_string()),
                ("Height", frame.size[1].to_string()),
                (
                    "Background",
                    if background[3] == 0 {
                        "transparent".into()
                    } else {
                        format!(
                            "#{:02X}{:02X}{:02X}{:02X}",
                            background[0], background[1], background[2], background[3]
                        )
                    },
                ),
            ],
            error: String::new(),
        });
        Ok(())
    }
    pub(super) fn apply_artboard(&mut self, id: Option<Uuid>, values: &[String]) -> Result<()> {
        let get = |i| {
            values.get(i).map(String::as_str).ok_or_else(|| {
                invalid("Artboard settings are incomplete. Reopen Artboard Settings and retry.")
            })
        };
        let number = |i| -> Result<f64> {
            get(i)?
                .trim()
                .parse()
                .map_err(|_| invalid("Artboard positions and dimensions must be numbers."))
        };
        let name = get(0)?.trim().to_owned();
        let mut frame = Transform::new(1, 1);
        frame.origin = [number(1)?, number(2)?];
        frame.size = [number(3)?, number(4)?];
        let background = if get(5)?.trim().eq_ignore_ascii_case("transparent") {
            [0; 4]
        } else {
            let raw = get(5)?.trim();
            let hex = raw.strip_prefix('#').unwrap_or(raw);
            if hex.len() == 8 {
                u32::from_str_radix(hex, 16)
                    .map(u32::to_be_bytes)
                    .map_err(|_| {
                        invalid(
                            "Use transparent, #RRGGBB or #RRGGBBAA for the artboard background.",
                        )
                    })?
            } else {
                compositor::palette::parse_hex(get(5)?)?
            }
        };
        self.session_mut().edit(
            if id.is_some() {
                "Edit Artboard"
            } else {
                "New Artboard"
            },
            |doc| {
                if let Some(id) = id {
                    artboard::update(doc, id, &name, frame, background)
                } else {
                    artboard::create(doc, &name, frame, background).map(|_| ())
                }
            },
        )
    }
    pub(super) fn artboard_from_layers(&mut self) -> Result<()> {
        self.session_mut().edit("Artboard from Layers", |doc| {
            artboard::from_selection(doc, "Artboard", [0; 4]).map(|_| ())
        })
    }
    pub(super) fn artboard_fields(
        &self,
        cx: &mut ViewContext<'_, Self>,
        fields: &[(&'static str, String)],
    ) -> Element {
        let mut contents = div().flex_col().gap(10.);
        for (i, (label, value)) in fields.iter().enumerate() {
            contents = contents.child(
                div()
                    .flex_row()
                    .items_center()
                    .gap(10.)
                    .child(text(*label).w(100.))
                    .child(
                        self.form_input_with_id(cx, i, value, self.size_field_id(i))
                            .w(180.),
                    ),
            );
        }
        contents.child(text("Moving the frame moves its layers. Resizing clips the contents without scaling them. Background: transparent, #RRGGBB or #RRGGBBAA.").text_size(12.).line_height(16.).wrap())
    }
}

#[cfg(test)]
mod tests;
