mod about;
mod actions;
mod adjustment_channel;
mod adjustment_fields;
mod adjustment_histogram;
mod adjustment_layers;
mod adjustment_preview;
mod adjustments;
#[cfg(test)]
mod alert_tests;
mod alerts;
mod appearance;
mod autoscroll;
mod background_controls;
mod brush_controls;
mod brush_cursor;
mod brush_smoothing;
mod brush_tip;
mod byte_count;
mod camera_raw;
mod canvas;
mod canvas_background;
mod canvas_content;
mod canvas_preview;
mod clipboard_jobs;
mod clone_preview;
mod closing;
mod color_picker;
#[cfg(test)]
mod color_picker_tests;
mod command_availability;
#[cfg(test)]
mod command_tests;
mod confirmations;
mod controls;
#[cfg(test)]
mod creation_history_tests;
mod crop;
mod crop_picker;
mod cursor_art;
mod curves;
mod dimensions;
mod dropdown;
#[cfg(test)]
mod duplicate_tests;
mod external_open;
mod file_dialogs;
mod file_drop;
mod file_jobs;
mod files;
mod filter_controls;
mod filter_preview;
mod floating;
mod floating_panel;
mod form_fields_view;
mod form_shortcuts;
mod forms;
mod gestures;
mod gradient;
mod gradient_controls;
mod gradient_map_controls;
mod gradient_overlay;
mod guide_grid;
mod history;
mod hue_controls;
mod hue_sampling;
mod icons;
mod image_size;
mod inversion;
mod jobs;
mod jpeg_export;
mod jpeg_preferences;
mod keymap;
mod layer_clipboard;
mod layer_cursor;
mod layer_drag;
mod layer_editing;
mod layer_effects;
mod layer_list;
mod layer_mask;
mod layer_opacity;
mod layer_preview;
mod layer_row;
mod layers;
mod layout_guides;
mod levels_controls;
mod levels_handles;
mod levels_sampling;
pub(crate) mod menus;
#[cfg(test)]
mod move_tests;
mod navigation_header;
mod new_canvas;
mod nudge;
mod numeric_fields;
#[cfg(test)]
mod object_selection_tests;
mod palette;
mod palette_controls;
mod panel_activation;
mod panel_layout;
mod parameter_controls;
mod pixel_clipboard;
mod pixel_editing;
mod pixel_grid;
#[cfg(test)]
mod pixel_operations_tests;
mod project_open;
mod project_sheets;
mod project_tab;
mod project_tools;
mod psd_conversion;
mod raw_develop;
mod rename;
mod sample_ring;
#[cfg(test)]
mod saved_state_tests;
mod scalar_controls;
mod selection_controls;
mod selection_draft;
#[cfg(test)]
mod selection_drag_tests;
mod selection_outline;
mod selection_paths;
mod selection_tools;
#[cfg(test)]
mod selection_tools_tests;
mod shape_controls;
mod shape_draft;
mod shortcut_editor;
mod shortcuts;
mod size_dialog;
#[cfg(test)]
mod size_dialog_tests;
mod size_picker;
mod snap_guides;
mod status_bar;
mod surfaces;
mod tab_strip;
#[cfg(test)]
mod tests;
mod text_editor;
mod thumbnails;
mod titlebar;
mod tool_cursor;
mod tool_defaults;
#[cfg(test)]
mod tool_header_tests;
mod tool_preferences;
#[cfg(test)]
mod transfer_tests;
mod transform_fields;
mod transform_header;
mod transform_overlay;
mod trim;
mod update_preferences;
mod updates;
mod wand_picker;
mod welcome;

use self::{
    canvas::Gesture,
    closing::{CloseIntent, CloseProgress},
    forms::Form,
    icons::Icon,
    project_tab::ProjectTab,
};
use compositor::{
    Result, adjustment::Kind, brush::Brush, document::Document, image_io, project, session::Session,
};
use quickgui::{
    Color, Element, Event, EventContext, Image, IntoElement, Key, Modifiers, View, ViewContext,
    button, div, text,
};
use std::{path::PathBuf, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tool {
    Idle,
    Move,
    Rectangle,
    Ellipse,
    Lasso,
    Polygon,
    Wand,
    Object,
    Crop,
    Brush,
    Erase,
    Clone,
    Heal,
    Blur,
    Smudge,
    Liquify,
    Gradient,
    Shape,
    Text,
    Eyedropper,
    Hand,
    Zoom,
}
impl Tool {
    const ALL: [(Self, &'static str); 21] = [
        (Self::Move, "V  Move"),
        (Self::Rectangle, "M  Select"),
        (Self::Ellipse, "   Ellipse"),
        (Self::Lasso, "L  Lasso"),
        (Self::Polygon, "   Polygon"),
        (Self::Wand, "W  Wand"),
        (Self::Crop, "C  Crop"),
        (Self::Brush, "B  Brush"),
        (Self::Erase, "E  Eraser"),
        (Self::Clone, "S  Clone"),
        (Self::Heal, "J  Heal"),
        (Self::Blur, "R  Blur"),
        (Self::Gradient, "G  Gradient"),
        (Self::Shape, "U  Shape"),
        (Self::Eyedropper, "I  Sample"),
        (Self::Hand, "H  Hand"),
        (Self::Zoom, "Z  Zoom"),
        (Self::Smudge, "   Smudge"),
        (Self::Liquify, "   Liquify"),
        (Self::Text, "T  Type"),
        (Self::Object, "O  Object Selection"),
    ];
    fn label(self) -> &'static str {
        if self == Self::Idle {
            return "Select a tool";
        }
        Self::ALL
            .iter()
            .find(|(t, _)| *t == self)
            .map_or("Move", |(_, s)| s.trim())
    }
}

#[derive(Clone, Copy)]
pub enum Action {
    New,
    Open,
    OpenClipboard,
    Import,
    Save,
    SaveAs,
    ExportPng,
    ExportTiff,
    ExportWebp,
    DevelopRaw,
    CameraRaw,
    RasterizeRaw,
    ExportPsd,
    OpenPsd,
    OpenRaw,
    ExportJpegFile,
    ExportJpeg,
    Undo,
    Redo,
    AddLayer,
    Duplicate,
    DeleteLayer,
    DeleteLayerBaked,
    DeleteLayerUnlinked,
    Group,
    Ungroup,
    MoveOutOfGroup,
    Merge,
    Raise,
    Lower,
    Rename,
    Transform,
    CanvasSize,
    ImageSize,
    FlipCanvasX,
    FlipCanvasY,
    CropSettings,
    Trim,
    PixelGrid,
    Rulers,
    Grid,
    Guides,
    LockGuides,
    ClearGuides,
    Snap,
    SnapGrid,
    SnapGuides,
    SnapLayers,
    SnapBounds,
    AddMask,
    HideMask,
    ToggleMask,
    LinkMask,
    DeleteMask,
    Clip,
    SelectSubject,
    LoadAlpha,
    LoadMask,
    SelectAll,
    Deselect,
    InvertSelection,
    FeatherSelection,
    Fill,
    FillBackground,
    Clear,
    FlipX,
    FlipY,
    InvertPixels,
    Color,
    Fit,
    Actual,
    ZoomIn,
    ZoomOut,
    Blend,
    Adjustment(Kind),
    AdjustPixels(Kind),
    EditAdjustment,
    CloseTab,
    Filter(compositor::filters::Filter),
    RemoveBackground,
    Copy,
    CopyMerged,
    Cut,
    Paste,
}

pub struct Editor {
    wayland_clipboard: Option<compositor::native_clipboard::wayland::WaylandClipboard>,
    tabs: Vec<ProjectTab>,
    current: usize,
    next_tab_number: usize,
    tools: project_tools::ProjectTools,
    tool_defaults: tool_defaults::Preferences,
    tab_scrolling: tab_strip::TabScrolling,
    launch_queue: crate::launch::LaunchQueue,
    space_pan: bool,
    pending_gradient: Option<gradient::PendingGradient>,
    pending_pixels: Option<floating::PendingPixels>,
    canvas_pointer: Option<compositor::geometry::Point>,
    sample_ring: Option<sample_ring::SampleRing>,
    dimension_link: Option<dimensions::DimensionLink>,
    transform_edit: Option<transform_header::TransformEdit>,
    image_sizing: image_size::ImageSizing,
    canvas_bounds: quickgui::LayoutBoundsHandle,
    layer_list: layer_list::LayerList,
    zoom_draft: Option<navigation_header::ZoomDraft>,
    color_picker_position: Option<[f32; 2]>,
    color_picker_shader: quickgui::CustomShader,
    panel_positions: floating_panel::PanelPositions,
    panel_activation: panel_activation::Activation,
    size_menus: size_picker::SizeMenus,
    keyboard_modifiers: Modifiers,
    pixel_clipboard: Option<compositor::clipboard::PixelClipboard>,
    layer_clipboard: Option<layer_clipboard::Copy>,
    gesture: Option<Gesture>,
    pending_layer_click: Option<uuid::Uuid>,
    window_drag: Option<quickgui::Point>,
    pixel_grid_shader: quickgui::CustomShader,
    guide_grid_shader: quickgui::CustomShader,
    gradient_overlay_shader: quickgui::CustomShader,
    status: String,
    errors: std::collections::VecDeque<alerts::Failure>,
    revision: u64,
    preview: Option<canvas_preview::CanvasPreview>,
    canvas_rendering: canvas_preview::CanvasRendering,
    clone_preview: clone_preview::ClonePreview,
    cursor_art: cursor_art::Atlas,
    thumbnails: thumbnails::ThumbnailCache,
    panel_layout: panel_layout::PanelLayout,
    selection_outline: Option<selection_outline::SelectionOutline>,
    selection_scroll: Option<autoscroll::SelectionScroll>,
    modal: Option<Form>,
    develop: Option<raw_develop::Develop>,
    raw_queue: std::collections::VecDeque<(raw_develop::Source, raw_develop::Target)>,
    retained_panel: Option<Form>,
    menus: menus::Menus,
    about_window: Option<quickgui::WindowHandle>,
    adjustment_channel_menu: quickgui::PopoverMenu,
    blend_picker: appearance::BlendPicker,
    opacity_draft: Option<layer_opacity::OpacityDraft>,
    slider_drag: Option<scalar_controls::SliderDrag>,
    adjustment_edit: Option<adjustments::AdjustmentEdit>,
    filter_edit: Option<filter_preview::FilterEdit>,
    camera_raw: camera_raw::Edit,
    camera_raw_last: compositor::camera_raw::Settings,
    background_mode: background_controls::Mode,
    jpeg_export: Option<jpeg_export::JpegExport>,
    rename: Option<rename::LayerRename>,
    keymap: keymap::Keymap,
    text_defaults: compositor::text::Text,
    text_renderer: Option<compositor::text::TextRenderer>,
    text_preview: Option<text_editor::Preview>,
    pending: bool,
    updates: updates::Updates,
    job: Option<jobs::Job>,
    file_job: Option<file_jobs::FileJob>,
    psd_conversion: Option<psd_conversion::Conversion>,
    layout_drag: Option<layout_guides::Drag>,
    clipboard_job: Option<clipboard_jobs::Job>,
    close_intent: Option<CloseProgress>,
}

impl Editor {
    pub fn report_startup_error(&mut self, message: String) {
        eprintln!("{message}");
        self.status = message;
    }

    #[cfg(test)]
    fn with_test_document() -> Self {
        let mut editor = Self::new(Vec::new()).unwrap();
        editor.tabs[0].set_document(Document::new(1280, 800).unwrap(), None);
        editor
    }

    pub fn new(paths: Vec<PathBuf>) -> Result<Self> {
        let mut tabs: Vec<ProjectTab> = Vec::new();
        let launch_queue = crate::launch::LaunchQueue::default();
        for path in paths {
            if compositor::raw::is_raw(&path) || compositor::psd::is_psd(&path)? {
                launch_queue.push(vec![path])?;
                continue;
            }
            if path.is_dir() {
                let path = path.canonicalize()?;
                if !tabs.iter().any(|tab: &ProjectTab| {
                    tab.session()
                        .is_some_and(|s| s.path.as_ref() == Some(&path))
                }) {
                    tabs.push(Session::new(project::load(&path)?, Some(path)).into());
                }
            } else {
                let layer = image_io::import(&path)?;
                let mut doc = Document::new(
                    layer.transform.size[0] as u32,
                    layer.transform.size[1] as u32,
                )?;
                doc.layers.clear();
                doc.add(layer)?;
                tabs.push(Session::new(doc, None).into());
            }
        }
        if tabs.is_empty() {
            tabs.push(ProjectTab::empty("Untitled".into()));
        }
        Ok(Self {
            wayland_clipboard: None,
            tabs,
            current: 0,
            tools: project_tools::ProjectTools::default(),
            tool_defaults: tool_defaults::Preferences::default(),
            next_tab_number: 2,
            tab_scrolling: tab_strip::TabScrolling::default(),
            launch_queue,
            space_pan: false,
            pending_gradient: None,
            pending_pixels: None,
            canvas_pointer: None,
            sample_ring: None,
            dimension_link: None,
            transform_edit: None,
            image_sizing: image_size::ImageSizing::default(),
            canvas_bounds: quickgui::LayoutBoundsHandle::new(),
            layer_list: layer_list::LayerList::default(),
            zoom_draft: None,
            color_picker_position: None,
            color_picker_shader: color_picker::spectrum::shader()?,
            panel_positions: floating_panel::PanelPositions::default(),
            panel_activation: panel_activation::Activation::default(),
            size_menus: size_picker::SizeMenus::new(),
            keyboard_modifiers: Modifiers::empty(),
            pixel_clipboard: None,
            layer_clipboard: None,
            gesture: None,
            pending_layer_click: None,
            pixel_grid_shader: pixel_grid::shader()?,
            guide_grid_shader: guide_grid::shader()?,
            gradient_overlay_shader: gradient_overlay::shader()?,
            status: String::new(),
            errors: std::collections::VecDeque::new(),
            revision: 0,
            preview: None,
            canvas_rendering: canvas_preview::CanvasRendering::default(),
            window_drag: None,
            clone_preview: clone_preview::ClonePreview::default(),
            cursor_art: cursor_art::Atlas::default(),
            thumbnails: thumbnails::ThumbnailCache::default(),
            panel_layout: panel_layout::PanelLayout::default(),
            selection_outline: None,
            selection_scroll: None,
            modal: None,
            develop: None,
            raw_queue: std::collections::VecDeque::new(),
            retained_panel: None,
            menus: menus::Menus::new()?,
            about_window: None,
            adjustment_channel_menu: quickgui::PopoverMenu::new([])
                .map_err(|e| compositor::invalid(e.to_string()))?,
            blend_picker: appearance::BlendPicker::new()?,
            opacity_draft: None,
            slider_drag: None,
            adjustment_edit: None,
            filter_edit: None,
            camera_raw: Default::default(),
            camera_raw_last: Default::default(),
            background_mode: background_controls::Mode::Basic,
            jpeg_export: None,
            rename: None,
            keymap: keymap::Keymap::default(),
            text_defaults: compositor::text::Text::default(),
            text_renderer: None,
            text_preview: None,
            pending: false,
            updates: updates::Updates::default(),
            job: None,
            file_job: None,
            psd_conversion: None,
            layout_drag: None,
            clipboard_job: None,
            close_intent: None,
        })
    }
    fn session(&self) -> &Session {
        self.tabs[self.current]
            .session()
            .expect("Document controls require an open document")
    }
    fn session_mut(&mut self) -> &mut Session {
        self.tabs[self.current]
            .session_mut()
            .expect("Document controls require an open document")
    }
    fn finish_pending_edits(&mut self) -> Result<()> {
        if !self.has_document() {
            return Ok(());
        }
        self.finish_opacity_input()?;
        self.finish_visibility_swipe()?;
        self.finish_zoom_input();
        self.finish_header_transform(true)?;
        self.finish_rename()?;
        self.commit_gradient()?;
        self.commit_pixels()
    }
    fn changed(&mut self, cx: &mut EventContext) {
        self.save_tool_defaults();
        self.revision = self.revision.wrapping_add(1);
        cx.invalidate();
    }
    fn result(&mut self, result: Result<()>, cx: &mut EventContext) {
        if let Err(error) = result {
            self.status = error.to_string();
        }
        self.changed(cx);
    }
    fn select_tool(&mut self, tool: Tool, cx: &mut EventContext) {
        if self.pending
            || self
                .adjustment_edit
                .as_ref()
                .is_some_and(|edit| edit.settings.kind == Kind::Levels)
        {
            return;
        }
        if self.tools.tool != tool
            && let Err(error) = self.finish_pending_edits()
        {
            self.result(Err(error), cx);
            return;
        }
        self.tools.crop_picker.close(cx);
        self.tools.mask_paint_picker.close(cx);
        self.tools.wand_picker.close(cx);
        self.tools.gradient_picker.close(cx);
        self.tools.transform_sampling.close(cx);
        if self.tools.tool != tool {
            if self.floating_panel_kind().is_none() && self.has_document() {
                self.session_mut().cancel();
            }
            self.gesture = None;
            self.tools.pending_crop = None;
            self.tools.polygon = None;
        }
        self.tools
            .tool_preferences
            .select(self.tools.tool, tool, &mut self.tools.brush);
        self.tools.tool = tool;
        if tool == Tool::Crop && self.tools.pending_crop.is_none() && self.has_document() {
            self.tools.crop_ratio = None;
            self.tools.crop_picker.select_id("free");
            let doc = &self.session().document;
            self.tools.pending_crop = Some(crop::CropPreview {
                frame: crop::initial_frame(doc),
                guides: [None; 2],
            });
        }
        self.sample_ring = None;
        self.status = self.tool_hint().into();
        self.changed(cx);
    }
    fn text_field(value: impl Into<Arc<str>>) -> Element {
        quickgui::text_input(value).focus(controls::focus_outline)
    }

    fn control(label: impl Into<Arc<str>>) -> Element {
        button()
            .flex_row()
            .items_center()
            .text_size(13.)
            .line_height(16.)
            .h(24.)
            .px(11.)
            .rounded(12.)
            .border(1., Color::TRANSPARENT)
            .bg(Color::rgb8(49, 49, 49))
            .hover(|s| s.bg(Color::rgb8(68, 68, 68)))
            .focus(controls::focus_outline)
            .child(text(label))
    }
    fn tool_header_control(label: impl Into<Arc<str>>) -> Element {
        Self::control(label).text_size(12.).line_height(15.)
    }
    fn action_button(
        &self,
        cx: &mut ViewContext<'_, Self>,
        id: u64,
        label: impl Into<Arc<str>>,
        action: Action,
    ) -> Element {
        Self::control(label).on_click(cx.listener(id, move |this, cx| this.action(action, cx)))
    }
}

impl View for Editor {
    fn event(&mut self, event: &Event, cx: &mut EventContext) {
        self.track_tab_drag(event, cx);
        match event {
            Event::KeyDown {
                key: Key::Character(key),
                modifiers,
                repeat: false,
                ..
            } if *modifiers == Modifiers::CONTROL
                && key.eq_ignore_ascii_case("q")
                && !matches!(&self.modal, Some(Form::Shortcuts(draft)) if draft.is_recording()) =>
            {
                cx.prevent_default();
                self.request_close(CloseIntent::Window, cx);
            }
            Event::FirstPresented if !self.has_document() && !self.pending => {
                let result = self.suggest_new_canvas(cx);
                self.operation_result(alerts::Operation::Clipboard, result, cx);
            }
            Event::ModifiersChanged(modifiers) => {
                self.selection_scroll_modifiers(*modifiers);
                if self.keyboard_modifiers != *modifiers {
                    self.keyboard_modifiers = *modifiers;
                    cx.invalidate();
                }
            }
            Event::KeyUp { key, .. } if self.keymap.pan_released(key) => {
                self.space_pan = false;
                cx.invalidate();
            }
            Event::Focused(focused) => {
                self.panel_activation
                    .focus(panel_activation::Window::Editor, *focused);
                if !focused {
                    if self.menus.is_open() {
                        self.menus.close();
                        cx.focus(quickgui::FocusHandle::new("workspace"));
                    }
                    self.keyboard_modifiers = Modifiers::empty();
                    self.space_pan = false;
                    self.sample_ring = None;
                    self.selection_scroll = None;
                }
                cx.invalidate();
            }
            Event::CloseRequested => {
                cx.prevent_close();
                self.request_close(CloseIntent::Window, cx);
            }
            Event::FilesDropped(files) if self.can_switch_projects() => {
                // Native drops also reach this event after typed targets. A typed
                // target has already set pending; otherwise use the current tab.
                let result = self.prepare_file_drop(
                    files,
                    Some(self.tabs[self.current].id),
                    cx.pointer_position().unwrap_or(quickgui::Point::ZERO),
                );
                self.operation_result(alerts::Operation::Import, result, cx);
            }
            _ => {}
        }
    }
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        self.sync_native_cursor(cx);
        cx.on_any_child_window_closed(|this, closed, cx| {
            if this.about_window == Some(closed) {
                this.about_window = None;
                this.panel_activation
                    .focus(panel_activation::Window::About, false);
                if this.modal.is_none() {
                    cx.focus(quickgui::FocusHandle::new("workspace"));
                }
                cx.invalidate();
            }
        });
        self.sync_picker_input(cx);
        self.sync_zoom_input(cx);
        self.sync_opacity_input(cx);
        self.sync_transform_sampling();
        self.sync_gradient_picker();
        self.tools
            .mask_paint_picker
            .select_id(if self.tools.mask_paint_white {
                "white"
            } else {
                "black"
            });
        self.start_external_open();
        self.start_job(cx);
        self.start_file_job(cx);
        self.start_clipboard_job(cx);
        self.start_update_job(cx);
        self.sync_raw_input(cx);
        self.start_raw_work(cx);
        let root = if self.develop.is_some() {
            self.raw_workspace(cx)
        } else if !self.has_document() {
            self.welcome_view(cx)
        } else {
            let workspace = self.workspace_view(cx);
            self.form_overlays(cx, workspace)
        };
        let root = root
            .children(self.psd_conversion_view(cx))
            .children(self.error_view(cx))
            .relative()
            .children(self.window_resize_edges(cx))
            .font_family("Inter Variable");
        root.on_action(cx.action_listener(
            "workspace",
            |this, action: &panel_activation::AboutFocus, cx| {
                if this.about_window.is_some() {
                    this.panel_activation
                        .focus(panel_activation::Window::About, action.0);
                    cx.invalidate();
                }
            },
        ))
        .on_action(
            cx.action_listener("workspace", |this, _: &closing::Quit, cx| {
                this.request_close(CloseIntent::Window, cx);
                if this.close_intent.is_some()
                    && let Some(window) = cx.window_handle()
                {
                    cx.focus_window(window);
                }
            }),
        )
    }
}

impl Editor {
    // Keep overlay construction out of the workspace render's stack frame.
    fn form_overlays(&mut self, cx: &mut ViewContext<'_, Self>, root: Element) -> Element {
        let mut root = root.child(self.menu_popup(cx));
        root = root.children(self.retained_panel_view(cx));
        if let Some(modal) = self.modal.clone() {
            let mut form = self.form_view(cx, modal);
            if self.panel_applying() {
                form.disable_subtree();
            }
            root = root.child(form);
        }
        root
    }
    // Keep workspace and dialog construction in separate frames. QuickGUI Elements
    // are large values, and unoptimized builder temporaries otherwise overflow the
    // default 2 MiB test-thread stack when a panel contains several sliders.
    fn workspace_content(&mut self, cx: &mut ViewContext<'_, Self>) -> Element {
        let size = cx.size();
        let canvas = self.canvas(
            cx,
            (size.width - 56. - self.panel_layout.width).max(1.),
            (size.height - 146.).max(1.),
        );
        self.workspace_layout(cx, canvas)
    }

    fn workspace_layout(&mut self, cx: &mut ViewContext<'_, Self>, mut canvas: Element) -> Element {
        let controls_blocked = self.pending || self.picking_color();
        let mut menu_bar = self.menu_bar(cx);
        let mut toolbar = self.toolbar(cx);
        let mut tools = self.tool_rail(cx);
        let mut panel = self.layers_panel(cx);
        let mut tool_options = self
            .tool_options(cx)
            .border_bottom(1., Color::rgb8(62, 62, 62));
        if controls_blocked {
            for control in [&mut menu_bar, &mut toolbar, &mut tools, &mut panel] {
                control.disable_subtree();
            }
        }
        if self.pending {
            canvas.disable_subtree();
        }
        if self.pending || self.picking_color() {
            tool_options.disable_subtree();
        }
        if !self.has_document()
            && matches!(
                self.tools.tool,
                Tool::Rectangle
                    | Tool::Ellipse
                    | Tool::Lasso
                    | Tool::Polygon
                    | Tool::Wand
                    | Tool::Object
                    | Tool::Crop
                    | Tool::Shape
                    | Tool::Gradient
            )
        {
            tool_options.disable_subtree();
            tool_options = tool_options.opacity(0.4);
        }
        div()
            .flex_col()
            .flex_1()
            .min_h(0.)
            .child(self.window_titlebar(cx, menu_bar))
            .child(toolbar)
            .child(tool_options)
            .child(
                div()
                    .flex_1()
                    .min_h(0.)
                    .flex_row()
                    .child(tools)
                    .child(canvas)
                    .child(panel),
            )
    }

    fn workspace_view(&mut self, cx: &mut ViewContext<'_, Self>) -> Element {
        self.start_camera_balance(cx);
        self.start_filter_preview(cx);
        self.start_adjustment_preview(cx);
        self.start_adjustment_histogram(cx);
        self.start_jpeg_preview(cx);
        self.start_thumbnails(cx);
        let content = self.workspace_content(cx);
        let canvas_focused = cx.is_focused(cx.focus_handle("workspace"));
        let key = cx.key_down_listener("workspace", move |this, event, cx| {
            if this.keymap.is_pan(&event.key, event.modifiers)
                && canvas_focused
                && !this.pending
                && this.rename.is_none()
                && (this.modal.is_none()
                    || this.floating_panel_kind().is_some()
                    || this.picking_color()
                    || this.adjustment_sampling())
                && (this.gesture.is_none() || matches!(this.gesture, Some(Gesture::Pan)))
            {
                this.space_pan = true;
                cx.prevent_default();
                cx.invalidate();
                return;
            }
            if this.picking_color() {
                match event.key {
                    Key::Escape | Key::Enter => {
                        let result = this.finish_color(event.key == Key::Enter);
                        if event.key == Key::Enter {
                            this.operation_result(alerts::Operation::Paint, result, cx);
                        } else {
                            this.result(result, cx);
                        }
                    }
                    _ => cx.propagate(),
                }
                return;
            }
            if this.floating_panel_kind().is_some() && !this.pending && this.rename.is_none() {
                if canvas_focused
                    && matches!(event.key, Key::Character(_))
                    && (event.modifiers - Modifiers::SHIFT).is_empty()
                    && !matches!(
                        this.floating_panel_kind(),
                        Some(floating_panel::PanelKind::Levels)
                    )
                {
                    this.key(&event.key, event.modifiers, cx);
                } else {
                    this.form_key(&event.key, event.modifiers, cx);
                }
                return;
            }
            if this.modal.is_some() || this.pending || this.rename.is_some() {
                cx.propagate();
                return;
            }
            this.key(&event.key, event.modifiers, cx);
        });
        div()
            .id("workspace")
            .relative()
            .focusable()
            .on_key_down(key)
            .on_drop(cx.drop_listener(
                "workspace",
                |this, files: &quickgui::DroppedFiles, event, cx| {
                    this.drop_files(files, Some(this.session().id), event, cx);
                },
            ))
            .size_full()
            .flex_col()
            .bg(Color::rgb8(30, 30, 30))
            .text_color(Color::rgb8(224, 224, 224))
            .child(content)
            .child(self.status_bar())
    }
}
