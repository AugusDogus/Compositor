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
    let mut budget = 768_u64 * 1024 * 1024;
    validate_resources(tree.root(), &mut budget, 0)?;
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

// Raster decoders and isolated/filter groups allocate independently of the viewport.
// Bound their aggregate working surfaces before handing the tree to the renderer.
fn validate_resources(group: &resvg::usvg::Group, remaining: &mut u64, depth: usize) -> Result<()> {
    let too_large = || {
        invalid(
            "SVG rendering exceeds the 768 MiB working-memory limit. Simplify groups and embedded images or export a smaller PNG.",
        )
    };
    if depth > 128 {
        return Err(too_large());
    }
    let bounds = group.abs_layer_bounding_box();
    let area = f64::from(bounds.width()).ceil() * f64::from(bounds.height()).ceil();
    if !area.is_finite() || area * 32. > *remaining as f64 {
        return Err(too_large());
    }
    *remaining -= (area * 32.) as u64;
    for node in group.children() {
        if let resvg::usvg::Node::Image(image) = node {
            let size = image.size().to_int_size();
            validate_size(size.width(), size.height())?;
            let bytes = u64::from(size.width()) * u64::from(size.height()) * 8;
            *remaining = remaining.checked_sub(bytes).ok_or_else(too_large)?;
        }
        if let resvg::usvg::Node::Group(child) = node {
            validate_resources(child, remaining, depth + 1)?;
        }
        let mut result = Ok(());
        node.subroots(|child| {
            if result.is_ok() {
                result = validate_resources(child, remaining, depth + 1);
            }
        });
        result?;
    }
    Ok(())
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
        assert!(decode(br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><g opacity="0.5"><rect width="100000" height="100000"/></g></svg>"#).is_err());
        assert!(
            decode(br#"<svg xmlns="http://www.w3.org/2000/svg" width="100000" height="100000"/>"#)
                .is_err()
        );
        let error = decode(br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><image href="/tmp/private.png" width="10" height="10"/></svg>"#).unwrap_err();
        assert!(error.to_string().contains("external images"));
    }
}
