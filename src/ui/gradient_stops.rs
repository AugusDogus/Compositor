//! Color stop editing is tool state. The existing gradient transaction owns canvas undo.
use super::*;
use compositor::gradient::{
    Gradient, Style,
    stops::{MAX_STOPS, Stops},
};
use quickgui::{MouseButton, PointerPhase};

#[derive(Clone)]
pub(super) struct Draft {
    original: Gradient,
    pub(super) selected: usize,
    position: String,
    opacity: String,
    error: String,
}

pub(super) fn ramp_view(stops: Stops, reversed: bool, mask: bool) -> Element {
    quickgui::canvas(move |bounds, painter| {
        let width = bounds.width.ceil() as usize;
        for x in 0..width {
            let t = x as f64 / width.saturating_sub(1).max(1) as f64;
            let color = stops.sample(if reversed { 1. - t } else { t });
            let [r, g, b, a] = if mask {
                compositor::gradient::mask_color(color)
            } else {
                color
            };
            painter.fill_rect(
                quickgui::Rect::new(x as f32, 0., 1., bounds.height),
                Color::rgba8(
                    (r * 255.).round() as u8,
                    (g * 255.).round() as u8,
                    (b * 255.).round() as u8,
                    (a * 255.).round() as u8,
                ),
            );
        }
    })
}

impl Editor {
    /// Selecting Custom restores its saved ramp, independently of the current preset.
    pub(super) fn open_gradient_stops(&mut self) -> Result<()> {
        self.open_stop_editor(self.tools.gradient.stops.clone())
    }

    /// Clicking the swatch edits its displayed colors, seeding a custom ramp from a preset.
    pub(super) fn edit_displayed_gradient_stops(&mut self) -> Result<()> {
        self.open_stop_editor(self.gradient_preview_ramp())
    }

    fn open_stop_editor(&mut self, stops: Stops) -> Result<()> {
        let original = self.tools.gradient.clone();
        self.tools.gradient.stops = stops;
        self.tools.gradient.style = Style::Custom;
        self.modal = Some(Form::GradientStops(Box::new(Draft {
            original,
            selected: 0,
            position: String::new(),
            opacity: String::new(),
            error: String::new(),
        })));
        self.sync_gradient_stop_fields();
        self.sync_gradient_picker();
        self.refresh_gradient()
    }

    fn sync_gradient_stop_fields(&mut self) {
        if let Some(Form::GradientStops(draft)) = &mut self.modal
            && let Some(stop) = self.tools.gradient.stops.as_slice().get(draft.selected)
        {
            draft.position = format!("{:.2}", stop.position * 100.);
            draft.opacity = format!("{:.2}", f64::from(stop.color[3]) / 255. * 100.);
            draft.error.clear();
        }
    }

    pub(super) fn finish_gradient_stops(&mut self, apply: bool) -> Result<()> {
        let Some(Form::GradientStops(draft)) = &self.modal else {
            return Ok(());
        };
        if apply && !draft.error.is_empty() {
            return Err(compositor::invalid(draft.error.clone()));
        }
        if !apply {
            self.tools.gradient = draft.original.clone();
        }
        self.modal = None;
        self.sync_gradient_picker();
        self.refresh_gradient()
    }

    fn gradient_stop_input(&mut self, position: bool, value: &str) -> Result<()> {
        let Some(Form::GradientStops(draft)) = &mut self.modal else {
            return Ok(());
        };
        if position {
            draft.position = value.into();
        } else {
            draft.opacity = value.into();
        }
        let number = |text: &str| {
            text.trim()
                .parse::<f64>()
                .ok()
                .filter(|value| (0. ..=100.).contains(value))
        };
        let (Some(position), Some(opacity)) = (number(&draft.position), number(&draft.opacity))
        else {
            draft.error = "Position and opacity must be numbers between 0 and 100%.".into();
            return Ok(());
        };
        let mut stop = self.tools.gradient.stops.as_slice()[draft.selected];
        stop.position = position / 100.;
        stop.color[3] = (opacity / 100. * 255.).round() as u8;
        draft.selected = self.tools.gradient.stops.update(draft.selected, stop)?;
        draft.error.clear();
        self.refresh_gradient()
    }

    pub(super) fn set_gradient_stop_color(&mut self, index: usize, color: [u8; 4]) -> Result<()> {
        let Some(mut stop) = self.tools.gradient.stops.as_slice().get(index).copied() else {
            return Err(compositor::invalid(
                "Select a gradient stop before changing its color.",
            ));
        };
        stop.color = color;
        self.tools.gradient.stops.update(index, stop)?;
        self.refresh_gradient()
    }

    fn add_gradient_stop(&mut self) -> Result<()> {
        let Some(Form::GradientStops(draft)) = &mut self.modal else {
            return Ok(());
        };
        let stops = self.tools.gradient.stops.as_slice();
        let left = draft.selected.min(stops.len() - 2);
        let position = (stops[left].position + stops[left + 1].position) / 2.;
        draft.selected = self.tools.gradient.stops.insert(position)?;
        self.sync_gradient_stop_fields();
        self.refresh_gradient()
    }

    fn remove_gradient_stop(&mut self) -> Result<()> {
        let Some(Form::GradientStops(draft)) = &mut self.modal else {
            return Ok(());
        };
        draft.selected = self.tools.gradient.stops.remove(draft.selected)?;
        self.sync_gradient_stop_fields();
        self.refresh_gradient()
    }
}

#[cfg(test)]
mod tests;

mod view;
