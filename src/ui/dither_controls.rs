//! Filter panel controls for the ten upstream dither styles.
use super::{dropdown::Dropdown, parameter_controls::Scale, scalar_controls::Scalar, *};
use compositor::{
    filters::dither::{Characters, Colors, PixelShape, Settings, Style},
    invalid,
    palette::parse_hex,
};
use quickgui::{PickerItem, SelectPopoverLayout};

pub(super) struct Menus {
    style: Dropdown<Style>,
    colors: Dropdown<Colors>,
    shape: Dropdown<PixelShape>,
}
fn menu<T>(items: impl IntoIterator<Item = (&'static str, T)>) -> Dropdown<T> {
    Dropdown::new(
        items
            .into_iter()
            .map(|(label, value)| PickerItem::new(label, value).id(label)),
    )
    .expect("Dither choices have unique names")
    .with_layout(SelectPopoverLayout::new(236., 25.).trigger_height(26.))
}
impl Menus {
    pub fn new() -> Self {
        Self {
            style: menu(Style::ALL.map(|s| (s.label(), s))),
            colors: menu([
                ("Black & White", Colors::BlackWhite),
                ("Two Colors", Colors::TwoColors),
                ("Original", Colors::Original),
            ]),
            shape: menu([("Square", PixelShape::Square), ("Dot", PixelShape::Dot)]),
        }
    }
    pub fn sync(&mut self, settings: Settings) {
        self.style.select_id(settings.style.label());
        self.colors.select_id(color_label(settings.colors));
        self.shape.select_id(shape_label(settings.pixel_shape));
        self.style.dismiss();
        self.colors.dismiss();
        self.shape.dismiss();
    }
}
fn color_label(value: Colors) -> &'static str {
    match value {
        Colors::BlackWhite => "Black & White",
        Colors::TwoColors => "Two Colors",
        Colors::Original => "Original",
    }
}
fn shape_label(value: PixelShape) -> &'static str {
    match value {
        PixelShape::Square => "Square",
        PixelShape::Dot => "Dot",
    }
}
fn hex(rgb: [u8; 3]) -> String {
    let [r, g, b] = rgb;
    format!("#{r:02X}{g:02X}{b:02X}")
}
pub(super) fn fields(s: Settings) -> (&'static str, Vec<(&'static str, String)>) {
    (
        "Dither",
        vec![
            ("Style", s.style.label().into()),
            ("Pixel Size", s.pixel_size.to_string()),
            ("Text Size", s.text_size.to_string()),
            ("Cell Size", s.cell_size.to_string()),
            ("Angle", s.angle.to_string()),
            ("Characters", s.characters.text()),
            ("Tones", s.levels.to_string()),
            ("Diffusion", s.diffusion.to_string()),
            ("Density", s.density.to_string()),
            ("Contrast", s.contrast.to_string()),
            ("Colors", color_label(s.colors).into()),
            ("Dark", hex(s.dark)),
            ("Light", hex(s.light)),
            ("Pixel Shape", shape_label(s.pixel_shape).into()),
            ("Light on Dark", u8::from(s.light_on_dark).to_string()),
        ],
    )
}
pub(super) fn values(values: &[String]) -> Result<Settings> {
    let value = |i| {
        values
            .get(i)
            .map(String::as_str)
            .ok_or_else(|| invalid("Dither controls are incomplete. Close and reopen the filter."))
    };
    let number = |i| {
        value(i)?
            .parse::<f32>()
            .ok()
            .filter(|n| n.is_finite())
            .ok_or_else(|| invalid("Enter a finite number for each dither setting."))
    };
    let integer = |i| -> Result<u8> {
        let n = number(i)?;
        if !(0. ..=255.).contains(&n) || n.fract() != 0. {
            return Err(invalid(
                "Pixel, cell and text sizes and tones must be whole numbers.",
            ));
        }
        Ok(n as u8)
    };
    let rgb = |i| -> Result<[u8; 3]> {
        let [r, g, b, _] = parse_hex(value(i)?)?;
        Ok([r, g, b])
    };
    let style = Style::ALL
        .into_iter()
        .find(|s| s.label() == value(0).unwrap_or_default())
        .ok_or_else(|| invalid("Choose a dither style from the menu."))?;
    let settings = Settings {
        style,
        pixel_size: integer(1)?,
        text_size: integer(2)?,
        cell_size: integer(3)?,
        angle: number(4)?,
        characters: Characters::parse(value(5)?)?,
        levels: integer(6)?,
        diffusion: number(7)?,
        density: number(8)?,
        contrast: number(9)?,
        colors: match value(10)? {
            "Black & White" => Colors::BlackWhite,
            "Two Colors" => Colors::TwoColors,
            "Original" => Colors::Original,
            _ => return Err(invalid("Choose dither colors from the menu.")),
        },
        dark: rgb(11)?,
        light: rgb(12)?,
        pixel_shape: match value(13)? {
            "Square" => PixelShape::Square,
            "Dot" => PixelShape::Dot,
            _ => return Err(invalid("Choose a dither pixel shape from the menu.")),
        },
        light_on_dark: match value(14)? {
            "0" => false,
            "1" => true,
            _ => return Err(invalid("Choose Light on Dark using its checkbox.")),
        },
    };
    settings.validate()?;
    Ok(settings)
}
fn trigger(label: &'static str) -> Element {
    Editor::tool_header_control(label)
        .w(236.)
        .flex_row()
        .items_center()
        .child(div().flex_1())
        .child(Icon::PopupChevron.element(14.))
}
fn row(label: &'static str, child: Element) -> Element {
    div()
        .flex_row()
        .items_center()
        .gap(10.)
        .child(
            text(label)
                .text_size(13.)
                .line_height(16.)
                .w(84.)
                .flex_shrink_0(),
        )
        .child(child)
}
impl Editor {
    pub(super) fn dither_fields(
        &self,
        cx: &mut ViewContext<'_, Self>,
        fields: &[(&'static str, String)],
    ) -> Element {
        let values: Vec<_> = fields.iter().map(|(_, value)| value.clone()).collect();
        // Keep the panel mounted while the user types an incomplete numeric/color value.
        let mut s = super::dither_controls::values(&values).unwrap_or_default();
        if let Some(style) = Style::ALL
            .into_iter()
            .find(|style| values.first().is_some_and(|v| v == style.label()))
        {
            s.style = style;
        }
        s.colors = match values.get(10).map(String::as_str) {
            Some("Two Colors") => Colors::TwoColors,
            Some("Original") => Colors::Original,
            _ => Colors::BlackWhite,
        };
        s.pixel_size = values
            .get(1)
            .and_then(|v| v.parse().ok())
            .unwrap_or(s.pixel_size);
        let mut rows = div().flex_col().gap(12.).flex_shrink_0().child(row(
            "Style",
            self.dither_menus.style.element(
                cx,
                "dither-style",
                "Style",
                |e| &mut e.dither_menus.style,
                trigger(s.style.label()),
                |e, value, cx| {
                    e.update_form_field(0, value.label());
                    e.changed(cx);
                },
            ),
        ));
        let numeric = [
            (1, "Pixel Size", (1., 32.)),
            (2, "Text Size", (6., 64.)),
            (3, "Cell Size", (4., 64.)),
            (4, "Angle", (-90., 90.)),
            (6, "Tones", (2., 8.)),
            (7, "Diffusion", (0., 100.)),
            (8, "Density", (-100., 100.)),
            (9, "Contrast", (-100., 100.)),
        ];
        for (index, label, range) in numeric {
            let visible = match index {
                1 => s.style != Style::Ascii,
                2 => s.style == Style::Ascii,
                3 | 4 => s.style.halftone(),
                6 => s.style.has_tones(),
                7 => s.style.diffuses(),
                _ => true,
            };
            if !visible {
                continue;
            }
            let Some((_, value)) = fields.get(index) else {
                continue;
            };
            let id = match index {
                1 => "dither-pixel-size",
                2 => "dither-text-size",
                3 => "dither-cell-size",
                4 => "dither-angle",
                6 => "dither-tones",
                7 => "dither-diffusion",
                8 => "dither-density",
                _ => "dither-contrast",
            };
            rows = rows.child(row(
                label,
                div()
                    .flex_row()
                    .items_center()
                    .gap(8.)
                    .child(self.scalar_slider(
                        cx,
                        id,
                        label,
                        Scalar::Parameter(index, Scale::Linear(0)),
                        range,
                        172.,
                    ))
                    .child(self.form_number_input(cx, index, value, 0).w(56.)),
            ));
        }
        if s.style == Style::Ascii
            && let Some((_, value)) = fields.get(5)
        {
            rows = rows.child(row("Characters", self.form_input(cx, 5, value).w(236.)));
        }
        rows = rows.child(row(
            "Colors",
            self.dither_menus.colors.element(
                cx,
                "dither-colors",
                "Colors",
                |e| &mut e.dither_menus.colors,
                trigger(color_label(s.colors)),
                |e, value, cx| {
                    e.update_form_field(10, color_label(value));
                    e.changed(cx);
                },
            ),
        ));
        if s.colors == Colors::TwoColors {
            for (index, label, rgb) in [(11, "Dark", s.dark), (12, "Light", s.light)] {
                rows = rows.child(row(
                    label,
                    div()
                        .flex_row()
                        .items_center()
                        .gap(8.)
                        .child(
                            div()
                                .w(26.)
                                .h(26.)
                                .rounded(4.)
                                .bg(Color::rgb8(rgb[0], rgb[1], rgb[2]))
                                .border(1., Color::WHITE)
                                .on_click(cx.listener(
                                    format!("dither-color-{index}"),
                                    move |e, cx| {
                                        e.open_dither_color_picker(index);
                                        e.changed(cx);
                                    },
                                )),
                        )
                        .child(self.form_input(cx, index, &values[index]).w(202.)),
                ));
            }
        }
        if s.style != Style::Ascii && s.pixel_size > 1 {
            rows = rows.child(row(
                "Pixel Shape",
                self.dither_menus.shape.element(
                    cx,
                    "dither-pixel-shape",
                    "Pixel Shape",
                    |e| &mut e.dither_menus.shape,
                    trigger(shape_label(s.pixel_shape)),
                    |e, value, cx| {
                        e.update_form_field(13, shape_label(value));
                        e.changed(cx);
                    },
                ),
            ));
        }
        if !s.style.has_tones() {
            rows = rows.child(self.form_toggle(cx, 14, "Light on Dark", s.light_on_dark));
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickgui::{Application, WindowOptions};
    #[test]
    fn settings_roundtrip_and_reject_fractional_pixel_sizes() {
        for style in Style::ALL {
            let expected = Settings {
                style,
                characters: Characters::parse(" .@█").unwrap(),
                ..Default::default()
            };
            let draft: Vec<_> = fields(expected)
                .1
                .into_iter()
                .map(|(_, value)| value)
                .collect();
            assert_eq!(values(&draft).unwrap(), expected);
            let mut invalid = draft;
            invalid[1] = "1.5".into();
            assert!(values(&invalid).is_err());
        }
    }
    #[test]
    fn style_menu_exposes_ascii_and_cancel_preserves_document() {
        let mut editor = Editor::with_test_document();
        compositor::edits::fill(
            &mut editor.session_mut().document,
            [120, 160, 80, 255],
            false,
            false,
        )
        .unwrap();
        let original = editor.session().document.clone();
        editor.open_dither().unwrap();
        let (mut cx, view) = Application::new()
            .into_test_context(WindowOptions::new("Dither").size(1500., 1000.), editor)
            .unwrap();
        let window = view.window_handle();
        assert!(cx.element_bounds(window, "dither-pixel-size").is_ok());
        cx.focus(window, "dither-style").unwrap();
        cx.simulate_keystrokes(window, "enter end enter").unwrap();
        cx.read(view, |editor| {
            let Some(Form::Edit { fields, .. }) = &editor.modal else {
                panic!("Dither panel closed");
            };
            assert_eq!(fields[0].1, "ASCII");
        })
        .unwrap();
        assert!(cx.element_bounds(window, "dither-text-size").is_ok());
        assert!(cx.element_bounds(window, "dither-pixel-size").is_err());
        cx.click(window, "form-cancel").unwrap();
        assert_eq!(
            cx.read(view, |editor| editor.session().document.clone())
                .unwrap(),
            original
        );
    }
    #[test]
    fn applying_dither_preserves_later_layer_properties() {
        let mut editor = Editor::with_test_document();
        compositor::edits::fill(
            &mut editor.session_mut().document,
            [120, 120, 120, 255],
            false,
            false,
        )
        .unwrap();
        editor.open_dither().unwrap();
        let draft: Vec<_> = fields(Settings::default())
            .1
            .into_iter()
            .map(|(_, value)| value)
            .collect();
        editor.apply_form(Action::Dither, draft).unwrap();
        let mut latest = editor.session().document.clone();
        latest.layers[0].opacity = 0.25;
        latest.layers[0].name = "Renamed while previewing".into();
        let result = editor.job.take().unwrap().run(latest).unwrap();
        assert_eq!(result.layers[0].opacity, 0.25);
        assert_eq!(result.layers[0].name, "Renamed while previewing");
        assert!(
            result.layers[0]
                .raster()
                .unwrap()
                .pixels()
                .any(|p| p[0] != 120)
        );
    }
}
