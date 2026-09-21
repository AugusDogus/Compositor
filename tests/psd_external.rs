//! Independent Photoshop-produced inputs, not ag-psd writer roundtrips.
//! Run scripts/fetch-psd-fixtures.sh, then this test target with --ignored.
//! These checks do not claim export verification in the Photoshop application.
use compositor::{
    adjustment::{Adjustment, CurvePoint, Kind},
    document::{Document, Layer, LayerContent, ShapeGeometry},
    project, psd, render,
};
use std::path::PathBuf;

fn fixture(name: &str) -> psd::Imported {
    let directory = std::env::var_os("COMPOSITOR_PSD_FIXTURE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/fixtures/psd-external")
        });
    psd::load(&directory.join(name)).unwrap_or_else(|error| {
        panic!("Cannot import {name}: {error}. Run scripts/fetch-psd-fixtures.sh first.")
    })
}

fn layer<'a>(doc: &'a Document, name: &str) -> &'a Layer {
    doc.layers.iter().find(|layer| layer.name == name).unwrap()
}

fn adjustment<'a>(doc: &'a Document, name: &str) -> &'a Adjustment {
    let LayerContent::Adjustment(adjustment) = &layer(doc, name).content else {
        panic!("{name} lost its editable adjustment");
    };
    adjustment
}

fn project_roundtrip(doc: &Document) -> Document {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("external.comp");
    project::save(doc, &path).unwrap();
    let restored = project::load(&path).unwrap();
    assert_eq!(restored.layers.len(), doc.layers.len());
    for (before, after) in doc.layers.iter().zip(&restored.layers) {
        assert_eq!(before.name, after.name);
        assert_eq!(before.shape, after.shape);
        assert_eq!(before.transform, after.transform);
        if let LayerContent::Adjustment(value) = &before.content {
            assert_eq!(value.as_ref(), adjustment(&restored, &before.name));
        }
    }
    assert_eq!(
        render::render(doc, doc.width, doc.height).unwrap(),
        render::render(&restored, doc.width, doc.height).unwrap()
    );
    restored
}

#[test]
#[ignore = "downloads external Photoshop artwork via scripts/fetch-psd-fixtures.sh"]
fn photoshop_cc2019_primitives_stay_editable_in_projects() {
    let imported = fixture("shapes.psd");
    let doc = &imported.document;
    assert_eq!((doc.width, doc.height, doc.layers.len()), (500, 500, 8));
    for (name, geometry, radius, origin, size) in [
        (
            "矩形 1",
            ShapeGeometry::Rectangle,
            0.,
            [18., 25.],
            [124., 75.],
        ),
        (
            "圆角矩形 1",
            ShapeGeometry::Rectangle,
            10.,
            [179., 20.],
            [157., 86.],
        ),
        (
            "椭圆 1",
            ShapeGeometry::Ellipse,
            0.,
            [391., 22.],
            [90., 158.],
        ),
        (
            "椭圆 2",
            ShapeGeometry::Ellipse,
            0.,
            [48., 152.],
            [143., 143.],
        ),
    ] {
        let source = layer(doc, name);
        let shape = source
            .shape
            .expect("Photoshop primitive must stay editable");
        assert_eq!(shape.geometry, geometry);
        assert_eq!(shape.corner_radius, radius);
        assert_eq!(source.transform.origin, origin);
        assert_eq!(source.transform.size, size);
    }
    for name in ["apple", "calendar", "pin"] {
        assert!(
            layer(doc, name)
                .raster()
                .unwrap()
                .pixels()
                .any(|p| p[3] != 0)
        );
        assert!(
            imported
                .report
                .changes
                .iter()
                .any(|s| s.starts_with(name) && s.contains("saved pixels"))
        );
    }
    let mut saved = project_roundtrip(doc);
    let target = layer(&saved, "圆角矩形 1").id;
    saved.active = Some(target);
    saved.selected = std::collections::HashSet::from([target]);
    let original = layer(&saved, "圆角矩形 1").transform;
    let mut resized = original;
    resized.size = [314., 172.];
    compositor::transform::apply(&mut saved, original, resized, false).unwrap();
    let resized = layer(&saved, "圆角矩形 1");
    assert_eq!(resized.raster().unwrap().dimensions(), (314, 172));
    assert_eq!(resized.shape, layer(doc, "圆角矩形 1").shape);
}

#[test]
#[ignore = "downloads external Photoshop artwork via scripts/fetch-psd-fixtures.sh"]
fn photoshop_cs6_adjustments_preserve_parameters_through_project_and_psd() {
    let imported = fixture("adjustments.psd");
    let doc = &imported.document;
    assert_eq!((doc.width, doc.height, doc.layers.len()), (200, 200, 4));
    let levels = adjustment(doc, "Levels 1");
    assert_eq!(levels.kind, Kind::Levels);
    assert_eq!(levels.levels.ranges[0].black, 10.);
    assert_eq!(levels.levels.ranges[0].white, 245.);
    let curves = adjustment(doc, "Curves 1");
    assert_eq!(curves.kind, Kind::Curves);
    assert_eq!(
        curves.curves.channels[0],
        [
            (0., 0.),
            (38., 17.),
            (212., 231.),
            (231., 250.),
            (255., 255.)
        ]
        .map(|(x, y)| CurvePoint { x, y })
    );
    let hue = adjustment(doc, "Hue/Saturation 1");
    assert_eq!(hue.kind, Kind::HueSaturation);
    assert_eq!(
        (hue.hue, hue.saturation, hue.lightness, hue.colorize),
        (0., -40., 5., false)
    );
    assert!(
        imported
            .report
            .changes
            .iter()
            .any(|s| s.starts_with("Exposure 1:") && s.contains("omitted"))
    );
    let saved = project_roundtrip(doc);
    let exported = psd::decode(&psd::encode(&saved).unwrap()).unwrap().document;
    for name in ["Levels 1", "Curves 1", "Hue/Saturation 1"] {
        assert_eq!(adjustment(doc, name), adjustment(&exported, name));
    }
    assert_eq!(
        render::render(doc, 200, 200).unwrap(),
        render::render(&exported, 200, 200).unwrap()
    );
}

#[test]
#[ignore = "downloads external Photoshop artwork via scripts/fetch-psd-fixtures.sh"]
fn photoshop_22_polygon_keeps_placement_and_reports_rasterization() {
    let imported = fixture("polygon.psd");
    let doc = &imported.document;
    assert_eq!((doc.width, doc.height, doc.layers.len()), (800, 800, 2));
    let polygon = layer(doc, "多边形 1");
    assert!(polygon.shape.is_none());
    assert_eq!(polygon.transform.origin, [303., 352.]);
    assert_eq!(polygon.transform.size, [193., 95.]);
    assert!(polygon.raster().unwrap().pixels().any(|p| p[3] != 0));
    assert!(
        imported
            .report
            .changes
            .iter()
            .any(|s| s.contains("Bézier vector paths are rasterized"))
    );
    project_roundtrip(doc);
}
