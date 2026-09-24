use crate::{Result, blend::Blend, geometry::Transform, invalid, selection::Selection};
use image::{GrayImage, RgbaImage};
use std::{collections::HashSet, sync::Arc};
use uuid::Uuid;

mod limits;
pub use limits::{MAX_SURFACE_PIXELS, document_pixel_budget};

/// Canvas geometry is sparse. Only materialized pixel assets share the raster budget.
pub fn validate_canvas_size(width: u32, height: u32) -> Result<()> {
    if !(1..=30_000).contains(&width) || !(1..=30_000).contains(&height) {
        return Err(invalid(
            "Canvas dimensions must be 1 to 30,000 pixels per side.",
        ));
    }
    Ok(())
}

pub fn validate_size(width: u32, height: u32) -> Result<()> {
    if width == 0
        || height == 0
        || width > 30_000
        || height > 30_000
        || u64::from(width) * u64::from(height) > MAX_SURFACE_PIXELS
    {
        return Err(invalid(
            "Dimensions must be 1 to 30,000 pixels per side and at most 200 million pixels total.",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mask {
    pub pixels: Arc<GrayImage>,
    pub enabled: bool,
    pub linked: bool,
    pub placement: Option<Transform>,
}

impl Mask {
    pub fn background(&self) -> f64 {
        if self.edge_tone() >= 0.5 { 1. } else { 0. }
    }

    /// Mean tone of the outermost pixels, used by the canvas-framed mask thumbnail.
    pub fn edge_tone(&self) -> f64 {
        let (w, h) = self.pixels.dimensions();
        if w == 0 || h == 0 {
            return 1.;
        }
        let mut total = 0_u64;
        let mut count = 0_u64;
        for x in 0..w {
            total += u64::from(self.pixels[(x, 0)][0]);
            count += 1;
            if h > 1 {
                total += u64::from(self.pixels[(x, h - 1)][0]);
                count += 1;
            }
        }
        for y in 1..h.saturating_sub(1) {
            total += u64::from(self.pixels[(0, y)][0]);
            count += 1;
            if w > 1 {
                total += u64::from(self.pixels[(w - 1, y)][0]);
                count += 1;
            }
        }
        total as f64 / (count * 255) as f64
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum LayerContent {
    Raster(Option<Arc<RgbaImage>>),
    Group,
    Adjustment(Box<crate::adjustment::Adjustment>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub id: Uuid,
    pub name: String,
    pub visible: bool,
    pub parent: Option<Uuid>,
    pub content: LayerContent,
    pub transform: Transform,
    pub opacity: f64,
    pub blend: Blend,
    pub mask: Option<Mask>,
    pub clip_source: Option<Uuid>,
    pub shape: Option<Shape>,
    pub text: Option<crate::text::Text>,
    pub raw: Option<Arc<crate::raw::RawAsset>>,
    pub effects: Option<crate::effects::LayerEffects>,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ShapeKind {
    Rectangle,
    Ellipse,
    Line,
}

impl ShapeKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Rectangle => "Rectangle",
            Self::Ellipse => "Ellipse",
            Self::Line => "Line",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::Rectangle => Self::Ellipse,
            Self::Ellipse => Self::Line,
            Self::Line => Self::Rectangle,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all_fields = "camelCase")]
pub enum ShapeGeometry {
    Rectangle,
    Ellipse,
    Line {
        line_width: f64,
        start: [f64; 2],
        end: [f64; 2],
    },
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shape {
    #[serde(flatten)]
    pub geometry: ShapeGeometry,
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub corner_radius: f64,
}

impl Shape {
    pub fn kind(self) -> ShapeKind {
        match self.geometry {
            ShapeGeometry::Rectangle => ShapeKind::Rectangle,
            ShapeGeometry::Ellipse => ShapeKind::Ellipse,
            ShapeGeometry::Line { .. } => ShapeKind::Line,
        }
    }
    pub fn valid(self) -> bool {
        [self.red, self.green, self.blue]
            .iter()
            .all(|v| (0. ..=1.).contains(v))
            && self.corner_radius.is_finite()
            && self.corner_radius >= 0.
            && match self.geometry {
                ShapeGeometry::Line {
                    line_width,
                    start,
                    end,
                } => {
                    (1. ..=5000.).contains(&line_width)
                        && start.iter().chain(&end).all(|v| (0. ..=1.).contains(v))
                }
                _ => true,
            }
    }
}

impl Layer {
    pub fn blank(name: impl Into<String>, width: u32, height: u32) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            visible: true,
            parent: None,
            content: LayerContent::Raster(None),
            transform: Transform::new(width, height),
            opacity: 1.,
            blend: Blend::Normal,
            mask: None,
            clip_source: None,
            shape: None,
            text: None,
            raw: None,
            effects: None,
        }
    }
    pub fn raster(&self) -> Option<&Arc<RgbaImage>> {
        match &self.content {
            LayerContent::Raster(pixels) => pixels.as_ref(),
            _ => None,
        }
    }
    pub fn is_group(&self) -> bool {
        matches!(self.content, LayerContent::Group)
    }
    /// Pixel edits require explicitly discarding the editable camera source first.
    pub fn require_rasterized(&self) -> Result<()> {
        if self.raw.is_some() {
            return Err(invalid(
                "This is an editable RAW layer. Choose Layer > Rasterize RAW Layer before editing its pixels, or paint on a separate layer. The RAW source and current image are unchanged.",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub id: Uuid,
    pub width: u32,
    pub height: u32,
    pub resolution: f64,
    pub layers: Vec<Layer>,
    pub active: Option<Uuid>,
    pub selected: HashSet<Uuid>,
    pub selection: Option<Selection>,
    pub guides: Vec<crate::guides::Guide>,
}

impl Document {
    pub fn new(width: u32, height: u32) -> Result<Self> {
        validate_canvas_size(width, height)?;
        let layer = Layer::blank("Layer 1", width, height);
        Ok(Self {
            id: Uuid::new_v4(),
            width,
            height,
            resolution: 72.,
            active: Some(layer.id),
            selected: HashSet::from([layer.id]),
            layers: vec![layer],
            selection: None,
            guides: Vec::new(),
        })
    }
    /// Visibility includes every parent folder, independently of opacity and clipping.
    pub fn layer_is_visible(&self, id: Uuid) -> bool {
        let Some(layer) = self.layer(id) else {
            return false;
        };
        let mut current = Some(layer);
        for _ in 0..=self.layers.len() {
            let Some(layer) = current else {
                return true;
            };
            if !layer.visible {
                return false;
            }
            current = layer.parent.and_then(|id| self.layer(id));
        }
        false
    }

    pub fn layer(&self, id: Uuid) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }
    pub fn active_layer(&self) -> Option<&Layer> {
        self.active.and_then(|id| self.layer(id))
    }
    pub fn active_layer_mut(&mut self) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| Some(l.id) == self.active)
    }
    pub fn select(&mut self, id: Uuid, extend: bool) {
        if self.layer(id).is_none() {
            return;
        }
        self.active = Some(id);
        if !extend {
            self.selected.clear();
        }
        self.selected.insert(id);
    }
    pub fn add(&mut self, layer: Layer) -> Result<()> {
        if self.layers.len() >= 10_000 {
            return Err(invalid("The project has reached the 10,000 layer limit."));
        }
        let id = layer.id;
        self.layers.push(layer);
        self.select(id, false);
        Ok(())
    }
    pub fn descendants(&self, id: Uuid) -> HashSet<Uuid> {
        let mut ids = HashSet::from([id]);
        loop {
            let before = ids.len();
            for layer in &self.layers {
                if layer.parent.is_some_and(|parent| ids.contains(&parent)) {
                    ids.insert(layer.id);
                }
            }
            if before == ids.len() {
                return ids;
            }
        }
    }
    pub fn validate(&self) -> Result<()> {
        crate::guides::validate(&self.guides)?;
        validate_canvas_size(self.width, self.height)?;
        if let Some(selection) = &self.selection {
            selection.validate()?;
        }
        if !(1. ..=9600.).contains(&self.resolution) || self.layers.len() > 10_000 {
            return Err(invalid(
                "Project resolution or layer count exceeds the supported limits.",
            ));
        }
        let ids: HashSet<_> = self.layers.iter().map(|l| l.id).collect();
        if ids.len() != self.layers.len() || self.active.is_some_and(|id| !ids.contains(&id)) {
            return Err(invalid(
                "Project contains duplicate layer IDs or a missing active layer.",
            ));
        }
        let mut pixels = 0_u64;
        let mut mask_pixels = 0_u64;
        let mut raw_sources = HashSet::new();
        let mut raw_bytes = 0_u64;
        for layer in &self.layers {
            if let Some(text) = &layer.text {
                text.validate()?;
                if layer.raster().is_none() || layer.shape.is_some() {
                    return Err(invalid(
                        "Editable text requires a cached raster and cannot also be a shape.",
                    ));
                }
            }
            if layer
                .effects
                .as_ref()
                .is_some_and(|effects| !effects.is_empty())
                && layer.raster().is_none()
            {
                return Err(invalid(
                    "Layer effects require a pixel, shape, or text layer. Folder and adjustment effects are not supported.",
                ));
            }
            if layer
                .effects
                .as_ref()
                .is_some_and(|effects| !effects.validate())
            {
                return Err(invalid("Layer effects contain invalid settings."));
            }
            if let Some(shape) = layer.shape
                && (!shape.valid() || layer.raster().is_none())
            {
                return Err(invalid("Shape metadata is invalid or has no raster asset."));
            }
            if let Some(raw) = &layer.raw {
                if layer.raster().is_none() || layer.text.is_some() || layer.shape.is_some() {
                    return Err(invalid(
                        "An editable RAW layer requires cached pixels and cannot also be a shape or text layer.",
                    ));
                }
                raw.validate()?;
                if raw_sources.insert(Arc::as_ptr(&raw.bytes)) {
                    raw_bytes += raw.bytes.len() as u64;
                    if raw_bytes > crate::raw::MAX_RAW_BYTES {
                        return Err(invalid(
                            "The project's embedded RAW sources exceed 512 MiB.",
                        ));
                    }
                }
            }
            if let LayerContent::Adjustment(adjustment) = &layer.content {
                adjustment.validate()?;
            }
            if !layer.transform.valid()
                || !(0. ..=1.).contains(&layer.opacity)
                || layer.name.trim().is_empty()
                || layer.name.len() > 16_384
                || (layer.is_group() && layer.blend != Blend::Normal)
            {
                return Err(invalid(format!(
                    "Layer '{}' has invalid metadata.",
                    layer.name
                )));
            }
            if let Some(image) = layer.raster() {
                if layer
                    .effects
                    .as_ref()
                    .is_some_and(|effects| !effects.validate_size(image.width(), image.height()))
                {
                    return Err(invalid(
                        "Layer effects exceed the 200 megapixel surface limit. Reduce the image size or effect distance, blur, or stroke size.",
                    ));
                }
                validate_size(image.width(), image.height())?;
                pixels += u64::from(image.width()) * u64::from(image.height());
            }
            if let Some(mask) = &layer.mask {
                validate_size(mask.pixels.width(), mask.pixels.height())?;
                mask_pixels += u64::from(mask.pixels.width()) * u64::from(mask.pixels.height());
                if mask.placement.is_some_and(|t| !t.valid()) {
                    return Err(invalid("Mask placement is invalid."));
                }
            }
            for (mut next, hierarchy, limit) in
                [(layer.parent, true, 64), (layer.clip_source, false, 256)]
            {
                let mut seen = HashSet::from([layer.id]);
                let mut depth = 0;
                while let Some(id) = next {
                    let Some(target) = self.layer(id) else {
                        return Err(invalid(
                            "A group or clipping mask refers to a missing layer.",
                        ));
                    };
                    depth += 1;
                    if !seen.insert(id)
                        || depth > limit
                        || (hierarchy && !target.is_group())
                        || (!hierarchy && (target.is_group() || layer.is_group()))
                    {
                        return Err(invalid(
                            "Layer hierarchy or clipping masks contain a cycle, invalid target, or excessive depth.",
                        ));
                    }
                    next = if hierarchy {
                        target.parent
                    } else {
                        target.clip_source
                    };
                }
            }
        }
        validate_pixel_budget(pixels + mask_pixels)?;
        Ok(())
    }
}

/// Count layers and masks together, independently of the single-surface limit.
pub fn validate_pixel_budget(pixels: u64) -> Result<()> {
    let limit = document_pixel_budget();
    if pixels > limit {
        return Err(invalid(format!(
            "Project exceeds this computer's {} million pixel budget across layers and masks. Reduce layer or mask dimensions, or split the project.",
            limit / 1_000_000
        )));
    }
    Ok(())
}
