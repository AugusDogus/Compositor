use super::*;

pub(super) fn focus_outline(style: quickgui::ElementStateStyle) -> quickgui::ElementStateStyle {
    style.border(2., Color::rgb8(89, 147, 211))
}

impl Editor {
    pub(super) fn toolbar(&mut self, cx: &mut ViewContext<'_, Self>) -> Element {
        let can_switch = self.can_switch_projects();
        let new = self
            .icon_action(cx, 100, Icon::Plus, "New canvas (Ctrl+N)", Action::New)
            .w(24.)
            .h(24.)
            .rounded(12.)
            .border(1., Color::rgb8(90, 90, 90))
            .bg(Color::rgb8(65, 65, 65))
            .disabled(!can_switch);
        // The drop ring overlays the circular button without changing its shape or hit area.
        let new_target = div()
            .absolute()
            .left(4.)
            .top(0.)
            .size(24., 24.)
            .rounded(6.)
            .disabled(!can_switch)
            .drag_over(|s| {
                s.outline_offset(2., Color::rgb8(0, 122, 255), -2.)
                    .cursor_copy()
            })
            .can_drop(move |drag: &super::layer_drag::LayerDrag| {
                can_switch && drag.operation == super::layer_drag::Transfer::Move
            })
            .on_drop(cx.drop_listener(
                "new-project-drop",
                |this, drag: &super::layer_drag::LayerDrag, _, cx| {
                    if !this.can_switch_projects() {
                        return;
                    }
                    let result = this.copy_drag_to_new_tab(drag);
                    this.result(result, cx);
                },
            ))
            .on_drop(cx.drop_listener(
                "new-project-drop",
                |this, files: &quickgui::DroppedFiles, event, cx| {
                    this.drop_files(files, None, event, cx);
                },
            ));
        let new = div()
            .relative()
            .w(32.)
            .flex_shrink_0()
            .flex_row()
            .justify_center()
            .child(new)
            .child(new_target);
        let zoom = self.toolbar_zoom_controls(cx);
        let tabs = self.tab_strip(cx);
        div()
            .h(46.)
            .flex_shrink_0()
            .px(12.)
            .gap(8.)
            .flex_row()
            .items_center()
            .bg(Color::rgb8(43, 43, 43))
            .border_bottom(1., Color::rgb8(62, 62, 62))
            .child(new)
            .child(tabs)
            .child(div().flex_1())
            .child(
                self.action_button(cx, 120, "Fit", Action::Fit)
                    .px(4.)
                    .tooltip("Fit canvas in window (Ctrl+0)")
                    .disabled_style(|s| s.opacity(0.4))
                    .disabled(!self.has_document()),
            )
            .child(
                self.action_button(cx, 121, "100%", Action::Actual)
                    .px(4.)
                    .tooltip("Actual pixels (Ctrl+1)")
                    .disabled_style(|s| s.opacity(0.4))
                    .disabled(!self.has_document()),
            )
            .child(zoom)
    }

    fn toolbar_zoom_controls(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        div()
            .flex_row()
            .flex_shrink_0()
            .rounded(12.)
            .bg(Color::rgb8(65, 65, 65))
            .child(
                self.icon_action(cx, 123, Icon::ZoomIn, "Zoom in (Ctrl++)", Action::ZoomIn)
                    .h(24.)
                    .rounded(12.)
                    .disabled(!self.has_document()),
            )
            .child(
                self.icon_action(cx, 122, Icon::ZoomOut, "Zoom out (Ctrl+-)", Action::ZoomOut)
                    .h(24.)
                    .rounded(12.)
                    .disabled(!self.has_document()),
            )
    }
    pub(super) fn tool_rail(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let brush = self.tools.tool_preferences.brush();
        let marquee = self.tools.tool_preferences.marquee();
        let lasso = self.tools.tool_preferences.lasso();
        let smear = self.tools.tool_preferences.smear();
        let mut tools = div()
            .id("tool-rail")
            .w(56.)
            .flex_shrink_0()
            .h_full()
            .flex_col()
            .items_center()
            .gap(10.)
            .padding(16., 0., 12., 0.)
            .overflow_y_scroll()
            .bg(Color::rgb8(36, 36, 36))
            .border_right(1., Color::rgb8(62, 62, 62));
        for (tool, icon, label) in [
            (Tool::Move, Icon::Move, "Move / Transform (V)"),
            (
                marquee,
                if marquee == Tool::Ellipse {
                    Icon::CircleDashed
                } else {
                    Icon::SquareDashed
                },
                "Marquee (M)",
            ),
            (
                lasso,
                if lasso == Tool::Polygon {
                    Icon::LassoSelect
                } else {
                    Icon::Lasso
                },
                "Lasso (L)",
            ),
            (Tool::Wand, Icon::WandSparkles, "Magic Wand (W)"),
            (
                Tool::Object,
                Icon::ScanSearch,
                "Object Selection (O): click an object or drag a box around it",
            ),
            (Tool::Crop, Icon::Crop, "Crop (C)"),
            (
                brush,
                if brush == Tool::Erase {
                    Icon::Eraser
                } else {
                    Icon::Paintbrush
                },
                "Brush (B) · Eraser (E)",
            ),
            (Tool::Heal, Icon::Bandage, "Spot Healing Brush (J)"),
            (
                Tool::Clone,
                Icon::Stamp,
                "Clone Stamp (S) · Alt-click sets the source",
            ),
            (smear, Icon::Droplet, "Smear (R)"),
            (Tool::Gradient, Icon::Gradient, "Gradient (G)"),
            (
                Tool::Shape,
                Icon::Shapes,
                "Shape (U) · Shift-U cycles Rectangle/Ellipse/Line",
            ),
            (Tool::Text, Icon::Type, "Type (T)"),
            (Tool::Eyedropper, Icon::Pipette, "Eyedropper (I)"),
            (Tool::Hand, Icon::Hand, "Hand (H)"),
            (Tool::Zoom, Icon::Zoom, "Zoom (Z)"),
        ] {
            let index = Tool::ALL
                .iter()
                .position(|(candidate, _)| *candidate == tool)
                .unwrap_or(0);
            let id = if tool == Tool::Text {
                quickgui::ElementId::from("text-tool")
            } else if tool == Tool::Object {
                quickgui::ElementId::from("object-tool")
            } else if matches!(tool, Tool::Brush | Tool::Erase) {
                quickgui::ElementId::from("brush-tool")
            } else {
                quickgui::ElementId::from(300 + index as u64)
            };
            let item = icon
                // SF Symbols' 17-point font size includes glyphs with different
                // bounds. Match their visible rail size, not the SVG viewBox.
                .button_with_icon_size(
                    label,
                    match icon {
                        Icon::Move
                        | Icon::Crop
                        | Icon::Paintbrush
                        | Icon::Hand
                        | Icon::Pipette
                        | Icon::WandSparkles
                        | Icon::Zoom => 20.,
                        Icon::SquareDashed | Icon::Bandage => 22.,
                        Icon::Shapes => 21.,
                        Icon::Stamp | Icon::LassoSelect | Icon::Gradient => 18.,
                        _ => 19.,
                    },
                )
                .w(36.)
                .h(36.)
                .text_color(Color::rgb8(225, 225, 225))
                .selected(self.tools.tool == tool)
                .rounded(7.)
                .border(
                    1.,
                    if self.tools.tool == tool {
                        Color::rgb8(79, 79, 79)
                    } else {
                        Color::TRANSPARENT
                    },
                )
                .bg(if self.tools.tool == tool {
                    Color::rgb8(65, 65, 65)
                } else {
                    Color::TRANSPARENT
                })
                .on_click(cx.listener(id, move |this, cx| this.select_tool(tool, cx)));
            tools = tools.child(item);
        }
        tools.child(self.palette_controls(cx).mt(8.))
    }
    pub(super) fn tool_options(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        if matches!(self.tools.tool, Tool::Zoom | Tool::Hand) {
            return self.navigation_header(cx);
        }
        if self.tools.tool == Tool::Move {
            return self.transform_header(cx);
        }
        self.generic_tool_options(cx)
    }
    fn generic_tool_options(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        if self.tools.tool == Tool::Gradient {
            return div()
                .h(42.)
                .flex_shrink_0()
                .overflow_x_scroll()
                .px(18.)
                .gap(12.)
                .flex_row()
                .items_center()
                .bg(Color::rgb8(38, 38, 38))
                .child(
                    text("Gradient")
                        .text_size(13.)
                        .line_height(16.)
                        .font_semibold()
                        .flex_shrink_0(),
                )
                .child(self.gradient_controls(cx));
        }
        let mut bar = div()
            .h(42.)
            .px(18.)
            .gap(if self.tools.tool == Tool::Crop {
                14.
            } else {
                12.
            })
            .flex_row()
            .items_center()
            .bg(Color::rgb8(38, 38, 38))
            .flex_shrink_0()
            .overflow_x_scroll()
            .child(
                text(match self.tools.tool {
                    Tool::Move => "Transform",
                    Tool::Rectangle | Tool::Ellipse => "Marquee",
                    Tool::Polygon | Tool::Lasso => "Lasso",
                    Tool::Wand => "Magic Wand",
                    Tool::Object => "Object Selection",
                    Tool::Clone => "Clone Stamp",
                    Tool::Heal => "Spot Healing",
                    Tool::Blur | Tool::Smudge | Tool::Liquify => "Smear",
                    Tool::Eyedropper => "Eyedropper",
                    Tool::Hand => "Pan",
                    _ => self
                        .tools
                        .tool
                        .label()
                        .split_once("  ")
                        .map_or(self.tools.tool.label(), |(_, name)| name),
                })
                .id("tool-header-title")
                .flex_shrink_0()
                .whitespace_nowrap()
                .font_semibold()
                .text_size(13.)
                .line_height(16.),
            );
        if matches!(
            self.tools.tool,
            Tool::Rectangle
                | Tool::Ellipse
                | Tool::Lasso
                | Tool::Polygon
                | Tool::Brush
                | Tool::Erase
                | Tool::Blur
                | Tool::Smudge
                | Tool::Liquify
        ) {
            bar = bar.child(self.tool_family_controls(cx));
        }
        if self.tools.tool == Tool::Eyedropper {
            bar = bar.child(
                Self::check_control("Sample Ring", self.tools.shows_sample_ring).on_click(
                    cx.listener("sample-ring-toggle", |this, cx| {
                        this.tools.shows_sample_ring = !this.tools.shows_sample_ring;
                        cx.invalidate();
                    }),
                ),
            );
        }
        if matches!(self.tools.tool, Tool::Clone | Tool::Heal) {
            bar = bar.child(self.brush_mode_controls(cx));
        }
        if matches!(
            self.tools.tool,
            Tool::Brush
                | Tool::Erase
                | Tool::Clone
                | Tool::Heal
                | Tool::Blur
                | Tool::Smudge
                | Tool::Liquify
        ) {
            bar = bar.child(self.brush_header(cx)).child(div().flex_1());
            if self.tools.tool == Tool::Clone && self.tools.clone_source.is_none() {
                bar = bar.child(
                    text("Alt-click to set the source")
                        .text_size(12.)
                        .line_height(15.)
                        .text_color(Color::rgb8(160, 160, 160))
                        .whitespace_nowrap(),
                );
            }
            if self.tools.mask_target {
                bar = bar.child(
                    text("Mask")
                        .text_size(12.)
                        .line_height(15.)
                        .text_color(Color::rgb8(160, 160, 160)),
                );
            }
        }

        if matches!(
            self.tools.tool,
            Tool::Rectangle
                | Tool::Ellipse
                | Tool::Lasso
                | Tool::Polygon
                | Tool::Wand
                | Tool::Object
        ) {
            bar = bar.child(self.selection_mode_controls(cx));
            if matches!(self.tools.tool, Tool::Wand | Tool::Object) {
                bar = bar.child(self.brush_mode_controls(cx));
            }
            if self.tools.tool != Tool::Rectangle {
                bar = bar.child(
                    Self::check_control("Anti-alias", self.tools.selection_antialiased)
                        .tooltip("Smooth selection edges; turn off for hard pixel edges")
                        .on_click(cx.listener("selection-antialias", |this, cx| {
                            this.tools.selection_antialiased = !this.tools.selection_antialiased;
                            cx.invalidate();
                        })),
                );
            }
            if self.tools.tool == Tool::Object {
                bar = bar.child(self.object_edge_control(cx));
            }
            bar = bar.child(self.selection_modify_controls(cx));
        }

        if self.tools.tool == Tool::Crop {
            bar = bar.child(self.crop_header(cx));
        }
        if self.tools.tool == Tool::Text {
            bar = bar.child(self.text_header(cx));
        }
        if self.tools.tool == Tool::Shape {
            bar = bar.child(self.shape_header(cx));
        }

        bar
    }
    fn tool_family_controls(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let modes: &[(Tool, &str)] = match self.tools.tool {
            Tool::Rectangle | Tool::Ellipse => {
                &[(Tool::Rectangle, "Rectangle"), (Tool::Ellipse, "Ellipse")]
            }
            Tool::Lasso | Tool::Polygon => {
                &[(Tool::Lasso, "Freehand"), (Tool::Polygon, "Polygonal")]
            }
            Tool::Brush | Tool::Erase => &[(Tool::Brush, "Paint"), (Tool::Erase, "Erase")],
            Tool::Blur | Tool::Smudge | Tool::Liquify => &[
                (Tool::Liquify, "Liquify"),
                (Tool::Blur, "Blur"),
                (Tool::Smudge, "Smudge"),
            ],
            _ => &[],
        };
        let mut row = div()
            .flex_row()
            .items_center()
            .rounded(6.)
            .bg(Color::rgb8(29, 29, 29))
            .p(2.)
            .flex_shrink_0();
        for &(tool, label) in modes {
            row = row.child(Self::segment(label, tool == self.tools.tool).on_click(
                cx.listener(format!("tool-mode-{tool:?}"), move |this, cx| {
                    this.select_tool(tool, cx)
                }),
            ));
        }
        if modes.is_empty() {
            div()
        } else {
            row.tooltip(match self.tools.tool {
                Tool::Rectangle | Tool::Ellipse => {
                    "Press M to switch between Rectangle and Ellipse"
                }
                Tool::Lasso | Tool::Polygon => "Press L to switch between Freehand and Polygonal",
                Tool::Brush | Tool::Erase => {
                    "Paint with the foreground color (B), or erase pixels away (E)"
                }
                _ => "Liquify pushes pixels · Blur softens · Smudge drags color along",
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, WindowOptions};

    struct ControlSample;
    impl View for ControlSample {
        fn render(&mut self, _: &mut ViewContext<'_, Self>) -> impl IntoElement {
            let mut column = div().flex_col().gap(12.).p(12.);
            for (id, height) in [("regular", 24.), ("large", 30.)] {
                column = column.child(
                    Editor::control("Normal")
                        .id(id)
                        .w(190.)
                        .h(height)
                        .child(div().flex_1())
                        .child(Icon::PopupChevron.element(14.).id(format!("{id}-glyph"))),
                );
            }
            column
        }
    }

    #[test]
    fn capsule_controls_center_glyphs_at_regular_and_large_heights() {
        let (mut cx, view) = Application::new()
            .font(crate::UI_FONT)
            .into_test_context(
                WindowOptions::new("Control alignment").size(240., 120.),
                ControlSample,
            )
            .unwrap();
        for id in ["regular", "large"] {
            let button = cx.element_bounds(view.window_handle(), id).unwrap();
            let glyph = cx
                .element_bounds(view.window_handle(), format!("{id}-glyph"))
                .unwrap();
            assert!(
                ((glyph.y + glyph.height / 2.) - (button.y + button.height / 2.)).abs() <= 0.5,
                "{id} glyph must be vertically centered: button={button:?}, glyph={glyph:?}"
            );
            assert_eq!(glyph.height, 14.);
        }
    }
}
