//! Fixture written by upstream ProjectStore on a physical macOS host.
use compositor::{guides::Axis, project};
use std::path::Path;

#[test]
fn native_swift_v10_project_preserves_unicode_colors_effects_and_pixels() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/macos/mac-v10-text-effects.comp");
    let document = project::load(&path).unwrap();
    assert_eq!([document.width, document.height], [96, 64]);
    assert_eq!(document.resolution, 144.);
    assert_eq!(document.guides.len(), 1);
    assert_eq!(document.guides[0].axis, Axis::Vertical);
    assert_eq!(document.guides[0].position, 12.5);
    let layer = &document.layers[0];
    assert_eq!(layer.transform.origin, [3.5, -2.]);
    assert_eq!(layer.transform.size, [64., 48.]);
    assert_eq!(layer.transform.rotation, 17.);
    assert!(layer.transform.flip_x);
    let text = layer.text.as_ref().unwrap();
    assert_eq!(text.content, "Mac 🙂 中 e\u{301}");
    assert_eq!(text.font_name, "Helvetica");
    assert_eq!(text.box_size, Some([240., 72.]));
    let runs = text.color_runs.as_ref().unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!((runs[0].location, runs[0].length), (4, 2));
    assert_eq!([runs[0].red, runs[0].green, runs[0].blue], [1., 0., 0.]);
    assert_eq!((runs[1].location, runs[1].length), (7, 1));
    assert_eq!([runs[1].red, runs[1].green, runs[1].blue], [0., 0.5, 1.]);
    let effects = layer.effects.as_ref().unwrap();
    assert!(effects.stroke.is_some());
    assert!(effects.shadow.is_some());
    assert!(effects.color_overlay.is_some());
    assert!(effects.inner_shadow.is_some());
    assert!(effects.outer_glow.is_some());
    assert!(effects.inner_glow.is_some());
    let pixels = layer.raster().unwrap();
    assert_eq!(pixels.dimensions(), (32, 24));
    assert_eq!(pixels[(0, 0)].0, [0, 0, 128, 255]);
    assert_eq!(pixels[(31, 23)].0, [217, 230, 128, 255]);

    let directory = tempfile::tempdir().unwrap();
    let saved = directory.path().join("linux.comp");
    project::save(&document, &saved).unwrap();
    assert_eq!(project::load(&saved).unwrap(), document);
}
