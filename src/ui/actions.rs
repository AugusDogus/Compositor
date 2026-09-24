use super::*;
use compositor::{edits, invalid};

impl Editor {
    pub(super) fn action(&mut self, action: Action, cx: &mut EventContext) {
        if !self.action_available(action) {
            return;
        }
        if matches!(action, Action::InvertPixels) {
            let result = self.invert();
            self.operation_result(alerts::Operation::Paint, result, cx);
            return;
        }
        if self.floating_panel_kind().is_some() && action.is_project_operation() {
            if let Err(error) = self.finish_pending_edits() {
                self.result(Err(error), cx);
                return;
            }
            self.tools.pending_crop = None;
            match action {
                Action::CanvasSize | Action::ImageSize | Action::Trim => self.open_form(action),
                Action::ExportJpeg | Action::ExportJpegFile => self.open_jpeg(None),
                _ => self.file_action(action, cx),
            }
            self.changed(cx);
            return;
        }
        let action = if matches!(action, Action::DeleteLayer)
            && self.tools.mask_target
            && self.session().document.selected.len() <= 1
            && self
                .session()
                .document
                .active_layer()
                .is_some_and(|layer| layer.mask.is_some())
        {
            Action::DeleteMask
        } else {
            action
        };
        // View commands change only the viewport, leaving edit previews and sheets intact.
        if !(matches!(
            action,
            Action::Fit | Action::Actual | Action::ZoomIn | Action::ZoomOut | Action::PixelGrid
        ) || matches!(action, Action::Undo | Action::Redo) && self.history_preserves_preview())
        {
            if !matches!(action, Action::Color | Action::Undo | Action::Redo)
                && let Err(error) = self.finish_pending_edits()
            {
                self.result(Err(error), cx);
                return;
            }
            self.size_menus.close(cx);
            self.tools.crop_picker.close(cx);
            self.tools.transform_sampling.close(cx);
            self.tools.mask_paint_picker.close(cx);
            self.tools.wand_picker.close(cx);
            self.tools.gradient_picker.close(cx);
            self.blend_picker.close();
            self.sample_ring = None;
            self.cancel_adjustment();
            if !matches!(
                action,
                Action::CropSettings
                    | Action::New
                    | Action::Open
                    | Action::CloseTab
                    | Action::AdjustPixels(Kind::HueSaturation)
            ) {
                self.tools.pending_crop = None;
            }
            if matches!(action, Action::AdjustPixels(kind) if kind != Kind::HueSaturation)
                || matches!(action, Action::CameraRaw | Action::Filter(_) | Action::RemoveBackground)
            {
                self.tools.polygon = None;
            }
            self.cancel_filter();
            self.jpeg_export = None;
            self.modal = None;
        }
        let previous_target = self.tabs[self.current]
            .session()
            .map(|session| (session.id, session.document.active));
        let color = self.palette_colors(self.tools.mask_target)[0];
        let mask = self.tools.mask_target;
        let result = match action {
            Action::Transform if self.can_float_selection() => self.begin_pixel_transform(),
            Action::Transform
                if compositor::transform::selection_bounds(&self.session().document, mask)
                    .is_none() =>
            {
                Err(invalid(
                    "Select a visible layer with pixels before transforming it.",
                ))
            }
            Action::New => {
                self.add_empty_tab();
                self.suggest_new_canvas(cx)
            }
            Action::OpenClipboard => self.queue_clipboard(clipboard_jobs::Request::Open, cx),
            Action::Transform => self.start_toolbar_transform(),
            Action::Trim => {
                self.open_form(action);
                Ok(())
            }
            Action::CanvasSize
            | Action::ImageSize
            | Action::CropSettings
            | Action::Color
            | Action::FeatherSelection => {
                self.open_form(action);
                if matches!(self.modal, Some(Form::MaskColor(_))) {
                    cx.focus(quickgui::FocusHandle::new("mask-color-false"));
                }
                Ok(())
            }
            Action::Adjustment(kind) => self.open_adjustment(Some(kind)),
            Action::Rename => {
                let result = self.begin_rename();
                if result.is_ok() {
                    cx.focus(quickgui::FocusHandle::new("layer-rename"));
                }
                result
            }
            Action::Filter(filter) => self.open_filter(filter),
            Action::CameraRaw => self.open_camera_raw(),
            Action::RemoveBackground => self.open_background(),
            Action::AdjustPixels(kind) => self.open_pixel_adjustment(kind),
            Action::EditAdjustment => self.open_adjustment(None),
            Action::ExportJpeg | Action::ExportJpegFile => {
                self.open_jpeg(None);
                Ok(())
            }
            Action::Open
            | Action::OpenPsd
            | Action::OpenRaw
            | Action::Import
            | Action::Save
            | Action::SaveAs
            | Action::ExportPng
            | Action::ExportTiff
            | Action::ExportWebp
            | Action::ExportPsd => {
                self.file_action(action, cx);
                Ok(())
            }
            Action::DevelopRaw => {
                let id = self
                    .session()
                    .document
                    .active
                    .ok_or_else(|| invalid("Select a RAW layer to develop."));
                id.and_then(|id| self.start_develop_layer(id))
            }
            Action::RasterizeRaw => self.session_mut().edit("Rasterize RAW Layer", |doc| {
                let layer = doc
                    .active_layer_mut()
                    .ok_or_else(|| invalid("Select a RAW layer to rasterize."))?;
                if layer.raw.take().is_none() {
                    return Err(invalid("The selected layer has no editable RAW source."));
                }
                Ok(())
            }),
            Action::Undo => {
                self.undo_document();
                Ok(())
            }
            Action::Copy | Action::CopyMerged | Action::Cut | Action::Paste => {
                self.clipboard_action(action, cx)
            }
            Action::Redo => {
                let was_empty = !self.has_document();
                self.redo_document();
                if was_empty && self.has_document() {
                    cx.focus(quickgui::FocusHandle::new("workspace"));
                }
                Ok(())
            }
            Action::AddLayer => {
                let result = self
                    .session_mut()
                    .edit("New Blank Layer", compositor::layer_ops::add_blank);
                if result.is_ok() {
                    self.tools.mask_target = false;
                    if let Some(parent) = self
                        .session()
                        .document
                        .active_layer()
                        .and_then(|l| l.parent)
                    {
                        self.session_mut().collapsed.remove(&parent);
                    }
                }
                result
            }
            Action::Duplicate => {
                let result = if self.session().document.selection.is_some() {
                    let mask = self.tools.mask_target;
                    self.session_mut().edit("Layer via Copy", |doc| {
                        let clip = compositor::clipboard::copy(doc, false, mask)?;
                        compositor::clipboard::paste(doc, clip)
                    })
                } else {
                    self.session_mut()
                        .edit("Duplicate Layer", compositor::layer_ops::duplicate_selected)
                };
                if result.is_ok() {
                    self.tools.mask_target = false;
                }
                result
            }
            Action::DeleteLayer
                if compositor::clipping::deletion_has_dependents(&self.session().document) =>
            {
                self.modal = Some(Form::DeleteLayers);
                Ok(())
            }
            Action::DeleteLayer | Action::DeleteLayerUnlinked => self.session_mut().delete_layers(),
            Action::DeleteLayerBaked => {
                self.queue(jobs::Job::DeleteLayersBaked);
                Ok(())
            }
            Action::Group => self.session_mut().group(),
            Action::Ungroup => self.session_mut().ungroup(),
            Action::MoveOutOfGroup => self
                .session_mut()
                .edit("Move Layer", compositor::layer_ops::move_out_of_group),
            Action::Raise => self.session_mut().reorder(true),
            Action::Lower => self.session_mut().reorder(false),
            Action::Merge => {
                let doc = &self.session().document;
                let label = if doc.selected.len() > 1 {
                    "Merge Layers"
                } else if doc.active_layer().is_some_and(|layer| layer.is_group()) {
                    "Merge Group"
                } else {
                    "Merge Down"
                };
                self.session_mut()
                    .edit(label, |doc| compositor::layer_ops::merge(doc, false))
            }
            Action::AddMask | Action::HideMask => {
                let label = if self.session().document.selection.is_some() {
                    "Add Mask from Selection"
                } else if matches!(action, Action::HideMask) {
                    "Add Hide-All Mask"
                } else {
                    "Add Reveal-All Mask"
                };
                let result = self.session_mut().edit(label, |doc| {
                    edits::add_mask(doc, matches!(action, Action::HideMask))
                });
                if result.is_ok() {
                    self.tools.mask_target = true;
                }
                result
            }
            Action::ToggleMask | Action::DeleteMask => {
                let label = if matches!(action, Action::DeleteMask) {
                    "Delete Layer Mask"
                } else if self
                    .session()
                    .document
                    .active_layer()
                    .and_then(|layer| layer.mask.as_ref())
                    .is_some_and(|mask| mask.enabled)
                {
                    "Disable Layer Mask"
                } else {
                    "Enable Layer Mask"
                };
                let result = self.session_mut().edit(label, |doc| {
                    let layer = doc
                        .active_layer_mut()
                        .ok_or_else(|| invalid("Select a layer with a mask."))?;
                    let mask = layer
                        .mask
                        .as_mut()
                        .ok_or_else(|| invalid("The selected layer has no mask."))?;
                    match action {
                        Action::ToggleMask => mask.enabled = !mask.enabled,
                        _ => layer.mask = None,
                    }
                    Ok(())
                });
                if result.is_ok() && matches!(action, Action::DeleteMask) {
                    self.tools.mask_target = false;
                }
                result
            }
            Action::Clip => self
                .session()
                .document
                .active
                .ok_or_else(|| invalid("Select a layer to clip."))
                .and_then(|target| self.toggle_clipping(target)),
            Action::SelectSubject => {
                self.queue(jobs::Job::SelectForeground(
                    compositor::object_selection::Settings {
                        target: compositor::object_selection::Target::Subject,
                        edge_offset: 0,
                        sample_all: self.tools.object_sample_all,
                        antialiased: self.tools.selection_antialiased,
                        mode: self.tools.selection_mode,
                    },
                ));
                Ok(())
            }
            Action::LoadAlpha | Action::LoadMask => {
                let mode = compositor::selection::SelectionMode::Replace;
                let antialiased = self.tools.selection_antialiased;
                let label = if matches!(action, Action::LoadMask) {
                    "Load Mask Selection"
                } else {
                    "Load Layer Selection"
                };
                self.session_mut().edit(label, |doc| {
                    edits::load_selection(
                        doc,
                        matches!(action, Action::LoadMask),
                        mode,
                        antialiased,
                    )
                })
            }
            Action::SelectAll => self.session_mut().edit("Select All", |doc| {
                doc.selection = Some(compositor::selection::Selection::marquee(
                    doc.width,
                    doc.height,
                    [0., 0.],
                    [doc.width as f64, doc.height as f64],
                    false,
                    false,
                )?);
                Ok(())
            }),
            Action::Deselect => self.session_mut().edit("Deselect", |doc| {
                doc.selection = None;
                Ok(())
            }),
            Action::InvertSelection => self.session_mut().edit("Inverse", |doc| {
                if let Some(s) = &doc.selection {
                    doc.selection = Some(s.invert(doc.width, doc.height)?);
                }
                Ok(())
            }),
            Action::Fill | Action::FillBackground | Action::Clear => {
                let clear = matches!(action, Action::Clear);
                let color = if matches!(action, Action::FillBackground) || clear && mask {
                    self.palette_colors(mask)[1]
                } else {
                    color
                };
                let label = if mask {
                    "Fill Mask"
                } else if clear {
                    "Clear"
                } else {
                    "Fill"
                };
                self.session_mut()
                    .edit(label, |doc| edits::fill(doc, color, clear && !mask, mask))
            }
            Action::Rulers
            | Action::Grid
            | Action::Guides
            | Action::LockGuides
            | Action::ClearGuides
            | Action::Snap
            | Action::SnapGrid
            | Action::SnapGuides
            | Action::SnapLayers
            | Action::SnapBounds => self.layout_action(action),
            Action::PixelGrid => {
                self.tools.pixel_grid = !self.tools.pixel_grid;
                Ok(())
            }
            Action::FlipCanvasX | Action::FlipCanvasY => {
                let label = if matches!(action, Action::FlipCanvasX) {
                    "Flip Canvas Horizontal"
                } else {
                    "Flip Canvas Vertical"
                };
                self.session_mut().edit(label, |doc| {
                    edits::flip_canvas(doc, matches!(action, Action::FlipCanvasX));
                    Ok(())
                })
            }
            Action::FlipX | Action::FlipY => self.session_mut().edit(
                if matches!(action, Action::FlipX) {
                    "Flip Horizontal"
                } else {
                    "Flip Vertical"
                },
                |doc| {
                    let old = compositor::transform::selection_bounds(doc, mask)
                        .ok_or_else(|| invalid("Select layers or a mask to flip."))?;
                    let horizontal = matches!(action, Action::FlipX);
                    let center = old.geometry_point([0.5, 0.5]);
                    let new = old.mirrored(horizontal, center[usize::from(!horizontal)]);
                    compositor::transform::apply(doc, old, new, mask)
                },
            ),
            Action::InvertPixels => self.invert(),
            Action::Blend => {
                let result = self.open_blend();
                if result.is_ok() {
                    cx.focus(quickgui::FocusHandle::new("blend-menu-items"));
                }
                result
            }
            Action::Fit => {
                self.session_mut().fit = true;
                self.session_mut().pan = [0., 0.];
                Ok(())
            }
            Action::Actual => {
                self.session_mut().zoom_at(1., [0., 0.]);
                Ok(())
            }
            Action::ZoomIn | Action::ZoomOut => {
                let factor = if matches!(action, Action::ZoomIn) {
                    1.25
                } else {
                    0.8
                };
                let session = self.session_mut();
                session.zoom_at(session.zoom * factor, [0., 0.]);
                Ok(())
            }
            Action::CloseTab => {
                self.request_close(CloseIntent::Tab(self.tabs[self.current].id), cx);
                Ok(())
            }
        };
        if let Some((id, active)) = previous_target
            && self.tabs[self.current]
                .session()
                .is_some_and(|session| session.id == id)
        {
            self.retain_mask_target(active);
        }
        if matches!(
            action,
            Action::AdjustPixels(_)
                | Action::Adjustment(_)
                | Action::EditAdjustment
                | Action::Filter(_)
                | Action::RemoveBackground
        ) && self.floating_panel_kind().is_some()
        {
            cx.focus(quickgui::Dialog::new("editor-dialog", true).popover_focus());
        }
        self.operation_result(alerts::Operation::for_action(action), result, cx);
    }

    fn select_tool_key(&mut self, key: &str, shift: bool, cx: &mut EventContext) {
        let tool = match key.to_lowercase().as_str() {
            "a" => Some(Tool::Idle),
            "v" => Some(Tool::Move),
            "m" => Some(self.tools.tool_preferences.marquee()),
            "l" => Some(self.tools.tool_preferences.lasso()),
            "w" => Some(if self.tools.tool == Tool::Wand {
                Tool::Object
            } else {
                Tool::Wand
            }),
            "o" => Some(Tool::Object),
            "c" => Some(Tool::Crop),
            "b" => Some(Tool::Brush),
            "e" => Some(Tool::Erase),
            "s" => Some(Tool::Clone),
            "j" => Some(Tool::Heal),
            "r" => Some(self.tools.tool_preferences.smear()),
            "g" => Some(Tool::Gradient),
            "t" => Some(Tool::Text),
            "u" => {
                if shift && self.tools.tool == Tool::Shape {
                    self.set_shape_kind(self.tools.shape_kind.next());
                }
                Some(Tool::Shape)
            }
            "i" => Some(Tool::Eyedropper),
            "h" => Some(Tool::Hand),
            "z" => Some(Tool::Zoom),
            _ => None,
        };
        if let Some(tool) = tool {
            cx.prevent_default();
            self.select_tool(tool, cx);
        }
    }

    fn tool_key(&mut self, c: &str, modifiers: Modifiers, cx: &mut EventContext) {
        let shift = modifiers.contains(Modifiers::SHIFT);
        if !modifiers.contains(Modifiers::ALT) && ["x", "d"].contains(&c.to_lowercase().as_str()) {
            cx.prevent_default();
            let result = self.change_palette(c.eq_ignore_ascii_case("x"));
            self.operation_result(alerts::Operation::Paint, result, cx);
            return;
        }
        if c.len() == 1
            && let Some(digit) = c.chars().next().and_then(|c| c.to_digit(10))
        {
            cx.prevent_default();
            let result = self.opacity_digit(digit as u8, std::time::Instant::now());
            if self.tools.tool == Tool::Gradient {
                self.operation_result(alerts::Operation::Paint, result, cx);
            } else {
                self.result(result, cx);
            }
            return;
        }
        if ["[", "]", "{", "}"].contains(&c) {
            cx.prevent_default();
            self.brush_step(matches!(c, "]" | "}"), shift || matches!(c, "{" | "}"));
            cx.invalidate();
            return;
        }
        self.select_tool_key(c, shift, cx);
    }

    pub(super) fn key(&mut self, key: &Key, modifiers: Modifiers, cx: &mut EventContext) {
        if self.layout_drag.is_some() {
            if *key == Key::Escape {
                self.layout_drag = None;
                cx.invalidate();
            }
            return;
        }
        if let Some((key, modifiers)) = self.keymap.translate(key, modifiers, false) {
            self.default_key(&key, modifiers, cx);
        } else {
            cx.prevent_default();
        }
    }

    fn default_key(&mut self, key: &Key, modifiers: Modifiers, cx: &mut EventContext) {
        if self.psd_conversion.is_some() || !self.errors.is_empty() {
            return;
        }
        let menu = if *key == Key::Function(10) && modifiers.is_empty() {
            Some(0)
        } else if modifiers == Modifiers::ALT {
            if let Key::Character(c) = key {
                match c.to_lowercase().as_str() {
                    "f" => Some(0),
                    "e" => Some(1),
                    "i" => Some(4),
                    "l" => Some(6),
                    "s" => Some(3),
                    "t" => Some(5),
                    "v" => Some(2),
                    "h" => Some(7),
                    _ => None,
                }
            } else {
                None
            }
        } else {
            None
        };
        if let Some(index) = menu {
            self.menus.activate(index, cx);
            cx.prevent_default();
            return;
        }
        if modifiers == Modifiers::CONTROL
            && matches!(key,Key::Character(c) if c.eq_ignore_ascii_case("q"))
        {
            self.request_close(CloseIntent::Window, cx);
            cx.prevent_default();
            return;
        }

        if !self.has_document() {
            if *key == Key::Enter && modifiers.is_empty() {
                cx.prevent_default();
                self.create_welcome_canvas(cx);
                return;
            }
            if (modifiers - Modifiers::SHIFT) == Modifiers::CONTROL
                && let Key::Character(c) = key
            {
                let action = match c.to_lowercase().as_str() {
                    "z" => Some(if modifiers.contains(Modifiers::SHIFT) {
                        Action::Redo
                    } else {
                        Action::Undo
                    }),
                    "y" => Some(Action::Redo),
                    "n" if modifiers == Modifiers::CONTROL => Some(Action::New),
                    "o" if modifiers == Modifiers::CONTROL => Some(Action::Open),
                    "v" if modifiers == Modifiers::CONTROL => Some(Action::Paste),
                    "w" if modifiers == Modifiers::CONTROL => Some(Action::CloseTab),
                    _ => None,
                };
                if let Some(action) = action {
                    cx.prevent_default();
                    self.action(action, cx);
                }
            }
            if (modifiers - Modifiers::SHIFT).is_empty()
                && let Key::Character(c) = key
            {
                self.tool_key(c, modifiers, cx);
            }
            return;
        }
        if self.transform_edit.is_some() && matches!(key, Key::Enter | Key::Escape) {
            let result = self.finish_toolbar_transform(*key == Key::Enter);
            self.result(result, cx);
            cx.prevent_default();
            return;
        }
        if matches!(
            self.gesture,
            Some(Gesture::Paint { .. } | Gesture::Warp { .. })
        ) && *key != Key::Escape
        {
            cx.prevent_default();
            return;
        }
        let ctrl = modifiers.contains(Modifiers::CONTROL);
        let shift = modifiers.contains(Modifiers::SHIFT);
        let alt = modifiers.contains(Modifiers::ALT);
        if shift
            && !ctrl
            && !alt
            && let Key::Character(c) = key
            && ["+", "=", "-", "_"].contains(&c.as_str())
        {
            cx.prevent_default();
            let result = self.step_blend(matches!(c.as_str(), "+" | "="));
            self.result(result, cx);
            return;
        }
        if *key == Key::Tab
            && modifiers.is_empty()
            && matches!(self.tools.tool, Tool::Wand | Tool::Object)
        {
            cx.prevent_default();
            self.select_tool(
                if self.tools.tool == Tool::Wand {
                    Tool::Object
                } else {
                    Tool::Wand
                },
                cx,
            );
            return;
        }
        let action = match key {
            Key::Character(c) if ctrl => match c.to_lowercase().as_str() {
                "n" if !alt => Some(if shift { Action::AddLayer } else { Action::New }),
                "o" => Some(Action::Open),
                "s" => Some(if shift && alt {
                    Action::ExportJpegFile
                } else if shift {
                    Action::SaveAs
                } else {
                    Action::Save
                }),
                "z" => Some(if shift { Action::Redo } else { Action::Undo }),
                "y" => Some(Action::Redo),
                "a" => Some(Action::SelectAll),
                "d" => Some(Action::Deselect),
                "j" => Some(Action::Duplicate),
                "c" => Some(if alt {
                    Action::CanvasSize
                } else if shift {
                    Action::CopyMerged
                } else {
                    Action::Copy
                }),
                "x" => Some(Action::Cut),
                "v" => Some(Action::Paste),
                "g" => Some(if alt {
                    Action::Clip
                } else if shift {
                    Action::Ungroup
                } else {
                    Action::Group
                }),
                "e" => Some(if shift {
                    Action::ExportPng
                } else {
                    Action::Merge
                }),
                "t" => Some(Action::Transform),
                "h" if !alt && self.tools.tool == Tool::Move => {
                    cx.prevent_default();
                    self.tools.show_transform_controls = !self.tools.show_transform_controls;
                    cx.invalidate();
                    None
                }
                "r" if !alt && !shift => Some(Action::Rulers),
                "\'" if !alt && !shift => Some(Action::Grid),
                ";" | ":" if !alt => Some(if shift { Action::Snap } else { Action::Guides }),
                "w" => Some(Action::CloseTab),
                "0" => Some(Action::Fit),
                "1" => Some(Action::Actual),
                "i" => Some(if alt {
                    Action::ImageSize
                } else if shift {
                    Action::InvertSelection
                } else {
                    Action::InvertPixels
                }),
                "u" => Some(Action::AdjustPixels(Kind::HueSaturation)),
                "l" => Some(Action::AdjustPixels(Kind::Levels)),
                "m" => Some(Action::AdjustPixels(Kind::Curves)),
                "]" => Some(Action::Raise),
                "[" => Some(Action::Lower),
                "+" | "=" => Some(Action::ZoomIn),
                "-" => Some(Action::ZoomOut),
                _ => None,
            },
            Key::Character(c) if !alt => {
                self.tool_key(c, modifiers, cx);
                None
            }
            Key::Delete | Key::Backspace if self.tools.polygon.is_some() => {
                cx.prevent_default();
                if let Some(draft) = &mut self.tools.polygon {
                    draft.points.pop();
                    if draft.points.is_empty() {
                        self.tools.polygon = None;
                    }
                }
                cx.invalidate();
                None
            }
            Key::Delete | Key::Backspace if alt => Some(Action::Fill),
            Key::Delete | Key::Backspace if ctrl => Some(Action::FillBackground),
            Key::Delete | Key::Backspace if shift => {
                Some(Action::Filter(compositor::filters::Filter::ContentFill))
            }
            Key::Delete | Key::Backspace => Some(if self.session().document.selection.is_some() {
                Action::Clear
            } else if self.tools.mask_target {
                Action::DeleteMask
            } else {
                Action::DeleteLayer
            }),
            Key::Escape => {
                cx.prevent_default();
                self.sample_ring = None;
                self.tools.pending_crop = None;
                self.gesture = None;
                self.pending_gradient = None;
                self.pending_pixels = None;
                self.tools.polygon = None;
                self.session_mut().cancel();
                self.changed(cx);
                None
            }
            Key::Enter if self.tools.pending_crop.is_some() => {
                cx.prevent_default();
                let result = self.commit_crop();
                self.operation_result(alerts::Operation::Crop, result, cx);
                None
            }
            Key::Enter if self.pending_gradient.is_some() || self.pending_pixels.is_some() => {
                cx.prevent_default();
                let result = self.finish_pending_edits();
                self.operation_result(alerts::Operation::Paint, result, cx);
                None
            }
            Key::Enter if self.tools.tool == Tool::Polygon => {
                cx.prevent_default();
                self.finish_polygon(cx);
                None
            }
            Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown => {
                cx.prevent_default();
                let result = self.nudge(key, modifiers);
                self.operation_result(alerts::Operation::Paint, result, cx);
                None
            }
            _ => None,
        };
        if let Some(action) = action {
            cx.prevent_default();
            self.action(action, cx);
        }
    }
}
