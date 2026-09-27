use super::*;
use image::Rgba;

fn session() -> Session {
    let mut document = Document::new(2, 1).unwrap();
    document.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        2,
        1,
        Rgba([200, 40, 80, 128]),
    ))));
    let mut session = Session::new(document, None);
    session
        .edit("Paint", |document| {
            document.layers[0].content = LayerContent::Raster(Some(Arc::new(
                RgbaImage::from_pixel(2, 1, Rgba([20, 100, 220, 64])),
            )));
            Ok(())
        })
        .unwrap();
    session
}

#[test]
fn fade_interpolates_premultiplied_color_and_is_undoable() {
    let mut session = session();
    let original = session.document.clone();
    let fade = session.fade().unwrap();
    assert_eq!(fade.label(), "Paint");
    session
        .edit("Fade", |document| fade.apply(document, 0.5))
        .unwrap();
    assert_eq!(
        session.document.layers[0].raster().unwrap()[(0, 0)].0,
        [140, 60, 127, 96]
    );
    session.undo();
    assert_eq!(session.document, original);
    assert!(!session.can_fade());
    session.redo();
    assert!(session.can_fade());
}

#[test]
fn endpoints_are_exact_and_invalid_or_stale_calls_preserve_pixels() {
    let session = session();
    let fade = session.fade().unwrap();
    let original = session.document.clone();
    let mut candidate = original.clone();
    fade.apply(&mut candidate, 1.).unwrap();
    assert_eq!(candidate, original);
    fade.apply(&mut candidate, 0.).unwrap();
    assert_eq!(candidate.layers[0], fade.before);
    for amount in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
        let mut candidate = original.clone();
        assert!(fade.apply(&mut candidate, amount).is_err());
        assert_eq!(candidate, original);
    }
    let before = candidate.clone();
    assert!(fade.apply(&mut candidate, 0.5).is_err());
    assert_eq!(candidate, before);
}

#[test]
fn subsequent_nonpixel_edits_and_pending_work_are_not_fadeable() {
    let mut session = session();
    session.begin("Pending").unwrap();
    assert!(!session.can_fade());
    session.cancel();
    assert!(session.can_fade());
    session
        .edit("Opacity", |doc| {
            doc.layers[0].opacity = 0.5;
            Ok(())
        })
        .unwrap();
    assert!(!session.can_fade());
    session
        .edit("Resize pixels", |doc| {
            doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::new(3, 1))));
            Ok(())
        })
        .unwrap();
    assert!(!session.can_fade());
}

#[test]
fn zero_fade_restores_editable_source_but_partial_fade_rasterizes() {
    let mut session = session();
    session.past.clear();
    let shape = crate::document::Shape {
        geometry: crate::document::ShapeGeometry::Rectangle,
        red: 0.2,
        green: 0.4,
        blue: 0.6,
        corner_radius: 0.,
    };
    session.document.layers[0].shape = Some(shape);
    session
        .edit("Filter", |doc| {
            doc.layers[0].shape = None;
            doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
                2,
                1,
                Rgba([50, 80, 100, 255]),
            ))));
            Ok(())
        })
        .unwrap();
    let fade = session.fade().unwrap();
    let mut candidate = session.document.clone();
    fade.apply(&mut candidate, 0.5).unwrap();
    assert!(candidate.layers[0].shape.is_none());
    let mut candidate = session.document.clone();
    fade.apply(&mut candidate, 0.).unwrap();
    assert_eq!(candidate.layers[0].shape, Some(shape));
}

#[test]
fn multilayer_edits_and_changed_masks_are_rejected() {
    for change_mask in [true, false] {
        let mut session = session();
        let mut second = session.document.layers[0].clone();
        second.id = uuid::Uuid::new_v4();
        session.document.layers.push(second);
        session
            .edit("Edit", |doc| {
                doc.layers[0].content = LayerContent::Raster(Some(Arc::new(
                    RgbaImage::from_pixel(2, 1, Rgba([90, 80, 70, 255])),
                )));
                if change_mask {
                    doc.layers[0].mask = Some(crate::document::Mask {
                        pixels: Arc::new(image::GrayImage::new(2, 1)),
                        enabled: true,
                        linked: true,
                        placement: None,
                    });
                } else {
                    doc.layers[1].content = doc.layers[0].content.clone();
                }
                Ok(())
            })
            .unwrap();
        assert!(!session.can_fade());
    }
}
