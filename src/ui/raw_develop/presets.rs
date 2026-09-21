//! Built-in starting points and local curve/crop resets for RAW development.
use super::*;

impl Editor {
    pub(super) fn raw_builtin_presets(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let mut buttons = div().flex_row().gap(4.).flex_wrap();
        for (id, label, preset) in [
            ("raw-preset-natural", "Natural", DevelopSettings::default()),
            (
                "raw-preset-landscape",
                "Landscape",
                DevelopSettings {
                    contrast: 12.,
                    highlights: -30.,
                    shadows: 20.,
                    vibrance: 20.,
                    clarity: 12.,
                    ..Default::default()
                },
            ),
            (
                "raw-preset-monochrome",
                "Black & white",
                DevelopSettings {
                    monochrome: true,
                    contrast: 18.,
                    ..Default::default()
                },
            ),
        ] {
            buttons = buttons.child(self.raw_button(cx, id, label, move |d| {
                d.edit(|settings| *settings = preset.clone());
                d.selected_mask = None;
                d.draw_mask = false;
            }));
        }
        buttons
    }

    pub(super) fn raw_curve_presets(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        let mut buttons = div().flex_row().gap(4.).flex_wrap();
        for (id, label, curve) in [
            ("raw-curve-linear", "Linear", [0., 0.25, 0.5, 0.75, 1.]),
            ("raw-curve-s", "S curve", [0., 0.18, 0.5, 0.82, 1.]),
            ("raw-curve-lift", "Lift blacks", [0.08, 0.28, 0.5, 0.75, 1.]),
        ] {
            buttons = buttons.child(self.raw_button(cx, id, label, move |d| {
                let channel = d.curve_channel;
                d.edit(|settings| settings.curves[channel] = curve);
            }));
        }
        buttons
    }

    pub(super) fn raw_crop_presets(&self, cx: &mut ViewContext<'_, Self>) -> Element {
        div()
            .flex_row()
            .gap(4.)
            .child(self.raw_button(cx, "raw-uncrop", "Uncrop", |d| {
                d.edit(|settings| settings.crop = [0., 0., 1., 1.]);
            }))
            .child(self.raw_button(cx, "raw-square", "Square", |d| {
                if let Some(ready) = &d.ready {
                    let aspect =
                        ready.full.camera.width() as f32 / ready.full.camera.height() as f32;
                    d.edit(|settings| settings.crop = square_crop(aspect));
                }
            }))
    }
}

fn square_crop(aspect: f32) -> [f32; 4] {
    if aspect >= 1. {
        let margin = (1. - 1. / aspect) * 0.5;
        [margin, 0., 1. - margin, 1.]
    } else {
        let margin = (1. - aspect) * 0.5;
        [0., margin, 1., 1. - margin]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_crop_uses_centered_sensor_aspect_in_both_orientations() {
        for aspect in [0.5, 1., 2.] {
            let [left, top, right, bottom] = square_crop(aspect);
            assert!(((right - left) * aspect - (bottom - top)).abs() < 1e-6);
            assert!((left + right - 1.).abs() < 1e-6);
            assert!((top + bottom - 1.).abs() < 1e-6);
        }
    }
}
