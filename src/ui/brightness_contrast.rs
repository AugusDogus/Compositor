//! Brightness/Contrast controls produce native Levels settings.
use super::*;
use compositor::adjustment::{Adjustment, BrightnessContrast, EditorHint};

pub(super) fn title(settings: &Adjustment) -> &'static str {
    if settings.brightness_contrast().is_some() {
        "Brightness/Contrast"
    } else {
        super::adjustment_layers::title(settings.kind)
    }
}

pub(super) fn reset(settings: &Adjustment) -> Adjustment {
    let mut reset = Adjustment::new(settings.kind);
    if settings.brightness_contrast().is_some() {
        reset.editor_hint = Some(EditorHint::BrightnessContrast(BrightnessContrast::default()));
    }
    reset
}

impl Editor {
    pub(super) fn open_brightness_contrast(&mut self, layer: bool) -> Result<()> {
        let settings = BrightnessContrast::default().adjustment()?;
        if layer {
            self.add_adjustment_settings(settings)?;
            self.open_adjustment(None)
        } else {
            self.open_pixel_adjustment_settings(settings)
        }
    }

    pub(super) fn editing_brightness_contrast(&self) -> bool {
        self.adjustment_edit
            .as_ref()
            .is_some_and(|edit| edit.settings.brightness_contrast().is_some())
    }
}

#[cfg(test)]
mod tests;
