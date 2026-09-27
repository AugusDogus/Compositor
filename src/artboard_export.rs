//! Export each visible artboard independently at its frame size.
use crate::{
    Result,
    document::{Document, LayerContent},
    export_batch, image_io, invalid, render,
};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub fn export(document: &Document, parent: &Path, title: &str) -> Result<PathBuf> {
    document.validate()?;
    let boards: Vec<_> = document
        .layers
        .iter()
        .filter(|l| l.visible && matches!(l.content, LayerContent::Artboard(_)))
        .collect();
    if boards.is_empty() {
        return Err(invalid(
            "There are no visible artboards to export. Show or create an artboard and retry.",
        ));
    }
    let mut total = 0_u64;
    for board in &boards {
        let size = dimensions(board.transform.size);
        crate::document::validate_size(size[0], size[1])?;
        total += u64::from(size[0]) * u64::from(size[1]);
        if total > 800_000_000 {
            return Err(invalid(
                "Artboard export exceeds 800 million pixels. Hide some artboards and export in smaller batches; no output folder was created.",
            ));
        }
    }
    export_batch::publish(
        parent,
        &format!("{}-artboards", export_batch::safe_stem(title)),
        |directory| {
            let mut cache = render::DownsampleCache::default();
            for (index, board) in boards.iter().enumerate() {
                let source = isolated(document, board.id);
                let size = dimensions(board.transform.size);
                let pixels = render::region_accelerated(
                    &source,
                    size[0],
                    size[1],
                    board.transform.origin,
                    [
                        board.transform.size[0] / f64::from(size[0]),
                        board.transform.size[1] / f64::from(size[1]),
                    ],
                    &mut cache,
                )?;
                let path = directory.join(format!(
                    "{:02}-{}.png",
                    index + 1,
                    export_batch::safe_stem(&board.name)
                ));
                image_io::export_pixels(pixels, document.resolution, &path, 100)?;
            }
            Ok(())
        },
    )
}
fn dimensions(size: [f64; 2]) -> [u32; 2] {
    size.map(|v| v.ceil() as u32)
}
fn isolated(document: &Document, id: Uuid) -> Document {
    let mut source = document.clone();
    let ids = document.descendants(id);
    source.layers.retain(|l| ids.contains(&l.id));
    source.active = Some(id);
    source.selected = HashSet::from([id]);
    source.selection = None;
    source
}

#[cfg(test)]
mod tests;
