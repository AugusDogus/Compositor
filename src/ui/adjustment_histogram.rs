use super::*;
use compositor::{document::Layer, histogram::Histogram, invalid, selection::Selection};
use uuid::Uuid;

/// Immutable inputs from before the adjustment preview changed the document.
#[derive(Clone)]
enum Source {
    Pixels(Box<Layer>, Option<Selection>),
    Below(Document, Uuid),
}

impl Source {
    fn calculate(&self) -> Result<Histogram> {
        match self {
            Self::Pixels(layer, selection) => Histogram::for_layer(layer, selection.as_ref()),
            Self::Below(document, layer) => {
                compositor::document::validate_size(document.width, document.height)?;
                Ok(Histogram::new(&compositor::render::below(
                    document, *layer,
                )?))
            }
        }
    }
}

enum State {
    Queued,
    Running,
    Ready(Box<Histogram>),
    Failed(String),
}

pub(super) struct AdjustmentHistogram {
    id: Uuid,
    source: Source,
    state: State,
}

impl AdjustmentHistogram {
    pub fn pixels(layer: Layer, selection: Option<Selection>) -> Self {
        Self::new(Source::Pixels(Box::new(layer), selection))
    }

    pub fn below(document: Document, layer: Uuid) -> Self {
        Self::new(Source::Below(document, layer))
    }

    fn new(source: Source) -> Self {
        Self {
            id: Uuid::new_v4(),
            source,
            state: State::Queued,
        }
    }

    pub fn ready(&self) -> Option<&Histogram> {
        match &self.state {
            State::Ready(histogram) => Some(histogram),
            _ => None,
        }
    }

    pub fn error(&self) -> Option<&str> {
        match &self.state {
            State::Failed(message) => Some(message),
            _ => None,
        }
    }

    pub fn retry(&mut self) {
        if matches!(self.state, State::Failed(_)) {
            self.id = Uuid::new_v4();
            self.state = State::Queued;
        }
    }
}

impl Editor {
    pub(super) fn start_adjustment_histogram(&mut self, cx: &ViewContext<'_, Self>) {
        let Some(job) = self
            .adjustment_edit
            .as_mut()
            .and_then(|edit| edit.histogram.as_mut())
        else {
            return;
        };
        if !matches!(job.state, State::Queued) {
            return;
        }
        let id = job.id;
        let source = job.source.clone();
        job.state = State::Running;
        let launched = cx.spawn_background(move || source.calculate(), move |this, result, cx| {
            let result = result.map_err(|error| invalid(format!(
                "Histogram calculation failed: {error}. Your image is unchanged. Retry the histogram."
            ))).and_then(|result| result);
            this.receive_adjustment_histogram(id, result);
            cx.invalidate();
        });
        if let Err(error) = launched {
            self.receive_adjustment_histogram(id, Err(invalid(format!(
                "Could not start histogram calculation: {error}. Your image is unchanged. Retry the histogram."
            ))));
        }
    }

    fn receive_adjustment_histogram(&mut self, id: Uuid, result: Result<Histogram>) {
        let Some(job) = self
            .adjustment_edit
            .as_mut()
            .and_then(|edit| edit.histogram.as_mut())
        else {
            return;
        };
        if job.id != id || !matches!(job.state, State::Running) {
            return;
        }
        job.state = match result {
            Ok(histogram) => State::Ready(Box::new(histogram)),
            Err(error) => State::Failed(error.to_string()),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compositor::levels_sample::LevelsSample;
    use quickgui::{Application, WindowOptions};

    #[test]
    fn levels_sampling_stays_in_the_panel_and_auto_or_reset_stops_it() {
        let mut editor = Editor::with_test_document();
        compositor::edits::fill(
            &mut editor.session_mut().document,
            [80, 120, 160, 255],
            false,
            false,
        )
        .unwrap();
        let original = editor.session().document.clone();
        editor.open_pixel_adjustment(Kind::Levels).unwrap();
        let job = editor
            .adjustment_edit
            .as_mut()
            .unwrap()
            .histogram
            .as_mut()
            .unwrap();
        job.state = State::Ready(Box::new(job.source.calculate().unwrap()));
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::new("Levels sampling controls").size(1280., 900.),
                editor,
            )
            .unwrap();
        let window = view.window_handle();
        for index in 0..5 {
            assert_eq!(
                cx.element_bounds(window, 50_000_u64 + index).unwrap().width,
                80.
            );
        }
        assert!(
            cx.accessibility_update(window)
                .unwrap()
                .nodes
                .iter()
                .any(|(_, node)| node.value() == Some("1.00"))
        );
        cx.focus(window, 50_001_u64).unwrap();
        cx.update(view, |e, cx| {
            e.update_form_field(1, "1.2345");
            e.changed(cx);
        })
        .unwrap();
        assert!(
            cx.accessibility_update(window)
                .unwrap()
                .nodes
                .iter()
                .any(|(_, node)| node.value() == Some("1.2345"))
        );
        cx.focus(window, "workspace").unwrap();
        assert!(
            cx.accessibility_update(window)
                .unwrap()
                .nodes
                .iter()
                .any(|(_, node)| node.value() == Some("1.23"))
        );
        cx.read(view, |e| {
            let Some(Form::Edit { fields, .. }) = &e.modal else {
                panic!("missing Levels form");
            };
            assert_eq!(fields[1].1, "1.2345");
        })
        .unwrap();
        cx.click(window, "adjustment-reset").unwrap();
        for mode in [LevelsSample::Black, LevelsSample::Gray, LevelsSample::White] {
            let id = format!("levels-sample-{}", mode.label());
            let before = cx.element_bounds(window, id.clone()).unwrap();
            cx.click(window, id.clone()).unwrap();
            assert_eq!(
                cx.read(view, |e| e.levels_sample_mode()).unwrap(),
                Some(mode)
            );
            assert_eq!(cx.element_bounds(window, id.clone()).unwrap(), before);
            cx.click(window, id.clone()).unwrap();
            assert!(cx.read(view, |e| e.levels_sample_mode().is_none()).unwrap());
            cx.click(window, id).unwrap();
        }
        for id in [70_000_u64, 70_001, 70_002] {
            cx.click(window, id).unwrap();
            assert!(cx.read(view, |e| e.levels_sample_mode().is_none()).unwrap());
            cx.click(window, "levels-sample-Black").unwrap();
        }
        cx.click(window, "adjustment-reset").unwrap();
        cx.read(view, |e| {
            assert!(e.levels_sample_mode().is_none());
            assert_eq!(
                e.adjustment_edit.as_ref().unwrap().settings.levels,
                compositor::adjustment::Levels::default()
            );
            assert_eq!(e.session().document, original);
            assert!(e.session().undo_label().is_none());
        })
        .unwrap();
        cx.click(window, "levels-sample-Gray").unwrap();
        cx.focus(window, "workspace").unwrap();
        cx.simulate_keystrokes(window, "enter").unwrap();
        cx.read(view, |e| {
            assert!(e.adjustment_edit.is_none());
            assert!(e.modal.is_none());
            assert_eq!(e.session().document, original);
            assert!(e.session().undo_label().is_none());
        })
        .unwrap();
    }

    #[test]
    fn histogram_uses_original_pixels_and_selection_without_blocking_open() {
        let mut editor = Editor::with_test_document();
        let mut doc = Document::new(2, 1).unwrap();
        compositor::edits::fill(&mut doc, [80, 120, 160, 255], false, false).unwrap();
        doc.selection = Some(Selection::rectangle(2, 1, [0., 0.], [1., 1.], false));
        editor.tabs = vec![Session::new(doc.clone(), None).into()];
        editor.open_pixel_adjustment(Kind::Levels).unwrap();
        let job = editor
            .adjustment_edit
            .as_mut()
            .unwrap()
            .histogram
            .as_mut()
            .unwrap();
        assert!(matches!(job.state, State::Queued));
        let id = job.id;
        let source = job.source.clone();
        job.state = State::Running;
        editor.session_mut().document.selection = None;
        let histogram = source.calculate().unwrap();
        assert_eq!(histogram.0[1][80], 1.);
        editor.receive_adjustment_histogram(id, Ok(histogram));
        assert!(
            editor
                .adjustment_edit
                .as_ref()
                .unwrap()
                .histogram
                .as_ref()
                .unwrap()
                .ready()
                .is_some()
        );
        editor.cancel_adjustment();
        assert_eq!(editor.session().document, doc);
    }

    #[test]
    fn old_histograms_cannot_update_reopened_dialogs_and_failures_can_retry() {
        let mut editor = Editor::with_test_document();
        editor.tabs = vec![Session::new(Document::new(2, 2).unwrap(), None).into()];
        compositor::edits::fill(&mut editor.session_mut().document, [80; 4], false, false).unwrap();
        editor.open_pixel_adjustment(Kind::Levels).unwrap();
        let old = editor
            .adjustment_edit
            .as_ref()
            .unwrap()
            .histogram
            .as_ref()
            .unwrap()
            .id;
        editor.cancel_adjustment();
        editor.open_pixel_adjustment(Kind::Curves).unwrap();
        let job = editor
            .adjustment_edit
            .as_mut()
            .unwrap()
            .histogram
            .as_mut()
            .unwrap();
        let current = job.id;
        job.state = State::Running;
        editor.receive_adjustment_histogram(old, Err(invalid("Obsolete worker")));
        assert!(
            editor
                .adjustment_edit
                .as_ref()
                .unwrap()
                .histogram
                .as_ref()
                .unwrap()
                .error()
                .is_none()
        );
        editor.receive_adjustment_histogram(current, Err(invalid("Worker failed")));
        let job = editor
            .adjustment_edit
            .as_mut()
            .unwrap()
            .histogram
            .as_mut()
            .unwrap();
        assert_eq!(job.error(), Some("Worker failed"));
        job.retry();
        assert!(matches!(job.state, State::Queued));
        assert_ne!(job.id, current);
        editor.cancel_adjustment();
        editor.receive_adjustment_histogram(current, Err(invalid("Late worker")));
        assert!(editor.adjustment_edit.is_none());
    }

    #[test]
    fn displayed_histograms_keep_full_scale_for_fractional_alpha_weights_and_empty_bins() {
        let mut opaque_graph = None;
        for alpha in [255, 64, 1, 0] {
            let mut editor = Editor::with_test_document();
            editor.tabs[0].set_document(Document::new(1, 1).unwrap(), None);
            compositor::edits::fill(
                &mut editor.session_mut().document,
                [128, 128, 128, alpha.max(1)],
                false,
                false,
            )
            .unwrap();
            editor.open_pixel_adjustment(Kind::Levels).unwrap();
            let job = editor
                .adjustment_edit
                .as_mut()
                .unwrap()
                .histogram
                .as_mut()
                .unwrap();
            let histogram = if alpha == 0 {
                Histogram([[0.; 256]; 4])
            } else {
                job.source.calculate().unwrap()
            };
            job.state = State::Ready(Box::new(histogram));
            let (mut cx, view) = Application::new()
                .into_test_context(
                    WindowOptions::new("Histogram display scale").size(1500., 900.),
                    editor,
                )
                .unwrap();
            let window = view.window_handle();
            let bounds = cx.element_bounds(window, "levels-histogram").unwrap();
            let screenshot = cx.capture_screenshot(window).unwrap();
            let scale = screenshot.width() as f32 / 1500.;
            let mut graph = Vec::new();
            for y in (bounds.y * scale).ceil() as u32..(bounds.bottom() * scale).floor() as u32 {
                for x in (bounds.x * scale).ceil() as u32..(bounds.right() * scale).floor() as u32 {
                    graph.push(screenshot.pixel(x, y).unwrap());
                }
            }
            if alpha == 0 {
                assert!(
                    graph.iter().all(|pixel| *pixel == graph[0]),
                    "Empty bins must show only the background"
                );
            } else if let Some(opaque) = &opaque_graph {
                assert!(
                    graph == *opaque,
                    "Histogram display changed with alpha {alpha}"
                );
            } else {
                assert!(graph.iter().any(|pixel| pixel[0] > 100));
                opaque_graph = Some(graph);
            }
        }
    }
}
