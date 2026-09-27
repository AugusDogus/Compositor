use super::*;
use crate::{document::LayerContent, geometry::Transform};
use image::Rgba;
use std::sync::Arc;

fn source() -> Document {
    let mut document = Document::new(6, 2).unwrap();
    document.layers[0].content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(6, 2, |x, _| {
            Rgba(if x < 2 {
                [255, 0, 0, 255]
            } else if x < 4 {
                [0, 255, 0, 255]
            } else {
                [0, 0, 255, 255]
            })
        }))));
    document
}

#[test]
fn parses_bounded_unique_dimensions_and_rejects_ambiguous_batches() {
    assert!(Batch::parse("1920 x 1080, 1080 × 1080, 20X30", Fit::Contain, Format::Png).is_ok());
    for sizes in [
        "",
        "-1x2",
        "1x0",
        "1.5x2",
        "1x2,",
        "30000x30000",
        "1x2, 1x2",
        "1x2x3",
    ] {
        assert!(
            Batch::parse(sizes, Fit::Contain, Format::Png).is_err(),
            "{sizes}"
        );
    }
    assert!(Batch::new(vec![[1, 2]; 17], Fit::Cover, Format::Png).is_err());
    assert!(Batch::new(vec![[1, 2]], Fit::Cover, Format::Jpeg { quality: 101 }).is_err());
}

#[test]
fn fit_pads_and_fill_crops_without_changing_the_source() {
    let document = source();
    let original = document.clone();
    let mut cache = render::DownsampleCache::default();
    let fit = render_frame(&document, [6, 6], Fit::Contain, &mut cache).unwrap();
    assert_eq!(fit[(0, 0)], Rgba([0; 4]));
    assert_eq!(fit[(0, 2)], Rgba([255, 0, 0, 255]));
    assert_eq!(fit[(5, 3)], Rgba([0, 0, 255, 255]));
    let fill = render_frame(&document, [2, 2], Fit::Cover, &mut cache).unwrap();
    assert!(fill.pixels().all(|pixel| *pixel == Rgba([0, 255, 0, 255])));
    assert_eq!(document, original);
}

#[test]
fn padding_does_not_reveal_off_canvas_pixels() {
    let mut document = source();
    document.width = 2;
    document.layers[0].transform = Transform::new(6, 2);
    let fit = render_frame(
        &document,
        [6, 2],
        Fit::Contain,
        &mut render::DownsampleCache::default(),
    )
    .unwrap();
    assert_eq!(fit[(0, 0)], Rgba([0; 4]));
    assert_eq!(fit[(2, 0)], Rgba([255, 0, 0, 255]));
    assert_eq!(fit[(4, 0)], Rgba([0; 4]));
}

#[test]
fn export_is_a_new_complete_folder_with_resolution_and_alpha() {
    let root = tempfile::tempdir().unwrap();
    let mut document = source();
    document.resolution = 300.;
    let batch = Batch::new(vec![[6, 6], [2, 2]], Fit::Contain, Format::Png).unwrap();
    let first = batch.export(&document, root.path(), "Poster").unwrap();
    let second = batch.export(&document, root.path(), "Poster").unwrap();
    assert_ne!(first, second);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
    let bytes = std::fs::read(first.join("Poster-6x6.png")).unwrap();
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let reader = decoder.read_info().unwrap();
    assert_eq!(
        reader.info().pixel_dims.unwrap().xppu,
        (300. / 0.0254_f64).round() as u32
    );
    let image = image_io::read_image(&first.join("Poster-6x6.png")).unwrap();
    assert_eq!(image.dimensions(), (6, 6));
    assert_eq!(image[(0, 0)][3], 0);
    assert!(second.join("Poster-2x2.png").is_file());
}

#[test]
fn occupied_symlink_is_preserved_and_invalid_document_leaves_no_outputs() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("untouched");
    std::fs::write(&target, "keep").unwrap();
    std::os::unix::fs::symlink(&target, root.path().join("Poster-exports")).unwrap();
    let batch = Batch::new(vec![[2, 2]], Fit::Cover, Format::Jpeg { quality: 100 }).unwrap();
    let output = batch.export(&source(), root.path(), "Poster").unwrap();
    assert_eq!(output.file_name().unwrap(), "Poster-exports-2");
    assert_eq!(std::fs::read_to_string(target).unwrap(), "keep");
    assert_eq!(
        image_io::read_image(&output.join("Poster-2x2.jpg"))
            .unwrap()
            .dimensions(),
        (2, 2)
    );
    let mut bad = source();
    bad.resolution = f64::NAN;
    assert!(batch.export(&bad, root.path(), "Invalid").is_err());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 3);
    assert!(!crate::export_batch::safe_stem("../../escape/🎨").contains('/'));
}

#[test]
fn jpeg_padding_and_partial_alpha_use_white_and_total_budget_is_checked() {
    let root = tempfile::tempdir().unwrap();
    let mut document = Document::new(16, 8).unwrap();
    document.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        16,
        8,
        Rgba([255, 0, 0, 128]),
    ))));
    let output = Batch::new(vec![[16, 32]], Fit::Contain, Format::Jpeg { quality: 100 })
        .unwrap()
        .export(&document, root.path(), "alpha")
        .unwrap();
    let image = image_io::read_image(&output.join("alpha-16x32.jpg")).unwrap();
    for (actual, expected) in [
        (image[(8, 1)], [255; 4]),
        (image[(8, 16)], [255, 127, 127, 255]),
    ] {
        for (actual, expected) in actual.0.into_iter().zip(expected) {
            assert!(
                actual.abs_diff(expected) <= 2,
                "{actual}, expected {expected}"
            );
        }
    }
    let sizes = vec![[20000, 10000], [10000, 20000], [25000, 8000], [8000, 25000]];
    assert!(Batch::new(sizes.clone(), Fit::Cover, Format::Png).is_ok());
    let mut oversized = sizes;
    oversized.push([1, 1]);
    assert!(Batch::new(oversized, Fit::Cover, Format::Png).is_err());
}
