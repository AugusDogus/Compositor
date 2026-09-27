use super::{Content, Style};
use crate::{
    Result,
    document::{Document, Layer, LayerContent},
    invalid,
    vector_path::BezierPath,
};
use uuid::Uuid;

pub fn create(
    document: &mut Document,
    name: &str,
    geometry: BezierPath,
    style: Style,
) -> Result<Uuid> {
    let (content, transform) = Content::from_document_path(geometry, style)?;
    let mut layer = Layer::blank(name, content.source().size[0], content.source().size[1]);
    layer.parent = document.active_layer().and_then(|layer| {
        if layer.is_group() {
            Some(layer.id)
        } else {
            layer.parent
        }
    });
    layer.transform = transform;
    layer.content = LayerContent::PathShape(Box::new(content));
    let id = layer.id;
    let mut next = document.clone();
    next.add(layer)?;
    // Reuse the normal insertion rules, including clipping-stack boundaries.
    if let Some(active) = document.active_layer() {
        let (parent, position) = if active.is_group() {
            (Some(active.id), crate::layer_ops::Position::Top)
        } else {
            (active.parent, crate::layer_ops::Position::Above(active.id))
        };
        crate::layer_ops::place_copies(&mut next, id, parent, position)?;
    }
    publish(document, next)?;
    Ok(id)
}

pub fn layer_path(document: &Document, id: Uuid) -> Result<BezierPath> {
    let layer = document
        .layer(id)
        .ok_or_else(|| invalid("The path shape no longer exists. Select an existing shape."))?;
    layer
        .path_shape()
        .ok_or_else(|| invalid("Select a path shape to edit its points."))?
        .document_path(layer.transform)
}

pub fn update(document: &mut Document, id: Uuid, geometry: BezierPath, style: Style) -> Result<()> {
    update_geometry(document, id, geometry, style, false)
}

pub fn update_local(
    document: &mut Document,
    id: Uuid,
    geometry: BezierPath,
    style: Style,
) -> Result<()> {
    update_geometry(document, id, geometry, style, true)
}

fn update_geometry(
    document: &mut Document,
    id: Uuid,
    geometry: BezierPath,
    style: Style,
    local: bool,
) -> Result<()> {
    let layer = document
        .layer(id)
        .ok_or_else(|| invalid("The path shape no longer exists. Select an existing shape."))?;
    let shape = layer
        .path_shape()
        .ok_or_else(|| invalid("Select a path shape to edit its points."))?;
    let (content, transform) = if local {
        shape.edited_local(geometry, style, layer.transform)?
    } else {
        shape.edited(geometry, style, layer.transform)?
    };
    let mut next = document.clone();
    let layer = next
        .layers
        .iter_mut()
        .find(|layer| layer.id == id)
        .ok_or_else(|| invalid("The path shape no longer exists."))?;
    if let Some(mask) = &mut layer.mask {
        mask.placement.get_or_insert(layer.transform);
    }
    layer.transform = transform;
    layer.content = LayerContent::PathShape(Box::new(content));
    publish(document, next)
}

pub fn rasterize(document: &mut Document, id: Uuid) -> Result<()> {
    let mut next = document.clone();
    let layer = next
        .layers
        .iter_mut()
        .find(|layer| layer.id == id)
        .ok_or_else(|| invalid("The path shape no longer exists. Select an existing shape."))?;
    let content = layer
        .path_shape()
        .ok_or_else(|| invalid("Select a path shape to rasterize."))?;
    layer.content = LayerContent::Raster(Some(content.pixels().clone()));
    publish(document, next)
}

fn publish(document: &mut Document, next: Document) -> Result<()> {
    next.validate()?;
    crate::project::validate_storage_metadata(&next)?;
    *document = next;
    Ok(())
}

/// An in-progress anchor gesture. The caller owns the document transaction and
/// must call `validate` before committing, or restore the original document.
/// Every preview retains local geometry and document pixel-budget checks.
#[derive(Clone)]
pub struct Gesture {
    id: Uuid,
    source: Content,
    transform: crate::geometry::Transform,
}
impl Gesture {
    pub fn begin(document: &Document, id: Uuid) -> Result<Self> {
        let layer = document
            .layer(id)
            .ok_or_else(|| invalid("The path shape was removed."))?;
        let source = layer
            .path_shape()
            .ok_or_else(|| invalid("Select a path shape to edit its points."))?
            .clone();
        Ok(Self {
            id,
            source,
            transform: layer.transform,
        })
    }
    pub fn update(&self, document: &mut Document, geometry: BezierPath) -> Result<()> {
        let (content, transform) = if self.transform.warp.is_some() {
            self.source
                .edited_local(geometry, self.source.source().style, self.transform)?
        } else {
            self.source
                .edited(geometry, self.source.source().style, self.transform)?
        };
        let layer = document
            .layers
            .iter_mut()
            .find(|l| l.id == self.id)
            .ok_or_else(|| invalid("The path shape was removed. Cancel the gesture."))?;
        if !layer.is_path_shape() {
            return Err(invalid(
                "The layer is no longer an editable path shape. Cancel the gesture.",
            ));
        }
        let original = layer.clone();
        if let Some(mask) = &mut layer.mask {
            mask.placement.get_or_insert(layer.transform);
        }
        layer.transform = transform;
        layer.content = LayerContent::PathShape(Box::new(content));
        if let Err(error) = document.validate() {
            if let Some(layer) = document.layers.iter_mut().find(|l| l.id == self.id) {
                *layer = original;
            }
            return Err(error);
        }
        Ok(())
    }
    pub fn validate(&self, document: &Document) -> Result<()> {
        crate::project::validate_storage_metadata(document)
    }
}
