use super::*;
use compositor::invalid;
use uuid::Uuid;

pub(super) struct LayerRename {
    pub layer: Uuid,
    session: Uuid,
    name: String,
}

impl Editor {
    pub(super) fn begin_rename(&mut self) -> Result<()> {
        let layer = self
            .session()
            .document
            .active_layer()
            .ok_or_else(|| invalid("Select a layer to rename."))?;
        self.rename = Some(LayerRename {
            layer: layer.id,
            session: self.session().id,
            name: layer.name.clone(),
        });
        self.status = "Rename layer: Enter saves, Escape cancels.".into();
        Ok(())
    }

    pub(super) fn finish_rename(&mut self) -> Result<()> {
        let Some(edit) = &self.rename else {
            return Ok(());
        };
        let id = edit.layer;
        let name = edit.name.trim().to_owned();
        if name.is_empty() {
            return Err(invalid("Enter a layer name before saving the rename."));
        }
        let session = self
            .tabs
            .iter_mut()
            .find(|s| s.id == edit.session)
            .and_then(ProjectTab::session_mut)
            .ok_or_else(|| invalid("The layer's project has closed."))?;
        session.edit("Rename Layer", |doc| {
            doc.layers
                .iter_mut()
                .find(|l| l.id == id)
                .ok_or_else(|| invalid("The layer being renamed was removed."))?
                .name = name;
            Ok(())
        })?;
        self.rename = None;
        self.status = self.tool_hint().into();
        Ok(())
    }

    pub(super) fn rename_input(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let Some(edit) = &self.rename else {
            return div();
        };
        let input = cx.input_listener("layer-rename", |this, value, cx| {
            if let Some(edit) = &mut this.rename {
                edit.name = value.to_owned();
            }
            this.changed(cx);
        });
        let workspace = cx.focus_handle("workspace");
        let key = cx.key_down_listener("layer-rename", move |this, event, cx| match event.key {
            Key::Enter => {
                let result = this.finish_rename();
                if result.is_ok() {
                    cx.focus(workspace);
                }
                this.result(result, cx);
            }
            Key::Escape => {
                this.rename = None;
                this.status = this.tool_hint().into();
                cx.focus(workspace);
                this.changed(cx);
            }
            _ => cx.propagate(),
        });
        Self::text_field(edit.name.clone())
            .id("layer-rename")
            .auto_focus()
            .w_full()
            .min_w(0.)
            .h(20.)
            .flex_shrink_0()
            .text_size(13.)
            .rounded(3.)
            .text_input_padding(5.)
            .bg(Color::rgb8(25, 25, 25))
            .on_mouse_down(
                quickgui::MouseButton::Left,
                cx.mouse_down_listener("layer-rename", |_, _, cx| cx.stop_propagation()),
            )
            .on_click(cx.listener("layer-rename", |_, cx| cx.stop_propagation()))
            .on_input(input)
            .on_key_down(key)
    }
}
