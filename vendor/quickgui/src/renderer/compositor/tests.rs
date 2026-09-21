use super::*;
use crate::renderer::{OffscreenRenderer, create_shared_font_system};
use crate::scene::PaintLayerKey;
use crate::{Assets, DropShadow, LayerEffects, PerformanceProfile, Quad, Size, Vector};

fn renderer() -> OffscreenRenderer {
    let fonts = create_shared_font_system(&Assets::default(), &[]).unwrap();
    pollster::block_on(OffscreenRenderer::new(PerformanceProfile::Balanced, fonts)).unwrap()
}

#[test]
fn collapsed_groups_hide_their_content_and_reappear_when_expanded() {
    let mut renderer = renderer();
    for scale in [1., 2.] {
        for (x, y) in [(0., 1.), (1., 1.), (1., 0.), (0., 0.), (1., 1.)] {
            let mut scene = Scene::new();
            scene.clear(UiColor::BLACK);
            let bounds = Rect::new(0., 0., 32., 32.);
            let group = scene
                .begin_group(
                    PaintLayerKey::default(),
                    bounds,
                    bounds,
                    LayerEffects {
                        transform: Transform2D::scale(x, y),
                        // Retain a group when the transform returns to identity.
                        color_matrix: ColorMatrix::from(crate::Filter::Brightness(0.5)),
                        ..Default::default()
                    },
                )
                .unwrap();
            scene.push_quad_in(group.content_key(), Quad::new(bounds, UiColor::WHITE));
            scene.end_group(group);
            scene.finish();
            let snapshot = renderer
                .render_to_snapshot(&scene, Size::new(32., 32.), scale)
                .unwrap();
            let center = snapshot
                .pixel((16. * scale) as u32, (16. * scale) as u32)
                .unwrap();
            if x == 0. || y == 0. {
                assert_eq!(center, [0, 0, 0, 255]);
            } else {
                assert!(center[0] > 100, "Expanded group failed to reappear");
            }
        }
    }
}

fn backdrop_scene(
    count: usize,
    opacity: f32,
    background: UiColor,
    isolated: bool,
    clip: Rect,
) -> Scene {
    let mut scene = Scene::new();
    scene.clear(background);
    for x in (0..96).step_by(4) {
        scene.push_quad(Quad::new(Rect::new(x as f32, 0., 2., 64.), UiColor::BLACK));
    }
    for index in 0..count {
        let bounds = Rect::new(8. + index as f32 * 4., 8., 48., 48.);
        let previous_opacity = scene.multiply_opacity(opacity);
        let group = scene
            .begin_group(
                PaintLayerKey::default(),
                bounds,
                clip,
                LayerEffects {
                    backdrop_blur: 4.,
                    backdrop_corners: Corners::all(8.),
                    // An invisible zero-radius shadow forces the existing offscreen path
                    // without changing the expected image. Compare both rendering paths.
                    drop_shadow: isolated
                        .then(|| DropShadow::new(Vector::ZERO, 0., UiColor::TRANSPARENT)),
                    ..Default::default()
                },
            )
            .unwrap();
        scene.push_quad_in(
            group.content_key(),
            Quad::new(bounds, UiColor::rgba8(120, 30, 20, 120)),
        );
        scene.push_quad_in(
            group.content_key(),
            Quad::new(Rect::new(bounds.x + 8., 20., 24., 16.), UiColor::WHITE),
        );
        scene.push_quad_in(
            group.content_key(),
            Quad::new(Rect::new(bounds.x + 20., 28., 20., 16.), UiColor::BLACK),
        );
        scene.end_group(group);
        scene.restore_opacity(previous_opacity);
    }
    scene.finish();
    scene
}

#[test]
fn direct_backdrops_match_isolated_compositing_through_background_and_opacity_changes() {
    let mut direct = renderer();
    for (opacity, background) in [
        (1., UiColor::WHITE),
        (1., UiColor::BLACK),
        (0.5, UiColor::WHITE),
        (1., UiColor::rgb8(30, 60, 120)),
    ] {
        let actual = direct
            .render_to_snapshot(
                &backdrop_scene(2, opacity, background, false, Rect::new(0., 0., 96., 64.)),
                Size::new(96., 64.),
                1.,
            )
            .unwrap();
        let expected = renderer()
            .render_to_snapshot(
                &backdrop_scene(2, opacity, background, true, Rect::new(0., 0., 96., 64.)),
                Size::new(96., 64.),
                1.,
            )
            .unwrap();
        let largest_difference = actual
            .rgba()
            .iter()
            .zip(expected.rgba())
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(
            largest_difference <= 1,
            "Direct and isolated source-over differ by {largest_difference} at opacity {opacity}"
        );
        let stats = direct.last_composite();
        assert_eq!(stats.skipped_layer_effects, 0);
        assert_eq!(stats.layer_passes, if opacity == 1. { 0 } else { 2 });
    }
}

#[test]
fn backdrop_siblings_share_three_textures_instead_of_allocating_per_menu() {
    let mut renderer = renderer();
    for count in [1, 2, crate::MAX_LAYERS_PER_FRAME] {
        renderer
            .render_to_snapshot(
                &backdrop_scene(
                    count,
                    1.,
                    UiColor::WHITE,
                    false,
                    Rect::new(0., 0., 96., 64.),
                ),
                Size::new(96., 64.),
                1.,
            )
            .unwrap();
        let stats = renderer.last_composite();
        assert_eq!(stats.skipped_layer_effects, 0);
        assert_eq!(stats.layer_passes, 0);
        assert_eq!(stats.blur_passes, count * 2);
        assert_eq!(stats.layer_texture_bytes, 3 * 96 * 64 * 4);
    }
}

#[test]
fn direct_backdrop_foreground_preserves_parent_clips() {
    for clip in [
        Rect::new(20., 24., 28., 20.),
        Rect::new(20.5, 24.2, 28., 20.3),
    ] {
        let actual = renderer()
            .render_to_snapshot(
                &backdrop_scene(2, 1., UiColor::WHITE, false, clip),
                Size::new(96., 64.),
                1.,
            )
            .unwrap();
        let expected = renderer()
            .render_to_snapshot(
                &backdrop_scene(2, 1., UiColor::WHITE, true, clip),
                Size::new(96., 64.),
                1.,
            )
            .unwrap();
        let difference = actual
            .rgba()
            .iter()
            .zip(expected.rgba())
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(
            difference <= 1,
            "Clipped foreground differs by {difference} for {clip:?}"
        );
    }
}

#[test]
fn nested_effects_preserve_parent_clipping_and_isolation() {
    let scene = |isolated: bool, child_effects: LayerEffects| {
        let mut scene = Scene::new();
        scene.clear(UiColor::WHITE);
        let bounds = Rect::new(8., 8., 64., 48.);
        let outer = scene
            .begin_group(
                PaintLayerKey::default(),
                bounds,
                Rect::new(20.5, 24.2, 28., 20.3),
                LayerEffects {
                    backdrop_blur: 4.,
                    drop_shadow: isolated
                        .then(|| DropShadow::new(Vector::ZERO, 0., UiColor::TRANSPARENT)),
                    ..Default::default()
                },
            )
            .unwrap();
        scene.push_quad_in(
            outer.content_key(),
            Quad::new(bounds, UiColor::rgba8(200, 20, 40, 60)),
        );
        let inner = scene
            .begin_group(
                outer.content_key(),
                Rect::new(24., 16., 32., 32.),
                Rect::new(0., 0., 96., 64.),
                child_effects,
            )
            .unwrap();
        scene.push_quad_in(
            inner.content_key(),
            Quad::new(
                Rect::new(24., 16., 32., 32.),
                UiColor::rgba8(20, 80, 200, 60),
            ),
        );
        scene.end_group(inner);
        scene.end_group(outer);
        scene.finish();
        scene
    };
    for effects in [
        LayerEffects {
            backdrop_blur: 4.,
            ..Default::default()
        },
        LayerEffects {
            blur: 2.,
            ..Default::default()
        },
        LayerEffects {
            blend: BlendMode::Screen,
            ..Default::default()
        },
    ] {
        let actual = renderer()
            .render_to_snapshot(&scene(false, effects), Size::new(96., 64.), 1.)
            .unwrap();
        let expected = renderer()
            .render_to_snapshot(&scene(true, effects), Size::new(96., 64.), 1.)
            .unwrap();
        let difference = actual
            .rgba()
            .iter()
            .zip(expected.rgba())
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(
            difference <= 1,
            "Nested effect {effects:?} differs by {difference}"
        );
    }
}
