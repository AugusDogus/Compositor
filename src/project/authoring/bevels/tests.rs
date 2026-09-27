use super::*;
use crate::{
    bevel::{Settings as Bevel, Style},
    effects::LayerEffects,
};
use image::{Rgba, RgbaImage};

fn document(style: Style, enabled: bool) -> Document {
    let mut doc = Document::new(11, 9).unwrap();
    doc.layers[0].content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(11, 9, |x, y| {
            Rgba([37, 91, 180, if x > 2 && y > 1 { 173 } else { 0 }])
        }))));
    doc.layers[0].effects = Some(LayerEffects {
        bevel: Some(Box::new(Bevel {
            enabled,
            style,
            size: 2.5,
            depth: 173.,
            angle: -48.,
            altitude: 63.,
            highlight_opacity: 0.3456789,
            shadow_opacity: 0.9876543,
        })),
        ..Default::default()
    });
    doc
}

#[test]
fn bevel_project_and_recovery_preserve_sources_styles_and_disabled_settings() {
    let directory = tempfile::tempdir().unwrap();
    for style in [Style::Inner, Style::Outer, Style::Emboss] {
        for enabled in [false, true] {
            let doc = document(style, enabled);
            for recovery in [false, true] {
                let path = directory
                    .path()
                    .join(format!("{style:?}-{enabled}-{recovery}.comp"));
                if recovery {
                    crate::project::save_recovery(&doc, &path).unwrap();
                } else {
                    crate::project::save(&doc, &path).unwrap();
                }
                assert_eq!(crate::project::load(&path).unwrap(), doc);
                let metadata: serde_json::Value = serde_json::from_slice(
                    &fs::read(path.join("authoring/adjustments.json")).unwrap(),
                )
                .unwrap();
                assert_eq!(metadata["version"], 7);
                if !recovery {
                    let projection = super::super::load_native(&path).unwrap();
                    let rendered_projection = crate::render::render(&projection, 11, 9).unwrap();
                    let rendered_source = crate::render::render(&doc, 11, 9).unwrap();
                    for (projected, source) in
                        rendered_projection.pixels().zip(rendered_source.pixels())
                    {
                        assert_eq!(projected[3], source[3]);
                        // Fully transparent RGB is not part of the rendered
                        // view and can differ after flattening and resampling.
                        if source[3] > 0 {
                            assert_eq!(projected, source);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn bevel_native_encoder_rejects_loss_and_psd_preserves_rendered_pixels() {
    for style in [Style::Inner, Style::Outer, Style::Emboss] {
        let doc = document(style, true);
        assert!(
            crate::project::native_metadata(&doc)
                .unwrap_err()
                .to_string()
                .contains("Bevel/Emboss")
        );
        assert!(
            crate::psd::export_report(&doc)
                .description()
                .contains("Bevel/Emboss")
        );
        let reopened = crate::psd::decode(&crate::psd::encode(&doc).unwrap())
            .unwrap()
            .document;
        assert_eq!(
            crate::render::render(&reopened, 11, 9).unwrap(),
            crate::render::render(&doc, 11, 9).unwrap()
        );
    }
}

#[test]
fn bevel_sidecar_rejects_invalid_settings_and_layer_references() {
    let directory = tempfile::tempdir().unwrap();
    for mode in [
        "version",
        "layer",
        "duplicate",
        "depth",
        "altitude",
        "style",
    ] {
        let path = directory.path().join(format!("{mode}.comp"));
        crate::project::save(&document(Style::Inner, true), &path).unwrap();
        let file = path.join("authoring/adjustments.json");
        let mut data: serde_json::Value =
            serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        match mode {
            "version" => data["version"] = 6.into(),
            "layer" => data["bevels"][0]["layer"] = Uuid::new_v4().to_string().into(),
            "duplicate" => {
                let copy = data["bevels"][0].clone();
                data["bevels"].as_array_mut().unwrap().push(copy);
            }
            "depth" => data["bevels"][0]["settings"]["depth"] = 0.into(),
            "altitude" => data["bevels"][0]["settings"]["altitude"] = 91.into(),
            _ => data["bevels"][0]["settings"]["style"] = "unknown".into(),
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
fn bevel_pattern_and_gradient_coexist_with_native_editable_shapes() {
    use crate::{
        path_shape, pattern,
        vector_path::{Anchor, BezierPath, Closure},
    };
    let mut doc = document(Style::Emboss, true);
    let mut effects = doc.layers[0].effects.clone().unwrap();
    effects.pattern_overlay = Some(Box::new(pattern::Overlay::new(
        pattern::Pattern::from_pixels("Tile", RgbaImage::from_pixel(1, 1, Rgba([255, 0, 0, 128])))
            .unwrap(),
    )));
    effects.gradient_overlay = Some(Box::default());
    let id = path_shape::create(
        &mut doc,
        "Triangle",
        BezierPath {
            anchors: [[1., 1.], [8., 1.], [4., 6.]].map(Anchor::corner).to_vec(),
            closure: Closure::Closed,
        },
        path_shape::Style {
            fill: Some([255; 4]),
            stroke: None,
        },
    )
    .unwrap();
    doc.layers.iter_mut().find(|l| l.id == id).unwrap().effects = Some(effects);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("combined.comp");
    crate::project::save(&doc, &path).unwrap();
    assert_eq!(crate::project::load(&path).unwrap(), doc);
}
