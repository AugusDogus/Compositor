use super::*;
use crate::{
    document::{LayerContent, Mask},
    effects::{LayerEffects, StrokeEffect},
    geometry::{Sampling, Transform},
};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use std::sync::Arc;

fn pixels(name: &str, color: [u8; 4]) -> Layer {
    let mut layer = Layer::blank(name, 2, 2);
    layer.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(2, 2, Rgba(color)))));
    layer.transform.origin = [4., 4.];
    layer.transform.sampling = Sampling::Nearest;
    layer
}
fn document(layers: Vec<Layer>) -> Document {
    let mut document = Document::new(32, 32).unwrap();
    document.layers = layers;
    document.select(document.layers[0].id, false);
    document
}

#[test]
fn clipped_stack_and_ancestor_masks_export_once_without_unrelated_pixels() {
    let mut folder = Layer::blank("Folder", 32, 32);
    folder.content = LayerContent::Group;
    folder.opacity = 0.5;
    folder.mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(1, 1, Luma([128]))),
        enabled: true,
        linked: true,
        placement: None,
    });
    let mut base = pixels("Base", [255, 0, 0, 128]);
    base.parent = Some(folder.id);
    let mut clipped = pixels("Clipped", [0, 255, 0, 255]);
    clipped.parent = Some(folder.id);
    clipped.clip_source = Some(base.id);
    let unrelated = pixels("Unrelated", [0, 0, 255, 255]);
    let document = document(vec![folder, unrelated, base, clipped]);
    let original = document.clone();
    let root = tempfile::tempdir().unwrap();
    let output = export(&document, root.path(), "Test").unwrap();
    assert_eq!(std::fs::read_dir(&output).unwrap().count(), 2);
    let base = image_io::read_image(&output.join("01-Base.png")).unwrap();
    assert_eq!(base.dimensions(), (2, 2));
    assert!(base.pixels().all(|p| *p == Rgba([128, 128, 0, 32])));
    let unrelated = image_io::read_image(&output.join("02-Unrelated.png")).unwrap();
    assert!(unrelated.pixels().all(|p| *p == Rgba([0, 0, 255, 255])));
    assert_eq!(document, original);
}

#[test]
fn hidden_content_empty_layers_and_adjustment_targets_are_excluded() {
    let base = pixels("Visible", [90, 120, 150, 255]);
    let mut hidden = pixels("Hidden", [255; 4]);
    hidden.visible = false;
    let mut folder = Layer::blank("Hidden Folder", 32, 32);
    folder.content = LayerContent::Group;
    folder.visible = false;
    let mut child = pixels("Hidden child", [255; 4]);
    child.parent = Some(folder.id);
    let mut adjustment = Layer::blank("Global Invert", 32, 32);
    adjustment.content = LayerContent::Adjustment(Box::new(crate::adjustment::Adjustment::new(
        crate::adjustment::Kind::Invert,
    )));
    let mut blank = Layer::blank("Empty", 32, 32);
    blank.parent = None;
    let document = document(vec![base, hidden, folder, child, adjustment, blank]);
    let root = tempfile::tempdir().unwrap();
    let output = export(&document, root.path(), "Test").unwrap();
    assert_eq!(std::fs::read_dir(&output).unwrap().count(), 1);
    assert_eq!(
        image_io::read_image(&output.join("01-Visible.png")).unwrap()[(0, 0)],
        Rgba([90, 120, 150, 255])
    );
}

#[test]
fn clipped_adjustment_is_retained_but_global_adjustment_is_not() {
    let base = pixels("Base", [20, 40, 60, 255]);
    let mut clipped = Layer::blank("Clipped Invert", 32, 32);
    clipped.content = LayerContent::Adjustment(Box::new(crate::adjustment::Adjustment::new(
        crate::adjustment::Kind::Invert,
    )));
    clipped.clip_source = Some(base.id);
    let mut global = clipped.clone();
    global.id = Uuid::new_v4();
    global.clip_source = None;
    let document = document(vec![base, clipped, global]);
    let root = tempfile::tempdir().unwrap();
    let output = export(&document, root.path(), "Test").unwrap();
    assert_eq!(
        image_io::read_image(&output.join("01-Base.png")).unwrap()[(0, 0)],
        Rgba([235, 215, 195, 255])
    );
}

#[test]
fn rotated_scaled_effect_bounds_are_preserved_and_trim_is_not_alpha_crop() {
    let mut layer = pixels("Styled", [255; 4]);
    layer.transform = Transform {
        origin: [14., 12.],
        size: [4., 2.],
        rotation: 90.,
        sampling: Sampling::Nearest,
        ..Transform::new(2, 2)
    };
    layer.effects = Some(LayerEffects {
        stroke: Some(StrokeEffect {
            size: 2.,
            red: 1.,
            green: 0.,
            blue: 0.,
            ..StrokeEffect::default()
        }),
        ..LayerEffects::default()
    });
    let document = document(vec![layer]);
    let root = tempfile::tempdir().unwrap();
    let output = export(&document, root.path(), "Effects").unwrap();
    let actual = image_io::read_image(&output.join("01-Styled.png")).unwrap();
    // A four-pixel source margin becomes eight pixels horizontally after scale;
    // the 90-degree rotation swaps the resulting 20 × 10 rectangle.
    assert_eq!(actual.dimensions(), (10, 20));
    let full = render::render(&document, 32, 32).unwrap();
    let expected = image::imageops::crop_imm(&full, 11, 3, 10, 20).to_image();
    for (a, b) in actual.as_raw().iter().zip(expected.as_raw()) {
        assert!(a.abs_diff(*b) <= 1);
    }
    assert!(actual.pixels().any(|p| p[3] == 0));
    assert!(
        actual
            .pixels()
            .any(|p| p[0] == 255 && p[1] == 0 && p[3] > 0)
    );
}

#[test]
fn sparse_canvas_exports_only_bounded_source_and_keeps_resolution() {
    let mut document = document(vec![pixels("Small", [255, 0, 0, 128])]);
    document.width = 30_000;
    document.height = 30_000;
    document.resolution = 300.;
    let root = tempfile::tempdir().unwrap();
    let output = export(&document, root.path(), "Large").unwrap();
    let file = output.join("01-Small.png");
    assert_eq!(image_io::read_image(&file).unwrap().dimensions(), (2, 2));
    let reader = png::Decoder::new(std::io::Cursor::new(std::fs::read(file).unwrap()))
        .read_info()
        .unwrap();
    assert_eq!(
        reader.info().pixel_dims.unwrap().xppu,
        (300. / 0.0254_f64).round() as u32
    );
}

#[test]
fn filenames_collisions_and_invalid_batches_preserve_existing_files() {
    let root = tempfile::tempdir().unwrap();
    let sentinel = root.path().join("untouched");
    std::fs::write(&sentinel, b"keep").unwrap();
    std::os::unix::fs::symlink(&sentinel, root.path().join("Test-layers")).unwrap();
    let document = document(vec![
        pixels("../same", [255; 4]),
        pixels("../same", [255; 4]),
    ]);
    let output = export(&document, root.path(), "Test").unwrap();
    assert_eq!(output.file_name().unwrap(), "Test-layers-2");
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"keep");
    assert_eq!(std::fs::read_dir(&output).unwrap().count(), 2);
    for entry in std::fs::read_dir(&output).unwrap() {
        assert!(entry.unwrap().path().is_file());
    }
    let mut bad = document.clone();
    bad.resolution = f64::NAN;
    assert!(export(&bad, root.path(), "Invalid").is_err());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 3);
    let blank = Document::new(32, 32).unwrap();
    assert!(!has_exportable_layers(&blank));
    assert!(export(&blank, root.path(), "Empty").is_err());
}

#[test]
fn cumulative_export_budget_fails_before_allocating_surfaces_or_staging() {
    let mut document = Document::new(10_000, 20_000).unwrap();
    document.layers.clear();
    for index in 0..5 {
        let mut layer = Layer::blank(format!("Large {index}"), 10_000, 20_000);
        layer.content =
            LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(1, 1, Rgba([255; 4])))));
        document.layers.push(layer);
    }
    document.select(document.layers[0].id, false);
    document.validate().unwrap();
    let root = tempfile::tempdir().unwrap();
    assert!(
        export(&document, root.path(), "Large")
            .unwrap_err()
            .to_string()
            .contains("800 million")
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn detached_raster_clipping_links_get_their_own_masked_exports() {
    let mut base = pixels("Base", [255, 0, 0, 128]);
    base.opacity = 0.5;
    let middle = pixels("Middle", [0, 0, 255, 255]);
    let mut detached = pixels("Detached", [0, 255, 0, 255]);
    detached.clip_source = Some(base.id);
    detached.opacity = 0.5;
    let document = document(vec![base, middle, detached]);
    let root = tempfile::tempdir().unwrap();
    let output = export(&document, root.path(), "Detached").unwrap();
    assert_eq!(std::fs::read_dir(&output).unwrap().count(), 3);
    let pixels = image_io::read_image(&output.join("03-Detached.png")).unwrap();
    assert!(pixels.pixels().all(|pixel| *pixel == Rgba([0, 255, 0, 32])));
}

#[test]
fn cross_folder_hidden_clipping_sources_keep_coverage_and_target_folder_masks() {
    let mut source_folder = Layer::blank("Source folder", 32, 32);
    source_folder.content = LayerContent::Group;
    source_folder.opacity = 0.1;
    let mut source = pixels("Source", [255, 0, 0, 128]);
    source.parent = Some(source_folder.id);
    source.visible = false;
    source.opacity = 0.5;
    // Half of the target is outside the coverage source after placement.
    source.transform.origin[0] = 5.;
    let mut target_folder = Layer::blank("Target folder", 32, 32);
    target_folder.content = LayerContent::Group;
    target_folder.opacity = 0.5;
    target_folder.mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(1, 1, Luma([128]))),
        enabled: true,
        linked: true,
        placement: None,
    });
    let mut target = pixels("Target", [0, 255, 0, 255]);
    target.parent = Some(target_folder.id);
    target.clip_source = Some(source.id);
    target.opacity = 0.5;
    let document = document(vec![source_folder, source, target_folder, target]);
    assert!(has_exportable_layers(&document));
    let root = tempfile::tempdir().unwrap();
    let output = export(&document, root.path(), "Cross folder").unwrap();
    assert_eq!(std::fs::read_dir(&output).unwrap().count(), 1);
    let pixels = image_io::read_image(&output.join("01-Target.png")).unwrap();
    assert_eq!(pixels[(0, 0)][3], 0);
    assert_eq!(pixels[(1, 0)], Rgba([0, 255, 0, 8]));
}
