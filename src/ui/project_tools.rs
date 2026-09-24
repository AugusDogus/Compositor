//! Tool controls belong to a project, following Swift EditorSession.
use super::*;

pub(super) struct ProjectTools {
    pub(super) tool: Tool,
    pub(super) brush: Brush,
    pub(super) brush_smoothing: f64,
    pub(super) tool_preferences: tool_preferences::ToolPreferences,
    pub(super) opacity_digits: shortcuts::OpacityDigits,
    pub(super) background: [u8; 4],
    pub(super) gradient: compositor::gradient::Gradient,
    pub(super) gradient_picker: dropdown::Dropdown<compositor::gradient::Style>,
    pub(super) shows_sample_ring: bool,
    pub(super) transform_ratio: bool,
    pub(super) transform_sampling: dropdown::Dropdown<compositor::geometry::Sampling>,
    pub(super) transform_auto_select: bool,
    pub(super) show_transform_controls: bool,
    pub(super) mask_target: bool,
    pub(super) mask_paint_white: bool,
    pub(super) mask_paint_picker: dropdown::Dropdown<bool>,
    pub(super) clone_source: Option<[f64; 2]>,
    pub(super) clone_offset: Option<[f64; 2]>,
    pub(super) clone_aligned: bool,
    pub(super) clone_sample_all: bool,
    pub(super) healing: compositor::filters::Healing,
    pub(super) selection_antialiased: bool,
    pub(super) selection_mode: compositor::selection::SelectionMode,
    pub(super) selection_expand_amount: u16,
    pub(super) selection_contract_amount: u16,
    pub(super) selection_feather_amount: u16,
    pub(super) wand_tolerance: u8,
    pub(super) wand_radius: usize,
    pub(super) wand_picker: dropdown::Dropdown<usize>,
    pub(super) wand_contiguous: bool,
    pub(super) wand_sample_all: bool,
    pub(super) object_sample_all: bool,
    pub(super) object_edge_offset: i8,
    pub(super) last_brush: Option<(uuid::Uuid, bool, [f64; 2])>,
    pub(super) polygon: Option<selection_tools::PolygonDraft>,
    pub(super) layout: compositor::guides::Settings,
    pub(super) shape_kind: compositor::document::ShapeKind,
    pub(super) shape_line_width: f64,
    pub(super) shape_radius: f64,
    pub(super) crop_ratio: Option<f64>,
    pub(super) crop_picker: dropdown::Dropdown<crop_picker::Ratio>,
    pub(super) pending_crop: Option<crop::CropPreview>,
    pub(super) pixel_grid: bool,
}

impl Default for ProjectTools {
    fn default() -> Self {
        Self {
            tool: Tool::Move,
            brush: Brush::default(),
            brush_smoothing: 0.,
            tool_preferences: tool_preferences::ToolPreferences::default(),
            opacity_digits: shortcuts::OpacityDigits::default(),
            background: [255; 4],
            gradient: compositor::gradient::Gradient::default(),
            gradient_picker: gradient_controls::new(),
            shows_sample_ring: true,
            transform_ratio: true,
            transform_sampling: transform_header::sampling_picker(),
            transform_auto_select: false,
            show_transform_controls: true,
            mask_target: false,
            mask_paint_white: false,
            mask_paint_picker: brush_controls::mask_picker(),
            clone_source: None,
            clone_offset: None,
            clone_aligned: true,
            clone_sample_all: false,
            healing: compositor::filters::Healing::ContentAware,
            selection_antialiased: true,
            selection_mode: compositor::selection::SelectionMode::Replace,
            selection_expand_amount: 1,
            selection_contract_amount: 1,
            selection_feather_amount: 2,
            wand_tolerance: 32,
            wand_radius: 0,
            wand_picker: wand_picker::new(),
            wand_contiguous: true,
            wand_sample_all: false,
            object_sample_all: true,
            object_edge_offset: 0,
            last_brush: None,
            polygon: None,
            layout: compositor::guides::Settings::default(),
            shape_kind: compositor::document::ShapeKind::Rectangle,
            shape_line_width: 2.,
            shape_radius: 0.,
            crop_ratio: None,
            crop_picker: crop_picker::new(),
            pending_crop: None,
            pixel_grid: true,
        }
    }
}

impl ProjectTools {
    fn dismiss_menus(&mut self) {
        self.gradient_picker.dismiss();
        self.transform_sampling.dismiss();
        self.mask_paint_picker.dismiss();
        self.wand_picker.dismiss();
        self.crop_picker.dismiss();
    }
}

impl Editor {
    /// Callers resolve blocking edits before switching. Settings and persistent
    /// crop/lasso drafts move with the project; pointer state belongs to the view.
    pub(super) fn activate_tab(&mut self, index: usize) {
        if index == self.current {
            return;
        }
        let toggles = tool_defaults::Toggles::capture(&self.tools);
        self.tools.dismiss_menus();
        if matches!(self.modal, Some(Form::Blend)) {
            self.modal = None;
            self.blend_picker.close();
        }
        std::mem::swap(&mut self.tools, &mut self.tabs[self.current].parked_tools);
        self.current = index;
        std::mem::swap(&mut self.tools, &mut self.tabs[index].parked_tools);
        toggles.apply(&mut self.tools);
        self.canvas_pointer = None;
        self.sample_ring = None;
        self.pending_layer_click = None;
        self.selection_scroll = None;
        self.preview = None;
        self.menus.close();
        self.status = self.tool_hint().into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compositor::geometry::Transform;
    use quickgui::{Application, WindowOptions};

    #[test]
    fn closing_background_current_and_last_tabs_keeps_the_surviving_projects_controls() {
        let mut editor = Editor::with_test_document();
        editor.tools.brush.diameter = 121.;
        editor.add_empty_tab();
        editor.tabs[1].set_document(Document::new(10, 10).unwrap(), None);
        editor.tools.brush.diameter = 242.;
        let middle = editor.tabs[1].id;
        editor.add_empty_tab();
        editor.tabs[2].set_document(Document::new(10, 10).unwrap(), None);
        editor.tools.brush.diameter = 363.;
        let last = editor.tabs[2].id;
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Close project controls").size(1500., 900.),
                editor,
            )
            .unwrap();
        for (target, expected, count) in [(middle, 363., 2), (last, 121., 1)] {
            cx.update(view, |e, cx| e.request_close(CloseIntent::Tab(target), cx))
                .unwrap();
            cx.click(view.window_handle(), "discard-close").unwrap();
            cx.read(view, |e| {
                assert_eq!(e.tabs.len(), count);
                assert_eq!(e.tools.brush.diameter, expected);
            })
            .unwrap();
        }
        cx.update(view, |e, cx| e.action(Action::CloseTab, cx))
            .unwrap();
        cx.click(view.window_handle(), "discard-close").unwrap();
        cx.read(view, |e| {
            assert_eq!(e.tabs.len(), 1);
            assert_eq!(e.current, 0);
            assert!(!e.has_document());
            assert_eq!(e.tools.brush.diameter, 40.);
        })
        .unwrap();
    }

    #[test]
    fn new_and_switched_projects_restore_their_own_controls_and_crop() {
        let mut editor = Editor::with_test_document();
        compositor::edits::add_mask(&mut editor.session_mut().document, false).unwrap();
        let original = editor.tabs[0].id;
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Project controls").size(1500., 900.),
                editor,
            )
            .unwrap();
        let mut frame = Transform::new(100, 50);
        frame.origin = [20., 30.];
        cx.update(view, |e, cx| {
            e.select_tool(Tool::Crop, cx);
            e.tools.pending_crop.as_mut().unwrap().frame = frame;
            e.tools.crop_ratio = Some(2.);
            e.tools.crop_picker.clear_selection();
            e.tools.mask_target = true;
            e.tools.brush.diameter = 123.;
            e.tools.brush.color = [20, 40, 60, 255];
            e.tools.clone_source = Some([7., 9.]);
            e.tools.pixel_grid = false;
            e.action(Action::New, cx);
            assert_eq!(e.tools.tool, Tool::Move);
            assert!(e.tools.pending_crop.is_none());
            assert!(!e.tools.mask_target);
            assert_eq!(e.tools.brush.diameter, 40.);
            assert_eq!(e.tools.brush.color, Brush::default().color);
            assert_eq!(e.tools.clone_source, None);
            assert!(!e.tools.pixel_grid);
            e.tabs[e.current].set_document(Document::new(30, 30).unwrap(), None);
            e.select_tool(Tool::Shape, cx);
            e.tools.shape_radius = 27.;
        })
        .unwrap();
        let other = cx.read(view, |e| e.tabs[1].id).unwrap();
        cx.click(view.window_handle(), format!("project-tab-{original}"))
            .unwrap();
        cx.read(view, |e| {
            assert_eq!(e.tools.tool, Tool::Crop);
            assert_eq!(e.tools.pending_crop.as_ref().unwrap().frame, frame);
            assert_eq!(e.current_crop_ratio(), Some(2.));
            assert!(e.tools.mask_target);
            assert_eq!(e.tools.brush.diameter, 123.);
            assert_eq!(e.tools.brush.color, [20, 40, 60, 255]);
            assert_eq!(e.tools.clone_source, Some([7., 9.]));
            assert!(!e.tools.pixel_grid);
            assert!(!e.can_edit_layers());
        })
        .unwrap();
        cx.click(view.window_handle(), format!("project-tab-{other}"))
            .unwrap();
        cx.read(view, |e| {
            assert_eq!(e.tools.tool, Tool::Shape);
            assert_eq!(e.tools.shape_radius, 27.);
            assert!(e.tools.pending_crop.is_none());
        })
        .unwrap();
        cx.update(view, |e, _| {
            let drag = layer_drag::LayerDrag {
                session: other,
                layer: e.session().document.active.unwrap(),
                operation: layer_drag::Transfer::Move,
            };
            assert!(e.copy_drag_to_tab(&drag, 0).is_err());
            assert_eq!(e.tabs[e.current].id, other);
            e.show_opened_projects(vec![project_open::OpenedProject::Existing(original)])
                .unwrap();
            assert_eq!(e.tools.pending_crop.as_ref().unwrap().frame, frame);
            e.show_opened_projects(vec![project_open::OpenedProject::Existing(other)])
                .unwrap();
            assert_eq!(e.tools.tool, Tool::Shape);
        })
        .unwrap();
        cx.update(view, |e, cx| {
            e.request_close(CloseIntent::Tab(original), cx)
        })
        .unwrap();
        cx.read(view, |e| assert_eq!(e.tools.tool, Tool::Crop))
            .unwrap();
        cx.update(view, |e, cx| {
            e.cancel_close();
            e.modal = None;
            e.changed(cx);
        })
        .unwrap();
        cx.read(view, |e| {
            assert_eq!(e.tabs[e.current].id, other);
            assert_eq!(e.tools.tool, Tool::Shape);
            assert_eq!(e.tools.shape_radius, 27.);
        })
        .unwrap();
    }
}
