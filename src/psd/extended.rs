//! Editable PSD encodings for Linux authoring adjustments.
use crate::adjustment::ExtendedAdjustment;
use ag_psd::psd::AdjustmentLayer;

pub(super) fn export(adjustment: &ExtendedAdjustment) -> Option<AdjustmentLayer> {
    match adjustment {
        ExtendedAdjustment::Threshold(settings) => Some(super::threshold::export(*settings)),
        ExtendedAdjustment::Posterize(settings) => Some(super::posterize::export(*settings)),
        ExtendedAdjustment::Vibrance(settings) => super::vibrance::export(*settings),
        ExtendedAdjustment::PhotoFilter(_) => None,
        ExtendedAdjustment::ChannelMixer(_) => super::channel_mixer::export(adjustment),
        ExtendedAdjustment::SelectiveColor(settings) => super::selective_color::export(settings),
    }
}
