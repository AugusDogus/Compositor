use super::*;
use crate::document::{LayerContent, Mask};
use image::{GrayImage, Luma, Rgba, RgbaImage};

fn document(value: u8) -> Document {
    let mut doc = Document::new(2, 2).unwrap();
    doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        2,
        2,
        Rgba([value; 4]),
    ))));
    doc
}

fn pixels(session: &mut Session, label: &str, value: u8) {
    session
        .edit(label, |doc| {
            doc.layers[0].content = document(value).layers.remove(0).content;
            Ok(())
        })
        .unwrap();
}

#[test]
fn shared_live_assets_cost_nothing_and_historical_rasters_and_masks_count_once() {
    let mut doc = document(1);
    doc.layers[0].mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(2, 2, Luma([255]))),
        enabled: true,
        linked: true,
        placement: None,
    });
    let mut duplicate = doc.layers[0].clone();
    duplicate.id = Uuid::new_v4();
    doc.add(duplicate).unwrap();
    let mut s = Session::new(doc, None);
    s.edit("Move", |doc| {
        doc.layers[0].transform.origin[0] = 1.;
        Ok(())
    })
    .unwrap();
    assert_eq!(s.retained_history_bytes(Canvas::Visible), 0);
    s.trim_history_to(Canvas::Visible, 100, 0);
    assert_eq!(s.undo_label(), Some("Move"));
    s.edit("Replace", |doc| {
        let replacement = document(2).layers.remove(0).content;
        let mask = Arc::new(GrayImage::from_pixel(2, 2, Luma([128])));
        for layer in &mut doc.layers {
            layer.content = replacement.clone();
            layer.mask.as_mut().unwrap().pixels = mask.clone();
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(s.retained_history_bytes(Canvas::Visible), 20);
    s.undo();
    assert_eq!(s.retained_history_bytes(Canvas::Visible), 20);
}

#[test]
fn a_single_oversized_entry_is_removed_without_changing_the_live_document() {
    let mut s = Session::new(document(1), None);
    pixels(&mut s, "Replace", 2);
    let current = s.document.clone();
    s.trim_history_to(Canvas::Visible, 100, 15);
    assert!(s.undo_label().is_none());
    assert!(s.redo_label().is_none());
    assert_eq!(s.document, current);
}

#[test]
fn budget_counts_both_directions_and_evicts_past_before_future() {
    let mut s = Session::new(document(1), None);
    pixels(&mut s, "Second", 2);
    pixels(&mut s, "Third", 3);
    s.undo();
    assert_eq!(s.retained_history_bytes(Canvas::Visible), 32);
    s.trim_history_to(Canvas::Visible, 100, 16);
    assert!(s.undo_label().is_none());
    assert_eq!(s.redo_label(), Some("Third"));
    assert_eq!(s.retained_history_bytes(Canvas::Visible), 16);
    s.redo();
    assert_eq!(s.undo_label(), Some("Third"));
    assert_eq!(s.retained_history_bytes(Canvas::Visible), 16);
}

#[test]
fn entry_limit_spans_both_directions_and_removes_the_farthest_redo_first() {
    let mut s = Session::new(document(1), None);
    for (label, value) in [("Second", 2), ("Third", 3), ("Fourth", 4)] {
        pixels(&mut s, label, value);
    }
    s.undo();
    s.trim_history_to(Canvas::Visible, 2, usize::MAX);
    s.undo();
    assert!(s.undo_label().is_none());
    assert_eq!(s.redo_label(), Some("Third"));
    s.trim_history_to(Canvas::Visible, 1, usize::MAX);
    s.redo();
    assert_eq!(s.undo_label(), Some("Third"));
    assert!(s.redo_label().is_none());
}

#[test]
fn creation_is_the_oldest_undo_but_the_nearest_redo_and_counts_its_assets_at_welcome() {
    let mut s = Session::created(document(1), "Create").unwrap();
    pixels(&mut s, "Second", 2);
    s.trim_history_to(Canvas::Visible, 1, usize::MAX);
    assert!(s.creation_label().is_none());
    assert_eq!(s.undo_label(), Some("Second"));

    let mut s = Session::created(document(1), "Create").unwrap();
    pixels(&mut s, "Second", 2);
    pixels(&mut s, "Third", 3);
    s.undo();
    s.undo();
    assert_eq!(s.retained_history_bytes(Canvas::Welcome), 48);
    s.trim_history_to(Canvas::Welcome, 100, 32);
    assert_eq!(s.creation_label(), Some("Create"));
    assert_eq!(s.redo_label(), Some("Second"));
    assert_eq!(s.retained_history_bytes(Canvas::Welcome), 32);
    s.trim_history_to(Canvas::Welcome, 100, 15);
    assert!(s.creation_label().is_none());
    assert!(s.redo_label().is_none());
    assert_eq!(s.retained_history_bytes(Canvas::Welcome), 0);
}

#[test]
fn undo_releases_a_future_image_that_exceeds_the_production_budget() {
    let mut s = Session::new(Document::new(8193, 8192).unwrap(), None);
    let image = Arc::new(RgbaImage::new(8193, 8192));
    let retained = Arc::downgrade(&image);
    s.edit("Large image", |doc| {
        doc.layers[0].content = LayerContent::Raster(Some(image));
        Ok(())
    })
    .unwrap();
    assert_eq!(s.undo_label(), Some("Large image"));
    s.undo();
    assert!(s.document.layers[0].raster().is_none());
    assert!(s.redo_label().is_none());
    assert!(retained.upgrade().is_none());
}
