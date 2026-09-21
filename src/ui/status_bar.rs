//! ContentView.swift status metrics and tool hints, with Linux shortcut names.
use super::*;

impl Editor {
    pub(super) fn tool_hint(&self) -> &'static str {
        match self.tools.tool {
            Tool::Idle => "No tool selected · Press a tool's key to pick one · Space to pan",
            Tool::Move => {
                "Drag to move · Handles to resize · Circle to rotate · 1–0 layer opacity · Space to pan"
            }
            Tool::Rectangle => {
                "Drag a rectangle · Shift add · Alt subtract · Shift again mid-drag square · Drag inside to move · Ctrl-drag moves pixels · Delete clears · Ctrl+D deselect"
            }
            Tool::Ellipse => {
                "Drag an ellipse · Shift add · Alt subtract · Shift again mid-drag circle · Drag inside to move · Delete clears · Ctrl+D deselect"
            }
            Tool::Lasso => {
                "Drag to select · Drag inside to move · Shift add · Alt subtract · Delete clears · Alt+Backspace/Ctrl+Backspace fill · Ctrl+D deselect"
            }
            Tool::Polygon => {
                "Click corners · Click start, double-click or Enter to close · Backspace removes corner · Escape cancel"
            }
            Tool::Object => {
                "Click an object or drag a box around it · Shift add · Alt subtract · Escape cancel · W switches to Magic Wand"
            }
            Tool::Wand => {
                "Click to select similar colors · Shift add · Alt subtract · Drag inside to move · Ctrl-drag moves pixels · Delete clears · Ctrl+D deselect"
            }
            Tool::Crop => "Drag to crop · Enter apply · Escape cancel · Space to pan",
            Tool::Brush => {
                "Drag to paint · [ ] size · Shift-[ ] hardness · 1–0 opacity · Escape cancel · Space to pan"
            }
            Tool::Erase => {
                "Drag to erase · [ ] size · Shift-[ ] hardness · 1–0 opacity · Escape cancel · Space to pan"
            }
            Tool::Clone => {
                "Alt-click to set the source · Drag to clone · [ ] size · Shift-[ ] hardness · 1–0 opacity · Space to pan"
            }
            Tool::Heal => {
                "Drag over blemishes to heal · [ ] size · Shift-[ ] hardness · Escape cancel · Space to pan"
            }
            Tool::Blur => {
                "Drag to soften · [ ] size · Shift-[ ] hardness · 1–0 strength · Space to pan"
            }
            Tool::Smudge => {
                "Drag to smudge · [ ] size · Shift-[ ] hardness · 1–0 strength · Space to pan"
            }
            Tool::Liquify => {
                "Drag to push pixels · [ ] size · Shift-[ ] hardness · 1–0 strength · Space to pan"
            }
            Tool::Gradient => {
                "Drag to draw · Drag ends to adjust · Shift 45° · 1–0 opacity · Enter apply · Escape cancel"
            }
            Tool::Shape if self.tools.shape_kind == compositor::document::ShapeKind::Line => {
                "Drag a line · Shift snaps to 45° · Alt draws from center · Shift-U cycles shape kind"
            }
            Tool::Shape if self.tools.shape_kind == compositor::document::ShapeKind::Ellipse => {
                "Drag to draw a shape on a new layer · Shift circle · Alt from center · Shift+U line · Escape cancel · Space to pan"
            }
            Tool::Shape => {
                "Drag to draw a shape on a new layer · Shift square · Alt from center · Shift+U ellipse · Escape cancel · Space to pan"
            }
            Tool::Text => {
                "Click to create or edit text · Drag a paragraph box · Ctrl+Enter applies text · Escape cancels"
            }
            Tool::Eyedropper => "Click to sample the foreground color · Space to pan",
            Tool::Hand => "Drag to pan · Pinch to zoom",
            Tool::Zoom => {
                "Click to zoom in · Alt-click to zoom out · Drag right or left to zoom smoothly · Space to pan"
            }
        }
    }

    pub(super) fn status_bar(&self) -> Element {
        let mut status = div().flex_row().gap(6.).items_center();
        if self.pending {
            status = status.child(icons::progress("status-progress", 12.));
        }
        status = status.child(
            text(if self.pending && self.status.is_empty() {
                "Working…".into()
            } else if self.status.is_empty() {
                self.tool_hint().into()
            } else {
                self.status.clone()
            })
            .id("status-message")
            .min_w(0.)
            .truncate(),
        );
        let mut bar = div()
            .h(30.)
            .border_top(1., Color::rgb8(62, 62, 62))
            .flex_shrink_0()
            .px(18.)
            .flex_row()
            .items_center()
            .bg(Color::rgb8(40, 40, 40))
            .text_size(11.)
            .font_features(
                quickgui::FontFeatures::new().enable(quickgui::FontFeatureTag::TABULAR_NUMBERS),
            )
            .text_color(Color::rgb8(165, 165, 165))
            .gap(16.);
        if let Some(session) = self.tabs[self.current].session() {
            let percent = format!("{:.1}", session.zoom * 100.);
            let percent = percent.strip_suffix(".0").unwrap_or(&percent);
            bar = bar
                .child(
                    text(format!("{percent}%"))
                        .id("zoom-status")
                        .w(62.)
                        .flex_shrink_0(),
                )
                .child(
                    text(format!(
                        "{} × {} px",
                        session.document.width, session.document.height
                    ))
                    .flex_shrink_0(),
                )
                .child(text("sRGB · Transparent").flex_shrink_0());
        } else {
            bar = bar.child(text("Ready when you are").flex_shrink_0());
        }
        bar.child(div().flex_1()).child(status.min_w(0.))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, WindowOptions};

    #[test]
    fn importing_status_survives_worker_dispatch_and_clears_for_other_work() {
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Import status").size(1500., 900.),
                Editor::with_test_document(),
            )
            .unwrap();
        let window = view.window_handle();
        for import in [true, false] {
            cx.update(view, |e, cx| {
                e.queue_file(if import {
                    file_jobs::FileJob::Import {
                        session: e.tabs[e.current].id,
                        paths: Vec::new(),
                        center: None,
                    }
                } else {
                    file_jobs::FileJob::Open(Vec::new())
                });
                // Dispatch consumes the queued job while the worker is still pending.
                e.file_job.take();
                cx.invalidate();
            })
            .unwrap();
            let tree = cx.accessibility_update(window).unwrap();
            assert!(tree.nodes.iter().any(|(_, node)| {
                node.label()
                    == Some(if import {
                        "Importing images…"
                    } else {
                        "Working…"
                    })
            }));
            assert!(cx.contains_element(window, "status-progress").unwrap());
        }
        cx.update(view, |e, cx| {
            e.pending = false;
            e.status = "Import complete".into();
            cx.invalidate();
        })
        .unwrap();
        assert!(!cx.contains_element(window, "status-progress").unwrap());
        let tree = cx.accessibility_update(window).unwrap();
        assert!(
            tree.nodes
                .iter()
                .any(|(_, node)| node.label() == Some("Import complete"))
        );
    }
}
