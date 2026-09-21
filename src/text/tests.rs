use super::*;
use crate::{
    document::{Document, Mask},
    project,
    session::Session,
};
use image::{GrayImage, Luma};

fn ink_bounds(image: &RgbaImage) -> [u32; 4] {
    let mut bounds = [image.width(), image.height(), 0, 0];
    for (x, y, p) in image.enumerate_pixels().filter(|(_, _, p)| p[3] > 0) {
        let _ = p;
        bounds[0] = bounds[0].min(x);
        bounds[1] = bounds[1].min(y);
        bounds[2] = bounds[2].max(x);
        bounds[3] = bounds[3].max(y);
    }
    bounds
}

#[test]
fn point_and_paragraph_text_shape_unicode_and_honor_spacing_alignment_color() {
    let mut renderer = TextRenderer::default();
    assert!(
        renderer
            .families()
            .iter()
            .any(|name| name == "Inter Variable")
    );
    let mut style = Text {
        content: "Office café 日本語\nSecond line".into(),
        red: 0.2,
        green: 0.4,
        blue: 0.8,
        ..Text::default()
    };
    let point = renderer.render(&style).unwrap();
    assert!(point.pixels().any(|p| p[3] > 0));
    assert!(
        point
            .pixels()
            .any(|p| p[3] == 255 && p.0[0..3] == [51, 102, 204])
    );
    style.tracking = 8.;
    let tracked = renderer.render(&style).unwrap();
    assert!(tracked.width() > point.width());
    style.tracking = 0.;
    style.leading = 180.;
    assert!(renderer.render(&style).unwrap().height() > point.height());
    style = Text {
        content: "Hello".into(),
        box_size: Some([500., 150.]),
        ..Text::default()
    };
    let left = renderer.render(&style).unwrap();
    style.alignment = Alignment::Right;
    let right = renderer.render(&style).unwrap();
    assert_eq!(left.dimensions(), (500, 150));
    assert!(ink_bounds(&right)[0] > ink_bounds(&left)[0] + 100);
    style.content = "hello hello hello hello hello hello".into();
    style.box_size = Some([240., 600.]);
    let wrapped = renderer.render(&style).unwrap();
    assert!(ink_bounds(&wrapped)[3] > 200);
}

#[test]
fn text_persistence_edits_transforms_masks_and_undo_preserve_editability() {
    let mut renderer = TextRenderer::default();
    let style = Text::default();
    let mut layer = new_layer(style.clone(), renderer.render(&style).unwrap(), [20., 30.]).unwrap();
    layer.transform.rotation = 35.;
    layer.transform.flip_x = true;
    layer.transform.size[0] *= 2.;
    layer.transform.size[1] *= 0.5;
    layer.mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(1, 1, Luma([200]))),
        enabled: true,
        linked: true,
        placement: None,
    });
    let old_transform = layer.transform;
    let anchor = old_transform.point([0., 0.]);
    let mut document = Document::new(1000, 1000).unwrap();
    document.add(layer).unwrap();
    let original = document.clone();
    let mut session = Session::new(document, None);
    let next = Text {
        content: "Editable text after reopen".into(),
        ..style
    };
    let pixels = renderer.render(&next).unwrap();
    let size = pixels.dimensions();
    session
        .edit("Edit Text", |doc| {
            update_layer(doc.active_layer_mut().unwrap(), next.clone(), pixels)
        })
        .unwrap();
    let edited = session.document.clone();
    let active = edited.active_layer().unwrap();
    let moved = active.transform.point([0., 0.]);
    assert!((anchor[0] - moved[0]).abs() < 0.0001 && (anchor[1] - moved[1]).abs() < 0.0001);
    assert_eq!(
        active.transform.size,
        [f64::from(size.0) * 2., f64::from(size.1) * 0.5]
    );
    assert_eq!(active.mask.as_ref().unwrap().placement, Some(old_transform));
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("text.comp");
    project::save(&edited, &path).unwrap();
    let loaded = project::load(&path).unwrap();
    let reopened = loaded.active_layer().unwrap();
    assert_eq!(reopened.text, active.text);
    assert_eq!(reopened.mask, active.mask);
    assert_eq!(reopened.transform, active.transform);
    assert!(reopened.raster() == active.raster());
    session.undo();
    assert_eq!(session.document, original);
    session.redo();
    assert_eq!(session.document, edited);
    crate::edits::fill(&mut session.document, [0; 4], true, false).unwrap();
    assert!(session.document.active_layer().unwrap().text.is_none());
}

#[test]
fn invalid_text_metadata_is_rejected_before_render_or_save() {
    let mut style = Text {
        font_size: f64::NAN,
        ..Text::default()
    };
    assert!(style.validate().is_err());
    style.font_size = 72.;
    style.box_size = Some([30_000., 30_000.]);
    assert!(style.validate().is_err());
    style.box_size = None;
    style.content = "𝄞".repeat(50_001);
    assert!(style.validate().is_err());
    let mut document = Document::new(2, 2).unwrap();
    document.layers[0].text = Some(Text::default());
    assert!(document.validate().is_err());
}

#[test]
fn macos_text_schema_round_trips_box_size_and_optional_legacy_point_text() {
    let mut data = serde_json::json!({"content":"Hello","fontName":"Helvetica","fontSize":72,"red":0,"green":0,"blue":0,"alignment":"Center","tracking":4,"leading":90,"boxSize":[300,150]});
    let style: Text = serde_json::from_value(data.clone()).unwrap();
    assert_eq!(style.box_size, Some([300., 150.]));
    assert_eq!(
        serde_json::from_value::<Text>(serde_json::to_value(&style).unwrap()).unwrap(),
        style
    );
    data.as_object_mut().unwrap().remove("boxSize");
    assert!(
        serde_json::from_value::<Text>(data)
            .unwrap()
            .box_size
            .is_none()
    );
}
