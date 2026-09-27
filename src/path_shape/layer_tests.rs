use super::*;
use crate::{
    document::{Document, Layer, LayerContent, Mask},
    geometry::Sampling,
    vector_path::Anchor,
};
use image::{GrayImage, Luma, Rgba};

fn geometry() -> BezierPath {
    BezierPath {
        anchors: [[8., 10.], [35., 10.], [25., 35.]]
            .map(Anchor::corner)
            .to_vec(),
        closure: Closure::Closed,
    }
}
fn style() -> Style {
    Style {
        fill: Some([240, 20, 80, 230]),
        stroke: Some(Stroke {
            width: 3.,
            color: [10, 200, 20, 255],
        }),
    }
}
fn document() -> Document {
    let mut doc = Document::new(64, 64).unwrap();
    create(&mut doc, "Triangle", geometry(), style()).unwrap();
    doc
}

#[test]
fn editable_path_layer_renders_clips_and_rasterizes_without_changing_appearance() {
    let mut doc = document();
    let id = doc.active.unwrap();
    let mut above = Layer::blank("Clipped blue", 64, 64);
    above.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        64,
        64,
        Rgba([0, 0, 255, 255]),
    ))));
    let above_id = above.id;
    doc.add(above).unwrap();
    crate::clipping::toggle(&mut doc, above_id).unwrap();
    assert_eq!(doc.active_layer().unwrap().clip_source, Some(id));
    let before = crate::render::render(&doc, 64, 64).unwrap();
    assert_eq!(before[(20, 20)], Rgba([0, 0, 255, 230]));
    assert_eq!(before[(50, 50)], Rgba([0; 4]));
    rasterize(&mut doc, id).unwrap();
    assert!(!doc.layer(id).unwrap().is_path_shape());
    assert_eq!(crate::render::render(&doc, 64, 64).unwrap(), before);
    assert!(doc.layer(id).unwrap().require_rasterized().is_ok());
}

#[test]
fn path_geometry_edits_freeze_mask_placement_and_reject_invalid_updates_atomically() {
    let mut doc = document();
    let id = doc.active.unwrap();
    let before = doc.active_layer().unwrap().transform;
    doc.active_layer_mut().unwrap().mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(3, 3, Luma([128]))),
        linked: true,
        enabled: true,
        placement: None,
    });
    let larger = geometry().mapped(|p| [p[0] + 30., p[1] + 15.]).unwrap();
    update(&mut doc, id, larger.clone(), style()).unwrap();
    assert_eq!(
        doc.active_layer().unwrap().mask.as_ref().unwrap().placement,
        Some(before)
    );
    assert_eq!(layer_path(&doc, id).unwrap(), larger);
    let original = doc.clone();
    assert!(update(&mut doc, id, BezierPath::default(), style()).is_err());
    assert_eq!(doc, original);
    assert!(create(&mut doc, "", geometry(), style()).is_err());
    assert_eq!(doc, original);
}

#[test]
fn pixel_edits_require_rasterization_but_mask_edits_preserve_geometry() {
    let mut doc = document();
    let original = doc.clone();
    assert!(crate::edits::fill(&mut doc, [0; 4], true, false).is_err());
    assert_eq!(doc, original);
    assert!(crate::floating::FloatingPixels::lift(&doc).is_err());
    crate::edits::add_mask(&mut doc, false).unwrap();
    crate::edits::fill(&mut doc, [128, 128, 128, 255], false, true).unwrap();
    assert!(doc.active_layer().unwrap().is_path_shape());
    assert_eq!(
        doc.active_layer().unwrap().path_shape().unwrap().source(),
        original
            .active_layer()
            .unwrap()
            .path_shape()
            .unwrap()
            .source()
    );
}

#[test]
fn duplicate_move_resize_and_canvas_rotation_retain_source_geometry() {
    let mut doc = document();
    let original_source = doc
        .active_layer()
        .unwrap()
        .path_shape()
        .unwrap()
        .source()
        .clone();
    crate::layer_ops::duplicate_active(&mut doc).unwrap();
    let id = doc.active.unwrap();
    let old = doc.active_layer().unwrap().transform;
    let next = Transform {
        size: old.size.map(|n| n * 2.),
        rotation: 35.,
        flip_y: true,
        ..old
    };
    crate::transform::apply(&mut doc, old, next, false).unwrap();
    let layer = doc.layer(id).unwrap();
    assert_eq!(layer.path_shape().unwrap().source(), &original_source);
    assert_eq!(
        layer.raster().unwrap().dimensions(),
        (next.size[0] as u32, next.size[1] as u32)
    );
    let before = layer_path(&doc, id).unwrap();
    crate::canvas_rotation::rotate(&mut doc, crate::canvas_rotation::QuarterTurn::Clockwise)
        .unwrap();
    let expected = before.mapped(|p| [64. - p[1], p[0]]).unwrap();
    let actual = layer_path(&doc, id).unwrap();
    for (a, b) in actual.anchors.iter().zip(expected.anchors) {
        assert!((a.point[0] - b.point[0]).hypot(a.point[1] - b.point[1]) < 1e-8);
    }
    let prior = doc.clone();
    assert!(crate::image_resize::resize(&mut doc, 128, 64, 72., Sampling::High).is_err());
    assert_eq!(doc, prior);
    crate::image_resize::resize(&mut doc, 128, 128, 72., Sampling::High).unwrap();
    assert_eq!(
        doc.layer(id).unwrap().path_shape().unwrap().source(),
        &original_source
    );
}

#[test]
fn path_shape_creation_stays_inside_active_artboard_and_undo_restores_sources() {
    let mut doc = Document::new(64, 64).unwrap();
    let board = crate::artboard::create(&mut doc, "Board", Transform::new(64, 64), [0; 4]).unwrap();
    let mut session = crate::session::Session::new(doc, None);
    session
        .edit("Add Path Shape", |doc| {
            create(doc, "Path", geometry(), style()).map(|_| ())
        })
        .unwrap();
    let id = session.document.active.unwrap();
    assert_eq!(session.document.layer(id).unwrap().parent, Some(board));
    session
        .edit("Restyle Path Shape", |doc| {
            update(
                doc,
                id,
                geometry(),
                Style {
                    fill: Some([0, 0, 0, 255]),
                    stroke: None,
                },
            )
        })
        .unwrap();
    assert!(!session.can_fade());
    session.undo();
    assert_eq!(
        session
            .document
            .layer(id)
            .unwrap()
            .path_shape()
            .unwrap()
            .source()
            .style,
        style()
    );
    session.undo();
    assert!(session.document.layer(id).is_none());
    session.redo();
    assert!(session.document.layer(id).unwrap().is_path_shape());
}

#[test]
fn project_and_recovery_roundtrip_path_shape_source_and_cached_resolution() {
    let mut doc = document();
    let old = doc.active_layer().unwrap().transform;
    let transform = Transform {
        size: [old.size[0] * 2., old.size[1] * 3.],
        rotation: 25.,
        flip_x: true,
        ..old
    };
    crate::transform::apply(&mut doc, old, transform, false).unwrap();
    let temp = tempfile::tempdir().unwrap();
    for recovery in [false, true] {
        let path = temp.path().join(format!("shape-{recovery}.comp"));
        if recovery {
            crate::project::save_recovery(&doc, &path).unwrap();
        } else {
            crate::project::save(&doc, &path).unwrap();
        }
        let restored = crate::project::load(&path).unwrap();
        assert_eq!(restored, doc);
        assert_eq!(
            crate::render::render(&restored, 64, 64).unwrap(),
            crate::render::render(&doc, 64, 64).unwrap()
        );
        if !recovery {
            let manifest: serde_json::Value =
                serde_json::from_slice(&std::fs::read(path.join("manifest.json")).unwrap())
                    .unwrap();
            assert_eq!(
                manifest["layers"].as_array().unwrap().len(),
                doc.layers.len()
            );
            assert!(
                manifest["layers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|layer| layer.get("shape").is_none_or(serde_json::Value::is_null))
            );
        }
    }
}

#[test]
fn psd_exports_raster_shape_layers_with_an_explicit_notice() {
    let doc = document();
    let before = doc.clone();
    assert!(
        crate::psd::export_report(&doc)
            .description()
            .contains("editable shape is rasterized")
    );
    let bytes = crate::psd::encode(&doc).unwrap();
    let reopened = crate::psd::decode(&bytes).unwrap().document;
    assert!(!reopened.layers.iter().any(Layer::is_path_shape));
    assert_eq!(
        crate::render::render(&doc, 64, 64).unwrap(),
        crate::render::render(&reopened, 64, 64).unwrap()
    );
    assert_eq!(doc, before);
}

#[test]
fn copying_external_clips_or_baking_deleted_bases_requires_rasterization() {
    let mut doc = document();
    let shape = doc.active.unwrap();
    let base = doc.layers[0].id;
    doc.active_layer_mut().unwrap().clip_source = Some(base);
    let mut destination = Document::new(64, 64).unwrap();
    let untouched = destination.clone();
    assert!(crate::layer_ops::copy_to_project(&doc, &mut destination, shape, [32., 32.]).is_err());
    assert_eq!(destination, untouched);
    doc.select(base, false);
    let original = doc.clone();
    assert!(crate::clipping::delete_selected(&mut doc, crate::clipping::DeleteMode::Bake).is_err());
    assert_eq!(doc, original);
    crate::clipping::delete_selected(&mut doc, crate::clipping::DeleteMode::Unlink).unwrap();
    assert!(doc.layer(shape).unwrap().is_path_shape());
    assert_eq!(doc.layer(shape).unwrap().clip_source, None);
}

#[test]
fn export_size_artboards_retain_editable_source_with_regenerated_cache() {
    use crate::export_sizes::{Batch, Fit, Format, Source};
    let mut doc = document();
    let source_id = doc.active.unwrap();
    let source = doc.active_layer().unwrap().clone();
    let board = Batch::new(vec![[128, 128]], Fit::Contain, Format::Png)
        .unwrap()
        .add_artboards(&mut doc, Source::Canvas)
        .unwrap()[0];
    let clone = doc
        .layers
        .iter()
        .find(|layer| layer.is_path_shape() && layer.id != source_id)
        .unwrap();
    assert_eq!(crate::artboard::owner(&doc, clone.id), Some(board));
    assert_eq!(
        clone.path_shape().unwrap().source(),
        source.path_shape().unwrap().source()
    );
    assert_eq!(
        clone.raster().unwrap().width(),
        source.raster().unwrap().width() * 2
    );
    assert_eq!(
        clone.raster().unwrap().height(),
        source.raster().unwrap().height() * 2
    );
    assert_eq!(doc.layer(source_id).unwrap(), &source);
}

#[test]
fn shape_geometry_uses_existing_selection_fill_and_stroke_on_a_separate_layer() {
    use crate::{
        brush::{Brush, sampled},
        path_operations::Operation,
        selection::SelectionMode,
    };
    let mut doc = document();
    let shape_id = doc.active.unwrap();
    let path = layer_path(&doc, shape_id).unwrap();
    let shape = doc.active_layer().unwrap().clone();
    let raster = doc.layers[0].id;
    doc.select(raster, false);
    Operation::Select {
        mode: SelectionMode::Replace,
        antialiased: true,
    }
    .apply_geometry(&mut doc, &path)
    .unwrap();
    assert!(doc.selection.as_ref().unwrap().coverage([20., 20.]) > 0.);
    Operation::Fill {
        color: [30, 40, 50, 255],
        mask: false,
        antialiased: true,
    }
    .apply_geometry(&mut doc, &path)
    .unwrap();
    assert_eq!(
        doc.active_layer().unwrap().raster().unwrap()[(20, 20)],
        Rgba([30, 40, 50, 255])
    );
    doc.selection = None;
    Operation::Stroke {
        brush: Brush {
            diameter: 2.,
            color: [255; 4],
            ..Brush::default()
        },
        shape: sampled::Shape::Round,
        mask: false,
    }
    .apply_geometry(&mut doc, &path)
    .unwrap();
    assert_eq!(doc.layer(shape_id).unwrap(), &shape);
    assert!(doc.paths.is_empty());
}
