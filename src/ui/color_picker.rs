use super::*;
use compositor::{
    adjustment::{Color as AdjustmentColor, GradientMap},
    palette::{Hsb, parse_hex},
};
use quickgui::{MouseButton, PointerEvent, PointerPhase};

#[derive(Clone, Copy)]
enum Target {
    Foreground,
    Background,
}
impl Target {
    fn index(self) -> usize {
        match self {
            Self::Foreground => 0,
            Self::Background => 1,
        }
    }
}

#[derive(Clone)]
struct Draft {
    hsb: Hsb,
    hex_draft: Option<String>,
    rgb: [String; 3],
}
impl Draft {
    fn new(rgb: [u8; 4]) -> Self {
        let mut draft = Self {
            hsb: Hsb::new(rgb),
            hex_draft: None,
            rgb: Default::default(),
        };
        draft.sync();
        draft
    }
    fn sync(&mut self) {
        let rgb = self.hsb.rgb();
        self.rgb = [rgb[0].to_string(), rgb[1].to_string(), rgb[2].to_string()];
    }

    fn hex(&self) -> String {
        let [r, g, b, _] = self.hsb.rgb();
        format!("{r:02X}{g:02X}{b:02X}")
    }
}

#[derive(Clone)]
enum Purpose {
    Palette,
    LayerEffect { form: Box<Form> },
    LayerText { form: Box<Form> },
    ForegroundText { form: Box<Form> },
    GradientMap { original: GradientMap },
    CanvasExtension { form: Box<Form> },
    JpegBackground { form: Box<Form> },
}

#[derive(Clone)]
pub(super) struct Picker {
    purpose: Purpose,
    colors: [Draft; 2],
    target: Target,
    text_preview: Option<(uuid::Uuid, Document)>,
}
impl Picker {
    pub(super) fn new(foreground: [u8; 4], background: [u8; 4]) -> Self {
        Self {
            purpose: Purpose::Palette,
            colors: [Draft::new(foreground), Draft::new(background)],
            target: Target::Foreground,
            text_preview: None,
        }
    }
    pub(super) fn select_background(&mut self) {
        self.target = Target::Background;
    }
    fn current(&self) -> &Draft {
        &self.colors[self.target.index()]
    }
    fn current_mut(&mut self) -> &mut Draft {
        &mut self.colors[self.target.index()]
    }
}

impl Editor {
    pub(super) fn open_text_color_picker(&mut self) {
        let Some(form @ Form::Text(_)) = self.modal.clone() else {
            return;
        };
        let style = match &form {
            Form::Text(draft) => draft.parsed().unwrap_or_else(|_| draft.style.clone()),
            _ => return,
        };
        let rgb = [
            (style.red * 255.).round() as u8,
            (style.green * 255.).round() as u8,
            (style.blue * 255.).round() as u8,
            255,
        ];
        let mut picker = Picker::new(rgb, [255; 4]);
        picker.purpose = Purpose::LayerText {
            form: Box::new(form),
        };
        self.modal = Some(Form::Color(Box::new(picker)));
    }
    pub(super) fn open_foreground_text_picker(&mut self) {
        self.open_text_color_picker();
        let foreground = self.tools.brush.color;
        if let Some(picker) = self.picker_mut()
            && let Purpose::LayerText { form } = &picker.purpose
        {
            picker.purpose = Purpose::ForegroundText { form: form.clone() };
            picker.colors[0] = Draft::new(foreground);
        }
    }

    pub(super) fn color_text_preview(&self) -> Option<&Document> {
        let Some(Form::Color(picker)) = &self.modal else {
            return None;
        };
        picker
            .text_preview
            .as_ref()
            .filter(|(session, _)| *session == self.session().id)
            .map(|(_, document)| document)
    }

    pub(super) fn open_effect_color_picker(&mut self) {
        let Some(form @ Form::Effects(_)) = self.modal.clone() else {
            return;
        };
        let rgb = match &form {
            Form::Effects(edit) => edit.rgb(),
            _ => return,
        };
        let mut picker = Picker::new(rgb, [255; 4]);
        picker.purpose = Purpose::LayerEffect {
            form: Box::new(form),
        };
        self.modal = Some(Form::Color(Box::new(picker)));
    }
    pub(super) fn open_jpeg_background_picker(&mut self) {
        let Some(
            form @ Form::Edit {
                action: Action::ExportJpeg,
                ..
            },
        ) = self.modal.clone()
        else {
            return;
        };
        let Some([r, g, b]) = self.jpeg_background() else {
            return;
        };
        let mut picker = Picker::new([r, g, b, 255], [255; 4]);
        picker.purpose = Purpose::JpegBackground {
            form: Box::new(form),
        };
        self.modal = Some(Form::Color(Box::new(picker)));
    }
    pub(super) fn open_canvas_extension_picker(&mut self) {
        let Some(
            form @ Form::Edit {
                action: Action::CanvasSize,
                ..
            },
        ) = self.modal.clone()
        else {
            return;
        };
        let color = match &form {
            Form::Edit { fields, .. } => fields
                .get(6)
                .and_then(|f| parse_hex(&f.1).ok())
                .unwrap_or([255; 4]),
            _ => [255; 4],
        };
        let mut picker = Picker::new(color, [255; 4]);
        picker.purpose = Purpose::CanvasExtension {
            form: Box::new(form),
        };
        self.modal = Some(Form::Color(Box::new(picker)));
    }
    pub(super) fn picking_color(&self) -> bool {
        matches!(self.modal, Some(Form::Color(_)))
    }

    fn picker_mut(&mut self) -> Option<&mut Picker> {
        match &mut self.modal {
            Some(Form::Color(picker)) => Some(picker),
            _ => None,
        }
    }

    pub(super) fn open_gradient_map_picker(&mut self) -> Result<()> {
        self.preview_adjustment()?;
        let Some(edit) = &self.adjustment_edit else {
            return Ok(());
        };
        if edit.settings.kind != Kind::GradientMap {
            return Ok(());
        }
        let map = edit.settings.gradient_map_settings.unwrap_or_default();
        let rgb = |c: AdjustmentColor| {
            [
                (c.red * 255.).round() as u8,
                (c.green * 255.).round() as u8,
                (c.blue * 255.).round() as u8,
                255,
            ]
        };
        let mut picker = Picker::new(rgb(map.shadows), rgb(map.highlights));
        picker.purpose = Purpose::GradientMap { original: map };
        self.modal = Some(Form::Color(Box::new(picker)));
        Ok(())
    }

    pub(super) fn preview_picker(&mut self) -> Result<()> {
        let Some(Form::Color(picker)) = &self.modal else {
            return Ok(());
        };
        if let Purpose::LayerText { form } | Purpose::ForegroundText { form } = &picker.purpose {
            let Form::Text(draft) = form.as_ref() else {
                return Ok(());
            };
            let mut draft = draft.clone();
            let [r, g, b, _] = picker.colors[0].hsb.rgb();
            draft.color = format!("#{r:02X}{g:02X}{b:02X}");
            let document = self.text_document_preview(&draft)?;
            let session = self.session().id;
            if let Some(picker) = self.picker_mut() {
                picker.text_preview = Some((session, document));
            }
            return Ok(());
        }
        if let Purpose::LayerEffect { form } = &picker.purpose {
            let original_form = form.clone();
            let color = picker.colors[0].hsb.rgb();
            let picker_form = self.modal.take();
            self.modal = Some(*original_form);
            self.change_effect(|edit| edit.set_color(color));
            self.modal = picker_form;
            return Ok(());
        }
        let Purpose::GradientMap { original } = &picker.purpose else {
            return Ok(());
        };
        let Some(edit) = &self.adjustment_edit else {
            return Ok(());
        };
        let mut settings = edit.settings.clone();
        let color = |draft: &Draft| {
            let [r, g, b, _] = draft.hsb.rgb();
            AdjustmentColor {
                red: f64::from(r) / 255.,
                green: f64::from(g) / 255.,
                blue: f64::from(b) / 255.,
            }
        };
        settings.gradient_map_settings = Some(GradientMap {
            shadows: color(&picker.colors[0]),
            highlights: color(&picker.colors[1]),
            ..*original
        });
        self.preview_adjustment_settings(settings)
    }

    pub(super) fn finish_color(&mut self, apply: bool) -> Result<()> {
        self.sample_ring = None;
        if apply {
            self.commit_picker_hex();
        }
        let Some(Form::Color(picker)) = &self.modal else {
            return Ok(());
        };
        let foreground_text = matches!(picker.purpose, Purpose::ForegroundText { .. });
        let chosen = picker.colors[0].hsb.rgb();
        match picker.purpose.clone() {
            Purpose::LayerText { mut form } | Purpose::ForegroundText { mut form } => {
                if apply && let Form::Text(draft) = form.as_mut() {
                    let [r, g, b, _] = picker.colors[0].hsb.rgb();
                    draft.color = format!("#{r:02X}{g:02X}{b:02X}");
                }
                if apply && foreground_text {
                    self.tools.brush.color = chosen;
                }
                self.modal = Some(*form);
            }
            Purpose::LayerEffect { form } => {
                let color = picker.colors[0].hsb.rgb();
                self.modal = Some(*form);
                self.change_effect(|edit| {
                    if apply {
                        edit.set_color(color);
                    }
                });
            }
            Purpose::JpegBackground { form } => {
                let [r, g, b, _] = picker.colors[0].hsb.rgb();
                self.modal = Some(*form);
                if apply {
                    self.set_jpeg_background([r, g, b]);
                }
            }
            Purpose::CanvasExtension { mut form } => {
                if apply
                    && let Form::Edit { fields, .. } = form.as_mut()
                    && let Some(field) = fields.get_mut(6)
                {
                    let [r, g, b, _] = picker.colors[0].hsb.rgb();
                    field.1 = format!("#{r:02X}{g:02X}{b:02X}");
                }
                self.modal = Some(*form);
            }
            Purpose::Palette => {
                if apply {
                    self.tools.brush.color = picker.colors[0].hsb.rgb();
                    self.tools.background = picker.colors[1].hsb.rgb();
                }
                self.modal = None;
                if apply {
                    self.refresh_gradient()?;
                }
            }
            Purpose::GradientMap { original } => {
                if apply {
                    self.preview_picker()?;
                } else if let Some(edit) = &self.adjustment_edit {
                    let mut settings = edit.settings.clone();
                    settings.gradient_map_settings = Some(original);
                    self.preview_adjustment_settings(settings)?;
                }
                self.show_adjustment_fields();
            }
        }
        Ok(())
    }

    pub(super) fn sample_picker(&mut self, event: &PointerEvent) {
        let Some(Form::Color(picker)) = &self.modal else {
            return;
        };
        let original = picker.current().hsb.rgb();
        self.sample_picker_at(event);
        if let Some(Form::Color(picker)) = &self.modal {
            let sampled = picker.current().hsb.rgb();
            self.update_sample_ring(event, original, sampled);
        }
    }

    fn sample_picker_at(&mut self, event: &PointerEvent) {
        if !matches!(event.phase, PointerPhase::Down | PointerPhase::Move)
            || event.button != MouseButton::Left
        {
            return;
        }
        self.commit_picker_hex();
        let (zoom, offset) = self.viewport(event.size.width, event.size.height);
        let point = [
            (event.local_position.x as f64 - offset[0]) / zoom,
            (event.local_position.y as f64 - offset[1]) / zoom,
        ];
        let doc = &self.session().document;
        if point
            .iter()
            .zip([doc.width, doc.height])
            .any(|(v, limit)| !v.is_finite() || *v < 0. || *v >= limit as f64)
        {
            return;
        }
        let pixel = match compositor::render::sample(doc, point.map(|v| v.floor() + 0.5)) {
            Ok(pixel) => pixel,
            Err(error) => {
                self.show_error(alerts::Operation::Paint, error.to_string());
                return;
            }
        };
        if pixel[3] <= 0. {
            return;
        }
        if let Some(picker) = self.picker_mut() {
            let draft = picker.current_mut();
            draft.hsb.set_rgb(pixel.map(|v| (v * 255.).round() as u8));
            draft.sync();
        }
    }

    fn picker_pointer(&mut self, event: &PointerEvent, hue: bool) {
        if event.button != MouseButton::Left
            || event.phase == PointerPhase::Cancel
            || event.size.width <= 0.
            || event.size.height <= 0.
            || !event.local_position.x.is_finite()
            || !event.local_position.y.is_finite()
        {
            return;
        }
        self.commit_picker_hex();
        if let Some(picker) = self.picker_mut() {
            let draft = picker.current_mut();
            if hue {
                draft.hsb.hue =
                    (1. - (event.local_position.y / event.size.height).clamp(0., 1.)) as f64 * 360.;
            } else {
                draft.hsb.saturation =
                    (event.local_position.x / event.size.width).clamp(0., 1.) as f64;
                draft.hsb.brightness =
                    (1. - (event.local_position.y / event.size.height).clamp(0., 1.)) as f64;
            }
            draft.sync();
        }
    }
}

mod inputs;
pub(super) mod spectrum;
mod view;

#[cfg(test)]
mod text_tests;
