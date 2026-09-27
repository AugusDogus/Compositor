//! Editable size variants use proportional placements and shared source assets.
use super::{Batch, Fit};
use crate::{
    Result,
    adjustment::Kind,
    artboard::Artboard,
    document::{Document, Layer, LayerContent, Mask},
    geometry::{Point, Transform},
    invalid, layer_ops,
};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use std::{collections::HashSet, sync::Arc};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Canvas,
    Artboard(Uuid),
}

struct Template {
    document: Document,
    frame: Transform,
    owner: Option<Layer>,
    root: Uuid,
}

impl Batch {
    /// Add every requested size as an editable artboard, or leave the document
    /// unchanged. File format/quality apply only to `export`, not these layers.
    pub fn add_artboards(&self, document: &mut Document, source: Source) -> Result<Vec<Uuid>> {
        document.validate()?;
        let template = Template::new(document, source)?;
        let frames = layout(document, &self.sizes)?;
        preflight(document, &template, frames.len())?;
        let mut next = document.clone();
        for frame in &frames {
            next.width = next
                .width
                .max((frame.origin[0] + frame.size[0]).ceil() as u32);
            next.height = next
                .height
                .max((frame.origin[1] + frame.size[1]).ceil() as u32);
        }
        let mut boards = Vec::with_capacity(frames.len());
        for frame in frames {
            let ratios = [
                frame.size[0] / template.frame.size[0],
                frame.size[1] / template.frame.size[1],
            ];
            let scale = match self.fit {
                Fit::Contain => ratios[0].min(ratios[1]),
                Fit::Cover => ratios[0].max(ratios[1]),
            };
            let origin = [
                frame.origin[0] + (frame.size[0] - template.frame.size[0] * scale) / 2.,
                frame.origin[1] + (frame.size[1] - template.frame.size[1] * scale) / 2.,
            ];
            let mut source = template.document.clone();
            for layer in &mut source.layers {
                let mut placement = mapped(layer.transform, template.frame.origin, origin, scale);
                if layer.raster().is_none()
                    && layer
                        .mask
                        .as_ref()
                        .is_none_or(|mask| mask.placement.is_some())
                {
                    // Empty/folder/adjustment geometry is unused without an
                    // implicit mask. Keep it valid without limiting its children.
                    placement.size = placement.size.map(|size| size.max(1.));
                }
                layer.transform = checked(placement)?;
                if let Some(placement) =
                    layer.mask.as_mut().and_then(|mask| mask.placement.as_mut())
                {
                    *placement = scaled(*placement, template.frame.origin, origin, scale)?;
                }
                scale_adjustment(layer, scale)?;
            }
            let mut board = Layer::blank(
                format!("{} × {}", frame.size[0] as u32, frame.size[1] as u32),
                1,
                1,
            );
            board.content = LayerContent::Artboard(Artboard { background: [0; 4] });
            board.transform = frame;
            if let Some(owner) = &template.owner {
                board.visible = owner.visible;
                board.opacity = owner.opacity;
                board.mask = owner.mask.clone();
                if let Some(mask) = &mut board.mask {
                    mask.placement = Some(scaled(
                        mask.placement.unwrap_or(owner.transform),
                        template.frame.origin,
                        origin,
                        scale,
                    )?);
                }
            }
            let board_id = board.id;
            next.add(board)?;
            layer_ops::copy_into(&source, &mut next, template.root)?;
            next.active_layer_mut()
                .ok_or_else(|| {
                    invalid("The copied source folder is missing. No artboards were added.")
                })?
                .parent = Some(board_id);
            boards.push(board_id);
        }
        // Select one frame so its settings and resize handles work immediately.
        next.active = boards.last().copied();
        next.selected = next.active.into_iter().collect();
        next.selection = None;
        next.validate()?;
        crate::project::validate_storage_metadata(&next)?;
        *document = next;
        Ok(boards)
    }
}

impl Template {
    fn new(document: &Document, source: Source) -> Result<Self> {
        let mut copy = document.clone();
        let (frame, owner) = match source {
            Source::Canvas => {
                if document.layers.iter().any(Layer::is_artboard) {
                    return Err(invalid(
                        "This canvas already contains artboards. Select one artboard to create editable size variants; no artboards were added.",
                    ));
                }
                (Transform::new(document.width, document.height), None)
            }
            Source::Artboard(id) => {
                let owner = document.layer(id).filter(|layer| layer.is_artboard()).ok_or_else(|| invalid("The source artboard no longer exists. Select an artboard and reopen Export Sizes."))?.clone();
                let ids = document.descendants(id);
                copy.layers
                    .retain(|layer| layer.id != id && ids.contains(&layer.id));
                for layer in &mut copy.layers {
                    if layer.parent == Some(id) {
                        layer.parent = None;
                    }
                }
                (owner.transform, Some(owner))
            }
        };
        let mut clip = Layer::blank("Source frame", 1, 1);
        clip.content = LayerContent::Group;
        clip.transform = frame;
        // An implicit white mask has zero coverage outside its layer rectangle.
        // It clips retained off-canvas pixels without destroying their editability.
        clip.mask = Some(Mask {
            pixels: Arc::new(GrayImage::from_pixel(1, 1, Luma([255]))),
            enabled: true,
            linked: true,
            placement: None,
        });
        let root = clip.id;
        for layer in &mut copy.layers {
            if layer.parent.is_none() {
                layer.parent = Some(root);
            }
        }
        if let Some(owner) = &owner
            && let LayerContent::Artboard(board) = owner.content
            && board.background[3] != 0
        {
            let mut background = Layer::blank("Artboard background", 1, 1);
            background.transform = frame;
            background.parent = Some(root);
            background.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
                1,
                1,
                Rgba(board.background),
            ))));
            copy.layers.insert(0, background);
        }
        copy.layers.insert(0, clip);
        copy.active = Some(root);
        copy.selected = HashSet::from([root]);
        copy.selection = None;
        copy.validate()?;
        if let Some(layer) = copy.layers.iter().find(|layer| {
            matches!(&layer.content, LayerContent::Adjustment(settings) if matches!(settings.kind, Kind::Grain | Kind::AddNoise))
        }) {
            return Err(invalid(format!("Editable size variants cannot preserve the document-coordinate noise pattern in '{}'. Merge that adjustment into pixels first, or export PNG/JPEG files; no artboards were added.", layer.name)));
        }
        Ok(Self {
            document: copy,
            frame,
            owner,
            root,
        })
    }
}

fn mapped(mut transform: Transform, old: Point, new: Point, scale: f64) -> Transform {
    for axis in 0..2 {
        transform.origin[axis] = (transform.origin[axis] - old[axis]) * scale + new[axis];
        transform.size[axis] *= scale;
    }
    transform
}
fn scaled(transform: Transform, old: Point, new: Point, scale: f64) -> Result<Transform> {
    checked(mapped(transform, old, new, scale))
}
fn checked(transform: Transform) -> Result<Transform> {
    if !transform.valid() {
        return Err(invalid(
            "A size variant would place a layer or mask outside the supported transform range, or shrink it below one pixel. Choose dimensions closer to the source size; no artboards were added.",
        ));
    }
    Ok(transform)
}

fn scale_adjustment(layer: &mut Layer, scale: f64) -> Result<()> {
    let LayerContent::Adjustment(settings) = &mut layer.content else {
        return Ok(());
    };
    match settings.kind {
        Kind::GaussianBlur => {
            settings.blur_radius = Some(settings.blur_radius.unwrap_or(10.) * scale)
        }
        Kind::MotionBlur => {
            settings.motion_distance = Some(settings.motion_distance.unwrap_or(10.) * scale)
        }
        _ => (),
    }
    settings.validate().map_err(|_| invalid(format!("Scaling '{}' by {scale:.3} would exceed its adjustment range. Choose dimensions closer to the source size or reduce that adjustment first; no artboards were added.", layer.name)))
}

fn layout(document: &Document, sizes: &[[u32; 2]]) -> Result<Vec<Transform>> {
    const GAP: f64 = 32.;
    let mut right = f64::from(document.width);
    let mut bottom = f64::from(document.height);
    for layer in document.layers.iter().filter(|layer| layer.is_artboard()) {
        right = right.max((layer.transform.origin[0] + layer.transform.size[0]).ceil());
        bottom = bottom.max((layer.transform.origin[1] + layer.transform.size[1]).ceil());
    }
    let (mut x, mut y, mut row_height) = (right + GAP, 0., bottom);
    let mut frames = Vec::with_capacity(sizes.len());
    for &[width, height] in sizes {
        if x + f64::from(width) > 30_000. {
            x = 0.;
            y += row_height + GAP;
            row_height = 0.;
        }
        if y + f64::from(height) > 30_000. {
            return Err(invalid(
                "These artboards cannot fit beside or below the current canvas within 30000 pixels per side. Choose fewer or smaller sizes, or copy the source into a smaller project; no artboards were added.",
            ));
        }
        frames.push(Transform {
            origin: [x, y],
            ..Transform::new(width, height)
        });
        x += f64::from(width) + GAP;
        row_height = row_height.max(f64::from(height));
    }
    Ok(frames)
}

fn pixel_count(layers: &[Layer]) -> u64 {
    layers
        .iter()
        .map(|layer| {
            layer.raster().map_or(0, |pixels| {
                u64::from(pixels.width()) * u64::from(pixels.height())
            }) + layer.mask.as_ref().map_or(0, |mask| {
                u64::from(mask.pixels.width()) * u64::from(mask.pixels.height())
            })
        })
        .sum()
}
fn preflight(document: &Document, template: &Template, count: usize) -> Result<()> {
    if document.layers.len() + count * (template.document.layers.len() + 1) > 10_000 {
        return Err(invalid(
            "These variants would exceed the 10,000 layer limit. Choose fewer sizes or simplify the source; no artboards were added.",
        ));
    }
    let owner_mask = template
        .owner
        .as_ref()
        .and_then(|owner| owner.mask.as_ref())
        .map_or(0, |mask| {
            u64::from(mask.pixels.width()) * u64::from(mask.pixels.height())
        });
    crate::document::validate_pixel_budget(
        pixel_count(&document.layers)
            + count as u64 * (pixel_count(&template.document.layers) + owner_mask),
    )
}

#[cfg(test)]
mod tests;
