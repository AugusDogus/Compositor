//! Editable type metadata compatible with Compositor's LayerTextStyle.
//! Glyph shaping and rasterization use the same Cosmic Text engine as QuickGUI.
use crate::{
    Result,
    document::{Layer, LayerContent, validate_size},
    geometry::Point,
    invalid,
};
use cosmic_text::{
    Align, Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache, Wrap,
};
use image::{Pixel, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const PADDING: f64 = 12.;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Alignment {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Text {
    pub content: String,
    pub font_name: String,
    pub font_size: f64,
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub alignment: Alignment,
    pub tracking: f64,
    pub leading: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub box_size: Option<Point>,
}
impl Default for Text {
    fn default() -> Self {
        Self {
            content: "Text".into(),
            font_name: "Inter Variable".into(),
            font_size: 72.,
            red: 0.,
            green: 0.,
            blue: 0.,
            alignment: Alignment::Left,
            tracking: 0.,
            leading: 0.,
            box_size: None,
        }
    }
}
impl Text {
    pub fn validate(&self) -> Result<()> {
        if self.content.encode_utf16().count() > 100_000
            || self.font_name.trim().is_empty()
            || self.font_name.len() > 1024
            || !(1. ..=2000.).contains(&self.font_size)
            || ![self.red, self.green, self.blue]
                .iter()
                .all(|v| (0. ..=1.).contains(v))
            || !(-100. ..=1000.).contains(&self.tracking)
            || !(0. ..=5000.).contains(&self.leading)
            || self.box_size.is_some_and(|s| {
                !s.iter().all(|v| (16. ..=30_000.).contains(v)) || s[0] * s[1] > 100_000_000.
            })
        {
            return Err(invalid(
                "Text settings exceed supported bounds. Use up to 100,000 characters, 1–2000 px type, and a box within 30,000 px and 100 megapixels.",
            ));
        }
        Ok(())
    }
    pub fn line_height(&self) -> f64 {
        if self.leading > 0. {
            self.leading
        } else {
            self.font_size * 1.2
        }
    }
    pub fn layer_name(&self) -> String {
        let name: String = self
            .content
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(40)
            .collect();
        if name.is_empty() { "Text".into() } else { name }
    }
}

pub struct TextRenderer {
    fonts: FontSystem,
    families: Vec<String>,
    font_names: Vec<String>,
}
impl Default for TextRenderer {
    fn default() -> Self {
        let fonts = FontSystem::new_with_fonts([cosmic_text::fontdb::Source::Binary(Arc::new(
            include_bytes!("../assets/fonts/InterVariable.ttf").to_vec(),
        ))]);
        let mut families: Vec<_> = fonts
            .db()
            .faces()
            .flat_map(|f| f.families.iter().map(|(name, _)| name.clone()))
            .collect();
        families.sort_by_key(|s| s.to_lowercase());
        families.dedup();
        let mut font_names = families.clone();
        font_names.extend(fonts.db().faces().map(|face| face.post_script_name.clone()));
        font_names.retain(|name| !name.is_empty());
        font_names.sort_by_key(|name| name.to_lowercase());
        font_names.dedup();
        Self {
            fonts,
            families,
            font_names,
        }
    }
}
impl TextRenderer {
    pub fn font_names(&self) -> &[String] {
        &self.font_names
    }
    pub fn families(&self) -> &[String] {
        &self.families
    }
    pub fn render(&mut self, style: &Text) -> Result<RgbaImage> {
        self.render_with_baseline(style).map(|(pixels, _)| pixels)
    }
    pub(crate) fn render_with_baseline(&mut self, style: &Text) -> Result<(RgbaImage, f64)> {
        style.validate()?;
        // Limit layout before shaping a huge unbroken string into an oversized asset.
        let mut buffer = Buffer::new(
            &mut self.fonts,
            Metrics::new(style.font_size as f32, style.line_height() as f32),
        );
        let family = self
            .families
            .iter()
            .find(|name| name.eq_ignore_ascii_case(&style.font_name))
            .map_or("Inter Variable", String::as_str);
        // macOS stores a face's PostScript name. Resolve it when present on Linux.
        let face = self
            .fonts
            .db()
            .faces()
            .find(|f| f.post_script_name.eq_ignore_ascii_case(&style.font_name))
            .map(|f| {
                (
                    f.families
                        .first()
                        .map(|(n, _)| n.clone())
                        .unwrap_or_else(|| family.into()),
                    f.weight,
                    f.style,
                )
            });
        let attrs = match &face {
            Some((name, weight, face_style)) => Attrs::new()
                .family(Family::Name(name))
                .weight(*weight)
                .style(*face_style),
            None => Attrs::new().family(Family::Name(family)),
        }
        .letter_spacing((style.tracking / style.font_size) as f32);
        let width = style.box_size.map(|s| (s[0] - 2. * PADDING).max(1.) as f32);
        buffer.set_wrap(if width.is_some() {
            Wrap::WordOrGlyph
        } else {
            Wrap::None
        });
        buffer.set_size(width, Some(30_001.));
        buffer.set_text(
            &style.content,
            &attrs,
            Shaping::Advanced,
            Some(match style.alignment {
                Alignment::Left => Align::Left,
                Alignment::Center => Align::Center,
                Alignment::Right => Align::Right,
            }),
        );
        buffer.shape_until_scroll(&mut self.fonts, false);
        let mut measured = [style.font_size * 0.1, style.line_height()];
        for run in buffer.layout_runs() {
            measured[0] = measured[0].max(f64::from(run.line_w));
            measured[1] = measured[1].max(f64::from(run.line_top + run.line_height));
        }
        let size = style
            .box_size
            .unwrap_or([measured[0] + PADDING * 2., measured[1] + PADDING * 2.]);
        let (width, height) = (
            size[0].ceil().max(16.) as u32,
            size[1].ceil().max(16.) as u32,
        );
        validate_size(width, height)?;
        if style.box_size.is_none() && measured[1] > 30_000. {
            return Err(invalid(
                "Text is too tall to rasterize. Shorten the text or reduce its size.",
            ));
        }
        buffer.set_size(
            Some((f64::from(width) - PADDING * 2.).max(1.) as f32),
            Some((f64::from(height) - PADDING * 2.).max(1.) as f32),
        );
        let baseline = buffer
            .layout_runs()
            .next()
            .map_or(PADDING + style.font_size * 0.8, |run| {
                PADDING + f64::from(run.line_y)
            });
        let mut pixels = RgbaImage::new(width, height);
        let mut cache = SwashCache::new();
        let color = Color::rgb(
            (style.red * 255.).round() as u8,
            (style.green * 255.).round() as u8,
            (style.blue * 255.).round() as u8,
        );
        buffer.draw(&mut self.fonts, &mut cache, color, |x, y, w, h, color| {
            for py in y..y + h as i32 {
                for px in x..x + w as i32 {
                    let (px, py) = (px + PADDING as i32, py + PADDING as i32);
                    if let Some(pixel) = pixels.get_pixel_mut_checked(px as u32, py as u32) {
                        pixel.blend(&Rgba(color.as_rgba()));
                    }
                }
            }
        });
        Ok((pixels, baseline))
    }
}

/// Fill changes the letters' color while retaining editable text and their placement.
pub fn recolor_layer(layer: &mut Layer, color: [u8; 4]) -> Result<()> {
    let mut style = layer
        .text
        .clone()
        .ok_or_else(|| invalid("Select an editable text layer."))?;
    let [red, green, blue, _] = color;
    style.red = f64::from(red) / 255.;
    style.green = f64::from(green) / 255.;
    style.blue = f64::from(blue) / 255.;
    if layer.text.as_ref() == Some(&style) {
        return Ok(());
    }
    let pixels = TextRenderer::default().render(&style)?;
    layer.content = LayerContent::Raster(Some(Arc::new(pixels)));
    layer.text = Some(style);
    Ok(())
}

pub fn new_layer(style: Text, pixels: RgbaImage, origin: Point) -> Result<Layer> {
    style.validate()?;
    validate_size(pixels.width(), pixels.height())?;
    let mut layer = Layer::blank(style.layer_name(), pixels.width(), pixels.height());
    layer.transform.origin = origin;
    if !layer.transform.valid() {
        return Err(invalid(
            "Text position exceeds supported canvas coordinates.",
        ));
    }
    layer.content = LayerContent::Raster(Some(Arc::new(pixels)));
    layer.text = Some(style);
    Ok(layer)
}

/// Preserve transformed upper-left corner, scale, flips, and mask placement on edits.
pub fn update_layer(layer: &mut Layer, style: Text, pixels: RgbaImage) -> Result<()> {
    style.validate()?;
    validate_size(pixels.width(), pixels.height())?;
    let old_style = layer
        .text
        .as_ref()
        .ok_or_else(|| invalid("Select an editable text layer."))?;
    let old = layer
        .raster()
        .ok_or_else(|| invalid("The text layer has no cached image."))?;
    let mut transform = layer.transform;
    let anchor = transform.point([0., 0.]);
    transform.size[0] *= f64::from(pixels.width()) / f64::from(old.width());
    transform.size[1] *= f64::from(pixels.height()) / f64::from(old.height());
    let moved = transform.point([0., 0.]);
    transform.origin[0] += anchor[0] - moved[0];
    transform.origin[1] += anchor[1] - moved[1];
    if !transform.valid() {
        return Err(invalid(
            "The edited text exceeds supported transform bounds.",
        ));
    }
    if layer.name == old_style.layer_name() {
        layer.name = style.layer_name();
    }
    if let Some(mask) = &mut layer.mask {
        mask.placement.get_or_insert(layer.transform);
    }
    layer.transform = transform;
    layer.content = LayerContent::Raster(Some(Arc::new(pixels)));
    layer.text = Some(style);
    Ok(())
}

#[cfg(test)]
mod tests;
