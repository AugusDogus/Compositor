use super::*;
use crate::{
    document::{LayerContent, Mask},
    geometry::{Sampling, projective::Projective},
};
use image::{GrayImage, Luma, Rgba, RgbaImage};
use std::sync::Arc;

fn document() -> Document {
    let mut doc = Document::new(200, 160).unwrap();
    doc.layers[0].content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(100, 80, |x, y| {
            Rgba([x as u8, y as u8, 50, 255])
        }))));
    doc.layers[0].transform = Transform::new(100, 80);
    doc
}
fn close(a: Point, b: Point) {
    assert!((a[0] - b[0]).hypot(a[1] - b[1]) < 1e-8, "{a:?} != {b:?}");
}
const UNIT: [Point; 4] = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]];
const QUAD: [Point; 4] = [[20., 10.], [145., 5.], [125., 130.], [5., 95.]];

#[test]
fn perspective_retains_source_pixels_with_flips_and_repeated_apply() {
    for flips in [[false, false], [true, false], [false, true], [true, true]] {
        let mut doc = document();
        doc.layers[0].transform.flip_x = flips[0];
        doc.layers[0].transform.flip_y = flips[1];
        doc.layers[0].transform.rotation = 23.;
        let source = doc.layers[0].raster().unwrap().clone();
        let old = doc.layers[0].transform;
        apply_perspective(&mut doc, old, QUAD, false).unwrap();
        for (point, expected) in UNIT.into_iter().zip(QUAD) {
            close(doc.layers[0].transform.geometry_point(point), expected);
            let mut source_point = point;
            if flips[0] {
                source_point[0] = 1. - source_point[0];
            }
            if flips[1] {
                source_point[1] = 1. - source_point[1];
            }
            close(doc.layers[0].transform.point(source_point), expected);
        }
        let second = QUAD.map(|p| [p[0] + 7., p[1] - 2.]);
        let old = doc.layers[0].transform;
        apply_perspective(&mut doc, old, second, false).unwrap();
        assert!(Arc::ptr_eq(&source, doc.layers[0].raster().unwrap()));
        for (point, expected) in UNIT.into_iter().zip(second) {
            close(doc.layers[0].transform.geometry_point(point), expected);
        }
    }
}

#[test]
fn linked_mask_horizon_rejects_entire_perspective_transaction() {
    let mut doc = document();
    doc.layers[0].mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(200, 80, Luma([255]))),
        enabled: true,
        linked: true,
        placement: Some(Transform {
            origin: [-100., 0.],
            ..Transform::new(200, 80)
        }),
    });
    let original = doc.clone();
    let old = doc.layers[0].transform;
    let error = apply_perspective(
        &mut doc,
        old,
        [[0., 0.], [25., 0.], [25., 20.], [0., 80.]],
        false,
    )
    .unwrap_err();
    assert!(error.to_string().contains("infinity"), "{error}");
    assert_eq!(doc, original);
}

#[test]
fn unlinked_implicit_mask_freezes_and_mask_only_preserves_source() {
    let mut doc = document();
    let old = doc.layers[0].transform;
    doc.layers[0].mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(100, 80, Luma([200]))),
        enabled: true,
        linked: false,
        placement: None,
    });
    let mask = doc.layers[0].mask.as_ref().unwrap().pixels.clone();
    apply_perspective(&mut doc, old, QUAD, false).unwrap();
    assert_eq!(doc.layers[0].mask.as_ref().unwrap().placement, Some(old));
    let image_transform = doc.layers[0].transform;
    apply_perspective(&mut doc, old, QUAD, true).unwrap();
    assert_eq!(doc.layers[0].transform, image_transform);
    let placed = doc.layers[0].mask.as_ref().unwrap();
    assert!(Arc::ptr_eq(&mask, &placed.pixels));
    for (p, expected) in UNIT.into_iter().zip(QUAD) {
        close(placed.placement.unwrap().geometry_point(p), expected);
    }
}

#[test]
fn linked_explicit_mask_follows_exact_document_mapping_without_resampling() {
    let mut doc = document();
    let old = doc.layers[0].transform;
    let mask_transform = Transform {
        origin: [20., 15.],
        rotation: 13.,
        ..Transform::new(30, 40)
    };
    doc.layers[0].mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(30, 40, Luma([200]))),
        enabled: true,
        linked: true,
        placement: Some(mask_transform),
    });
    let pixels = doc.layers[0].mask.as_ref().unwrap().pixels.clone();
    apply_perspective(&mut doc, old, QUAD, false).unwrap();
    let actual = doc.layers[0].mask.as_ref().unwrap();
    assert!(Arc::ptr_eq(&pixels, &actual.pixels));
    let new_mapping = Projective::new(QUAD).unwrap();
    for p in [[0., 0.], [0.25, 0.75], [1., 1.]] {
        close(
            actual.placement.unwrap().point(p),
            new_mapping
                .apply(old.unit(mask_transform.point(p)))
                .unwrap(),
        );
    }
}

#[test]
fn projective_resize_drag_has_no_jump_and_keeps_anchor_fixed() {
    let mut doc = document();
    let old = doc.layers[0].transform;
    apply_perspective(&mut doc, old, QUAD, false).unwrap();
    let original = doc.layers[0].transform;
    for handle in 0..8 {
        let point = original.resize_handles()[handle];
        let unchanged = drag(
            original,
            point,
            point,
            Handle::Resize(handle),
            false,
            false,
            false,
        );
        for p in UNIT {
            close(unchanged.point(p), original.point(p));
        }
        let resized = drag(
            original,
            point,
            [point[0] + 10., point[1] + 7.],
            Handle::Resize(handle),
            false,
            false,
            false,
        );
        close(
            resized.resize_handles()[(handle + 4) % 8],
            original.resize_handles()[(handle + 4) % 8],
        );
    }
}

#[test]
fn projective_rebind_preserves_flipped_rotated_source_coordinates() {
    let original = Transform {
        origin: [20., 30.],
        rotation: 31.,
        flip_x: true,
        flip_y: true,
        sampling: Sampling::Smooth,
        warp: Some(Projective::new([[0.1, 0.], [1., 0.2], [0.8, 1.], [0., 0.9]]).unwrap()),
        ..Transform::new(100, 80)
    };
    let offset = [-0.05, -0.1];
    let size = [1.1, 1.2];
    let rebound = original.rebind(offset, size).unwrap();
    for p in UNIT {
        close(
            original.point(p),
            rebound.point([(p[0] - offset[0]) / size[0], (p[1] - offset[1]) / size[1]]),
        );
    }
}

#[test]
fn projective_mirror_reflects_document_coordinates_and_round_trips() {
    let mut doc = document();
    let old = doc.layers[0].transform;
    apply_perspective(&mut doc, old, QUAD, false).unwrap();
    let original = doc.layers[0].transform;
    for horizontal in [true, false] {
        let flipped = original.mirrored(horizontal, 80.).unwrap();
        let restored = flipped.mirrored(horizontal, 80.).unwrap();
        for p in [[0., 0.], [0.3, 0.7], [1., 1.]] {
            let mut expected = original.point(p);
            expected[usize::from(!horizontal)] = 160. - expected[usize::from(!horizontal)];
            close(flipped.point(p), expected);
            close(restored.point(p), original.point(p));
        }
    }
}

#[test]
fn projective_paint_uses_document_circle_and_local_source_pixel_footprint() {
    use crate::brush::{Brush, PaintMode, Stroke};
    let mut doc = document();
    doc.layers[0].content = LayerContent::Raster(Some(Arc::new(RgbaImage::new(100, 80))));
    let old = doc.layers[0].transform;
    apply_perspective(&mut doc, old, QUAD, false).unwrap();
    let transform = doc.layers[0].transform;
    let brush = Brush {
        diameter: 25.,
        hardness: 1.,
        opacity: 1.,
        color: [255, 0, 0, 255],
    };
    let center = [72., 60.];
    let mut stroke =
        Stroke::start(&mut doc, center, brush, PaintMode::Paint, false, false).unwrap();
    stroke.finish(&mut doc).unwrap();
    assert_eq!(doc.layers[0].transform, transform);
    let mapping = transform.mapping().unwrap();
    for (x, y, pixel) in doc.layers[0].raster().unwrap().enumerate_pixels() {
        let u = [(f64::from(x) + 0.5) / 100., (f64::from(y) + 0.5) / 80.];
        let point = transform.point(u);
        let derivative = mapping.derivative(u).unwrap();
        let aa = (derivative[0][0].hypot(derivative[0][1]) / 100.)
            .min(derivative[1][0].hypot(derivative[1][1]) / 80.)
            .max(0.001);
        let expected = (((12.5 - (point[0] - center[0]).hypot(point[1] - center[1])) / aa + 0.5)
            .clamp(0., 1.)
            * 255.)
            .round() as u8;
        assert!(
            pixel[3].abs_diff(expected) <= 1,
            "({x},{y}): {} vs {expected}",
            pixel[3]
        );
    }
}

#[test]
fn perspective_source_expansion_preserves_pixels_masks_and_placement() {
    let mut doc = document();
    let old = doc.layers[0].transform;
    apply_perspective(&mut doc, old, QUAD, false).unwrap();
    doc.layers[0].mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(100, 80, Luma([180]))),
        enabled: true,
        linked: true,
        placement: None,
    });
    let old = doc.layers[0].transform;
    let before = doc.layers[0].raster().unwrap().clone();
    let source_point = old.point([-0.03, 0.5]);
    let expansion = crate::raster_extent::expand(
        &mut doc.layers[0],
        [
            source_point[0] - 1.,
            source_point[1] - 1.,
            source_point[0] + 1.,
            source_point[1] + 1.,
        ],
    )
    .unwrap()
    .unwrap();
    for y in 0..80 {
        for x in 0..100 {
            let nx = x + expansion.offset[0];
            let ny = y + expansion.offset[1];
            assert_eq!(doc.layers[0].raster().unwrap()[(nx, ny)], before[(x, y)]);
            assert_eq!(
                doc.layers[0].mask.as_ref().unwrap().pixels[(nx, ny)][0],
                180
            );
            close(
                old.point([(f64::from(x) + 0.5) / 100., (f64::from(y) + 0.5) / 80.]),
                doc.layers[0].transform.point([
                    (f64::from(nx) + 0.5) / f64::from(expansion.size[0]),
                    (f64::from(ny) + 0.5) / f64::from(expansion.size[1]),
                ]),
            );
        }
    }
}

#[test]
fn perspective_expansion_rejects_horizon_before_changing_pixels() {
    let mut doc = document();
    let old = doc.layers[0].transform;
    apply_perspective(
        &mut doc,
        old,
        [[0., 0.], [25., 0.], [25., 20.], [0., 80.]],
        false,
    )
    .unwrap();
    let original = doc.layers[0].clone();
    let error = crate::raster_extent::expand(&mut doc.layers[0], [0., 0., 40., 80.])
        .err()
        .expect("inverse horizon must reject expansion");
    assert!(error.to_string().contains("infinity"), "{error}");
    assert_eq!(doc.layers[0], original);
}

#[test]
fn perspective_image_resize_preserves_source_pixels_and_mapping() {
    let mut doc = document();
    let old = doc.layers[0].transform;
    apply_perspective(&mut doc, old, QUAD, false).unwrap();
    let old = doc.layers[0].transform;
    let source = doc.layers[0].raster().unwrap().clone();
    crate::image_resize::resize(&mut doc, 300, 320, 72., Sampling::Smooth).unwrap();
    assert!(Arc::ptr_eq(&source, doc.layers[0].raster().unwrap()));
    for p in [[0., 0.], [0.3, 0.7], [1., 1.]] {
        let expected = old.point(p);
        close(
            doc.layers[0].transform.point(p),
            [expected[0] * 1.5, expected[1] * 2.],
        );
    }
}

#[test]
fn perspective_effect_padding_preserves_source_grid_and_rejects_horizon() {
    let mut doc = document();
    let old = doc.layers[0].transform;
    apply_perspective(&mut doc, old, QUAD, false).unwrap();
    doc.layers[0].effects = Some(crate::effects::LayerEffects {
        stroke: Some(crate::effects::StrokeEffect {
            size: 2.,
            ..Default::default()
        }),
        ..Default::default()
    });
    let old = doc.layers[0].transform;
    let effects = doc.layers[0].effects.as_ref().unwrap();
    let margin = f64::from(effects.margin());
    let padded = crate::effects::rendered_transform(&doc.layers[0]).unwrap();
    for p in [[0., 0.], [0.3, 0.7], [1., 1.]] {
        close(
            old.point(p),
            padded.point([
                (p[0] * 100. + margin) / (100. + 2. * margin),
                (p[1] * 80. + margin) / (80. + 2. * margin),
            ]),
        );
    }
    let mut doc = document();
    doc.layers[0].effects = Some(crate::effects::LayerEffects {
        stroke: Some(crate::effects::StrokeEffect {
            size: 40.,
            ..Default::default()
        }),
        ..Default::default()
    });
    let original = doc.clone();
    let old = doc.layers[0].transform;
    let result = apply_perspective(
        &mut doc,
        old,
        [[0., 0.], [25., 0.], [25., 20.], [0., 80.]],
        false,
    );
    assert!(result.is_err());
    assert_eq!(doc, original);
}

#[test]
fn affine_group_resize_retains_rotated_layer_shear_exactly() {
    let old = Transform::new(200, 100);
    let new = Transform::new(300, 230);
    let layer = Transform {
        origin: [20., 17.],
        rotation: 37.,
        ..Transform::new(60, 45)
    };
    let followed = layer.following(old, new).unwrap();
    assert!(followed.warp.is_some());
    for p in [[0., 0.], [0.3, 0.7], [1., 1.]] {
        close(followed.point(p), new.point(old.unit(layer.point(p))));
        close(followed.unit(followed.point(p)), p);
    }
}
