use compositor::{
    document::{Document, Layer, LayerContent},
    layer_ops,
    session::Session,
};

#[test]
fn folded_distortion_composites_overlapping_triangles_and_rejects_collapsed_ones() {
    use compositor::{distort, geometry::Transform};
    let mut doc = Document::new(20, 20).unwrap();
    let image = image::RgbaImage::from_pixel(10, 10, image::Rgba([200, 80, 40, 128]));
    doc.layers[0].content = LayerContent::Raster(Some(std::sync::Arc::new(image)));
    doc.layers[0].transform = Transform::new(10, 10);
    let corners = [[0., 0.], [10., 10.], [10., 0.], [0., 10.]];
    assert!(distort::usable_corners(corners));
    distort::apply(&mut doc, Transform::new(10, 10), corners, false).unwrap();
    let pixels = doc.layers[0].raster().unwrap();
    assert_eq!(pixels[(5, 2)], image::Rgba([200, 80, 40, 192]));
    assert_eq!(pixels[(1, 7)], image::Rgba([200, 80, 40, 128]));
    assert_eq!(pixels[(5, 8)][3], 0);
    doc.validate().unwrap();
    assert!(!distort::usable_corners([
        [0., 0.],
        [10., 0.],
        [20., 0.],
        [0., 10.]
    ]));
}

#[test]
fn line_shape_keeps_round_ends_direction_width_and_editability_after_resize_and_save() {
    use compositor::{
        document::{Shape, ShapeGeometry},
        edits, project, transform,
    };
    let mut doc = Document::new(100, 100).unwrap();
    let style = Shape {
        geometry: ShapeGeometry::Line {
            line_width: 4.,
            start: [0., 0.],
            end: [1., 1.],
        },
        red: 1.,
        green: 0.,
        blue: 0.,
        corner_radius: 0.,
    };
    edits::shape(&mut doc, [50., 20.], [10., 20.], style).unwrap();
    let layer = doc.active_layer().unwrap();
    assert_eq!(layer.transform.origin, [8., 18.]);
    assert_eq!(layer.raster().unwrap().dimensions(), (44, 4));
    assert_eq!(layer.raster().unwrap()[(22, 2)][3], 255);
    assert!(layer.raster().unwrap()[(0, 0)][3] < 255);
    let original = layer.transform;
    let mut resized = original;
    resized.size = [88., 8.];
    transform::apply(&mut doc, original, resized, false).unwrap();
    let layer = doc.active_layer().unwrap();
    assert_eq!(layer.raster().unwrap().dimensions(), (88, 8));
    assert_eq!(layer.raster().unwrap()[(44, 0)][3], 0);
    assert_eq!(layer.raster().unwrap()[(44, 4)][3], 255);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("line.comp");
    project::save(&doc, &path).unwrap();
    let restored = project::load(&path).unwrap();
    assert_eq!(restored.layers, doc.layers);
    let shape = restored.active_layer().unwrap().shape.unwrap();
    let ShapeGeometry::Line {
        start,
        end,
        line_width,
    } = shape.geometry
    else {
        panic!("lost line geometry");
    };
    assert!(start[0] > end[0]);
    assert_eq!(line_width, 4.);
}

#[test]
fn saved_guides_follow_canvas_resize_image_resize_flip_and_crop() {
    use compositor::{
        edits,
        geometry::Sampling,
        guides::{Axis, Guide},
        image_resize, project,
    };
    let mut doc = Document::new(100, 80).unwrap();
    doc.guides = vec![
        Guide {
            id: uuid::Uuid::new_v4(),
            axis: Axis::Vertical,
            position: 20.,
        },
        Guide {
            id: uuid::Uuid::new_v4(),
            axis: Axis::Horizontal,
            position: 30.,
        },
    ];
    edits::canvas_size(&mut doc, 200, 160, [0.5, 0.5]).unwrap();
    assert_eq!(
        doc.guides.iter().map(|g| g.position).collect::<Vec<_>>(),
        [70., 70.]
    );
    image_resize::resize(&mut doc, 100, 80, 72., Sampling::Nearest).unwrap();
    edits::flip_canvas(&mut doc, true);
    edits::crop(&mut doc, [10., 5.], [90., 75.]).unwrap();
    assert_eq!(
        doc.guides.iter().map(|g| g.position).collect::<Vec<_>>(),
        [55., 30.]
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("guides.comp");
    project::save(&doc, &path).unwrap();
    assert_eq!(project::load(&path).unwrap().guides, doc.guides);
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["version"], 8);
    manifest["version"] = serde_json::json!(7);
    std::fs::write(
        path.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    assert!(project::load(&path).is_err());
    doc.guides.clear();
    project::save(&doc, &path).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["version"], 7);
}

#[test]
fn guide_and_grid_snapping_obey_visibility_and_master_switch() {
    use compositor::{
        geometry::Transform,
        guides::{Axis, Guide, Settings},
    };
    let mut doc = Document::new(100, 100).unwrap();
    doc.guides.push(Guide {
        id: uuid::Uuid::new_v4(),
        axis: Axis::Vertical,
        position: 22.,
    });
    let original = Transform::new(10, 10);
    let mut settings = Settings {
        to_grid: false,
        to_layers: false,
        to_bounds: false,
        ..Settings::default()
    };
    assert_eq!(
        settings.snap_move(&doc, original, [20., 20.], 3.).0,
        [22., 20.]
    );
    settings.guides = false;
    assert_eq!(
        settings.snap_move(&doc, original, [20., 20.], 3.).0,
        [20., 20.]
    );
    settings.grid = true;
    settings.to_grid = true;
    assert_eq!(
        settings.snap_move(&doc, original, [23., 23.], 3.).0,
        [24., 24.]
    );
    settings.snap = false;
    assert_eq!(
        settings.snap_move(&doc, original, [23., 23.], 3.).0,
        [23., 23.]
    );
}

#[test]
fn duplicate_folder_remaps_nested_clipping_and_preserves_sibling_order_and_undo() {
    let mut session = Session::new(Document::new(4, 4).unwrap(), None);
    let base = session.document.active.unwrap();
    let mut clipped = Layer::blank("Clipped", 4, 4);
    clipped.clip_source = Some(base);
    session.document.add(clipped).unwrap();
    session.document.selected.insert(base);
    session.group().unwrap();
    let inner = session.document.active.unwrap();
    session.group().unwrap();
    let outer = session.document.active.unwrap();
    let mut above = Layer::blank("Above", 4, 4);
    above.content = LayerContent::Group;
    let above_id = above.id;
    session.document.add(above).unwrap();
    session.document.select(outer, false);
    let before = session.document.clone();
    session
        .edit("Duplicate Folder", layer_ops::duplicate_active)
        .unwrap();
    let doc = &session.document;
    doc.validate().unwrap();
    let copy = doc.active.unwrap();
    assert_eq!(doc.descendants(copy).len(), 4);
    assert_eq!(doc.selected.len(), 1);
    assert_eq!(
        doc.layers
            .iter()
            .filter(|l| l.parent.is_none())
            .map(|l| l.id)
            .collect::<Vec<_>>(),
        vec![outer, copy, above_id]
    );
    let members = doc.descendants(copy);
    let copied_inner = doc
        .layers
        .iter()
        .find(|l| members.contains(&l.id) && l.id != copy && l.is_group())
        .unwrap();
    assert_ne!(copied_inner.id, inner);
    let copied_top = doc
        .layers
        .iter()
        .find(|l| members.contains(&l.id) && l.name == "Clipped")
        .unwrap();
    assert_eq!(copied_top.parent, Some(copied_inner.id));
    assert_ne!(copied_top.clip_source, Some(base));
    assert!(members.contains(&copied_top.clip_source.unwrap()));
    session.undo();
    assert_eq!(session.document, before);
    session.redo();
    session.document.validate().unwrap();
    assert_eq!(session.document.active, Some(copy));
}
