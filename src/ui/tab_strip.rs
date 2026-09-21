use super::*;

const TAB_GAP: f32 = 6.;
const EDGE_FADE: f32 = 28.;
const NEW_SLOT_WIDTH: f32 = 80.;

#[derive(Default)]
pub(super) struct TabScrolling {
    scroll: quickgui::ScrollAreaState,
    selected: Option<uuid::Uuid>,
    pub(super) dragging: bool,
    laid_out_dragging: bool,
    viewport: quickgui::LayoutBoundsHandle,
    tabs: std::collections::HashMap<uuid::Uuid, quickgui::LayoutBoundsHandle>,
}

impl TabScrolling {
    fn layout(&mut self, width: f32, tabs: &[ProjectTab], index: usize) -> f32 {
        self.tabs
            .retain(|id, _| tabs.iter().any(|tab| tab.id == *id));
        let mut total = 0.;
        let mut selected_range = (0., 0.);
        for (i, tab) in tabs.iter().enumerate() {
            let bounds = self.tabs.entry(tab.id).or_default();
            let tab_width = bounds.bounds().map_or(195., |b| b.width);
            if i == index {
                selected_range = (total, total + tab_width);
            }
            total += tab_width + TAB_GAP;
        }
        total = (total - TAB_GAP).max(0.) + EDGE_FADE;
        if self.dragging {
            total += NEW_SLOT_WIDTH + TAB_GAP;
            selected_range = (total - EDGE_FADE - NEW_SLOT_WIDTH, total - EDGE_FADE);
        }
        let drag_changed = self.dragging != self.laid_out_dragging;
        self.laid_out_dragging = self.dragging;
        let resized = self.scroll.viewport().width != width;
        let relaid = self.scroll.content().width != total;
        self.scroll.set_geometry(
            quickgui::Size::new(width, 34.),
            quickgui::Size::new(total, 34.),
        );
        let id = tabs[index].id;
        if self.selected != Some(id) || resized || relaid || drag_changed {
            let (left, right) = selected_range;
            let offset = self.scroll.offset().x;
            let next = if left < offset + EDGE_FADE {
                (left - EDGE_FADE).max(0.)
            } else if right > offset + width - EDGE_FADE {
                right + EDGE_FADE - width
            } else {
                offset
            };
            self.scroll.set_offset(quickgui::Vector::new(next, 0.));
            self.selected = Some(id);
        }
        total
    }
}

impl Editor {
    // ProjectWorkspace.canSwitch permits an ordinary layer transform, which
    // commits on switching, but keeps gradient and floating-pixel drafts open.
    pub(super) fn can_switch_projects(&self) -> bool {
        self.errors.is_empty()
            && !self.pending
            && self.gesture.is_none()
            && self.rename.is_none()
            && self.pending_gradient.is_none()
            && self.pending_pixels.is_none()
            && self.close_intent.is_none()
            && matches!(self.modal, None | Some(Form::Blend))
    }

    pub(super) fn track_tab_drag(&mut self, event: &Event, cx: &mut EventContext) {
        let dragging = match event {
            Event::FilesHovered(files) => Some(!files.paths().is_empty()),
            Event::FilesHoverCancelled
            | Event::FilesDropped(_)
            | Event::ExternalDragEnded(_)
            | Event::Focused(false)
            | Event::MouseButton {
                button: quickgui::MouseButton::Left,
                pressed: false,
            }
            | Event::KeyDown {
                key: Key::Escape, ..
            } => Some(false),
            _ => None,
        };
        if let Some(dragging) = dragging
            && self.tab_scrolling.dragging != dragging
        {
            self.tab_scrolling.dragging = dragging;
            if !dragging {
                // Drag hover delivery can leave the prior canvas/row position cached.
                let pointer = cx.pointer_position();
                self.canvas_pointer = pointer
                    .zip(self.canvas_bounds.bounds())
                    .filter(|(point, bounds)| bounds.contains(*point))
                    .map(|(point, bounds)| {
                        [(point.x - bounds.x) as f64, (point.y - bounds.y) as f64]
                    });
                self.layer_list.cursors.pointer = pointer.filter(|point| {
                    self.layer_list
                        .cursors
                        .viewport
                        .bounds()
                        .is_some_and(|bounds| bounds.contains(*point))
                });
            }
            cx.invalidate();
        }
    }

    fn new_tab_drop_slot(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let enabled = self.can_switch_projects();
        static OUTLINE: std::sync::OnceLock<quickgui::Svg> = std::sync::OnceLock::new();
        let outline = OUTLINE
            .get_or_init(|| {
                quickgui::Svg::from_bytes(include_bytes!("../../assets/new-tab-drop.svg"))
                    .expect("Embedded drop outline must be valid SVG")
            })
            .clone();
        let target = quickgui::svg(outline)
            .id("new-tab-drop")
            .absolute()
            .left(0.)
            .top(0.)
            .w(NEW_SLOT_WIDTH)
            .h(28.)
            .rounded(14.)
            .bg(Color::rgb8(51, 51, 51))
            // Tint only the dashed surface, keeping the label outside its drag-state inheritance.
            .text_color(Color::rgb8(160, 160, 160))
            .tooltip("Drop to open in a new canvas")
            .accessibility_label("Drop into new canvas")
            .drag_over(|s| {
                s.bg(Color::rgba8(0, 122, 255, 77))
                    .text_color(Color::TRANSPARENT)
                    .outline_offset(2., Color::rgb8(0, 122, 255), -2.)
                    .cursor_copy()
            })
            .can_drop(move |drag: &super::layer_drag::LayerDrag| {
                enabled && drag.operation == super::layer_drag::Transfer::Move
            })
            .on_drop(cx.drop_listener(
                "new-tab-drop",
                |this, drag: &super::layer_drag::LayerDrag, _, cx| {
                    if !this.can_switch_projects() {
                        return;
                    }
                    let result = this.copy_drag_to_new_tab(drag);
                    this.result(result, cx);
                },
            ))
            .on_drop(cx.drop_listener(
                "new-tab-drop",
                |this, files: &quickgui::DroppedFiles, event, cx| {
                    this.drop_files(files, None, event, cx);
                },
            ));
        div()
            .relative()
            .w(NEW_SLOT_WIDTH)
            .h(28.)
            .flex_shrink_0()
            .flex_row()
            .items_center()
            .justify_center()
            .gap(5.)
            .child(target)
            .child(Icon::Plus.element(12.))
            .child(text("New").text_size(12.).font_medium())
    }
    pub(super) fn tab_strip(&mut self, cx: &mut ViewContext<'_, Self>) -> Element {
        let enabled = self.can_switch_projects();
        let width = self
            .tab_scrolling
            .viewport
            .bounds()
            .map_or((cx.size().width - 352.).max(200.), |bounds| bounds.width);
        let total = self.tab_scrolling.layout(width, &self.tabs, self.current);
        let mut tabs = div()
            .absolute()
            .left(0.)
            .top(3.)
            .h(28.)
            .flex_row()
            .gap(TAB_GAP)
            .w(total)
            .translate(-self.tab_scrolling.scroll.offset().x, 0.);
        for (index, session) in self.tabs.iter().enumerate() {
            let accepts_layers = enabled && self.can_receive_tab_layers(index);
            let session_id = session.id;
            let id = format!("project-tab-{session_id}");
            let name = session.title();
            let mut label = div()
                .flex_row()
                .items_center()
                .gap(5.)
                .opacity(if !enabled && index != self.current {
                    0.4
                } else {
                    1.
                })
                .min_w(35.)
                .max_w(155.);
            if session.dirty() {
                label = label.child(
                    div()
                        .w(5.)
                        .h(5.)
                        .rounded(2.5)
                        .flex_shrink_0()
                        .bg(Color::rgb8(220, 220, 220))
                        .accessibility_label("Unsaved changes"),
                );
            }
            let title = text(name.clone()).text_size(12.).truncate().min_w(0.);
            label = label.child(if index == self.current {
                title.font_semibold()
            } else {
                title.font_medium()
            });
            let drop = cx.drop_listener(
                id.clone(),
                move |this, drag: &super::layer_drag::LayerDrag, _, cx| {
                    if !this.can_switch_projects() {
                        return;
                    }
                    let result = this.copy_drag_to_tab(drag, index);
                    this.result(result, cx);
                },
            );
            tabs = tabs.child(
                div()
                    .flex_row()
                    .items_center()
                    .h(28.)
                    .flex_shrink_0()
                    .rounded(14.)
                    .report_bounds(self.tab_scrolling.tabs[&session_id].clone())
                    .tooltip(name.clone())
                    .child(
                        div()
                            .padding(0., 8., 0., 11.)
                            .h(28.)
                            .flex_row()
                            .items_center()
                            .child(label),
                    )
                    .child(
                        button()
                            .w(21.)
                            .h(28.)
                            .padding(0., 5., 0., 0.)
                            .bg(Color::TRANSPARENT)
                            .text_color(Color::rgb8(155, 155, 155))
                            .flex_row()
                            .items_center()
                            .justify_center()
                            .flex_shrink_0()
                            .child(Icon::X.element(9.))
                            .accessibility_label(format!("Close {name}"))
                            .tooltip(format!("Close {name}"))
                            .disabled(!enabled)
                            .disabled_style(|style| style.opacity(0.4))
                            .on_click(cx.listener(
                                format!("close-tab-{session_id}"),
                                move |this, cx| {
                                    cx.stop_propagation();
                                    this.request_close(CloseIntent::Tab(session_id), cx);
                                },
                            )),
                    )
                    .drag_over(|s| {
                        s.bg(Color::rgba8(0, 122, 255, 77))
                            .outline_offset(2., Color::rgb8(0, 122, 255), -2.)
                            .cursor_copy()
                    })
                    .can_drop(move |drag: &super::layer_drag::LayerDrag| {
                        accepts_layers
                            && drag.operation == super::layer_drag::Transfer::Move
                            && drag.session != session_id
                    })
                    .on_drop(drop)
                    .on_drop(cx.drop_listener(
                        id.clone(),
                        move |this, files: &quickgui::DroppedFiles, event, cx| {
                            this.drop_files(files, Some(session_id), event, cx);
                        },
                    ))
                    .bg(if index == self.current {
                        Color::rgb8(68, 68, 68)
                    } else {
                        Color::rgb8(50, 50, 50)
                    })
                    .border(
                        1.,
                        if index == self.current {
                            Color::rgb8(90, 90, 90)
                        } else {
                            Color::rgb8(60, 60, 60)
                        },
                    )
                    .on_click(cx.listener(id.clone(), move |this, cx| {
                        if this.current != index && this.can_switch_projects() {
                            if let Err(error) = this.finish_pending_edits() {
                                this.result(Err(error), cx);
                                return;
                            }
                            this.activate_tab(index);
                            this.changed(cx);
                        }
                    }))
                    .disabled(!enabled && index != self.current),
            );
        }
        if self.tab_scrolling.dragging && enabled {
            tabs = tabs.child(self.new_tab_drop_slot(cx));
        }
        self.tab_viewport(cx, tabs)
    }

    fn tab_viewport(&self, cx: &mut ViewContext<'_, Self>, tabs: Element) -> Element {
        let scroll = cx.scroll_wheel_listener("project-tabs-viewport", |this, event, cx| {
            let delta = event.delta.pixel_delta(40.);
            let distance = if delta.x.abs() > delta.y.abs() {
                delta.x
            } else {
                delta.y
            };
            this.tab_scrolling
                .scroll
                .scroll_by(quickgui::Vector::new(-distance, 0.));
            cx.prevent_default();
            cx.invalidate();
        });
        let viewport = div()
            .id("project-tabs-viewport")
            .accessibility_label("Project tabs")
            .h(34.)
            .w((cx.size().width - 352.).max(200.))
            .min_w(0.)
            .report_bounds(self.tab_scrolling.viewport.clone())
            .relative()
            .overflow_hidden()
            .on_scroll_wheel(scroll)
            .child(tabs);
        // The toolbar has an opaque uniform background, so these passive fades
        // match the source tab mask without intercepting clicks on the tabs.
        let viewport = viewport.child(leading_edge_fade(self.tab_scrolling.scroll.offset().x > 1.));
        viewport.child(
            div()
                .absolute()
                .right(0.)
                .top(0.)
                .w(EDGE_FADE)
                .h(34.)
                .bg_linear_gradient(
                    quickgui::GradientDirection::ToRight,
                    [Color::TRANSPARENT, Color::rgb8(43, 43, 43)],
                ),
        )
    }
}

fn leading_edge_fade(scrolled: bool) -> Element {
    div()
        .id("project-tabs-leading-fade")
        .absolute()
        .left(0.)
        .top(0.)
        .w(EDGE_FADE)
        .h(34.)
        // Scaling this passive gradient is visually the same as animating the mask's
        // width, without changing tab layout or adding application-owned timers.
        .transform_origin(0., 0.)
        .scale(if scrolled { 1. } else { 0. }, 1.)
        .transition(
            quickgui::Transition::new(std::time::Duration::from_millis(150))
                .with_properties(quickgui::TransitionProperties::TRANSFORM)
                .with_easing(edge_fade_ease_out),
        )
        .bg_linear_gradient(
            quickgui::GradientDirection::ToRight,
            [Color::rgb8(43, 43, 43), Color::TRANSPARENT],
        )
}

fn edge_fade_ease_out(progress: f32) -> f32 {
    if progress <= 0. || progress >= 1. {
        return progress;
    }
    // Standard ease-out timing curve: cubic Bézier (0, 0), (0.58, 1).
    let (mut low, mut high) = (0., 1.);
    for _ in 0..16 {
        let t = (low + high) * 0.5;
        let x = 3. * (1. - t) * t * t * 0.58 + t * t * t;
        if x < progress {
            low = t;
        } else {
            high = t;
        }
    }
    let t = (low + high) * 0.5;
    3. * t * t - 2. * t * t * t
}

#[cfg(test)]
mod fade_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, ScrollDelta, ScrollWheelEvent, Vector, WindowOptions};

    #[test]
    fn project_controls_preserve_blocking_drafts_and_commit_ordinary_transforms() {
        for floating in [false, true] {
            let mut editor = Editor::with_test_document();
            let mut doc = Document::new(20, 20).unwrap();
            compositor::edits::fill(&mut doc, [50, 120, 200, 255], false, false).unwrap();
            if floating {
                doc.selection = Some(compositor::selection::Selection::rectangle(
                    20,
                    20,
                    [2., 2.],
                    [10., 10.],
                    false,
                ));
            }
            editor.tabs = vec![
                Session::new(doc, None).into(),
                ProjectTab::empty("Other".into()),
            ];
            let other = editor.tabs[1].id;
            let (mut cx, view) = Application::new()
                .into_test_context(
                    WindowOptions::new("Project drafts").size(1500., 900.),
                    editor,
                )
                .unwrap();
            cx.update(view, |e, cx| {
                if floating {
                    e.begin_pixel_transform().unwrap();
                } else {
                    e.begin_gradient([0., 0.]).unwrap();
                }
                e.changed(cx);
            })
            .unwrap();
            for id in [
                quickgui::ElementId::from(format!("project-tab-{other}")),
                format!("close-tab-{other}").into(),
                100_u64.into(),
            ] {
                assert!(
                    matches!(
                        cx.click(view.window_handle(), id),
                        Err(quickgui::TestAppError::NotClickable { .. })
                    ),
                    "Project control enabled during a pending draft: {id:?}"
                );
            }
            cx.update(view, |e, cx| {
                e.action(Action::New, cx);
                e.action(Action::CloseTab, cx);
                e.request_close(CloseIntent::Tab(other), cx);
            })
            .unwrap();
            cx.read(view, |e| {
                assert_eq!(e.current, 0);
                assert_eq!(e.tabs.len(), 2);
                assert_eq!(e.pending_pixels.is_some(), floating);
                assert_eq!(e.pending_gradient.is_some(), !floating);
            })
            .unwrap();
            cx.update(view, |e, cx| {
                if e.pending_gradient.take().is_some() {
                    e.session_mut().cancel();
                }
                e.finish_toolbar_transform(false).unwrap();
                e.start_toolbar_transform().unwrap();
                e.changed(cx);
            })
            .unwrap();
            let current = cx.read(view, |e| e.tabs[0].id).unwrap();
            cx.click(view.window_handle(), format!("project-tab-{current}"))
                .unwrap();
            cx.read(view, |e| assert!(e.transform_edit.is_some()))
                .unwrap();
            cx.click(view.window_handle(), format!("project-tab-{other}"))
                .unwrap();
            cx.read(view, |e| {
                assert_eq!(e.current, 1);
                assert!(e.transform_edit.is_none());
            })
            .unwrap();
        }
    }

    #[test]
    fn drag_slot_scrolls_into_view_and_cancellation_restores_selected_tab() {
        let mut editor = Editor::new(Vec::new()).unwrap();
        for _ in 0..20 {
            editor.add_empty_tab();
        }
        editor.activate_tab(0);
        let selected = editor.tabs[0].id;
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Drop slot").size(1000., 700.), editor)
            .unwrap();
        let window = view.window_handle();
        assert!(cx.element_bounds(window, "new-tab-drop").is_err());
        cx.update(view, |e, cx| {
            e.event(
                &Event::FilesHovered(quickgui::DroppedFiles::new([std::path::PathBuf::from(
                    "photo.png",
                )])),
                cx,
            )
        })
        .unwrap();
        let viewport = cx.element_bounds(window, "project-tabs-viewport").unwrap();
        assert_eq!(
            viewport.width, 648.,
            "Tab strip must retain the source window-width allowance"
        );
        let slot = cx.element_bounds(window, "new-tab-drop").unwrap();
        assert!(slot.x >= viewport.x && slot.x + slot.width <= viewport.x + viewport.width);
        assert_eq!(cx.read(view, |e| e.current).unwrap(), 0);
        cx.update(view, |e, cx| e.event(&Event::FilesHoverCancelled, cx))
            .unwrap();
        assert!(cx.element_bounds(window, "new-tab-drop").is_err());
        let tab = cx
            .element_bounds(window, format!("project-tab-{selected}"))
            .unwrap();
        assert!(tab.x >= viewport.x && tab.x + tab.width <= viewport.x + viewport.width);
        cx.update(view, |e, cx| {
            e.tab_scrolling.dragging = true;
            cx.invalidate();
        })
        .unwrap();
        assert!(cx.element_bounds(window, "new-tab-drop").is_ok());
        cx.update(view, |e, cx| {
            e.event(
                &Event::MouseButton {
                    button: quickgui::MouseButton::Left,
                    pressed: false,
                },
                cx,
            )
        })
        .unwrap();
        assert!(cx.element_bounds(window, "new-tab-drop").is_err());
    }

    #[test]
    fn many_tabs_keep_selection_and_close_visible_and_scroll_without_switching_projects() {
        let mut editor = Editor::new(Vec::new()).unwrap();
        for _ in 0..100 {
            editor.add_empty_tab();
        }
        let last = editor.tabs[100].id;
        let first = editor.tabs[0].id;
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Many tabs").size(1280., 850.), editor)
            .unwrap();
        let window = view.window_handle();
        let viewport = cx.element_bounds(window, "project-tabs-viewport").unwrap();
        assert_eq!(
            viewport.width, 928.,
            "Tab strip must retain the source window-width allowance"
        );
        let bounds = cx
            .element_bounds(window, format!("project-tab-{last}"))
            .unwrap();
        assert!(bounds.x >= viewport.x);
        assert!(bounds.x + bounds.width <= viewport.x + viewport.width);
        let close = cx
            .element_bounds(window, format!("close-tab-{last}"))
            .unwrap();
        assert!(close.x + close.width <= 1280.);
        let before = cx
            .read(view, |e| e.tab_scrolling.scroll.offset().x)
            .unwrap();
        cx.simulate_scroll_wheel(
            window,
            "project-tabs-viewport",
            ScrollWheelEvent {
                delta: ScrollDelta::Pixels(Vector::new(200., 0.)),
                ..ScrollWheelEvent::default()
            },
        )
        .unwrap();
        assert!(
            cx.read(view, |e| e.tab_scrolling.scroll.offset().x < before)
                .unwrap()
        );
        assert!(
            cx.simulate_scroll_wheel(
                window,
                "project-tabs-viewport",
                ScrollWheelEvent {
                    delta: ScrollDelta::Pixels(Vector::new(30_000., 0.)),
                    ..ScrollWheelEvent::default()
                }
            )
            .unwrap()
        );
        cx.read(view, |e| {
            assert_eq!(e.current, 100);
            assert_eq!(e.tab_scrolling.scroll.offset().x, 0.);
        })
        .unwrap();
        cx.click(window, format!("project-tab-{first}")).unwrap();
        assert_eq!(cx.read(view, |e| e.current).unwrap(), 0);
        cx.click(window, format!("close-tab-{first}")).unwrap();
        cx.read(view, |e| {
            assert_eq!(e.tabs.len(), 100);
            assert!(e.tabs.iter().all(|tab| tab.id != first));
        })
        .unwrap();
    }
}
