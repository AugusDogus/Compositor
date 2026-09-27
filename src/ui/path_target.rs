//! A Pen target is either a saved working path or an editable shape layer.
use super::*;
use compositor::{
    invalid,
    vector_path::{Anchor, BezierPath, Closure, SavedPath},
};
use uuid::Uuid;

/// Project editable markers independently of the curve. Off-curve handles may
/// reach the horizon even while the source curve and its anchors remain valid.
pub(super) fn markers(
    geometry: &BezierPath,
    placement: Option<(compositor::geometry::Transform, [u32; 2])>,
) -> Result<Vec<Anchor>> {
    geometry.validate()?;
    let Some((transform, size)) = placement else {
        return Ok(geometry.anchors.clone());
    };
    let project = |point: compositor::geometry::Point| {
        transform.try_point([point[0] / f64::from(size[0]), point[1] / f64::from(size[1])])
    };
    geometry
        .anchors
        .iter()
        .map(|anchor| {
            Ok(Anchor {
                point: project(anchor.point).map_err(|error| invalid(error.to_string()))?,
                incoming: anchor.incoming.and_then(|point| project(point).ok()),
                outgoing: anchor.outgoing.and_then(|point| project(point).ok()),
            })
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Saved(Uuid),
    Shape(Uuid),
}
impl Target {
    pub(super) fn key(self) -> String {
        match self {
            Self::Saved(id) => id.to_string(),
            Self::Shape(id) => format!("shape-{id}"),
        }
    }
    pub(super) fn name(self, doc: &Document) -> Option<&str> {
        match self {
            Self::Saved(id) => doc
                .paths
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.name.as_str()),
            Self::Shape(id) => doc
                .layer(id)
                .filter(|l| l.is_path_shape())
                .map(|l| l.name.as_str()),
        }
    }
    pub(super) fn snapshot(self, doc: &Document) -> Result<SavedPath> {
        match self {
            Self::Saved(id) => doc
                .paths
                .iter()
                .find(|p| p.id == id)
                .cloned()
                .ok_or_else(|| {
                    invalid("The saved path is no longer available. Choose another path.")
                }),
            Self::Shape(id) => Ok(SavedPath {
                id,
                name: self
                    .name(doc)
                    .ok_or_else(|| invalid("The path shape was removed. Choose another path."))?
                    .to_owned(),
                geometry: compositor::path_shape::layer_path(doc, id)?,
            }),
        }
    }
    /// Perspective gestures keep source cubics and carry their placement separately.
    pub(super) fn edit_snapshot(self, doc: &Document) -> Result<SavedPath> {
        if let Self::Shape(id) = self
            && let Some(layer) = doc.layer(id)
            && layer.transform.warp.is_some()
            && let Some(shape) = layer.path_shape()
        {
            return Ok(SavedPath {
                id,
                name: layer.name.clone(),
                geometry: shape.source().geometry.clone(),
            });
        }
        self.snapshot(doc)
    }
    pub(super) fn placement(
        self,
        doc: &Document,
    ) -> Option<(compositor::geometry::Transform, [u32; 2])> {
        let Self::Shape(id) = self else {
            return None;
        };
        let layer = doc.layer(id)?;
        layer.transform.warp?;
        Some((layer.transform, layer.path_shape()?.source().size))
    }
    pub(super) fn closure(self, doc: &Document) -> Option<Closure> {
        match self {
            Self::Saved(id) => doc
                .paths
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.geometry.closure),
            Self::Shape(id) => doc
                .layer(id)?
                .path_shape()
                .map(|p| p.source().geometry.closure),
        }
    }
    pub(super) fn replace(self, doc: &mut Document, geometry: BezierPath) -> Result<()> {
        match self {
            Self::Saved(id) => {
                super::paths::path_mut(doc, id)?.geometry = geometry;
                compositor::vector_path::validate(&doc.paths)
            }
            Self::Shape(id) => {
                let style = doc
                    .layer(id)
                    .and_then(|l| l.path_shape())
                    .ok_or_else(|| invalid("The path shape was removed. Choose another path."))?
                    .source()
                    .style;
                if doc
                    .layer(id)
                    .is_some_and(|layer| layer.transform.warp.is_some())
                {
                    compositor::path_shape::update_local(doc, id, geometry, style)
                } else {
                    compositor::path_shape::update(doc, id, geometry, style)
                }
            }
        }
    }
}
