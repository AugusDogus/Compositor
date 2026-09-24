use super::*;
impl Edit {
    pub(in crate::ui) fn fields(&self) -> Vec<(&'static str, String)> {
        self.bindings()
            .into_iter()
            .map(|binding| {
                (
                    binding.metadata().0,
                    binding.value(&self.settings).to_string(),
                )
            })
            .collect()
    }

    /// Parse only numeric drafts. Curves, guides and group choices remain typed settings.
    pub(in crate::ui) fn parse_fields(&mut self, values: &[String]) -> Result<Settings> {
        let bindings = self.bindings();
        if bindings.len() != values.len() {
            return Err(compositor::invalid(
                "Camera Raw controls changed. Reopen the filter to edit these settings.",
            ));
        }
        let mut settings = self.settings.clone();
        for (binding, value) in bindings.into_iter().zip(values) {
            binding.set(&mut settings, value)?;
        }
        settings.validate()?;
        self.settings = settings.clone();
        Ok(settings)
    }
}
