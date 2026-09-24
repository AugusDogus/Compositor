use super::*;
use compositor::invalid;
use quickgui::Dialog;

#[derive(Clone)]
pub(super) enum Form {
    Effects(Box<super::layer_effects::EffectsEditor>),
    Updates,
    Trim(compositor::trim::Options),
    Shortcuts(Box<super::shortcut_editor::Draft>),
    Text(Box<super::text_editor::Draft>),
    Blend,
    MaskColor(palette::MaskSwatch),
    Color(Box<super::color_picker::Picker>),
    Edit {
        title: &'static str,
        action: Action,
        fields: Vec<(&'static str, String)>,
        error: String,
    },
    Close,
    DeleteLayers,
}
impl Form {
    pub(super) fn confirm_close() -> Self {
        Self::Close
    }
}

impl Editor {
    // Keep heading builder temporaries off the frame that builds the sheet's controls.
    fn sheet_heading(title: &'static str, title2: bool) -> Element {
        let heading = if title2 {
            text(title).text_size(17.).line_height(22.).font_bold()
        } else {
            text(title).text_size(13.).font_semibold()
        };
        div().h(22.).flex_shrink_0().child(heading)
    }

    pub(super) fn open_form(&mut self, action: Action) {
        if matches!(action, Action::Trim) {
            self.modal = Some(Form::Trim(Default::default()));
            return;
        }
        if matches!(action, Action::Color) {
            if matches!(self.modal, Some(Form::Text(_))) {
                self.open_foreground_text_picker();
                return;
            }
            self.modal = Some(if self.tools.mask_target {
                Form::MaskColor(palette::MaskSwatch::Foreground)
            } else {
                Form::Color(Box::new(super::color_picker::Picker::new(
                    self.tools.brush.color,
                    self.tools.background,
                )))
            });
            return;
        }
        if matches!(action, Action::CanvasSize | Action::ImageSize) {
            self.retain_tool_panel();
        }
        if matches!(action, Action::New) {
            let fields = vec![("Width", "1920".into()), ("Height", "1080".into())];
            self.dimension_link = super::dimensions::DimensionLink::for_form(action, &fields);
            self.modal = Some(Form::Edit {
                title: "New canvas",
                action,
                fields,
                error: String::new(),
            });
            return;
        }
        if matches!(action, Action::ImageSize) {
            self.image_sizing = super::image_size::ImageSizing::default();
        }
        if matches!(action, Action::CanvasSize) {
            self.size_menus.custom_fill = [255; 3];
        }
        let doc = if matches!(action, Action::CanvasSize | Action::ImageSize) {
            self.session().committed_document()
        } else {
            &self.session().document
        };
        let (title, fields): (&'static str, Vec<(&'static str, String)>) = match action {
            Action::FeatherSelection => (
                "Feather Selection",
                vec![(
                    "Radius (px)",
                    self.tools.selection_feather_amount.to_string(),
                )],
            ),
            Action::Filter(filter) => Self::filter_fields(filter),
            Action::CameraRaw => ("Camera Raw Filter", self.camera_raw.fields()),
            Action::RemoveBackground => (
                "Remove Background",
                vec![
                    ("Refine edges (0 to 40)", "12".into()),
                    ("Matte contrast (0 to 100)", "25".into()),
                    ("Shift edge (-10 to 10)", "0".into()),
                ],
            ),
            Action::Transform => {
                let t = if let Some(edit) = &self.pending_pixels {
                    edit.placement.bounds()
                } else if doc.selection.is_some() && !self.tools.mask_target {
                    compositor::floating::FloatingPixels::lift(doc)
                        .map(|p| p.placement)
                        .unwrap_or(compositor::geometry::Transform::new(doc.width, doc.height))
                } else {
                    compositor::transform::selection_bounds(doc, self.tools.mask_target)
                        .unwrap_or(compositor::geometry::Transform::new(doc.width, doc.height))
                };
                (
                    "Transform layer",
                    vec![
                        ("X", t.origin[0].to_string()),
                        ("Y", t.origin[1].to_string()),
                        ("Width", t.size[0].to_string()),
                        ("Height", t.size[1].to_string()),
                        ("Angle", t.rotation.to_string()),
                        (
                            "Scale (%)",
                            (t.size[0] / self.transform_pixel_size(t.size)[0] * 100.).to_string(),
                        ),
                        (
                            "Sampling (high, smooth, nearest)",
                            match t.sampling {
                                compositor::geometry::Sampling::High => "high",
                                compositor::geometry::Sampling::Smooth => "smooth",
                                compositor::geometry::Sampling::Nearest => "nearest",
                            }
                            .into(),
                        ),
                    ],
                )
            }
            Action::CanvasSize => (
                "Canvas Size",
                vec![
                    ("Width", doc.width.to_string()),
                    ("Height", doc.height.to_string()),
                    ("Horizontal anchor (left, center, right)", "center".into()),
                    ("Vertical anchor (top, center, bottom)", "center".into()),
                    ("Units (px, percent, inches, cm)", "px".into()),
                    ("Relative dimensions (0 or 1)", "0".into()),
                    (
                        "Extension fill (transparent, foreground, background, black, white, #RRGGBB)",
                        "transparent".into(),
                    ),
                ],
            ),
            Action::ImageSize => (
                "Image Size",
                vec![
                    ("Width", doc.width.to_string()),
                    ("Height", doc.height.to_string()),
                    ("Resolution", doc.resolution.to_string()),
                    ("Sampling (high, smooth, nearest)", "high".into()),
                ],
            ),
            Action::CropSettings => (
                "Crop ratio",
                vec![(
                    "Ratio (free, original, or width:height)",
                    self.tools
                        .crop_ratio
                        .map_or_else(|| "free".into(), |r| format!("{r}:1")),
                )],
            ),
            _ => return,
        };
        self.dimension_link = super::dimensions::DimensionLink::for_form(action, &fields);
        if matches!(action, Action::Transform)
            && let Some(link) = &mut self.dimension_link
        {
            link.locked = self.tools.transform_ratio;
        }
        self.modal = Some(Form::Edit {
            title,
            action,
            fields,
            error: String::new(),
        });
    }

    pub(super) fn form_view(&mut self, cx: &mut ViewContext<'_, Self>, form: Form) -> Element {
        if let Form::Shortcuts(draft) = &form {
            return self.shortcuts_view(cx, draft);
        }
        if let Form::Text(draft) = &form {
            return self.text_editor_view(cx, draft);
        }
        if let Form::Color(picker) = &form {
            return self.color_picker_view(cx, picker);
        }
        if matches!(form, Form::Blend) {
            return self.blend_popover(cx);
        }
        if let Form::MaskColor(target) = form {
            return self.mask_color_popover(cx, target);
        }
        self.sync_size_menus(&form);
        self.dialog_view(cx, form)
    }

    fn dialog_view(&mut self, cx: &mut ViewContext<'_, Self>, form: Form) -> Element {
        let panel_kind = self.form_panel_kind(&form);
        let retained = panel_kind.is_some() && self.retained_panel.is_some();
        let project_sheet = panel_kind.is_none() && self.retained_panel.is_some();
        let dialog = Dialog::new(
            if project_sheet {
                "project-sheet"
            } else {
                "editor-dialog"
            },
            true,
        )
        .initial_focus(if project_sheet {
            60_000_u64
        } else {
            50_000_u64
        })
        .restore_focus_to("workspace")
        .dismiss_on_backdrop(false);
        let title = match &form {
            Form::Edit { title, .. } => *title,
            Form::Effects(_) => "Layer Effects",
            Form::Updates => "Updates",
            Form::Trim(_) => "Trim",
            Form::Text(_) => "Text",
            Form::Shortcuts(_) => "Keyboard Shortcuts",
            Form::Close | Form::DeleteLayers => return self.confirmation_view(cx, &form),
            Form::Color(_) => "Color picker",
            Form::Blend => "Blend Mode",
            Form::MaskColor(target) => target.title(),
        };
        let cancel = cx.listener(
            if retained {
                "retained-panel-cancel"
            } else {
                "form-cancel"
            },
            |this, cx| {
                this.cancel_form(cx);
            },
        );
        let size_sheet = matches!(
            form,
            Form::Edit {
                action: Action::CanvasSize | Action::ImageSize,
                ..
            }
        );
        let filter_sheet = matches!(
            form,
            Form::Edit {
                action: Action::CameraRaw | Action::Filter(_) | Action::RemoveBackground,
                ..
            }
        ) || (panel_kind.is_some()
            && self.adjustment_edit.as_ref().is_some_and(|e| {
                matches!(
                    e.settings.kind,
                    Kind::Exposure | Kind::Grain | Kind::Curves | Kind::GradientMap
                )
            }));
        let jpeg_sheet = matches!(
            form,
            Form::Edit {
                action: Action::ExportJpeg,
                ..
            }
        );
        let width = match &form {
            Form::Edit {
                action: Action::ExportJpeg,
                ..
            } => 608.,
            Form::Edit {
                action: Action::CanvasSize,
                ..
            } => 450.,
            Form::Edit {
                action: Action::ImageSize,
                ..
            } => 430.,
            _ if self
                .adjustment_edit
                .as_ref()
                .is_some_and(|e| e.settings.kind == Kind::HueSaturation) =>
            {
                460.
            }
            _ if filter_sheet => 380.,
            _ => 440.,
        };
        let available = cx.size();
        let spacing = if matches!(
            form,
            Form::Edit {
                action: Action::ImageSize,
                ..
            }
        ) {
            18.
        } else {
            16.
        };
        let mut contents = div()
            .w(width)
            .max_h((available.height - if panel_kind.is_some() { 176. } else { 80. }).max(200.))
            .overflow_y_scroll()
            .p(24.)
            .gap(spacing)
            .flex_col()
            .bg(Color::rgb8(45, 45, 45));
        if panel_kind.is_none() {
            contents = contents
                .border(1., Color::rgb8(90, 90, 90))
                .rounded(8.)
                .shadow(super::surfaces::panel_shadow())
                .child(dialog.title_with(Self::sheet_heading(title, size_sheet || jpeg_sheet)));
        }
        match form {
            Form::Text(draft) => return self.text_editor_view(cx, &draft),
            Form::Shortcuts(draft) => return self.shortcuts_view(cx, &draft),
            Form::Trim(options) => {
                contents = contents.child(self.trim_controls(cx, options));
            }
            Form::Effects(edit) => {
                contents = contents.child(self.effects_controls(cx, &edit));
            }
            Form::Updates => {
                contents = contents
                    .child(self.update_controls(cx))
                    .child(Self::control("Close").on_click(cancel));
            }
            Form::Close | Form::DeleteLayers => return self.confirmation_view(cx, &form),
            Form::MaskColor(target) => return self.mask_color_popover(cx, target),
            Form::Blend => {
                contents = contents.child(self.blend_controls(cx));
            }
            Form::Color(picker) => return self.color_picker_view(cx, &picker),
            Form::Edit {
                action,
                fields,
                error,
                ..
            } => {
                let filter_sheet = matches!(
                    action,
                    Action::CameraRaw | Action::Filter(_) | Action::RemoveBackground
                );
                if size_sheet {
                    contents = contents.child(self.size_dialog_view(cx, action, &fields));
                } else {
                    if matches!(action, Action::EditAdjustment)
                        && let Some(controls) = self.adjustment_controls(cx)
                    {
                        contents = contents.child(
                            controls.flex_shrink_0().opacity(if self.panel_applying() {
                                0.4
                            } else {
                                1.
                            }),
                        );
                    }
                    if matches!(action, Action::CameraRaw) {
                        contents = contents.child(self.camera_panel_header(cx));
                    }
                    if matches!(action, Action::RemoveBackground) {
                        contents = contents.child(
                            self.background_controls(cx, action)
                                .flex_shrink_0()
                                .opacity(if self.panel_applying() { 0.4 } else { 1. }),
                        );
                    }
                    if jpeg_sheet {
                        contents = contents.child(self.jpeg_controls(cx, &error).flex_shrink_0());
                    }
                    if self.dimension_link.is_some() && matches!(action, Action::Transform) {
                        contents =
                            contents.child(self.dimension_controls(cx, action).flex_shrink_0());
                    }
                    let fields_hidden = jpeg_sheet
                        || fields.is_empty()
                        || (matches!(action, Action::RemoveBackground)
                            && self.background_mode == super::background_controls::Mode::Basic)
                        || self.adjustment_edit.as_ref().is_some_and(|e| {
                            matches!(e.settings.kind, Kind::Curves | Kind::GradientMap)
                        });
                    if !fields_hidden {
                        contents = contents.child(
                            self.form_fields_view(cx, action, &fields)
                                .opacity(if self.panel_applying() { 0.4 } else { 1. }),
                        );
                    }
                    if matches!(action, Action::EditAdjustment) {
                        contents = contents.child(
                            self.adjustment_footer(cx)
                                .opacity(if self.panel_applying() { 0.4 } else { 1. }),
                        );
                    }
                    let description = match action {
                        Action::Filter(compositor::filters::Filter::ContentFill) => {
                            Some("Fill the selection using surrounding pixels from this layer.")
                        }
                        Action::Filter(compositor::filters::Filter::Lens { .. }) => Some(
                            "Positive straightens lines that bow outward (barrel); negative, lines that bow inward (pincushion).",
                        ),
                        _ => None,
                    };
                    if let Some(description) = description {
                        let body_text = matches!(
                            action,
                            Action::Filter(compositor::filters::Filter::ContentFill)
                        );
                        contents = contents.child(
                            text(description)
                                .text_size(if body_text { 13. } else { 12. })
                                .line_height(if body_text { 16. } else { 15. })
                                .text_color(if body_text {
                                    Color::rgb8(224, 224, 224)
                                } else {
                                    Color::rgb8(180, 180, 180)
                                })
                                .wrap(),
                        );
                    }
                    if filter_sheet {
                        contents = contents.child(
                            self.filter_preview_controls(cx)
                                .flex_shrink_0()
                                .opacity(if self.panel_applying() { 0.4 } else { 1. }),
                        );
                        if !error.is_empty() {
                            contents = contents.child(
                                text(error.clone())
                                    .wrap()
                                    .text_size(13.)
                                    .line_height(16.)
                                    .text_color(Color::rgb8(255, 159, 10)),
                            );
                        }
                        if self
                            .current_document()
                            .is_some_and(|doc| doc.selection.is_some())
                        {
                            contents = contents.child(
                                text("Limited to the selection")
                                    .text_size(12.)
                                    .line_height(15.)
                                    .text_color(Color::rgb8(180, 180, 180)),
                            );
                        }
                        contents = contents.child(Self::divider());
                    }
                }
                if !filter_sheet && !jpeg_sheet && !error.is_empty() {
                    contents = contents.child(
                        text(error.clone())
                            .wrap()
                            .text_sm()
                            .text_color(Color::rgb8(255, 160, 135)),
                    );
                }
                let apply = cx.listener(
                    if retained {
                        "retained-panel-apply"
                    } else {
                        "form-apply"
                    },
                    move |this, cx| {
                        this.size_menus.close(cx);
                        this.submit_form(cx);
                        this.changed(cx);
                    },
                );
                let mut footer = div().flex_row().items_center().gap(8.).flex_shrink_0();
                if jpeg_sheet {
                    footer = footer.child(self.jpeg_status(&error).flex_1().min_w(0.));
                }
                footer = footer.child(
                    Self::control("Cancel")
                        .opacity(if self.panel_applying() { 0.4 } else { 1. })
                        .on_click(cancel),
                );
                if !jpeg_sheet {
                    footer = footer.child(div().flex_1());
                }
                if let Some(progress) = self.panel_progress(action) {
                    footer = footer.child(progress);
                }
                let unavailable = self.panel_applying()
                    || self.automatic_filter_unavailable(action)
                    || (size_sheet && self.size_result(action, &fields).is_err())
                    || (jpeg_sheet && !self.jpeg_ready());
                contents = contents.child(
                    footer.child(Self::form_apply_button(action, unavailable).on_click(apply)),
                );
            }
        }
        self.mount_form(cx, dialog, contents, width, title, panel_kind)
    }

    // Keep the button's large Element temporaries out of the frame that also
    // constructs adjustment fields and sliders on the default test-thread stack.
    fn form_apply_button(action: Action, unavailable: bool) -> Element {
        Self::control(if matches!(action, Action::ExportJpeg) {
            "Export…"
        } else if matches!(action, Action::ImageSize) {
            "Resize"
        } else {
            "OK"
        })
        .bg(if unavailable {
            Color::rgb8(55, 62, 72)
        } else {
            Color::rgb8(0, 122, 255)
        })
        .hover(|s| s.bg(Color::rgb8(24, 137, 255)))
        .text_color(if unavailable {
            Color::rgb8(139, 139, 139)
        } else {
            Color::WHITE
        })
        .disabled(unavailable)
    }

    pub(super) fn submit_form(&mut self, cx: &mut EventContext) {
        let Some(Form::Edit { action, fields, .. }) = &self.modal else {
            return;
        };
        if self.pending || self.automatic_filter_unavailable(*action) {
            return;
        }
        if matches!(action, Action::ExportJpeg) {
            // Enter follows the same readiness gate as the disabled Export button.
            if self.jpeg_ready() {
                let result = self.finish_jpeg(cx);
                self.operation_result(alerts::Operation::ExportJpeg, result, cx);
            }
            return;
        }
        let size_operation = matches!(action, Action::CanvasSize | Action::ImageSize)
            && self.size_result(*action, fields).is_ok();
        let action = *action;
        let values = fields.iter().map(|(_, value)| value.clone()).collect();
        match self.apply_form(action, values) {
            Ok(()) if !self.panel_applying() => self.finish_form(),
            Ok(()) => {}
            Err(error) => {
                if size_operation {
                    self.show_error(alerts::Operation::for_action(action), error.to_string());
                }
                if let Some(Form::Edit { error: message, .. }) = &mut self.modal {
                    *message = error.to_string();
                }
            }
        }
    }

    pub(super) fn apply_form(&mut self, action: Action, values: Vec<String>) -> Result<()> {
        let value = |index: usize| {
            values
                .get(index)
                .map(|s| s.as_str())
                .ok_or_else(|| invalid("A required field is missing."))
        };
        let number = |index: usize| -> Result<f64> {
            let n: f64 = value(index)?
                .trim()
                .parse()
                .map_err(|_| invalid("Enter a valid number in every numeric field."))?;
            if n.is_finite() {
                Ok(n)
            } else {
                Err(invalid("Values must be finite numbers."))
            }
        };
        let dimension = |index| -> Result<u32> {
            let n = number(index)?;
            if n.fract() != 0. || !(1. ..=30_000.).contains(&n) {
                return Err(invalid(
                    "Dimensions must be whole numbers from 1 to 30,000.",
                ));
            }
            Ok(n as u32)
        };
        match action {
            Action::FeatherSelection => {
                let amount = number(0)?;
                if amount.fract() != 0. || !(1. ..=250.).contains(&amount) {
                    return Err(invalid(
                        "Enter a whole feather amount from 1 to 250 pixels.",
                    ));
                }
                self.apply_selection_feather(amount as u16)?;
            }
            Action::CameraRaw => {
                if !self.filter_source_is_current() {
                    self.cancel_filter();
                    return Ok(());
                }
                let settings = self.camera_raw.parse_fields(&values)?;
                let (source, _) = self.filter_source()?;
                self.begin_filter_commit();
                self.queue(jobs::Job::CameraRaw {
                    settings: Box::new(settings),
                    source,
                });
            }
            Action::Filter(filter) => {
                if !self.filter_source_is_current() {
                    self.cancel_filter();
                    return Ok(());
                }
                let filter = Self::filter_values(filter, &values)?;
                if matches!(filter, compositor::filters::Filter::ContentFill) {
                    return self.commit_content_fill();
                }
                let (source, mask) = self.filter_source()?;
                self.begin_filter_commit();
                self.queue(jobs::Job::Filter {
                    filter,
                    source,
                    mask,
                });
            }
            Action::RemoveBackground => {
                if !self.filter_source_is_current() {
                    self.cancel_filter();
                    return Ok(());
                }
                if self.automatic_filter_unavailable(action) {
                    return Err(invalid(
                        "Wait for a successful background preview before applying. If the preview failed, turn Preview off and on to retry. Your original pixels are preserved.",
                    ));
                }
                let quality = self.background_values(&values)?;
                let subject = self.background_subject(quality)?;
                let (source, _) = self.filter_source()?;
                self.begin_filter_commit();
                self.queue(jobs::Job::RemoveBackground {
                    quality,
                    subject,
                    source,
                });
            }

            Action::New => {
                let doc = Document::new(dimension(0)?, dimension(1)?)?;
                if self.has_document() {
                    self.add_empty_tab();
                }
                self.tabs[self.current].create_document(doc)?;
                self.tools.polygon = None;
            }
            Action::Transform => {
                let origin = [number(0)?, number(1)?];
                let size = [number(2)?, number(3)?];
                let rotation = number(4)?;
                if number(5)? <= 0. {
                    return Err(invalid("Scale must be greater than zero."));
                }
                let sampling = match value(6)?.trim().to_lowercase().as_str() {
                    "high" => compositor::geometry::Sampling::High,
                    "smooth" => compositor::geometry::Sampling::Smooth,
                    "nearest" => compositor::geometry::Sampling::Nearest,
                    _ => {
                        return Err(invalid(
                            "Choose high, smooth, or nearest for transform sampling.",
                        ));
                    }
                };
                let mask = self.tools.mask_target;
                if let Some(edit) = &self.pending_pixels {
                    let t = compositor::geometry::Transform {
                        origin,
                        size,
                        rotation,
                        sampling,
                        ..edit.placement.bounds()
                    };
                    self.preview_pixels(edit.placement.following(t))?;
                } else {
                    let label = if self.session().document.selection.is_some() && !mask {
                        "Transform Selection"
                    } else {
                        self.transform_history_label(false)
                    };
                    self.session_mut().edit(label, |doc| {
                        if doc.selection.is_some() && !mask {
                            let pixels = compositor::floating::FloatingPixels::lift(doc)?;
                            let transform = compositor::geometry::Transform {
                                origin,
                                size,
                                rotation,
                                sampling,
                                ..pixels.placement
                            };
                            *doc = pixels.preview(transform, false)?;
                            return Ok(());
                        }
                        let old = compositor::transform::selection_bounds(doc, mask)
                            .ok_or_else(|| invalid("Select a layer to transform."))?;
                        let new = compositor::geometry::Transform {
                            origin,
                            size,
                            rotation,
                            sampling,
                            ..old
                        };
                        compositor::transform::apply(doc, old, new, mask)?;
                        compositor::transform::set_sampling(doc, sampling, mask);
                        Ok(())
                    })?;
                }
            }
            Action::CropSettings => {
                let ratio = value(0)?.trim().to_lowercase();
                self.tools.crop_ratio = match ratio.as_str() {
                    "free" => None,
                    "original" => Some(
                        self.session().document.width as f64
                            / self.session().document.height as f64,
                    ),
                    _ => {
                        let (a, b) = ratio.split_once(':').ok_or_else(|| {
                            invalid("Enter free, original, or a ratio such as 16:9.")
                        })?;
                        let parse = |v: &str| {
                            v.trim()
                                .parse::<f64>()
                                .map_err(|_| invalid("Ratio sides must be positive numbers."))
                        };
                        let (a, b) = (parse(a)?, parse(b)?);
                        if !a.is_finite()
                            || !b.is_finite()
                            || a <= 0.
                            || b <= 0.
                            || !(1. / 30_000. ..=30_000.).contains(&(a / b))
                        {
                            return Err(invalid(
                                "Ratio sides must be finite positive numbers within the supported canvas proportions.",
                            ));
                        }
                        Some(a / b)
                    }
                };
                self.tools.crop_picker.clear_selection();
                self.tools.crop_picker.select_id(ratio.as_str());
                self.change_crop_ratio();
            }
            Action::CanvasSize => {
                let unit = match value(4)?.trim().to_lowercase().as_str() {
                    "px" => compositor::canvas_size::Unit::Pixels,
                    "percent" => compositor::canvas_size::Unit::Percent,
                    "inches" => compositor::canvas_size::Unit::Inches,
                    "cm" => compositor::canvas_size::Unit::Centimeters,
                    _ => {
                        return Err(invalid(
                            "Choose px, percent, inches, or cm for canvas units.",
                        ));
                    }
                };
                let relative = match value(5)?.trim() {
                    "0" => false,
                    "1" => true,
                    _ => return Err(invalid("Relative dimensions accepts 0 or 1.")),
                };
                let size = compositor::canvas_size::dimensions(
                    &self.session().document,
                    [number(0)?, number(1)?],
                    unit,
                    relative,
                )?;
                let fill = match value(6)?.trim().to_lowercase().as_str() {
                    "transparent" => None,
                    "foreground" => Some(self.tools.brush.color),
                    "background" => Some(self.tools.background),
                    "black" => Some([0, 0, 0, 255]),
                    "white" => Some([255; 4]),
                    color => Some(parse_color(color)?),
                };
                let horizontal = match value(2)?.trim().to_lowercase().as_str() {
                    "left" => 0.,
                    "center" => 0.5,
                    "right" => 1.,
                    _ => {
                        return Err(invalid(
                            "Choose left, center, or right for the horizontal anchor.",
                        ));
                    }
                };
                let vertical = match value(3)?.trim().to_lowercase().as_str() {
                    "top" => 0.,
                    "center" => 0.5,
                    "bottom" => 1.,
                    _ => {
                        return Err(invalid(
                            "Choose top, center, or bottom for the vertical anchor.",
                        ));
                    }
                };
                self.session_mut().edit_committed("Canvas Size", |doc| {
                    compositor::canvas_size::resize(doc, size, [horizontal, vertical], fill)
                })?;
                self.session_mut().fit = true;
                self.refresh_adjustment_document();
                self.refresh_filter_document();
            }
            Action::ImageSize => {
                let resolution = number(2)?;
                let original = [
                    self.session().document.width,
                    self.session().document.height,
                ];
                let [w, h] =
                    self.image_sizing
                        .dimensions([number(0)?, number(1)?], original, resolution)?;
                let sampling = match value(3)?.trim().to_lowercase().as_str() {
                    "high" => compositor::geometry::Sampling::High,
                    "smooth" => compositor::geometry::Sampling::Smooth,
                    "nearest" => compositor::geometry::Sampling::Nearest,
                    _ => {
                        return Err(invalid(
                            "Choose high, smooth, or nearest for image sampling.",
                        ));
                    }
                };
                self.session_mut().edit_committed("Image Size", |doc| {
                    compositor::image_resize::resize(doc, w, h, resolution, sampling)
                })?;
                self.session_mut().fit = true;
                self.refresh_adjustment_document();
                self.refresh_filter_document();
            }
            Action::Color => return self.finish_color(true),
            Action::EditAdjustment => self.finish_adjustment()?,
            _ => {}
        }
        Ok(())
    }
}

pub(super) fn parse_color(text: &str) -> Result<[u8; 4]> {
    compositor::palette::parse_hex(text)
}
