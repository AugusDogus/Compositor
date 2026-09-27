use crate::Result;
use image::RgbaImage;
use rayon::prelude::*;

pub(super) fn unsharp(
    image: &RgbaImage,
    amount: f64,
    radius: f64,
    threshold: f64,
) -> Result<RgbaImage> {
    super::finishing::range(amount, 0., 500.)?;
    super::finishing::range(radius, 0.1, 250.)?;
    super::finishing::range(threshold, 0., 255.)?;
    if amount == 0. {
        return Ok(image.clone());
    }
    let blurred = super::gaussian_rgba(image, radius as f32)?;
    let mut result = image.clone();
    result
        .par_chunks_exact_mut(4)
        .zip(blurred.par_chunks_exact(4))
        .for_each(|(pixel, base)| {
            if pixel[3] == 0 {
                return;
            }
            for c in 0..3 {
                let detail = f64::from(pixel[c]) - f64::from(base[c]);
                if detail.abs() > threshold {
                    pixel[c] = (f64::from(pixel[c]) + detail * amount / 100.)
                        .clamp(0., 255.)
                        .round() as u8;
                }
            }
        });
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn unsharp_respects_selection_and_rejects_invalid_values_transactionally() {
        use crate::{
            document::{Document, LayerContent},
            filters::{Filter, apply},
            selection::Selection,
        };
        use std::sync::Arc;
        let mut doc = Document::new(9, 9).unwrap();
        let pixels = RgbaImage::from_fn(9, 9, |x, _| Rgba([if x < 4 { 80 } else { 160 }; 4]));
        doc.layers[0].content = LayerContent::Raster(Some(Arc::new(pixels.clone())));
        doc.selection = Some(Selection::rectangle(9, 9, [0., 0.], [4., 9.], false));
        let original = doc.clone();
        for (amount, radius, threshold) in [(501., 2., 0.), (100., 0., 0.), (100., 2., f64::NAN)] {
            assert!(
                apply(
                    &mut doc,
                    Filter::UnsharpMask {
                        amount,
                        radius,
                        threshold
                    },
                    false
                )
                .is_err()
            );
            assert_eq!(doc, original);
        }
        apply(
            &mut doc,
            Filter::UnsharpMask {
                amount: 100.,
                radius: 2.,
                threshold: 0.,
            },
            false,
        )
        .unwrap();
        let result = doc.layers[0].raster().unwrap();
        assert!(result[(3, 4)][0] < pixels[(3, 4)][0]);
        assert_eq!(result[(4, 4)], pixels[(4, 4)]);
        assert_eq!(doc.layers[0].transform, original.layers[0].transform);
    }

    #[test]
    fn unsharp_increases_edge_contrast_without_changing_alpha_or_flat_fields() {
        let flat = RgbaImage::from_pixel(9, 9, Rgba([71, 83, 111, 37]));
        assert_eq!(unsharp(&flat, 500., 2., 0.).unwrap(), flat);
        let edge = RgbaImage::from_fn(9, 9, |x, _| {
            if x < 4 {
                Rgba([80, 80, 80, 255])
            } else {
                Rgba([160, 160, 160, 128])
            }
        });
        let result = unsharp(&edge, 100., 2., 0.).unwrap();
        assert!(result[(3, 4)][0] < 80);
        assert!(result[(4, 4)][0] > 160);
        assert!(
            result
                .pixels()
                .zip(edge.pixels())
                .all(|(a, b)| a[3] == b[3])
        );
        assert_eq!(unsharp(&edge, 0., 2., 0.).unwrap(), edge);
        assert_eq!(unsharp(&edge, 100., 2., 255.).unwrap(), edge);
    }

    #[test]
    fn unsharp_ignores_hidden_colors_at_transparent_edges() {
        let image = RgbaImage::from_fn(9, 9, |x, _| {
            if x < 4 {
                Rgba([255, 0, 0, 0])
            } else {
                Rgba([71, 83, 111, 37])
            }
        });
        assert_eq!(unsharp(&image, 500., 2., 0.).unwrap(), image);
    }
}
