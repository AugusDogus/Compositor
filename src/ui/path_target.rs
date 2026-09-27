//! A Pen target is either a saved working path or an editable shape layer.
use super::*;
use compositor::{
    invalid,
    vector_path::{BezierPath, Closure, SavedPath},
};
use uuid::Uuid;

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
                compositor::path_shape::update(doc, id, geometry, style)
            }
        }
    }
}
