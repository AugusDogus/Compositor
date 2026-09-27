use super::*;
use crate::blend_if::{Range, Settings};
use image::{Rgba, RgbaImage};

fn pixels(name: &str, color: [u8; 4]) -> Layer {
    let mut layer = Layer::blank(name, 1, 1);
    layer.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(1, 1, Rgba(color)))));
    layer
}
fn settings() -> Settings {
    Settings {
        underlying: Range::new([128, 128], [255, 255]).unwrap(),
        ..Default::default()
    }
}
fn document() -> Document {
    let mut doc = Document::new(1, 1).unwrap();
    doc.layers[0] = pixels("Backdrop", [0, 0, 0, 255]);
    doc.add(Layer::blank("Empty", 1, 1)).unwrap();
    let mut top = pixels("Conditional", [255, 0, 0, 255]);
    top.blend_if = Some(settings());
    doc.add(top).unwrap();
    doc
}
fn rejected_unchanged(doc: &mut Document) {
    let before = doc.clone();
    let error = super::super::merge(doc, false).unwrap_err().to_string();
    assert!(
        error.contains("Blend If") && error.contains("Merge All"),
        "{error}"
    );
    assert_eq!(*doc, before);
}
fn accepted_unchanged_image(doc: &mut Document, all: bool) {
    let before = render::render(doc, 1, 1).unwrap();
    super::super::merge(doc, all).unwrap();
    assert_eq!(render::render(doc, 1, 1).unwrap(), before);
    doc.validate().unwrap();
}

#[test]
fn blend_if_merge_down_rejects_lost_backdrop_without_mutating_document() {
    let mut doc = document();
    assert_eq!(
        render::render(&doc, 1, 1).unwrap()[(0, 0)],
        Rgba([0, 0, 0, 255])
    );
    rejected_unchanged(&mut doc);
    accepted_unchanged_image(&mut doc, true);
}

#[test]
fn blend_if_merge_accepts_complete_context_and_ignores_hidden_backdrops() {
    let mut doc = document();
    doc.selected = doc.layers.iter().map(|layer| layer.id).collect();
    accepted_unchanged_image(&mut doc, false);
    let mut doc = document();
    doc.layers[0].visible = false;
    accepted_unchanged_image(&mut doc, false);
    let mut doc = document();
    doc.layers[2].blend_if.as_mut().unwrap().enabled = false;
    accepted_unchanged_image(&mut doc, false);
}

#[test]
fn blend_if_merge_accepts_source_only_range_without_backdrop() {
    let mut doc = document();
    doc.layers[2].blend_if = Some(Settings {
        source: Range::new([0, 100], [255, 255]).unwrap(),
        ..Default::default()
    });
    accepted_unchanged_image(&mut doc, false);
}

#[test]
fn blend_if_merge_requires_hidden_clipping_dependencies_and_external_consumers() {
    let mut doc = document();
    let base = doc.layers[0].id;
    doc.layers[0].visible = false;
    doc.layers[2].clip_source = Some(base);
    rejected_unchanged(&mut doc);
    let mut doc = document();
    doc.layers[2].clip_source = Some(doc.layers[0].id);
    doc.active = Some(doc.layers[1].id);
    doc.selected = HashSet::from([doc.layers[0].id, doc.layers[1].id]);
    rejected_unchanged(&mut doc);
}

#[test]
fn blend_if_merge_stack_is_self_contained_despite_external_backdrop() {
    let mut doc = document();
    doc.layers[1] = pixels("Base", [255; 4]);
    doc.layers[2].clip_source = Some(doc.layers[1].id);
    accepted_unchanged_image(&mut doc, false);
}

#[test]
fn blend_if_merge_folder_requires_external_live_backdrop() {
    let mut doc = document();
    let mut folder = Layer::blank("Folder", 1, 1);
    folder.content = LayerContent::Group;
    let id = folder.id;
    doc.layers[1].parent = Some(id);
    doc.layers[2].parent = Some(id);
    doc.layers.insert(1, folder);
    doc.select(id, false);
    rejected_unchanged(&mut doc);
    doc.layers[0].visible = false;
    accepted_unchanged_image(&mut doc, false);
}

#[test]
fn blend_if_merge_child_requires_ancestor_coverage_context() {
    let mut doc = document();
    let mut folder = Layer::blank("Folder", 1, 1);
    folder.content = LayerContent::Group;
    folder.opacity = 0.5;
    let id = folder.id;
    doc.layers[1].parent = Some(id);
    doc.layers[2].parent = Some(id);
    doc.layers[0].visible = false;
    doc.layers.insert(1, folder);
    rejected_unchanged(&mut doc);
    doc.select(id, false);
    accepted_unchanged_image(&mut doc, false);
}

#[test]
fn blend_if_merge_rejects_lost_hidden_ancestor_or_nested_mask() {
    let mut doc = document();
    let mut folder = Layer::blank("Hidden folder", 1, 1);
    folder.content = LayerContent::Group;
    folder.visible = false;
    doc.layers[1].parent = Some(folder.id);
    doc.layers[2].parent = Some(folder.id);
    doc.layers.insert(1, folder);
    rejected_unchanged(&mut doc);

    let mut doc = document();
    doc.layers[0].visible = false;
    doc.layers[0].blend_if = Some(settings());
    doc.layers[1].clip_source = Some(doc.layers[0].id);
    doc.layers[2].clip_source = Some(doc.layers[1].id);
    doc.layers[2].blend_if = None;
    rejected_unchanged(&mut doc);
}

#[test]
fn blend_if_merge_artboard_needs_its_background_but_not_neighboring_boards() {
    let mut doc = document();
    let mut board = Layer::blank("Board", 1, 1);
    board.content = LayerContent::Artboard(crate::artboard::Artboard {
        background: [0, 0, 0, 255],
    });
    let id = board.id;
    doc.layers[1].parent = Some(id);
    doc.layers[2].parent = Some(id);
    doc.layers.insert(1, board);
    rejected_unchanged(&mut doc);
    doc.select(id, false);
    accepted_unchanged_image(&mut doc, false);
    // A top-level condition also reads earlier artboards as its backdrop.
    let mut doc = document();
    doc.layers[0].content = LayerContent::Artboard(crate::artboard::Artboard {
        background: [0, 0, 0, 255],
    });
    rejected_unchanged(&mut doc);
}
