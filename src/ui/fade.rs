//! Fade keeps the completed edit as its source across every asynchronous preview.
use super::*;
use compositor::invalid;

pub(super) fn opacity(values: &[String]) -> Result<f64> {
    let amount = values
        .first()
        .and_then(|value| value.trim().parse::<f64>().ok());
    match amount {
        Some(amount) if (0. ..=100.).contains(&amount) => Ok(amount / 100.),
        _ => Err(invalid(
            "Enter a Fade opacity from 0 to 100%. The last edit is preserved.",
        )),
    }
}

impl Editor {
    pub(super) fn can_fade(&self) -> bool {
        self.can_adjust_colors() && self.session().can_fade()
    }

    pub(super) fn editing_fade(&self) -> bool {
        matches!(
            self.tool_form(),
            Some(Form::Edit {
                action: Action::Fade,
                ..
            })
        )
    }

    pub(super) fn apply_fade(&mut self, values: &[String]) -> Result<()> {
        let amount = opacity(values)?;
        if !self.filter_source_is_current() {
            return Err(invalid(
                "The source changed after Fade opened. Cancel Fade and reopen it for the latest edit. Current pixels are preserved.",
            ));
        }
        let source = self.fade_source()?;
        self.begin_filter_commit();
        self.queue(jobs::Job::Fade { source, amount });
        Ok(())
    }
}
