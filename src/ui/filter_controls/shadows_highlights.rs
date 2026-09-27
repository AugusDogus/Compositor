use compositor::{Result, invalid, shadows_highlights::Settings};

pub(in crate::ui) fn fields(settings: Settings) -> Vec<(&'static str, String)> {
    vec![
        ("Shadows (%)", settings.shadows().to_string()),
        ("Highlights (%)", settings.highlights().to_string()),
        ("Radius (px)", settings.radius().to_string()),
    ]
}

pub(in crate::ui) fn parse(values: &[String]) -> Result<Settings> {
    let [shadows, highlights, radius] = values else {
        return Err(invalid("Enter Shadows, Highlights and Radius values."));
    };
    let number = |value: &str, label| {
        value.trim().parse::<f64>().map_err(|_| {
            invalid(format!(
                "Enter a number for {label}. The current settings are preserved."
            ))
        })
    };
    Settings::new(
        number(shadows, "Shadows")?,
        number(highlights, "Highlights")?,
        number(radius, "Radius")?,
    )
}

#[cfg(test)]
mod tests;
