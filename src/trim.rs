//! Trim the rendered canvas while keeping layer pixels and metadata editable.
use crate::{Result, document::Document};
use image::RgbaImage;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Basis {
    #[default]
    Transparent,
    TopLeft,
    BottomRight,
}
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub basis: Basis,
    /// Left, top, right, bottom edges.
    pub edges: [bool; 4],
}
impl Default for Options {
    fn default() -> Self {
        Self {
            basis: Basis::Transparent,
            edges: [true; 4],
        }
    }
}
fn bounds(image: &RgbaImage, options: Options) -> Option<[u32; 4]> {
    if image.width() == 0 || image.height() == 0 || !options.edges.contains(&true) {
        return None;
    }
    // Upstream compares premultiplied RGBA, ignoring invisible RGB values.
    let premultiply = |p: image::Rgba<u8>| {
        [
            (u16::from(p[0]) * u16::from(p[3]) / 255) as u8,
            (u16::from(p[1]) * u16::from(p[3]) / 255) as u8,
            (u16::from(p[2]) * u16::from(p[3]) / 255) as u8,
            p[3],
        ]
    };
    let target = match options.basis {
        Basis::Transparent => None,
        Basis::TopLeft => Some(premultiply(image[(0, 0)])),
        Basis::BottomRight => Some(premultiply(image[(image.width() - 1, image.height() - 1)])),
    };
    let mut found = [image.width(), image.height(), 0, 0];
    for (x, y, p) in image.enumerate_pixels() {
        let keep = target.map_or(p[3] > 0, |target| premultiply(*p) != target);
        if keep {
            found = [
                found[0].min(x),
                found[1].min(y),
                found[2].max(x + 1),
                found[3].max(y + 1),
            ];
        }
    }
    if found[2] == 0 {
        return None;
    }
    let full = [0, 0, image.width(), image.height()];
    Some(std::array::from_fn(|i| {
        if options.edges[i] { found[i] } else { full[i] }
    }))
}
pub fn apply(doc: &mut Document, options: Options) -> Result<()> {
    let image = crate::render::render(doc, doc.width, doc.height)?;
    let Some([left, top, right, bottom]) = bounds(&image, options) else {
        return Ok(());
    };
    if [left, top, right, bottom] == [0, 0, doc.width, doc.height] {
        return Ok(());
    }
    crate::edits::crop(
        doc,
        [left as f64, top as f64],
        [right as f64, bottom as f64],
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oversized_trim_preserves_the_document() {
        let mut doc = Document::new(30_000, 30_000).unwrap();
        let original = doc.clone();
        let error = apply(&mut doc, Options::default()).unwrap_err();
        assert!(error.to_string().contains("200 million pixels"));
        assert_eq!(doc, original);
    }

    #[test]
    fn trim_color_transparency_and_independent_edges() {
        let mut image = RgbaImage::from_pixel(8, 6, image::Rgba([255, 0, 0, 0]));
        image.put_pixel(2, 1, image::Rgba([20, 30, 40, 128]));
        image.put_pixel(4, 3, image::Rgba([80, 90, 10, 255]));
        for basis in [Basis::Transparent, Basis::TopLeft, Basis::BottomRight] {
            assert_eq!(
                bounds(
                    &image,
                    Options {
                        basis,
                        ..Default::default()
                    }
                ),
                Some([2, 1, 5, 4])
            );
        }
        assert_eq!(
            bounds(
                &image,
                Options {
                    edges: [false, true, true, false],
                    ..Default::default()
                }
            ),
            Some([0, 1, 5, 6])
        );
        assert_eq!(
            bounds(
                &image,
                Options {
                    edges: [false; 4],
                    ..Default::default()
                }
            ),
            None
        );
        assert_eq!(bounds(&RgbaImage::new(8, 6), Options::default()), None);
    }
    #[test]
    fn trim_preserves_source_pixels_and_is_undoable() {
        let mut doc = Document::new(10, 10).unwrap();
        let image = RgbaImage::from_fn(10, 10, |x, y| {
            image::Rgba([
                20,
                30,
                40,
                if (2..6).contains(&x) && (3..7).contains(&y) {
                    255
                } else {
                    0
                },
            ])
        });
        doc.layers[0].content =
            crate::document::LayerContent::Raster(Some(std::sync::Arc::new(image)));
        let original = doc.clone();
        let mut session = crate::session::Session::new(doc, None);
        session
            .edit("Trim", |d| apply(d, Options::default()))
            .unwrap();
        assert_eq!([session.document.width, session.document.height], [4, 4]);
        assert_eq!(session.document.layers[0].transform.origin, [-2., -3.]);
        assert_eq!(
            session.document.layers[0].raster(),
            original.layers[0].raster()
        );
        session.undo();
        assert_eq!(session.document, original);
    }
}
