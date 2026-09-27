use compositor::{Result, invalid, vibrance::Vibrance};

pub(in crate::ui) fn fields(settings: Vibrance) -> Vec<(&'static str, String)> {
    vec![
        ("Vibrance (%)", settings.vibrance().to_string()),
        ("Saturation (%)", settings.saturation().to_string()),
    ]
}
pub(in crate::ui) fn parse(values: &[String]) -> Result<Vibrance> {
    let [vibrance, saturation] = values else {
        return Err(invalid(
            "Enter Vibrance and Saturation values from -100% to 100%.",
        ));
    };
    let parse = |value: &str, label| {
        value
            .trim()
            .parse::<f32>()
            .map_err(|_| invalid(format!("Enter a number from -100% to 100% for {label}.")))
    };
    Vibrance::new(
        parse(vibrance, "Vibrance")?,
        parse(saturation, "Saturation")?,
    )
}

#[cfg(test)]
mod tests;
