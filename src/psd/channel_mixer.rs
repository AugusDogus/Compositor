//! PSD mixr uses signed integer percentages in encoded RGB.
use crate::{
    Result,
    adjustment::{ChannelMixer, ExtendedAdjustment},
    invalid,
};
use ag_psd::psd as ps;

pub(super) fn import(source: &ps::ChannelMixerAdjustment) -> Result<ExtendedAdjustment> {
    let row = |value: &Option<ps::ChannelMixerChannel>| -> Result<[f64; 4]> {
        let value = value
            .as_ref()
            .ok_or_else(|| invalid("PSD Channel Mixer is missing an output channel."))?;
        Ok([value.red, value.green, value.blue, value.constant])
    };
    let mut settings = ChannelMixer {
        monochrome: source.monochrome.unwrap_or(false),
        ..Default::default()
    };
    if settings.monochrome {
        settings.rows[0] = row(&source.gray)?;
    } else {
        settings.rows = [row(&source.red)?, row(&source.green)?, row(&source.blue)?];
    }
    settings.validate()?;
    Ok(ExtendedAdjustment::ChannelMixer(settings))
}

pub(super) fn export(source: &ExtendedAdjustment) -> Option<ps::AdjustmentLayer> {
    let ExtendedAdjustment::ChannelMixer(settings) = source else {
        return None;
    };
    // Do not silently truncate the Linux editor's fractional percentages.
    let active_rows = if settings.monochrome {
        &settings.rows[..1]
    } else {
        &settings.rows[..]
    };
    if active_rows
        .iter()
        .flatten()
        .any(|value| value.fract() != 0.)
    {
        return None;
    }
    let [red, green, blue] = settings.rows.map(|[red, green, blue, constant]| {
        Some(ps::ChannelMixerChannel {
            red,
            green,
            blue,
            constant,
        })
    });
    Some(ps::AdjustmentLayer::ChannelMixer(
        ps::ChannelMixerAdjustment {
            monochrome: Some(settings.monochrome),
            gray: if settings.monochrome {
                red.clone()
            } else {
                None
            },
            red,
            green,
            blue,
            ..Default::default()
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        document::{Document, Layer, LayerContent},
        render,
    };
    use std::sync::Arc;
    #[test]
    fn channel_mixer_psd_keeps_editable_coefficients_and_rendered_output() {
        for monochrome in [false, true] {
            let mut doc = Document::new(3, 2).unwrap();
            doc.layers[0].content = LayerContent::Raster(Some(Arc::new(
                image::RgbaImage::from_pixel(3, 2, image::Rgba([50, 100, 150, 200])),
            )));
            let settings = ChannelMixer {
                rows: [
                    [0., 0., 100., -10.],
                    [50., 100., -30., 20.],
                    [100., 0., 0., 0.],
                ],
                monochrome,
            };
            let mut layer = Layer::blank("Mixer", 3, 2);
            layer.opacity = 0.75;
            layer.content = LayerContent::ExtendedAdjustment(Box::new(
                ExtendedAdjustment::ChannelMixer(settings),
            ));
            doc.add(layer).unwrap();
            let output = super::super::encode(&doc).unwrap();
            let reopened = super::super::decode(&output).unwrap().document;
            assert_eq!(reopened.layers.len(), 2);
            let LayerContent::ExtendedAdjustment(adjustment) = &reopened.layers[1].content else {
                panic!("Mixer flattened");
            };
            let ExtendedAdjustment::ChannelMixer(actual) = **adjustment else {
                panic!("Wrong adjustment");
            };
            assert_eq!(actual.monochrome, monochrome);
            assert_eq!(actual.rows[0], settings.rows[0]);
            if !monochrome {
                assert_eq!(actual, settings);
            }
            let expected = render::render(&doc, 3, 2).unwrap();
            let actual = render::render(&reopened, 3, 2).unwrap();
            for (a, b) in actual.as_raw().iter().zip(expected.as_raw()) {
                assert!(a.abs_diff(*b) <= 1);
            }
        }
    }
    #[test]
    fn fractional_mixer_exports_a_reported_rendered_copy() {
        let mut doc = Document::new(2, 2).unwrap();
        let mut settings = ChannelMixer::default();
        settings.rows[0][1] = 0.25;
        doc.layers[0].content =
            LayerContent::ExtendedAdjustment(Box::new(ExtendedAdjustment::ChannelMixer(settings)));
        assert!(export(&ExtendedAdjustment::ChannelMixer(settings)).is_none());
        assert!(
            super::super::export_report(&doc)
                .description()
                .contains("All layers are flattened")
        );
    }
}

#[cfg(test)]
mod active_rows_tests {
    use super::*;
    #[test]
    fn inactive_fractional_rows_do_not_flatten_a_monochrome_mix() {
        let mut settings = ChannelMixer {
            monochrome: true,
            ..Default::default()
        };
        settings.rows[1][0] = 0.5;
        assert!(export(&ExtendedAdjustment::ChannelMixer(settings)).is_some());
        settings.rows[0][0] = 0.5;
        assert!(export(&ExtendedAdjustment::ChannelMixer(settings)).is_none());
    }
}
