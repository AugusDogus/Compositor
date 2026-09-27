use compositor::{
    adjustment::ExtendedAdjustment,
    document::{Document, Layer, LayerContent, Mask},
    filters::{self, Filter},
    project, psd, render,
    selection::Selection,
    shadows_highlights::Settings,
};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use std::sync::Arc;

fn document() -> Document {
    let mut doc = Document::new(12, 8).unwrap();
    doc.layers[0].content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(12, 8, |x, y| {
            Rgba([
                (x * 19 + 12) as u8,
                (y * 27 + 7) as u8,
                91,
                if x == 0 { 0 } else { 193 },
            ])
        }))));
    let mut layer = Layer::blank("Shadows/Highlights", 12, 8);
    layer.content = LayerContent::ExtendedAdjustment(Box::new(
        ExtendedAdjustment::ShadowsHighlights(Settings::new(63.125, 27.875, 4.25).unwrap()),
    ));
    layer.opacity = 0.65;
    layer.mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_fn(12, 8, |_, y| {
            Luma([if y < 4 { 255 } else { 64 }])
        })),
        enabled: true,
        linked: true,
        placement: None,
    });
    doc.add(layer).unwrap();
    doc
}

#[test]
fn shadows_project_and_recovery_retain_sources_parameters_masks_and_rendering() {
    let original = document();
    let expected = render::render(&original, 12, 8).unwrap();
    let directory = tempfile::tempdir().unwrap();
    for recovery in [false, true] {
        let path = directory.path().join(if recovery {
            "recovery.comp"
        } else {
            "project.comp"
        });
        if recovery {
            project::save_recovery(&original, &path).unwrap();
        } else {
            project::save(&original, &path).unwrap();
        }
        assert!(path.join("linux-editing.json").is_file());
        let restored = project::load(&path).unwrap();
        assert_eq!(restored, original);
        assert_eq!(render::render(&restored, 12, 8).unwrap(), expected);
        assert_eq!(restored.layers[0].raster(), original.layers[0].raster());
    }
}

#[test]
fn shadows_native_compatibility_copy_retains_the_visible_result() {
    let original = document();
    let expected = render::render(&original, 12, 8).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("compatibility.comp");
    project::save(&original, &path).unwrap();
    // Simulate a reader unaware of the Linux editing sidecar.
    std::fs::remove_file(path.join("linux-editing.json")).unwrap();
    std::fs::remove_dir_all(path.join("authoring")).unwrap();
    let compatibility = project::load(&path).unwrap();
    let actual = render::render(&compatibility, 12, 8).unwrap();
    for (a, b) in actual.pixels().zip(expected.pixels()) {
        assert_eq!(a[3], b[3]);
        if b[3] != 0 {
            assert_eq!(a, b);
        }
    }
}

#[test]
fn shadows_psd_explicitly_reports_flattening_and_retains_the_composite() {
    let original = document();
    let report = psd::export_report(&original).description();
    assert!(report.contains("Shadows/Highlights"), "{report}");
    assert!(report.contains("flattened"), "{report}");
    let expected = render::render(&original, 12, 8).unwrap();
    let imported = psd::decode(&psd::encode(&original).unwrap()).unwrap();
    assert_eq!(imported.document.layers.len(), 1);
    assert!(!matches!(
        imported.document.layers[0].content,
        LayerContent::ExtendedAdjustment(_)
    ));
    let actual = render::render(&imported.document, 12, 8).unwrap();
    for (a, b) in actual.pixels().zip(expected.pixels()) {
        assert_eq!(a[3], b[3]);
        if b[3] != 0 {
            assert_eq!(a, b);
        }
    }
}

#[test]
fn direct_shadows_filter_preserves_selection_mask_source_snapshot_and_alpha() {
    let mut doc = document();
    doc.layers.pop();
    doc.active = Some(doc.layers[0].id);
    doc.layers[0].mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(12, 8, Luma([177]))),
        enabled: true,
        linked: true,
        placement: None,
    });
    doc.selection = Some(Selection::from_mask(GrayImage::from_fn(12, 8, |x, _| {
        Luma([if x < 6 { 255 } else { 0 }])
    })));
    let original = doc.clone();
    let source = original.layers[0].raster().unwrap().clone();
    filters::apply(
        &mut doc,
        Filter::ShadowsHighlights(Settings::new(0., 0., 4.).unwrap()),
        false,
    )
    .unwrap();
    assert_eq!(doc, original);
    filters::apply(
        &mut doc,
        Filter::ShadowsHighlights(Settings::new(100., 0., 4.).unwrap()),
        false,
    )
    .unwrap();
    let result = doc.layers[0].raster().unwrap();
    assert!(result[(3, 3)][0] > source[(3, 3)][0]);
    for y in 0..8 {
        for x in 0..12 {
            assert_eq!(result[(x, y)][3], source[(x, y)][3]);
            if x >= 6 || x == 0 {
                assert_eq!(result[(x, y)], source[(x, y)]);
            }
        }
    }
    assert_eq!(doc.selection, original.selection);
    assert_eq!(doc.layers[0].mask, original.layers[0].mask);
    assert_eq!(doc.layers[0].transform, original.layers[0].transform);
    assert!(Arc::ptr_eq(original.layers[0].raster().unwrap(), &source));
    assert_eq!(source[(3, 3)], Rgba([69, 88, 91, 193]));
}
