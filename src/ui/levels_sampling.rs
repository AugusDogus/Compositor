use super::*;
use compositor::{document::Layer, geometry::Point, levels_sample::LevelsSample};

enum Source {
    Pixels(Layer),
    Composite(Document, uuid::Uuid),
}

pub(super) struct LevelsSampling {
    source: Source,
    canvas: [u32; 2],
    mode: Option<LevelsSample>,
}

impl LevelsSampling {
    pub fn pixels(layer: Layer, canvas: [u32; 2]) -> Self {
        Self {
            source: Source::Pixels(layer),
            canvas,
            mode: None,
        }
    }

    pub fn composite(document: Document, layer: uuid::Uuid) -> Self {
        Self {
            canvas: [document.width, document.height],
            source: Source::Composite(document, layer),
            mode: None,
        }
    }

    fn color(&self, point: Point) -> Option<[f64; 3]> {
        if point
            .iter()
            .zip(self.canvas)
            .any(|(v, limit)| !v.is_finite() || *v < 0. || *v >= limit as f64)
        {
            return None;
        }
        let rgba = match &self.source {
            Source::Pixels(layer) => {
                let unit = layer.transform.unit(point);
                if unit.iter().any(|v| !(0. ..1.).contains(v)) {
                    return None;
                }
                let image = layer.raster()?;
                image[(
                    (unit[0] * image.width() as f64).floor() as u32,
                    (unit[1] * image.height() as f64).floor() as u32,
                )]
                    .0
                    .map(|v| v as f64 / 255.)
            }
            Source::Composite(document, layer) => {
                compositor::render::sample_below(document, *layer, point)
            }
        };
        (rgba[3] > 0.).then_some([rgba[0], rgba[1], rgba[2]])
    }
}

impl Editor {
    pub(super) fn levels_sample_mode(&self) -> Option<LevelsSample> {
        self.adjustment_edit
            .as_ref()?
            .levels_sampling
            .as_ref()?
            .mode
    }

    fn arm_levels_sample(&mut self, mode: LevelsSample) -> Result<()> {
        self.preview_adjustment()?;
        if let Some(sample) = self
            .adjustment_edit
            .as_mut()
            .and_then(|edit| edit.levels_sampling.as_mut())
        {
            sample.mode = (sample.mode != Some(mode)).then_some(mode);
        }
        if self.levels_sample_mode().is_none() {
            self.stop_adjustment_sampling();
            return Ok(());
        }
        self.status = format!(
            "Levels: click the original image to set {}. Click the eyedropper again to stop.",
            mode.label().to_lowercase()
        );
        Ok(())
    }

    pub(super) fn stop_levels_sampling(&mut self) {
        if let Some(sample) = self
            .adjustment_edit
            .as_mut()
            .and_then(|edit| edit.levels_sampling.as_mut())
        {
            sample.mode = None;
        }
    }

    pub(super) fn sample_levels(&mut self, point: Point) -> Result<()> {
        let Some(edit) = &mut self.adjustment_edit else {
            return Ok(());
        };
        let Some(sample) = &edit.levels_sampling else {
            return Ok(());
        };
        let Some(mode) = sample.mode else {
            return Ok(());
        };
        let Some(rgb) = sample.color(point) else {
            self.status = "Choose a nontransparent pixel inside the original image. The Levels settings are unchanged.".into();
            return Ok(());
        };
        edit.settings.levels = mode.apply(&edit.settings.levels, rgb)?;
        self.show_adjustment_fields();
        self.preview_adjustment()
    }

    pub(super) fn levels_sample_controls(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let mut row = div().flex_row().items_center().gap(5.).child(
            text("Sample")
                .text_size(10.)
                .line_height(13.)
                .text_color(Color::rgb8(170, 170, 170)),
        );
        let workspace = cx.focus_handle("workspace");
        for mode in [LevelsSample::Black, LevelsSample::Gray, LevelsSample::White] {
            let id = format!("levels-sample-{}", mode.label());
            row = row.child(
                Self::control(mode.label())
                    .flex_row_reverse()
                    .gap(5.)
                    .selected(self.levels_sample_mode() == Some(mode))
                    .selected_style(|s| s.bg(Color::rgb8(65, 107, 158)))
                    .child(Icon::Pipette.element(13.))
                    .id(id.clone())
                    .on_click(cx.listener(id, move |this, cx| {
                        let result = this.arm_levels_sample(mode);
                        if let Err(error) = result
                            && let Some(Form::Edit { error: message, .. }) = &mut this.modal
                        {
                            *message = error.to_string();
                        }
                        cx.focus(workspace);
                        this.changed(cx);
                    })),
            );
        }
        let mut controls = div().flex_col().gap(12.).child(row);
        if let Some(mode) = self.levels_sample_mode() {
            controls = controls.child(
                text(format!(
                    "Click the original layer to set {}. Click the eyedropper again to stop.",
                    mode.label().to_lowercase()
                ))
                .text_size(10.)
                .line_height(13.)
                .text_color(Color::rgb8(180, 180, 180))
                .wrap(),
            );
        }
        controls
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_original_transformed_pixels_and_cancel_preserves_document() {
        let mut editor = Editor::with_test_document();
        let mut doc = Document::new(20, 20).unwrap();
        compositor::edits::fill(&mut doc, [51, 102, 153, 255], false, false).unwrap();
        doc.layers[0].transform.origin = [5., 0.];
        doc.layers[0].transform.size = [10., 10.];
        editor.tabs = vec![Session::new(doc.clone(), None).into()];
        editor.open_pixel_adjustment(Kind::Levels).unwrap();
        editor.arm_levels_sample(LevelsSample::Gray).unwrap();
        editor.sample_levels([7., 2.]).unwrap();
        let settings = editor.adjustment_edit.as_ref().unwrap().settings.clone();
        assert_ne!(settings.levels, compositor::adjustment::Levels::default());
        editor.sample_levels([7., 2.]).unwrap();
        assert_eq!(editor.adjustment_edit.as_ref().unwrap().settings, settings);
        editor.sample_levels([0., 2.]).unwrap();
        assert_eq!(editor.adjustment_edit.as_ref().unwrap().settings, settings);
        editor.cancel_adjustment();
        assert_eq!(editor.session().document, doc);
    }

    #[test]
    fn adjustment_layer_samples_only_the_underlying_composite() {
        let mut editor = Editor::with_test_document();
        let mut doc = Document::new(2, 2).unwrap();
        compositor::edits::fill(&mut doc, [51, 102, 153, 255], false, false).unwrap();
        let original = doc.layers[0].raster().unwrap().clone();
        editor.tabs = vec![Session::new(doc, None).into()];
        editor.open_adjustment(Some(Kind::Levels)).unwrap();
        editor.arm_levels_sample(LevelsSample::Black).unwrap();
        editor.sample_levels([0.5, 0.5]).unwrap();
        assert_eq!(
            editor
                .adjustment_edit
                .as_ref()
                .unwrap()
                .settings
                .levels
                .ranges[2]
                .black,
            102.
        );
        editor.sample_levels([0.5, 0.5]).unwrap();
        assert_eq!(
            editor
                .adjustment_edit
                .as_ref()
                .unwrap()
                .settings
                .levels
                .ranges[2]
                .black,
            102.
        );
        editor.stop_levels_sampling();
        editor.finish_adjustment().unwrap();
        assert!(std::sync::Arc::ptr_eq(
            editor.session().document.layers[0].raster().unwrap(),
            &original
        ));
    }
}
