use super::*;
use image::Rgba;

#[test]
fn channel_mixer_handles_cross_channels_constant_negative_weights_and_clipping() {
    let settings = ChannelMixer {
        rows: [
            [0., 100., 0., 0.],
            [0., 0., 100., 10.],
            [-100., 0., 0., 100.],
        ],
        monochrome: false,
    };
    assert_eq!(settings.pixel([100, 150, 200, 128]), [150, 226, 155, 128]);
    assert_eq!(settings.pixel([100, 150, 200, 0]), [100, 150, 200, 0]);
    let clipped = ChannelMixer {
        rows: [
            [200., 200., 200., 200.],
            [-200., -200., -200., -200.],
            [0., 0., 0., 50.],
        ],
        monochrome: false,
    };
    assert_eq!(clipped.pixel([100, 150, 200, 255]), [255, 0, 128, 255]);
}

#[test]
fn channel_mixer_monochrome_uses_red_row_and_keeps_other_settings() {
    let settings = ChannelMixer {
        rows: [[30., 60., 10., 0.], [0., 0., 100., 0.], [100., 0., 0., 0.]],
        monochrome: true,
    };
    assert_eq!(settings.pixel([100, 150, 200, 64]), [140, 140, 140, 64]);
    let color = ChannelMixer {
        monochrome: false,
        ..settings
    };
    assert_eq!(color.pixel([100, 150, 200, 64]), [140, 200, 100, 64]);
    assert_eq!(
        ChannelMixer::default().pixel([100, 150, 200, 64]),
        [100, 150, 200, 64]
    );
}

#[test]
fn channel_mixer_selection_identity_and_invalid_settings_preserve_sources() {
    use crate::{
        document::{Document, LayerContent},
        filters::{self, Filter},
        selection::Selection,
    };
    use image::{GrayImage, Luma};
    use std::sync::Arc;
    let mut doc = Document::new(3, 1).unwrap();
    doc.layers[0].content =
        LayerContent::Raster(Some(Arc::new(RgbaImage::from_fn(3, 1, |x, _| {
            Rgba([100, 150, 200, if x == 0 { 0 } else { 128 }])
        }))));
    doc.selection = Some(Selection::from_mask(GrayImage::from_fn(3, 1, |x, _| {
        Luma([if x == 2 { 0 } else { 128 }])
    })));
    let original = doc.clone();
    filters::apply(&mut doc, Filter::ChannelMixer(Default::default()), false).unwrap();
    assert_eq!(doc, original);
    for invalid in [201., f64::NAN, f64::INFINITY, -200.1] {
        let mut settings = ChannelMixer::default();
        settings.rows[2][3] = invalid;
        assert!(filters::apply(&mut doc, Filter::ChannelMixer(settings), false).is_err());
        assert_eq!(doc, original);
    }
    let settings = ChannelMixer {
        rows: [[0., 100., 0., 0.], [0., 0., 100., 0.], [100., 0., 0., 0.]],
        monochrome: false,
    };
    filters::apply(&mut doc, Filter::ChannelMixer(settings), false).unwrap();
    let pixels = doc.layers[0].raster().unwrap();
    assert_eq!(pixels[(0, 0)], Rgba([100, 150, 200, 0]));
    assert_eq!(pixels[(1, 0)], Rgba([125, 175, 150, 128]));
    assert_eq!(pixels[(2, 0)], Rgba([100, 150, 200, 128]));
}
