//! Shape creation and styling share the Pen's saved-path gestures.
use super::path_target::Target;
use super::paths::{Active, Mode};
use super::*;
use compositor::{
    invalid,
    path_shape::{self, Stroke, Style},
};
use uuid::Uuid;

impl Editor {
    pub(super) fn path_shape_button(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let shape = matches!(
            self.tools.paths.active.map(|a| a.target),
            Some(Target::Shape(_))
        );
        self.tool_header_control(if shape {
            "Shape Style…"
        } else {
            "Create Shape"
        })
        .flex_shrink_0()
        .disabled(!self.can_edit_layers() || self.tools.paths.active.is_none())
        .on_click(cx.listener("path-shape", |e, cx| {
            let result = match e.tools.paths.active.map(|a| a.target) {
                Some(Target::Shape(id)) => e.open_path_shape(id),
                Some(Target::Saved(_)) => e.create_path_shape(),
                None => Ok(()),
            };
            e.result(result, cx);
        }))
    }
    fn create_path_shape(&mut self) -> Result<()> {
        self.finish_path_drag(true)?;
        let path = self
            .active_path()
            .cloned()
            .ok_or_else(|| invalid("Select a saved path to create a shape."))?;
        let style = Style {
            fill: Some(self.tools.brush.color),
            stroke: None,
        };
        let mut created = None;
        self.session_mut().edit("Create Path Shape", |doc| {
            created = Some(path_shape::create(doc, &path.name, path.geometry, style)?);
            Ok(())
        })?;
        if let Some(id) = created {
            self.edit_path_shape_geometry(id)?;
        }
        Ok(())
    }
    pub(super) fn edit_path_shape_geometry(&mut self, id: Uuid) -> Result<()> {
        self.finish_pending_edits()?;
        path_shape::layer_path(&self.session().document, id)?;
        self.session_mut().select_layer(id, false);
        self.tools.tool = Tool::Pen;
        self.tools.mask_target = false;
        self.tools.paths.active = Some(Active {
            target: Target::Shape(id),
            selected: None,
            mode: Mode::Editing,
        });
        Ok(())
    }
    pub(super) fn open_selected_path_shape(&mut self) -> Result<()> {
        let id = self
            .session()
            .document
            .active
            .ok_or_else(|| invalid("Select a path shape to edit its style."))?;
        self.open_path_shape(id)
    }
    pub(super) fn open_path_shape(&mut self, id: Uuid) -> Result<()> {
        let shape = self
            .session()
            .document
            .layer(id)
            .and_then(|l| l.path_shape())
            .ok_or_else(|| invalid("The path shape is unavailable. Select another shape."))?;
        let style = shape.source().style;
        self.modal = Some(Form::Edit {
            title: "Path Shape Style",
            action: Action::EditPathShape(id),
            fields: vec![
                ("Fill (hex or none)", color_text(style.fill)),
                (
                    "Stroke (hex or none)",
                    color_text(style.stroke.map(|s| s.color)),
                ),
                (
                    "Stroke width (source px)",
                    style.stroke.map_or(1., |s| s.width).to_string(),
                ),
            ],
            error: String::new(),
        });
        Ok(())
    }
    pub(super) fn apply_path_shape(&mut self, id: Uuid, values: &[String]) -> Result<()> {
        let [fill, stroke, width] = values else {
            return Err(invalid(
                "Shape settings are incomplete. Reopen Path Shape Settings.",
            ));
        };
        let fill = parse_color(fill)?;
        let stroke = parse_color(stroke)?
            .map(|color| -> Result<Stroke> {
                let width = width
                    .trim()
                    .parse()
                    .map_err(|_| invalid("Enter a stroke width between 0.01 and 30,000 pixels."))?;
                Ok(Stroke { width, color })
            })
            .transpose()?;
        let style = Style { fill, stroke };
        style.validate()?;
        self.session_mut().edit("Style Path Shape", |doc| {
            let geometry = path_shape::layer_path(doc, id)?;
            path_shape::update(doc, id, geometry, style)
        })
    }
    pub(super) fn rasterize_path_shape(&mut self) -> Result<()> {
        let id = self
            .session()
            .document
            .active
            .ok_or_else(|| invalid("Select a path shape to rasterize."))?;
        self.session_mut()
            .edit("Rasterize Path Shape", |doc| path_shape::rasterize(doc, id))
    }
}
fn color_text(color: Option<[u8; 4]>) -> String {
    color.map_or_else(
        || "none".into(),
        |[r, g, b, a]| format!("#{r:02X}{g:02X}{b:02X}{a:02X}"),
    )
}
fn parse_color(text: &str) -> Result<Option<[u8; 4]>> {
    let raw = text.trim();
    if raw.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    let hex = raw.strip_prefix('#').unwrap_or(raw);
    if hex.len() == 8 {
        return u32::from_str_radix(hex, 16)
            .map(|v| Some(v.to_be_bytes()))
            .map_err(|_| invalid("Use none, #RRGGBB or #RRGGBBAA for shape colors."));
    }
    compositor::palette::parse_hex(raw).map(Some)
}

#[cfg(test)]
mod tests;
