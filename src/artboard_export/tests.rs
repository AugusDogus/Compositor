use super::*;
use crate::{artboard, document::Layer, geometry::Transform};
use image::{Rgba, RgbaImage};
use std::sync::Arc;
#[test]
fn overlapping_artboards_export_independently_with_masks_hidden_content_and_safe_names() {
    let mut doc = Document::new(8, 8).unwrap();
    let a = artboard::create(&mut doc, "../A", Transform::new(4, 4), [255, 0, 0, 255]).unwrap();
    let b = artboard::create(&mut doc, "B", Transform::new(4, 4), [0, 0, 255, 255]).unwrap();
    let mut hidden = Layer::blank("Hidden", 4, 4);
    hidden.parent = Some(a);
    hidden.visible = false;
    hidden.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        4,
        4,
        Rgba([0, 255, 0, 255]),
    ))));
    doc.add(hidden).unwrap();
    doc.layers.iter_mut().find(|l| l.id == b).unwrap().opacity = 0.5;
    let root = tempfile::tempdir().unwrap();
    let first = export(&doc, root.path(), "Boards").unwrap();
    let second = export(&doc, root.path(), "Boards").unwrap();
    assert_ne!(first, second);
    let files: Vec<_> = std::fs::read_dir(first)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(files.len(), 2);
    for file in files {
        let img = image_io::read_image(&file).unwrap();
        assert_eq!(img.dimensions(), (4, 4));
        let expected = if file
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("01")
        {
            [255, 0, 0, 255]
        } else {
            [0, 0, 255, 128]
        };
        assert!(img.pixels().all(|p| p.0 == expected));
    }
}
#[test]
fn individual_layer_export_uses_board_clip_without_adding_background() {
    let mut doc = Document::new(8, 8).unwrap();
    let id = artboard::create(&mut doc, "Board", Transform::new(4, 4), [255; 4]).unwrap();
    let mut child = Layer::blank("Child", 8, 8);
    child.parent = Some(id);
    child.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        8,
        8,
        Rgba([255, 0, 0, 128]),
    ))));
    doc.add(child).unwrap();
    let root = tempfile::tempdir().unwrap();
    let out = crate::layer_export::export(&doc, root.path(), "Layers").unwrap();
    let image = image_io::read_image(&out.join("01-Child.png")).unwrap();
    assert_eq!(image[(1, 1)], Rgba([255, 0, 0, 128]));
    assert_eq!(image[(6, 6)][3], 0);
}

#[test]
fn artboard_png_applies_local_adjustments_but_excludes_root_adjustments() {
    use crate::adjustment::{Adjustment, Kind};
    let mut doc = Document::new(8, 8).unwrap();
    let board =
        artboard::create(&mut doc, "Board", Transform::new(4, 4), [200, 40, 20, 255]).unwrap();
    let mut local = Layer::blank("Local invert", 4, 4);
    local.parent = Some(board);
    local.content = LayerContent::Adjustment(Box::new(Adjustment::new(Kind::Invert)));
    doc.add(local).unwrap();
    let mut root = Layer::blank("Root invert", 8, 8);
    root.content = LayerContent::Adjustment(Box::new(Adjustment::new(Kind::Invert)));
    doc.add(root).unwrap();
    let original = doc.clone();
    let composite = render::render(&doc, 8, 8).unwrap();
    assert_eq!(composite[(1, 1)], Rgba([200, 40, 20, 255]));
    let directory = tempfile::tempdir().unwrap();
    let output = export(&doc, directory.path(), "Export").unwrap();
    let png = image_io::read_image(&output.join("01-Board.png")).unwrap();
    assert_eq!(png.dimensions(), (4, 4));
    assert!(
        png.pixels()
            .all(|pixel| *pixel == Rgba([55, 215, 235, 255]))
    );
    assert_eq!(doc, original);
}
