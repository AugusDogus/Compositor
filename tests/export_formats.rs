use compositor::{
    document::{Document, Layer, LayerContent},
    image_io, render,
};
use image::{ImageFormat, Rgba, RgbaImage};
use std::sync::Arc;

fn composite() -> Document {
    let mut document = Document::new(20, 16).unwrap();
    document.resolution = 300.;
    document.layers[0].content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(20, 16, |x, y| {
            if x < 2 {
                Rgba([0; 4])
            } else {
                Rgba([30, 90, (y * 11) as u8, 128])
            }
        }))));
    let mut top = Layer::blank("Opaque foreground", 8, 8);
    top.transform.origin = [4., 4.];
    top.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        8,
        8,
        Rgba([240, 50, 10, 255]),
    ))));
    document.add(top).unwrap();
    let mut hidden = Layer::blank("Hidden", 20, 16);
    hidden.visible = false;
    hidden.content = LayerContent::Raster(Some(Arc::new(RgbaImage::from_pixel(
        20,
        16,
        Rgba([0, 255, 0, 255]),
    ))));
    document.add(hidden).unwrap();
    document
}

#[test]
fn tiff_and_lossless_webp_export_visible_composite_with_alpha() {
    let directory = tempfile::tempdir().unwrap();
    let document = composite();
    let before = document.clone();
    let expected = render::render(&document, 20, 16).unwrap();
    assert_eq!(expected[(0, 0)], Rgba([0; 4]));
    assert_eq!(expected[(5, 5)], Rgba([240, 50, 10, 255]));
    assert_eq!(expected[(18, 10)][3], 128);
    for (extension, format) in [
        ("tif", ImageFormat::Tiff),
        ("tiff", ImageFormat::Tiff),
        ("webp", ImageFormat::WebP),
    ] {
        let path = directory.path().join(format!("composite.{extension}"));
        image_io::export(&document, &path, 85).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(image::guess_format(&bytes).unwrap(), format);
        let decoded = image::load_from_memory(&bytes).unwrap().into_rgba8();
        assert_eq!(
            decoded, expected,
            "{extension} must retain color and alpha losslessly"
        );
        assert_eq!(image_io::read_image(&path).unwrap(), expected);
        assert_eq!(
            document, before,
            "export must not flatten or edit the project"
        );
        if format == ImageFormat::WebP {
            assert!(
                bytes.windows(4).any(|value| value == b"VP8L"),
                "WebP payload must be lossless"
            );
        } else {
            let mut tiff = tiff::decoder::Decoder::new(std::io::Cursor::new(bytes)).unwrap();
            assert_eq!(tiff.colortype().unwrap(), tiff::ColorType::RGBA(8));
        }
    }
}

#[test]
fn failed_image_export_preserves_existing_destination_and_project() {
    let directory = tempfile::tempdir().unwrap();
    let mut document = composite();
    document.resolution = 0.;
    let before = document.clone();
    for extension in ["tiff", "webp"] {
        let path = directory.path().join(format!("keep.{extension}"));
        std::fs::write(&path, b"existing image").unwrap();
        assert!(image_io::export(&document, &path, 85).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"existing image");
        assert_eq!(document, before);
    }
}
