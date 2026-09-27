//! Independent color and opacity stop editing within the effects transaction.
use super::*;
use compositor::{
    gradient_overlay::{Overlay, Style},
    invalid,
};
#[cfg(test)]
mod tests;
mod view;

#[derive(Clone, Copy, Default, PartialEq)]
enum Channel {
    #[default]
    Color,
    Opacity,
}
#[derive(Clone, Default)]
pub(super) struct Draft {
    channel: Channel,
    selected: usize,
    position: String,
    value: String,
    pub(super) error: String,
}
impl EffectsEditor {
    pub(super) fn overlay_color(&self) -> [u8; 4] {
        self.effects
            .gradient_overlay
            .as_ref()
            .and_then(|s| s.stops.as_slice().get(self.gradient.selected))
            .map_or([0, 0, 0, 255], |s| s.color)
    }
    pub(super) fn set_overlay_color(&mut self, color: [u8; 4]) {
        let Some(overlay) = &mut self.effects.gradient_overlay else {
            return;
        };
        let Some(mut stop) = overlay
            .stops
            .as_slice()
            .get(self.gradient.selected)
            .copied()
        else {
            return;
        };
        stop.color = color;
        match overlay.stops.update(self.gradient.selected, stop) {
            Ok(index) => {
                self.gradient.selected = index;
                self.sync_overlay_stop();
            }
            Err(error) => self.gradient.error = error.to_string(),
        }
    }

    pub(super) fn sync_overlay_stop(&mut self) {
        self.gradient.error.clear();
        let Some(overlay) = &self.effects.gradient_overlay else {
            self.gradient = Draft::default();
            return;
        };
        let draft = &mut self.gradient;
        let (position, value) = match draft.channel {
            Channel::Color => {
                draft.selected = draft.selected.min(overlay.stops.as_slice().len() - 1);
                let stop = overlay.stops.as_slice()[draft.selected];
                (
                    stop.position,
                    format!(
                        "#{:02X}{:02X}{:02X}",
                        stop.color[0], stop.color[1], stop.color[2]
                    ),
                )
            }
            Channel::Opacity => {
                draft.selected = draft
                    .selected
                    .min(overlay.opacity_stops.as_slice().len() - 1);
                let stop = overlay.opacity_stops.as_slice()[draft.selected];
                (stop.position, format!("{:.2}", stop.opacity * 100.))
            }
        };
        draft.position = format!("{:.2}", position * 100.);
        draft.value = value;
    }
    fn update_overlay_stop(&mut self, edit_position: bool) -> Result<()> {
        let percent = |text: &str| -> Result<f64> {
            text.trim()
                .parse::<f64>()
                .ok()
                .filter(|v| (0. ..=100.).contains(v))
                .map(|v| v / 100.)
                .ok_or_else(|| invalid("Enter a position or opacity between 0 and 100%."))
        };
        let Some(overlay) = &mut self.effects.gradient_overlay else {
            return Ok(());
        };
        let draft = &mut self.gradient;
        draft.selected = match draft.channel {
            Channel::Color => {
                let mut stop = overlay.stops.as_slice()[draft.selected];
                if edit_position {
                    stop.position = percent(&draft.position)?;
                } else {
                    stop.color = compositor::palette::parse_hex(&draft.value)?;
                }
                overlay.stops.update(draft.selected, stop)?
            }
            Channel::Opacity => {
                let mut stop = overlay.opacity_stops.as_slice()[draft.selected];
                if edit_position {
                    stop.position = percent(&draft.position)?;
                } else {
                    stop.opacity = percent(&draft.value)?;
                }
                overlay.opacity_stops.update(draft.selected, stop)?
            }
        };
        // Keep valid field edits in the draft while another field is incomplete.
        // Publish the preview only after all displayed values validate.
        percent(&draft.position)?;
        match draft.channel {
            Channel::Color => {
                compositor::palette::parse_hex(&draft.value)?;
            }
            Channel::Opacity => {
                percent(&draft.value)?;
            }
        }
        Ok(())
    }
}
impl Editor {
    fn overlay_stop_input(&mut self, position: bool, value: &str) {
        self.change_effect(|edit| {
            if position {
                edit.gradient.position = value.into();
            } else {
                edit.gradient.value = value.into();
            }
            edit.gradient.error = edit
                .update_overlay_stop(position)
                .err()
                .map_or_else(String::new, |e| e.to_string());
        });
    }
    fn overlay_stop_select(&mut self, channel: Channel, selected: usize) {
        self.change_effect(|edit| {
            edit.gradient.channel = channel;
            edit.gradient.selected = selected;
            edit.sync_overlay_stop();
        });
    }
    fn overlay_stop_count(&mut self, add: bool) {
        self.change_effect(|edit| {
            let Some(overlay) = &mut edit.effects.gradient_overlay else {
                return;
            };
            let draft = &mut edit.gradient;
            let result = match draft.channel {
                Channel::Color => {
                    if add {
                        let stops = overlay.stops.as_slice();
                        let left = draft.selected.min(stops.len() - 2);
                        overlay
                            .stops
                            .insert((stops[left].position + stops[left + 1].position) / 2.)
                    } else {
                        overlay.stops.remove(draft.selected)
                    }
                }
                Channel::Opacity => {
                    if add {
                        let stops = overlay.opacity_stops.as_slice();
                        let left = draft.selected.min(stops.len() - 2);
                        overlay
                            .opacity_stops
                            .insert((stops[left].position + stops[left + 1].position) / 2.)
                    } else {
                        overlay.opacity_stops.remove(draft.selected)
                    }
                }
            };
            match result {
                Ok(index) => {
                    draft.selected = index;
                    edit.sync_overlay_stop();
                }
                Err(error) => draft.error = error.to_string(),
            }
        });
    }
}
