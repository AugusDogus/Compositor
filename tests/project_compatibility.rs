//! Independent JSON follows ProjectManifest/ProjectLayerRecord in
//! Compositor/IO/ProjectStore.swift, including fields omitted by older versions.
use compositor::{blend::Blend, document::LayerContent, project};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use serde_json::json;

#[test]
fn swift_project_versions_preserve_defaults_hierarchy_masks_and_adjustments() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("source.comp");
    std::fs::create_dir_all(path.join("images")).unwrap();
    let id = |n: u32| format!("AAAAAAAA-0000-0000-0000-{n:012X}");
    let transform = json!({"origin":[3.0, 4.0], "size":[20.0, 12.0],
        "rotation":23.0, "flipX":true, "flipY":false, "sampling":"High quality"});
    RgbaImage::from_pixel(2, 2, Rgba([23, 45, 67, 180]))
        .save(path.join("images").join(format!("{}.png", id(1))))
        .unwrap();
    for n in [1, 2] {
        GrayImage::from_pixel(2, 2, Luma([192]))
            .save(path.join("images").join(format!("{}.mask.png", id(n))))
            .unwrap();
    }
    for version in 1..=7 {
        let mut base = json!({"id":id(1), "name":"Pixels", "isVisible":true,
            "transform":transform, "imageFile":format!("{}.png", id(1))});
        if version >= 2 {
            base["parentID"] = json!(id(2));
        }
        if version >= 3 {
            base["opacity"] = json!(0.63);
            base["blendMode"] = json!("Multiply");
        }
        if version >= 4 {
            base["maskFile"] = json!(format!("{}.mask.png", id(1)));
        }
        let mut layers = vec![base];
        if version >= 2 {
            let mut group = json!({"id":id(2), "name":"Folder", "isVisible":true,
                "transform":transform, "isGroup":true});
            if version >= 6 {
                group["maskFile"] = json!(format!("{}.mask.png", id(2)));
                group["maskEnabled"] = json!(false);
                group["maskLinked"] = json!(false);
                group["maskPlacement"] = transform.clone();
            }
            layers.push(group);
        }
        if version >= 5 {
            layers.push(json!({"id":id(3), "name":"Clipped", "isVisible":true,
                "transform":transform, "parentID":id(2), "maskSourceID":id(1)}));
        }
        if version >= 7 {
            layers.push(json!({"id":id(4), "name":"Hue adjustment", "isVisible":true,
                "transform":transform, "adjustment":{
                    "kind":"Hue/Saturation", "hue":0.0, "saturation":0.0,
                    "lightness":0.0, "colorize":false,
                    "hsvSettings":{"range":"Reds", "colorize":false, "invertRange":true,
                        "adjustments":["Reds",{"hue":35.0,"saturation":20.0,"lightness":-10.0}],
                        "bands":["Reds",{"falloffStart":310.0,"rangeStart":345.0,"rangeEnd":15.0,"falloffEnd":50.0}]}
                }}));
        }
        let manifest = json!({"format":"com.compositor.project", "version":version,
            "colorSpace":"sRGB", "documentID":id(10), "width":80, "height":60,
            "activeLayerID":id(1), "layers":layers});
        std::fs::write(
            path.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let doc = project::load(&path).unwrap();
        assert_eq!(doc.resolution, 72.);
        assert_eq!(
            doc.layers[0].raster().unwrap()[(0, 0)],
            Rgba([23, 45, 67, 180])
        );
        assert_eq!(doc.layers[0].transform.rotation, 23.);
        assert!(doc.layers[0].transform.flip_x);
        assert_eq!(
            doc.layers[0].blend,
            if version >= 3 {
                Blend::Multiply
            } else {
                Blend::Normal
            }
        );
        if version >= 4 {
            let mask = doc.layers[0].mask.as_ref().unwrap();
            assert!(mask.enabled && mask.linked);
            assert!(mask.placement.is_none());
            assert_eq!(mask.pixels[(0, 0)][0], 192);
        }
        if version >= 5 {
            assert_eq!(doc.layers[2].clip_source, Some(doc.layers[0].id));
        }
        if version >= 6 {
            let mask = doc.layers[1].mask.as_ref().unwrap();
            assert!(!mask.enabled && !mask.linked);
            assert_eq!(mask.placement, Some(doc.layers[0].transform));
        }
        if version == 7 {
            let LayerContent::Adjustment(a) = &doc.layers[3].content else {
                panic!("Missing adjustment")
            };
            let hue = a.hsv_settings.as_ref().unwrap();
            assert!(hue.invert_range);
            assert_eq!(hue.adjustments[0].1.hue, 35.);
            assert_eq!(hue.bands[0].1.falloff_start, 310.);
        }
        let saved = directory.path().join(format!("version-{version}.comp"));
        project::save(&doc, &saved).unwrap();
        assert_eq!(project::load(&saved).unwrap(), doc);
    }
}
