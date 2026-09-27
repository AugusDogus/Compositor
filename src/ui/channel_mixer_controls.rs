use super::*;

impl Editor {
    pub(super) fn channel_mixer_fields(
        &self,
        cx: &mut ViewContext<'_, Self>,
        action: Action,
        fields: &[(&'static str, String)],
    ) -> Element {
        let monochrome = fields.get(12).is_some_and(|(_, value)| value == "1");
        let output = fields.get(13).map_or("red", |(_, value)| value.as_str());
        let channel = if monochrome {
            0
        } else {
            match output {
                "green" => 1,
                "blue" => 2,
                _ => 0,
            }
        };
        let mut rows = div().flex_col().gap(16.).flex_shrink_0();
        rows = rows.child(self.form_toggle(cx, 12, "Monochrome", monochrome));
        if monochrome {
            rows = rows.child(
                text("Gray output (red channel mix)")
                    .text_size(13.)
                    .line_height(16.),
            );
        } else {
            rows = rows.child(self.channel_mixer_output(cx, output));
        }
        for index in channel * 4..channel * 4 + 4 {
            if let Some((_, value)) = fields.get(index)
                && let Some(control) = self.parameter_control(cx, action, index, value)
            {
                rows = rows.child(control);
            }
        }
        rows
    }
    fn channel_mixer_output(&self, cx: &mut ViewContext<'_, Self>, output: &str) -> Element {
        let choices = self.field_choices(
            cx,
            13,
            output,
            &[("Red", "red"), ("Green", "green"), ("Blue", "blue")],
        );
        div()
            .flex_row()
            .items_center()
            .gap(12.)
            .child(text("Output").text_size(13.).line_height(16.))
            .child(choices)
    }
}

#[cfg(test)]
mod tests;

pub(super) fn fields(
    settings: compositor::adjustment::ChannelMixer,
) -> Vec<(&'static str, String)> {
    let mut fields: Vec<_> = settings
        .rows
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(index, value)| {
            (
                ["Red input", "Green input", "Blue input", "Constant"][index % 4],
                value.to_string(),
            )
        })
        .collect();
    fields.push((
        "Monochrome (0 or 1)",
        u8::from(settings.monochrome).to_string(),
    ));
    fields.push(("Output channel", "red".into()));
    fields
}
pub(super) fn parse(values: &[String]) -> Result<compositor::adjustment::ChannelMixer> {
    use compositor::{adjustment::ChannelMixer, invalid};
    let mut settings = ChannelMixer::default();
    for (index, coefficient) in settings.rows.iter_mut().flatten().enumerate() {
        *coefficient = values
            .get(index)
            .and_then(|s| s.trim().parse().ok())
            .ok_or_else(|| invalid("Enter a Channel Mixer coefficient between -200% and 200%."))?;
    }
    settings.monochrome = match values.get(12).map(|s| s.trim()) {
        Some("0") => false,
        Some("1") => true,
        _ => return Err(invalid("Monochrome must be 0 or 1.")),
    };
    if !matches!(
        values.get(13).map(String::as_str),
        Some("red" | "green" | "blue")
    ) {
        return Err(invalid("Choose a Channel Mixer output channel."));
    }
    settings.validate()?;
    Ok(settings)
}
