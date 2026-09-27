use super::paths::{Active, Mode, path_mut};
use super::*;
use compositor::vector_path::Closure;

#[derive(Clone, Copy)]
pub(super) enum Command {
    New,
    Continue,
    Close,
    Delete,
    Select,
    Fill,
    Stroke,
}
impl Editor {
    pub(super) fn path_header(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let path = self.active_path();
        let picker = self.tools.paths.picker.element(
            cx,
            self.colors,
            ("path-picker", "Saved paths"),
            |e| &mut e.tools.paths.picker,
            self.tool_header_control(path.map_or("New path", |p| p.name.as_str()))
                .w(145.)
                .overflow_hidden()
                .whitespace_nowrap(),
            |e, id, cx| {
                let result = e.finish_path_drag(true);
                if result.is_ok() {
                    e.cancel_path_rename();
                    e.tools.paths.active = id.map(|id| Active {
                        id,
                        selected: None,
                        mode: Mode::Editing,
                    });
                }
                e.result(result, cx);
            },
        );
        let mut row = self
            .tool_header_shell()
            .child(picker)
            .child(self.path_rename_button(cx));
        for (id, label, command) in [
            ("path-new", "New", Command::New),
            ("path-continue", "Continue", Command::Continue),
            (
                "path-close",
                if path.is_some_and(|p| p.geometry.closure == Closure::Closed) {
                    "Open"
                } else {
                    "Close"
                },
                Command::Close,
            ),
            ("path-select", "Selection", Command::Select),
            ("path-fill", "Fill", Command::Fill),
            ("path-stroke", "Stroke", Command::Stroke),
            ("path-delete", "Delete", Command::Delete),
        ] {
            let disabled = !self.can_edit_layers()
                || (path.is_none() && !matches!(command, Command::New))
                || (matches!(command, Command::Fill | Command::Stroke) && !self.can_edit_pixels());
            row = row.child(
                self.tool_header_control(label)
                    .flex_shrink_0()
                    .disabled(disabled)
                    .on_click(cx.listener(id, move |e, cx| {
                        let result = e.path_command(command);
                        e.result(result, cx);
                    })),
            );
        }
        row
    }
    pub(super) fn path_command(&mut self, command: Command) -> Result<()> {
        if !self.can_edit_layers() {
            return Ok(());
        }
        self.finish_path_drag(true)?;
        if matches!(command, Command::New) {
            self.cancel_path_rename();
            self.tools.paths.active = None;
            return Ok(());
        }
        let Some(active) = self.tools.paths.active else {
            return Ok(());
        };
        match command {
            Command::New => {}
            Command::Continue | Command::Close => {
                self.session_mut().edit("Change Path Closure", |doc| {
                    let path = path_mut(doc, active.id)?;
                    path.geometry.closure = if matches!(command, Command::Continue)
                        || path.geometry.closure == Closure::Closed
                    {
                        Closure::Open
                    } else {
                        Closure::Closed
                    };
                    Ok(())
                })?;
                self.tools.paths.active = Some(Active {
                    mode: if matches!(command, Command::Continue) {
                        Mode::Drawing
                    } else {
                        Mode::Editing
                    },
                    ..active
                });
            }
            Command::Delete => {
                self.cancel_path_rename();
                self.session_mut().edit("Delete Path", |doc| {
                    doc.paths.retain(|p| p.id != active.id);
                    Ok(())
                })?;
                self.tools.paths.active = None;
            }
            Command::Select | Command::Fill | Command::Stroke => {
                use compositor::path_operations::Operation;
                let operation = match command {
                    Command::Select => Operation::Select {
                        mode: self.tools.selection_mode,
                        antialiased: self.tools.selection_antialiased,
                    },
                    Command::Fill => Operation::Fill {
                        color: self.palette_colors(self.tools.mask_target)[0],
                        mask: self.tools.mask_target,
                        antialiased: self.tools.selection_antialiased,
                    },
                    _ => Operation::Stroke {
                        brush: Brush {
                            color: self.palette_colors(self.tools.mask_target)[0],
                            ..self.tools.brush
                        },
                        shape: self.tools.brush_shape.clone(),
                        mask: self.tools.mask_target,
                    },
                };
                self.queue(jobs::Job::Path {
                    path: active.id,
                    operation,
                });
            }
        }
        Ok(())
    }
}
