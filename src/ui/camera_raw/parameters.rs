use super::*;
use crate::ui::{parameter_controls::Scale, scalar_controls::Scalar};
use compositor::camera_raw::scalars;
impl Edit {
    fn parameter(&self, index: usize) -> Option<(&'static str, f64, f64)> {
        let fields = match self.group {
            Group::Light => scalars::Light::ranges(),
            Group::Color => scalars::Color::ranges(),
            Group::Effects => scalars::Effects::ranges(),
            Group::Curve => scalars::Curve::ranges(),
            Group::Detail => scalars::Detail::ranges(),
            Group::Optics => scalars::Optics::ranges(),
            Group::Calibration => scalars::Calibration::ranges(),
            Group::Geometry => {
                let mut v = scalars::Geometry::ranges();
                for _ in &self.settings.guides {
                    v.extend([
                        ("Guide start X", 0., 1.),
                        ("Guide start Y", 0., 1.),
                        ("Guide end X", 0., 1.),
                        ("Guide end Y", 0., 1.),
                    ]);
                }
                v
            }
            Group::Mixer => {
                if self.mixer_page == MixerPage::Families {
                    vec![
                        ("Hue", -100., 100.),
                        ("Saturation", -100., 100.),
                        ("Luminance", -100., 100.),
                    ]
                } else {
                    vec![
                        ("Hue", 0., 360.),
                        ("Saturation", 0., 1.),
                        ("Luminance", 0., 1.),
                        ("Hue shift", -100., 100.),
                        ("Saturation shift", -100., 100.),
                        ("Luminance shift", -100., 100.),
                        ("Hue range", 5., 180.),
                        ("Saturation range", 0.05, 1.),
                        ("Luminance range", 0.05, 1.),
                    ]
                }
            }
            Group::Grading => vec![
                ("Hue", 0., 360.),
                ("Saturation", 0., 100.),
                ("Luminance", -100., 100.),
                ("Blending", 0., 100.),
                ("Balance", -100., 100.),
            ],
        };
        fields.get(index).copied()
    }
}
impl Editor {
    pub(in crate::ui) fn camera_parameter_control(
        &self,
        cx: &mut ViewContext<'_, Self>,
        index: usize,
        value: &str,
    ) -> Option<Element> {
        let (label, min, max) = self.camera_raw.parameter(index)?;
        let decimals = if max <= 5. { 2 } else { 0 };
        let ids = [
            "camera-param-0",
            "camera-param-1",
            "camera-param-2",
            "camera-param-3",
            "camera-param-4",
            "camera-param-5",
            "camera-param-6",
            "camera-param-7",
            "camera-param-8",
            "camera-param-9",
            "camera-param-10",
            "camera-param-11",
            "camera-param-12",
            "camera-param-13",
            "camera-param-14",
            "camera-param-15",
            "camera-param-16",
            "camera-param-17",
            "camera-param-18",
            "camera-param-19",
            "camera-param-20",
            "camera-param-21",
            "camera-param-22",
        ];
        Some(
            div()
                .flex_row()
                .items_center()
                .gap(8.)
                .child(
                    text(label)
                        .text_size(12.)
                        .line_height(15.)
                        .w(128.)
                        .flex_shrink_0()
                        .wrap(),
                )
                .child(self.scalar_slider(
                    cx,
                    *ids.get(index)?,
                    label,
                    Scalar::Parameter(index, Scale::Linear(decimals)),
                    (min, max),
                    120.,
                ))
                .child(
                    self.form_number_input(cx, index, value, usize::from(decimals))
                        .w(56.),
                ),
        )
    }
}
