use super::*;
use compositor::{document::validate_size, invalid};

#[derive(Clone, Copy, Default)]
pub(super) enum ImageSizing {
    #[default]
    Pixels,
    Percent,
    Inches,
    Centimeters,
    PrintInches,
    PrintCentimeters,
}

/// Image resizing allocates pixels; canvas resizing may retain sparse, transparent space.
#[derive(Clone, Copy)]
pub(super) enum ScrubTarget {
    Image,
    Canvas { relative: bool },
}

impl ImageSizing {
    pub fn resamples(self) -> bool {
        !matches!(self, Self::PrintInches | Self::PrintCentimeters)
    }

    fn pixels(self, value: f64, original: u32, resolution: f64) -> f64 {
        match self {
            Self::Pixels => value,
            Self::Percent => value * original as f64 / 100.,
            Self::Inches | Self::PrintInches => value * resolution,
            Self::Centimeters | Self::PrintCentimeters => value * resolution / 2.54,
        }
    }

    fn displayed(self, pixels: f64, original: u32, resolution: f64) -> f64 {
        pixels / self.pixels(1., original, resolution)
    }

    /// `current` contains the edited axis followed by its perpendicular axis, in pixels.
    /// Ranges and sensitivity are returned in the same units as the adjacent number field.
    pub fn scrub_bounds(
        self,
        original: u32,
        current: [u32; 2],
        resolution: f64,
        locked: bool,
        target: ScrubTarget,
    ) -> ((f64, f64), f64) {
        if !self.resamples() {
            return (
                (
                    self.displayed(f64::from(original), original, 9600.),
                    self.displayed(f64::from(original), original, 1.),
                ),
                self.displayed(0.01, original, 1.),
            );
        }
        let [dimension, other] = current.map(f64::from);
        let limit = match target {
            ScrubTarget::Image => compositor::document::MAX_SURFACE_PIXELS as f64,
            ScrubTarget::Canvas { .. } => 30_000. * 30_000.,
        };
        let max = if locked {
            (limit * dimension / other)
                .sqrt()
                .min(30_000. * dimension / other)
                .min(30_000.)
        } else {
            (limit / other).min(30_000.)
        };
        let min = if locked {
            (dimension / other).max(1.)
        } else {
            1.
        };
        let offset = match target {
            ScrubTarget::Canvas { relative: true } => f64::from(original),
            _ => 0.,
        };
        (
            (
                self.displayed(min - offset, original, resolution),
                self.displayed(max.max(min) - offset, original, resolution),
            ),
            self.displayed(1., original, resolution),
        )
    }

    pub fn dimensions(
        self,
        values: [f64; 2],
        original: [u32; 2],
        resolution: f64,
    ) -> Result<[u32; 2]> {
        if !(1. ..=9600.).contains(&resolution) {
            return Err(invalid(
                "Resolution must be between 1 and 9600 pixels per inch.",
            ));
        }
        if values.iter().any(|v| !v.is_finite() || *v <= 0.) {
            return Err(invalid("Enter positive, finite image dimensions."));
        }
        if !self.resamples() {
            return Ok(original);
        }
        let pixels: [f64; 2] =
            std::array::from_fn(|i| self.pixels(values[i], original[i], resolution).round());
        if pixels.iter().any(|v| !(1. ..=30_000.).contains(v)) {
            return Err(invalid(
                "The resulting image must be 1 to 30,000 pixels per side. Adjust its dimensions or resolution.",
            ));
        }
        let size = pixels.map(|v| v as u32);
        if size != original {
            validate_size(size[0], size[1])?;
        }
        Ok(size)
    }
}

fn numbers(fields: &[(&str, String)]) -> Result<[f64; 3]> {
    let mut values = [0.; 3];
    for (index, value) in values.iter_mut().enumerate() {
        *value = fields
            .get(index)
            .and_then(|f| f.1.parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v > 0.)
            .ok_or_else(|| {
                invalid(
                    "Enter positive dimensions and resolution before changing image size units.",
                )
            })?;
    }
    Ok(values)
}

impl Editor {
    pub(super) fn change_image_sizing(&mut self, next: ImageSizing) -> Result<()> {
        let original = [
            self.session().document.width,
            self.session().document.height,
        ];
        let Some(Form::Edit {
            action: Action::ImageSize,
            fields,
            ..
        }) = &mut self.modal
        else {
            return Ok(());
        };
        let values = numbers(fields)?;
        for i in 0..2 {
            let pixels = if next.resamples() {
                self.image_sizing.pixels(values[i], original[i], values[2])
            } else {
                original[i] as f64
            };
            fields[i].1 = next.displayed(pixels, original[i], values[2]).to_string();
        }
        self.image_sizing = next;
        if !next.resamples()
            && let Some(link) = &mut self.dimension_link
        {
            link.locked = true;
            link.ratio = original[0] as f64 / original[1] as f64;
        }
        Ok(())
    }

    pub(super) fn image_size_input(&mut self, index: usize, value: &str) {
        let original = [
            self.session().document.width,
            self.session().document.height,
        ];
        let mode = self.image_sizing;
        let ratio = self
            .dimension_link
            .as_ref()
            .filter(|link| link.locked)
            .map(|link| link.ratio);
        let Some(Form::Edit {
            action: Action::ImageSize,
            fields,
            ..
        }) = &mut self.modal
        else {
            return;
        };
        if let Some(field) = fields.get_mut(index) {
            field.1 = value.to_owned();
        }
        let Ok(value) = value.parse::<f64>() else {
            return;
        };
        if !value.is_finite() || value <= 0. {
            return;
        }
        if !mode.resamples() {
            let resolution = match index {
                0 | 1 => original[index] as f64 / mode.pixels(value, original[index], 1.),
                2 => value,
                _ => return,
            };
            if !resolution.is_finite() || resolution <= 0. {
                return;
            }
            fields[2].1 = resolution.to_string();
            for i in 0..2 {
                if i != index {
                    fields[i].1 = mode
                        .displayed(original[i] as f64, original[i], resolution)
                        .to_string();
                }
            }
        } else if index < 2
            && let Some(ratio) = ratio
        {
            let other = 1 - index;
            let ratio = if matches!(mode, ImageSizing::Percent) {
                ratio / (original[0] as f64 / original[1] as f64)
            } else {
                ratio
            };
            let paired = if index == 0 {
                value / ratio
            } else {
                value * ratio
            };
            if paired.is_finite() {
                fields[other].1 = if matches!(mode, ImageSizing::Pixels) {
                    paired.round()
                } else {
                    paired
                }
                .to_string();
            }
        }
    }

    pub(super) fn image_resample_control(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        Self::check_control("Resample", self.image_sizing.resamples())
            .text_size(13.)
            .line_height(16.)
            .on_click(cx.listener("image-size-resample", |this, cx| {
                use ImageSizing::*;
                let next = match this.image_sizing {
                    PrintInches => Inches,
                    PrintCentimeters => Centimeters,
                    Centimeters => PrintCentimeters,
                    _ => PrintInches,
                };
                let result = this.change_image_sizing(next);
                if let Some(Form::Edit { error, .. }) = &mut this.modal {
                    *error = result.err().map_or_else(String::new, |e| e.to_string());
                }
                this.changed(cx);
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(editor: &Editor) -> Vec<String> {
        match &editor.modal {
            Some(Form::Edit { fields, .. }) => {
                fields.iter().map(|(_, value)| value.clone()).collect()
            }
            _ => panic!("Expected Image Size fields"),
        }
    }

    #[test]
    fn scrub_bounds_preserve_pixel_steps_and_respect_locked_surface_limits_in_every_unit() {
        for sizing in [
            ImageSizing::Pixels,
            ImageSizing::Percent,
            ImageSizing::Inches,
            ImageSizing::Centimeters,
        ] {
            let ((min, max), step) =
                sizing.scrub_bounds(200, [200, 100], 100., true, ScrubTarget::Image);
            assert!((sizing.pixels(step, 200, 100.) - 1.).abs() < 1e-10);
            assert_eq!(
                sizing
                    .dimensions([min, min / 2.], [200, 200], 100.)
                    .unwrap(),
                [2, 1]
            );
            assert_eq!(
                sizing
                    .dimensions([max, max / 2.], [200, 200], 100.)
                    .unwrap(),
                [20_000, 10_000]
            );
        }
    }

    #[test]
    fn canvas_scrub_allows_sparse_dimensions_and_print_scrub_limits_resolution() {
        let bounds = ImageSizing::Percent.scrub_bounds(
            200,
            [200, 100],
            100.,
            false,
            ScrubTarget::Canvas { relative: true },
        );
        assert_eq!(bounds, ((-99.5, 14_900.), 0.5));
        assert_eq!(
            ImageSizing::Pixels
                .scrub_bounds(
                    30_000,
                    [30_000, 30_000],
                    72.,
                    false,
                    ScrubTarget::Canvas { relative: false }
                )
                .0,
            (1., 30_000.)
        );
        for sizing in [ImageSizing::PrintInches, ImageSizing::PrintCentimeters] {
            let ((min, max), _) =
                sizing.scrub_bounds(200, [200, 100], 100., true, ScrubTarget::Image);
            assert!((200. / sizing.pixels(min, 200, 1.) - 9600.).abs() < 1e-9);
            assert!((200. / sizing.pixels(max, 200, 1.) - 1.).abs() < 1e-9);
        }
    }

    #[test]
    fn unit_changes_preserve_pixels_and_linked_percentages_survive_partial_input() {
        let mut editor = Editor::with_test_document();
        let mut doc = Document::new(1000, 500).unwrap();
        doc.resolution = 100.;
        editor.tabs = vec![Session::new(doc, None).into()];
        editor.open_form(Action::ImageSize);
        editor.change_image_sizing(ImageSizing::Percent).unwrap();
        assert_eq!(&fields(&editor)[..2], &["100", "100"]);
        editor.update_dimension(0, "");
        editor.update_dimension(0, "40");
        assert_eq!(&fields(&editor)[..2], &["40", "40"]);
        editor.change_image_sizing(ImageSizing::Inches).unwrap();
        assert_eq!(&fields(&editor)[..2], &["4", "2"]);
        editor.change_image_sizing(ImageSizing::Pixels).unwrap();
        assert_eq!(&fields(&editor)[..2], &["400", "200"]);
        assert_eq!(
            ImageSizing::Centimeters
                .dimensions([10.16, 5.08], [1000, 500], 100.)
                .unwrap(),
            [400, 200]
        );
        assert!(
            ImageSizing::Inches
                .dimensions([1000., 1000.], [1000, 500], 100.)
                .is_err()
        );
    }

    #[test]
    fn print_size_apply_changes_only_resolution_and_undo_restores_it() {
        let mut editor = Editor::with_test_document();
        let mut doc = Document::new(100, 50).unwrap();
        doc.resolution = 100.;
        compositor::edits::fill(&mut doc, [10, 20, 30, 128], false, false).unwrap();
        editor.tabs = vec![Session::new(doc.clone(), None).into()];
        editor.open_form(Action::ImageSize);
        editor
            .change_image_sizing(ImageSizing::PrintInches)
            .unwrap();
        editor.update_dimension(0, "2");
        assert_eq!(&fields(&editor)[..3], &["2", "1", "50"]);
        let (mut cx, view) = quickgui::Application::new()
            .into_test_context(
                quickgui::WindowOptions::new("Print size").size(1280., 900.),
                editor,
            )
            .unwrap();
        cx.click(view.window_handle(), "form-apply").unwrap();
        let changed = cx.read(view, |e| e.session().document.clone()).unwrap();
        assert_eq!(
            (changed.width, changed.height, changed.resolution),
            (100, 50, 50.)
        );
        assert_eq!(changed.layers, doc.layers);
        assert!(Arc::ptr_eq(
            changed.layers[0].raster().unwrap(),
            doc.layers[0].raster().unwrap()
        ));
        cx.update(view, |e, _| e.session_mut().undo()).unwrap();
        assert_eq!(
            cx.read(view, |e| e.session().document.clone()).unwrap(),
            doc
        );
    }
}
