//! Keep all nine mixes while showing the four controls for the chosen range.
use super::*;
use compositor::{
    invalid,
    selective_color::{Mode, Range, SelectiveColor},
};

const MODE: usize = 36;
const RANGE: usize = 37;

impl Editor {
    pub(super) fn selective_color_fields(
        &self,
        cx: &mut ViewContext<'_, Self>,
        action: Action,
        fields: &[(&'static str, String)],
    ) -> Element {
        let chosen = fields
            .get(RANGE)
            .map_or("Reds", |(_, value)| value.as_str());
        let range = Range::ALL
            .into_iter()
            .find(|range| range.name() == chosen)
            .unwrap_or_default();
        let mut controls = div().flex_col().gap(10.).flex_shrink_0();
        for ranges in Range::ALL.chunks(3) {
            let choices: Vec<_> = ranges
                .iter()
                .map(|range| (range.name(), range.name()))
                .collect();
            controls = controls.child(self.field_choices(cx, RANGE, chosen, &choices));
        }
        for index in range.index() * 4..range.index() * 4 + 4 {
            if let Some((_, value)) = fields.get(index)
                && let Some(control) = self.parameter_control(cx, action, index, value)
            {
                controls = controls.child(control);
            }
        }
        let mode = fields
            .get(MODE)
            .map_or("relative", |(_, value)| value.as_str());
        controls.child(self.field_choices(
            cx,
            MODE,
            mode,
            &[("Relative", "relative"), ("Absolute", "absolute")],
        ))
    }
}

pub(super) fn fields(settings: SelectiveColor) -> Vec<(&'static str, String)> {
    let mut fields: Vec<_> = settings
        .adjustments
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(index, value)| {
            (
                ["Cyan", "Magenta", "Yellow", "Black"][index % 4],
                value.to_string(),
            )
        })
        .collect();
    fields.push((
        "Method",
        match settings.mode {
            Mode::Relative => "relative",
            Mode::Absolute => "absolute",
        }
        .into(),
    ));
    fields.push(("Color range", Range::Reds.name().into()));
    fields
}

pub(super) fn parse(values: &[String]) -> Result<SelectiveColor> {
    let mut settings = SelectiveColor::default();
    for (index, coefficient) in settings.adjustments.iter_mut().flatten().enumerate() {
        *coefficient = values
            .get(index)
            .and_then(|value| value.trim().parse().ok())
            .ok_or_else(|| invalid("Enter Selective Color values between -100% and 100%."))?;
    }
    settings.mode = match values.get(MODE).map(String::as_str) {
        Some("relative") => Mode::Relative,
        Some("absolute") => Mode::Absolute,
        _ => return Err(invalid("Choose Relative or Absolute for Selective Color.")),
    };
    if !values
        .get(RANGE)
        .is_some_and(|value| Range::ALL.iter().any(|range| range.name() == value))
    {
        return Err(invalid("Choose a Selective Color range."));
    }
    settings.validate()?;
    Ok(settings)
}

#[cfg(test)]
mod tests;
