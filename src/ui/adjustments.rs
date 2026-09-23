use super::adjustment_fields;
use super::adjustment_histogram::AdjustmentHistogram;
use super::*;
use compositor::{
    adjustment::{Adjustment, Channel, ColorRange, HueSaturation, RangeAdjustment},
    document::{Layer, LayerContent},
    invalid,
};

pub(super) struct AdjustmentEdit {
    pub settings: Adjustment,
    pub(super) channel_popup: bool,
    pub(super) original: Layer,
    target: Target,
    pub(super) preview: bool,
    pub(super) preview_job: Option<super::adjustment_preview::AdjustmentPreview>,
    pub curve: super::curves::CurveState,
    pub(super) hue_sampling: super::hue_sampling::HueSampling,
    pub(super) hue_band_drag: Option<super::hue_controls::HueBandDrag>,
    pub(super) histogram: Option<AdjustmentHistogram>,
    pub(super) levels_handles: super::levels_handles::Handles,
    pub(super) levels_sampling: Option<super::levels_sampling::LevelsSampling>,
}

enum Target {
    Layer,
    Pixels {
        selection: Option<compositor::selection::Selection>,
        prepared: Option<Box<Layer>>,
    },
}

impl AdjustmentEdit {
    pub(super) fn pixel_selection(&self) -> Option<&compositor::selection::Selection> {
        match &self.target {
            Target::Pixels { selection, .. } => selection.as_ref(),
            Target::Layer => None,
        }
    }

    pub(super) fn cache_pixel_preview(&mut self, layer: Layer) {
        if let Target::Pixels { prepared, .. } = &mut self.target {
            *prepared = Some(Box::new(layer));
        }
    }

    // Swift removes the Hue/Levels pixel preview and skips the commit for identity settings.
    fn identity_pixels(&self, settings: &Adjustment) -> bool {
        matches!(self.target, Target::Pixels { .. })
            && match settings.kind {
                Kind::Levels => settings
                    .levels
                    .ranges
                    .iter()
                    .all(|r| *r == compositor::adjustment::LevelRange::default()),
                Kind::HueSaturation => settings.hsv_settings.as_ref().map_or(
                    !settings.colorize
                        && settings.hue == 0.
                        && settings.saturation == 0.
                        && settings.lightness == 0.,
                    |hsv| {
                        !hsv.colorize
                            && hsv
                                .adjustments
                                .iter()
                                .all(|(_, a)| *a == RangeAdjustment::default())
                    },
                ),
                _ => false,
            }
    }
}

impl Editor {
    pub(super) fn editing_adjustment_layer(&self) -> bool {
        self.adjustment_edit
            .as_ref()
            .is_some_and(|edit| !matches!(edit.target, Target::Pixels { .. }))
    }

    pub(super) fn open_pixel_adjustment(&mut self, kind: Kind) -> Result<()> {
        if self.tools.mask_target {
            return Err(invalid(
                "Color adjustments edit image pixels. Select the layer instead of its mask.",
            ));
        }
        let doc = &self.session().document;
        let original = doc
            .active_layer()
            .filter(|l| l.raster().is_some())
            .cloned()
            .ok_or_else(|| invalid("Select a layer containing pixels to adjust its colors."))?;
        let histogram = matches!(kind, Kind::Levels | Kind::Curves)
            .then(|| AdjustmentHistogram::pixels(original.clone(), doc.selection.clone()));
        let levels_sampling = (kind == Kind::Levels).then(|| {
            super::levels_sampling::LevelsSampling::pixels(
                original.clone(),
                [doc.width, doc.height],
            )
        });
        let preview_job = original
            .raster()
            .filter(|p| u64::from(p.width()) * u64::from(p.height()) >= 1_000_000)
            .map(|_| super::adjustment_preview::AdjustmentPreview::new(self.session().id));
        let selection = doc.selection.clone();
        self.session_mut()
            .begin(super::adjustment_layers::title(kind))?;
        self.adjustment_edit = Some(AdjustmentEdit {
            settings: Adjustment::new(kind),
            channel_popup: false,
            original,
            target: Target::Pixels {
                selection,
                prepared: None,
            },
            preview: true,
            preview_job,
            curve: super::curves::CurveState::None,
            hue_sampling: super::hue_sampling::HueSampling::Off,
            hue_band_drag: None,
            histogram,
            levels_sampling,
            levels_handles: Default::default(),
        });
        self.show_adjustment_fields();
        self.preview_adjustment()
    }

    pub(super) fn open_adjustment(&mut self, kind: Option<Kind>) -> Result<()> {
        if let Some(kind) = kind {
            self.add_adjustment_layer(kind)?;
        }
        let mut settings = match self
            .session()
            .document
            .active_layer()
            .map(|layer| &layer.content)
        {
            Some(LayerContent::Adjustment(settings)) => settings.as_ref().clone(),
            _ => return Err(invalid("Select an adjustment layer to edit its settings.")),
        };
        if settings.kind == Kind::Invert {
            return Ok(());
        }
        if settings.kind == Kind::HueSaturation && settings.hsv_settings.is_none() {
            settings.hsv_settings = Some(HueSaturation {
                adjustments: vec![(
                    ColorRange::Master,
                    RangeAdjustment {
                        hue: settings.hue,
                        saturation: settings.saturation,
                        lightness: settings.lightness,
                    },
                )],
                colorize: settings.colorize,
                ..HueSaturation::default()
            });
        }
        self.session_mut().begin(format!(
            "Edit {} Adjustment",
            super::adjustment_layers::title(settings.kind)
        ))?;
        let original = self
            .session()
            .document
            .active_layer()
            .cloned()
            .ok_or_else(|| invalid("The adjustment layer is missing."))?;
        let histogram = matches!(settings.kind, Kind::Levels | Kind::Curves)
            .then(|| AdjustmentHistogram::below(self.session().document.clone(), original.id));
        let levels_sampling = (settings.kind == Kind::Levels).then(|| {
            super::levels_sampling::LevelsSampling::composite(
                self.session().document.clone(),
                original.id,
            )
        });
        self.adjustment_edit = Some(AdjustmentEdit {
            original,
            target: Target::Layer,
            preview: true,
            preview_job: None,
            curve: super::curves::CurveState::None,
            hue_sampling: super::hue_sampling::HueSampling::Off,
            settings,
            channel_popup: false,
            hue_band_drag: None,
            histogram,
            levels_sampling,
            levels_handles: Default::default(),
        });
        self.show_adjustment_fields();
        Ok(())
    }

    pub(super) fn show_adjustment_fields(&mut self) {
        if let Some(edit) = &self.adjustment_edit {
            self.modal = Some(Form::Edit {
                title: super::adjustment_layers::title(edit.settings.kind),
                action: Action::EditAdjustment,
                fields: adjustment_fields::fields(&edit.settings),
                error: String::new(),
            });
        }
    }

    pub(super) fn preview_adjustment(&mut self) -> Result<()> {
        if !matches!(
            self.modal,
            Some(Form::Edit {
                action: Action::EditAdjustment,
                ..
            })
        ) {
            return Ok(());
        }
        let Some(edit) = &mut self.adjustment_edit else {
            return Ok(());
        };
        if let Some(job) = &mut edit.preview_job {
            job.changed(None);
        }
        let Some(Form::Edit { fields, .. }) = &self.modal else {
            return Ok(());
        };
        let settings = adjustment_fields::parse(
            &edit.settings,
            &fields.iter().map(|(_, s)| s.clone()).collect::<Vec<_>>(),
        )?;
        self.preview_adjustment_settings(settings)
    }

    pub(super) fn preview_adjustment_settings(&mut self, settings: Adjustment) -> Result<()> {
        let Some(edit) = &mut self.adjustment_edit else {
            return Ok(());
        };
        let show_preview = edit.preview && !edit.identity_pixels(&settings);
        edit.settings = settings.clone();
        if let Some(job) = &mut edit.preview_job {
            job.changed(show_preview.then(|| settings.clone()));
            if show_preview {
                return Ok(());
            }
        }
        if let Target::Pixels {
            selection,
            prepared,
        } = &mut edit.target
        {
            if show_preview {
                let mut result = edit.original.clone();
                compositor::pixel_adjustment::apply(&mut result, &settings, selection.as_ref())?;
                *prepared = Some(Box::new(result));
            } else {
                *prepared = None;
            }
            self.refresh_adjustment_document();
            return Ok(());
        }
        let original = edit.original.clone();
        let mut result = self
            .session()
            .committed_document()
            .layer(original.id)
            .unwrap_or(&original)
            .clone();
        if show_preview {
            result.content = LayerContent::Adjustment(Box::new(settings));
        }
        let layer = self
            .session_mut()
            .document
            .layers
            .iter_mut()
            .find(|layer| layer.id == result.id)
            .ok_or_else(|| invalid("The adjustment layer is missing."))?;
        *layer = result;
        Ok(())
    }

    pub(super) fn refresh_adjustment_document(&mut self) {
        if !self.has_document() {
            return;
        }
        let Some(edit) = &self.adjustment_edit else {
            return;
        };
        let Target::Pixels { prepared, .. } = &edit.target else {
            return;
        };
        let mut document = self.session().committed_document().clone();
        if edit.preview
            && let Some(prepared) = prepared
        {
            super::layer_preview::overlay(&mut document, &edit.original, prepared, false);
        }
        self.session_mut().document = document;
    }

    pub(super) fn adjustment_source_is_current(&self) -> bool {
        if !self.has_document() {
            return false;
        }
        self.adjustment_edit.as_ref().is_none_or(|edit| {
            !matches!(edit.target, Target::Pixels { .. })
                || self
                    .session()
                    .committed_document()
                    .layers
                    .iter()
                    .find(|layer| layer.id == edit.original.id)
                    .and_then(|layer| layer.raster())
                    .zip(edit.original.raster())
                    .is_some_and(|(current, original)| Arc::ptr_eq(current, original))
        })
    }

    pub(super) fn refresh_adjustment(&mut self) {
        let result = self.preview_adjustment();
        if self.adjustment_edit.is_some()
            && let Some(Form::Edit {
                action: Action::EditAdjustment,
                error,
                ..
            }) = &mut self.modal
        {
            *error = result.err().map_or_else(String::new, |e| e.to_string());
        }
    }

    pub(super) fn finish_adjustment(&mut self) -> Result<()> {
        if !self.adjustment_source_is_current() {
            self.cancel_adjustment();
            return Ok(());
        }
        if self
            .adjustment_edit
            .as_ref()
            .is_some_and(|edit| edit.preview_job.is_some())
        {
            self.preview_adjustment()?;
            let edit = self
                .adjustment_edit
                .as_ref()
                .ok_or_else(|| invalid("The color adjustment was closed."))?;
            if edit.identity_pixels(&edit.settings) {
                self.cancel_adjustment();
                return Ok(());
            }
            let job = jobs::Job::AdjustColors {
                settings: Box::new(edit.settings.clone()),
                original: Box::new(edit.original.clone()),
                selection: edit.pixel_selection().cloned(),
            };
            self.stop_adjustment_sampling();
            if let Some(worker) = self
                .adjustment_edit
                .as_mut()
                .and_then(|edit| edit.preview_job.as_mut())
            {
                worker.begin_commit();
            }
            self.queue(job);
            return Ok(());
        }
        if let Some(edit) = &mut self.adjustment_edit {
            edit.preview = true;
        }
        self.preview_adjustment()?;
        self.session_mut().commit()?;
        self.adjustment_edit = None;
        self.status = self.tool_hint().into();
        Ok(())
    }

    pub(super) fn cancel_adjustment(&mut self) {
        if self.adjustment_edit.take().is_some() {
            if let Some(session) = self.tabs[self.current].history_session_mut() {
                session.cancel();
            }
            self.status = self.tool_hint().into();
        }
    }

    pub(super) fn choose_adjustment_channel(&mut self, index: usize) -> Result<()> {
        self.preview_adjustment()?;
        if let Some(edit) = &mut self.adjustment_edit {
            edit.channel_popup = false;
            edit.curve = super::curves::CurveState::None;
            let channel = [Channel::RGB, Channel::Red, Channel::Green, Channel::Blue]
                .get(index)
                .copied();
            match edit.settings.kind {
                Kind::Levels => {
                    if let Some(channel) = channel {
                        edit.settings.levels.channel = channel;
                    }
                }
                Kind::Curves => {
                    if let Some(channel) = channel {
                        edit.settings.curves.channel = channel;
                    }
                }
                Kind::HueSaturation => {
                    let settings = edit
                        .settings
                        .hsv_settings
                        .get_or_insert_with(HueSaturation::default);
                    if let Some(range) = [
                        ColorRange::Master,
                        ColorRange::Reds,
                        ColorRange::Yellows,
                        ColorRange::Greens,
                        ColorRange::Cyans,
                        ColorRange::Blues,
                        ColorRange::Magentas,
                    ]
                    .get(index)
                    {
                        settings.range = *range;
                    }
                }
                _ => {}
            }
        }
        self.show_adjustment_fields();
        self.preview_adjustment()
    }
    pub(super) fn adjustment_controls(&self, cx: &mut ViewContext<'_, Self>) -> Option<Element> {
        let edit = self.adjustment_edit.as_ref()?;
        if matches!(edit.settings.kind, Kind::Exposure | Kind::Grain) {
            return None;
        }
        let mut controls = div().flex_col().gap(if edit.settings.kind == Kind::Levels {
            16.
        } else {
            12.
        });
        if edit.settings.kind == Kind::GradientMap {
            return Some(self.gradient_map_controls(cx));
        }
        let label = match edit.settings.kind {
            Kind::Levels => Some(format!("{:?}", edit.settings.levels.channel)),
            Kind::Curves => Some(format!("{:?}", edit.settings.curves.channel)),
            Kind::HueSaturation => Some(format!(
                "{:?}",
                edit.settings
                    .hsv_settings
                    .as_ref()
                    .map_or(ColorRange::Master, |s| s.range)
            )),
            _ => None,
        };
        if let Some(label) = label {
            let selector = Self::control(label)
                .w(if edit.settings.kind == Kind::HueSaturation {
                    160.
                } else {
                    126.
                })
                .gap(12.)
                .child(div().flex_1())
                .disabled(
                    edit.settings.kind == Kind::HueSaturation
                        && edit
                            .settings
                            .hsv_settings
                            .as_ref()
                            .is_some_and(|s| s.colorize),
                )
                .child(Icon::PopupChevron.element(14.))
                .on_click(cx.listener("adjustment-channel", |this, cx| {
                    this.toggle_adjustment_channel(cx);
                }));
            controls = controls.child(if edit.settings.kind == Kind::HueSaturation {
                div()
                    .flex_row()
                    .items_center()
                    .flex_wrap()
                    .gap(12.)
                    .child(selector)
                    .child(div().flex_1())
                    .child(self.hue_sample_controls(cx))
            } else {
                div()
                    .flex_row()
                    .items_center()
                    .gap(8.)
                    .child(text("Channel").text_size(13.).line_height(16.))
                    .child(selector)
            });
        }
        if edit.settings.kind == Kind::Curves {
            controls = controls.child(self.curve_editor(cx));
        }
        if edit.settings.kind == Kind::Levels {
            controls = controls.child(self.levels_histogram(cx));
        }

        Some(controls)
    }
    pub(super) fn adjustment_footer(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let Some(edit) = &self.adjustment_edit else {
            return div();
        };
        let mut controls = div().flex_col().gap(16.);
        if edit.settings.kind == Kind::HueSaturation {
            if edit
                .settings
                .hsv_settings
                .as_ref()
                .is_some_and(|s| s.range != ColorRange::Master && !s.colorize)
            {
                controls = controls.child(self.hue_spectrum(cx));
            }
            controls = controls.child(self.hue_options(cx));
            if edit.pixel_selection().is_some() {
                controls = controls.child(
                    text("Limited to the selection")
                        .text_size(12.)
                        .line_height(15.)
                        .text_color(Color::rgb8(180, 180, 180)),
                );
            }
            return controls.child(Self::divider());
        }
        if edit.settings.kind == Kind::Levels {
            controls = controls.child(self.levels_sample_controls(cx));
            let mut row = div().flex_row().gap(5.);
            for (index, (label, mode)) in [
                ("Contrast", compositor::histogram::AutoLevels::Contrast),
                ("Color", compositor::histogram::AutoLevels::Color),
                (
                    "Color + neutral midtones",
                    compositor::histogram::AutoLevels::Neutral,
                ),
            ]
            .into_iter()
            .enumerate()
            {
                row = row.child(
                    Self::control(label)
                        .disabled(
                            edit.histogram
                                .as_ref()
                                .and_then(AdjustmentHistogram::ready)
                                .is_none(),
                        )
                        .on_click(cx.listener(70_000 + index as u64, move |this, cx| {
                            if let Some(edit) = &mut this.adjustment_edit
                                && let Some(histogram) =
                                    edit.histogram.as_ref().and_then(AdjustmentHistogram::ready)
                            {
                                edit.settings.levels = histogram.automatic(mode);
                            }
                            this.stop_adjustment_sampling();
                            this.show_adjustment_fields();
                            this.refresh_adjustment();
                            this.changed(cx);
                        })),
                );
            }
            controls = controls.child(
                div()
                    .flex_col()
                    .gap(6.)
                    .child(
                        text("Auto")
                            .text_size(10.)
                            .line_height(13.)
                            .text_color(Color::rgb8(180, 180, 180)),
                    )
                    .child(row),
            );
        }
        let mut row = div()
            .flex_row()
            .items_center()
            .gap(8.)
            .child(
                Self::check_control("Preview", edit.preview)
                    .text_size(13.)
                    .line_height(16.)
                    .on_click(cx.listener("adjustment-preview", |this, cx| {
                        if let Some(edit) = &mut this.adjustment_edit {
                            edit.preview = !edit.preview;
                        }
                        this.refresh_adjustment();
                        this.changed(cx);
                    })),
            )
            .child(div().flex_1().min_w(0.));
        if edit.settings.kind == Kind::Levels {
            row = row.child(Self::control("Reset").on_click(cx.listener(
                "adjustment-reset",
                |this, cx| {
                    this.stop_adjustment_sampling();
                    if let Some(edit) = &mut this.adjustment_edit {
                        edit.settings = Adjustment::new(edit.settings.kind);
                    }
                    this.show_adjustment_fields();
                    this.refresh_adjustment();
                    this.changed(cx);
                },
            )));
        }
        controls = controls.child(row);
        if edit.settings.kind == Kind::Levels {
            controls = controls.child(
                text(match &edit.target {
                    Target::Layer => "Underlying pixels · alpha-weighted histogram",
                    Target::Pixels {
                        selection: Some(_), ..
                    } => "Original pixels · selection and alpha-weighted histogram",
                    Target::Pixels {
                        selection: None, ..
                    } => "Original pixels · alpha-weighted histogram",
                })
                .text_size(10.)
                .line_height(13.)
                .text_color(Color::rgb8(180, 180, 180))
                .wrap(),
            );
        }
        controls.child(Self::divider())
    }
}
