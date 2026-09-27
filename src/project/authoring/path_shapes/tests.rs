use super::super::*;
use crate::{
    path_shape::{self, Style},
    vector_path::{Anchor, BezierPath, Closure},
};

#[test]
fn malformed_path_shape_metadata_is_rejected_before_image_decode() {
    let mut doc = Document::new(20, 20).unwrap();
    let id = path_shape::create(
        &mut doc,
        "Path",
        BezierPath {
            anchors: [[3., 3.], [16., 3.], [16., 16.]]
                .map(Anchor::corner)
                .to_vec(),
            closure: Closure::Closed,
        },
        Style {
            fill: Some([255; 4]),
            stroke: None,
        },
    )
    .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("Invalid.comp");
    for case in 0..6 {
        crate::project::save(&doc, &path).unwrap();
        let settings_path = path.join(SOURCE).join(SETTINGS);
        let mut settings: serde_json::Value = read_json(temp.path(), &settings_path).unwrap();
        match case {
            0 => settings["path_shapes"][0]["source"]["size"] = serde_json::json!([0, 20]),
            1 => settings["path_shapes"][0]["source"]["style"]["fill"] = serde_json::Value::Null,
            2 => settings["path_shapes"][0]["layer"] = Uuid::new_v4().to_string().into(),
            3 => {
                let duplicate = settings["path_shapes"][0].clone();
                settings["path_shapes"]
                    .as_array_mut()
                    .unwrap()
                    .push(duplicate);
            }
            4 => settings["version"] = 3.into(),
            _ => {
                settings["path_shapes"][0]["source"]["geometry"]["anchors"][0]["point"] =
                    serde_json::json!([-10., 0.])
            }
        }
        write_json(&settings_path, &settings).unwrap();
        fs::write(
            path.join(SOURCE).join("images").join(asset_name(id, false)),
            b"invalid PNG",
        )
        .unwrap();
        write_json(
            &path.join(NAME),
            &Index {
                version: 1,
                purpose: Purpose::Project,
                files: bound_files(&path).unwrap(),
            },
        )
        .unwrap();
        let error = crate::project::load(&path).unwrap_err().to_string();
        assert!(
            error.contains("shape")
                || error.contains("Path")
                || error.contains("snapshot")
                || error.contains("Dimensions"),
            "case {case}: {error}"
        );
        let OpenResult::RenderedCopyAvailable { fingerprint, .. } = inspect_open(&path).unwrap()
        else {
            panic!("Expected native raster fallback");
        };
        let fallback = load_rendered_copy(&path, &fingerprint).unwrap();
        assert!(!fallback.layers.iter().any(Layer::is_path_shape));
        assert_eq!(
            crate::render::render(&fallback, 20, 20).unwrap(),
            crate::render::render(&doc, 20, 20).unwrap()
        );
    }
}
