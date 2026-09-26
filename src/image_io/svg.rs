//! SVG imports become a raster layer at the document's intrinsic pixel dimensions.
use crate::{Result, document::validate_size, invalid, native_pixels};
use image::RgbaImage;
use std::{
    io::Read,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub(super) fn read(path: &Path) -> Result<RgbaImage> {
    const LIMIT: u64 = 16 * 1024 * 1024;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT {
        return Err(invalid(
            "SVG exceeds the 16 MiB import limit. Simplify it or export a PNG.",
        ));
    }
    decode(&bytes)
}

fn decode(bytes: &[u8]) -> Result<RgbaImage> {
    let external = Arc::new(AtomicBool::new(false));
    let found_external = external.clone();
    let mut options = resvg::usvg::Options::default();
    // Imported artwork must not read arbitrary files referenced by an SVG.
    options.image_href_resolver.resolve_string = Box::new(move |_, _| {
        found_external.store(true, Ordering::Relaxed);
        None
    });
    options.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_data(bytes, &options).map_err(|error| {
        invalid(format!(
            "Could not read SVG: {error}. Repair the file or export a PNG."
        ))
    })?;
    if external.load(Ordering::Relaxed) {
        return Err(invalid(
            "SVG references external images. Embed those images in the SVG or export a PNG, then import again.",
        ));
    }
    let size = tree.size().to_int_size();
    validate_size(size.width(), size.height())?;
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(size.width(), size.height()).ok_or_else(|| {
            invalid("Not enough memory to rasterize this SVG. Export it at a smaller size.")
        })?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    let pixels = RgbaImage::from_raw(size.width(), size.height(), pixmap.take())
        .ok_or_else(|| invalid("SVG renderer returned an invalid pixel buffer."))?;
    Ok(native_pixels::unpremultiply(pixels))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn import_preserves_intrinsic_dimensions_and_straight_alpha() {
        let image = decode(br#"<svg xmlns="http://www.w3.org/2000/svg" width="12" height="8"><rect width="6" height="8" fill="red" opacity="0.5"/></svg>"#).unwrap();
        assert_eq!(image.dimensions(), (12, 8));
        assert_eq!(image[(2, 2)].0, [255, 0, 0, 128]);
        assert_eq!(image[(10, 2)][3], 0);
    }
    #[test]
    fn viewbox_only_documents_and_text_render() {
        let image = decode(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 80 30"><text x="2" y="22" font-size="20">Hello</text></svg>"#).unwrap();
        assert_eq!(image.dimensions(), (80, 30));
        assert!(image.pixels().filter(|p| p[3] > 0).count() > 30);
    }
    #[test]
    fn oversized_and_external_documents_are_rejected() {
        assert!(
            decode(br#"<svg xmlns="http://www.w3.org/2000/svg" width="100000" height="100000"/>"#)
                .is_err()
        );
        let error = decode(br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><image href="/tmp/private.png" width="10" height="10"/></svg>"#).unwrap_err();
        assert!(error.to_string().contains("external images"));
    }
}
