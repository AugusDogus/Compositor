use compositor::{
    background,
    document::{Document, LayerContent},
    image_io,
};

#[test]
#[ignore = "Requires native inference files and COMPOSITOR_TEST_PHOTO pointing to a subject photo; supports CUDA and CPU"]
fn local_background_removal_keeps_source_pixels_and_adds_a_nonuniform_mask() {
    let path = std::env::var_os("COMPOSITOR_TEST_PHOTO").expect("Set COMPOSITOR_TEST_PHOTO");
    let layer = image_io::import(std::path::Path::new(&path)).unwrap();
    let original = layer.raster().unwrap().clone();
    let mut doc = Document::new(original.width(), original.height()).unwrap();
    doc.layers.clear();
    doc.add(layer).unwrap();
    background::remove(&mut doc, background::Quality::Basic).unwrap();
    let layer = doc.active_layer().unwrap();
    assert!(matches!(layer.content, LayerContent::Raster(_)));
    assert!(std::sync::Arc::ptr_eq(layer.raster().unwrap(), &original));
    let mask = &layer.mask.as_ref().unwrap().pixels;
    assert!(mask.pixels().any(|p| p[0] < 16));
    assert!(mask.pixels().any(|p| p[0] > 240));
}

#[test]
#[ignore = "Requires COMPOSITOR_TEST_HEIC pointing to a HEIC image"]
fn heic_import_decodes_to_rgba() {
    let path = std::env::var_os("COMPOSITOR_TEST_HEIC").expect("Set COMPOSITOR_TEST_HEIC");
    let pixels = image_io::read_image(std::path::Path::new(&path)).unwrap();
    assert!(pixels.width() > 0 && pixels.height() > 0);
    assert!(pixels.pixels().any(|p| p[3] > 0));
}

#[test]
#[ignore = "Requires COMPOSITOR_TEST_CMYK and COMPOSITOR_TEST_CMYK_REFERENCE with a profiled CMYK JPEG/TIFF and color-managed PNG reference"]
fn cmyk_import_matches_color_managed_reference() {
    let path = std::env::var_os("COMPOSITOR_TEST_CMYK").expect("Set COMPOSITOR_TEST_CMYK");
    let reference = std::env::var_os("COMPOSITOR_TEST_CMYK_REFERENCE")
        .expect("Set COMPOSITOR_TEST_CMYK_REFERENCE");
    let pixels = image_io::read_image(std::path::Path::new(&path)).unwrap();
    let encoded = std::fs::read(&path).unwrap();
    assert_eq!(image_io::read_encoded(&encoded).unwrap(), pixels);
    let expected = image_io::read_image(std::path::Path::new(&reference)).unwrap();
    assert_eq!(pixels.dimensions(), expected.dimensions());
    let difference = pixels
        .as_raw()
        .iter()
        .zip(expected.as_raw())
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap();
    assert!(
        difference <= 5,
        "Maximum channel difference: {difference}; first pixel {:?}, expected {:?}",
        pixels[(0, 0)],
        expected[(0, 0)]
    );
}
