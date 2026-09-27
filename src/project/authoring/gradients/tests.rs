use super::*;
use crate::gradient_overlay::{OpacityStop, OpacityStops, Overlay, Style};
use image::{Rgba, RgbaImage};

fn document(enabled: bool) -> Document {
    let mut doc = Document::new(7, 5).unwrap();
    doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        7,
        5,
        Rgba([37, 91, 180, 173]),
    ))));
    doc.layers[0].effects = Some(crate::effects::LayerEffects {
        gradient_overlay: Some(Box::new(Overlay {
            enabled,
            style: Style::Radial,
            reverse: true,
            angle: -135.,
            opacity: 0.73,
            opacity_stops: OpacityStops::new(vec![
                OpacityStop {
                    position: 0.,
                    opacity: 0.123456789,
                },
                OpacityStop {
                    position: 0.37,
                    opacity: 0.5,
                },
                OpacityStop {
                    position: 1.,
                    opacity: 1.,
                },
            ])
            .unwrap(),
            ..Default::default()
        })),
        ..Default::default()
    });
    doc
}
#[test]
fn gradient_overlay_project_recovery_and_projection_keep_independent_stops() {
    let directory = tempfile::tempdir().unwrap();
    for enabled in [false, true] {
        let doc = document(enabled);
        for recovery in [false, true] {
            let path = directory.path().join(format!("{enabled}-{recovery}.comp"));
            if recovery {
                crate::project::save_recovery(&doc, &path).unwrap();
            } else {
                crate::project::save(&doc, &path).unwrap();
            }
            let reopened = crate::project::load(&path).unwrap();
            assert_eq!(reopened, doc);
            let metadata: serde_json::Value =
                serde_json::from_slice(&fs::read(path.join("authoring/adjustments.json")).unwrap())
                    .unwrap();
            assert_eq!(metadata["version"], 6);
            if !recovery {
                let projection = super::super::load_native(&path).unwrap();
                assert_eq!(
                    crate::render::render(&projection, 7, 5).unwrap(),
                    crate::render::render(&doc, 7, 5).unwrap()
                );
            }
        }
    }
}
#[test]
fn gradient_overlay_native_encoder_rejects_loss_and_psd_preserves_rendered_pixels() {
    let doc = document(true);
    assert!(
        crate::project::native_metadata(&doc)
            .unwrap_err()
            .to_string()
            .contains("Gradient Overlay")
    );
    assert!(
        crate::psd::export_report(&doc)
            .description()
            .contains("Gradient Overlay")
    );
    let reopened = crate::psd::decode(&crate::psd::encode(&doc).unwrap())
        .unwrap()
        .document;
    assert_eq!(
        crate::render::render(&reopened, 7, 5).unwrap(),
        crate::render::render(&doc, 7, 5).unwrap()
    );
}
#[test]
fn gradient_overlay_sidecar_rejects_invalid_settings_and_references() {
    let directory = tempfile::tempdir().unwrap();
    for mode in [
        "version",
        "layer",
        "duplicate",
        "angle",
        "opacity",
        "count",
        "color-alpha",
    ] {
        let path = directory.path().join(format!("{mode}.comp"));
        crate::project::save(&document(true), &path).unwrap();
        let file = path.join("authoring/adjustments.json");
        let mut data: serde_json::Value =
            serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        match mode {
            "version" => data["version"] = 5.into(),
            "layer" => data["gradients"][0]["layer"] = Uuid::new_v4().to_string().into(),
            "duplicate" => {
                let copy = data["gradients"][0].clone();
                data["gradients"].as_array_mut().unwrap().push(copy);
            }
            "angle" => data["gradients"][0]["overlay"]["angle"] = 361.into(),
            "opacity" => data["gradients"][0]["overlay"]["opacityStops"][0]["opacity"] = 2.into(),
            "count" => data["gradients"][0]["overlay"]["opacityStops"] = serde_json::json!([]),
            _ => data["gradients"][0]["overlay"]["stops"][0]["color"][3] = 128.into(),
        }
        fs::write(file, serde_json::to_vec(&data).unwrap()).unwrap();
        super::super::write_json(
            &path.join(super::super::NAME),
            &super::super::Index {
                version: 1,
                purpose: super::super::Purpose::Project,
                files: super::super::bound_files(&path).unwrap(),
            },
        )
        .unwrap();
        assert!(crate::project::load(&path).is_err(), "{mode}");
    }
}

#[test]
fn gradient_and_pattern_sources_coexist_with_editable_path_geometry() {
    use crate::{
        path_shape, pattern,
        vector_path::{Anchor, BezierPath, Closure},
    };
    let mut doc = document(true);
    let mut effects = doc.layers[0].effects.clone().unwrap();
    effects.pattern_overlay = Some(Box::new(pattern::Overlay::new(
        pattern::Pattern::from_pixels("Tile", RgbaImage::from_pixel(1, 1, Rgba([255, 0, 0, 128])))
            .unwrap(),
    )));
    let id = path_shape::create(
        &mut doc,
        "Triangle",
        BezierPath {
            anchors: [[1., 1.], [5., 1.], [3., 4.]].map(Anchor::corner).to_vec(),
            closure: Closure::Closed,
        },
        path_shape::Style {
            fill: Some([255; 4]),
            stroke: None,
        },
    )
    .unwrap();
    doc.layers
        .iter_mut()
        .find(|layer| layer.id == id)
        .unwrap()
        .effects = Some(effects);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Combined.comp");
    crate::project::save(&doc, &path).unwrap();
    assert_eq!(crate::project::load(&path).unwrap(), doc);
    assert_eq!(
        crate::render::render(&super::super::load_native(&path).unwrap(), 7, 5).unwrap(),
        crate::render::render(&doc, 7, 5).unwrap()
    );
}
