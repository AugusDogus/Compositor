use super::path_target::Target;
use super::paths::{Active, Mode};
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
        let target = self.tools.paths.active.map(|a| a.target);
        let doc = self.current_document();
        let picker = self.tools.paths.picker.element(
            cx,
            self.colors,
            ("path-picker", "Paths and shape layers"),
            |e| &mut e.tools.paths.picker,
            self.tool_header_control(
                target
                    .and_then(|t| doc.and_then(|doc| t.name(doc)))
                    .unwrap_or("New path"),
            )
            .w(145.)
            .overflow_hidden()
            .whitespace_nowrap(),
            |e, id, cx| {
                let result = e.finish_path_drag(true);
                if result.is_ok() {
                    e.cancel_path_rename();
                    e.tools.paths.active = id.map(|target| Active {
                        target,
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
                if target.and_then(|t| doc.and_then(|doc| t.closure(doc))) == Some(Closure::Closed)
                {
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
                || (target.is_none() && !matches!(command, Command::New))
                || (matches!(command, Command::Delete) && matches!(target, Some(Target::Shape(_))))
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
        row.child(self.path_shape_button(cx))
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
                    let mut geometry = active.target.edit_snapshot(doc)?.geometry;
                    geometry.closure = if matches!(command, Command::Continue)
                        || geometry.closure == Closure::Closed
                    {
                        Closure::Open
                    } else {
                        Closure::Closed
                    };
                    active.target.replace(doc, geometry)
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
                let Target::Saved(id) = active.target else {
                    return Ok(());
                };
                self.cancel_path_rename();
                self.session_mut().edit("Delete Path", |doc| {
                    doc.paths.retain(|p| p.id != id);
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
                    path: active.target,
                    operation,
                });
            }
        }
        Ok(())
    }
}
