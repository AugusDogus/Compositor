//! Single-opacity white guides shared by the crop overlay and Curves graph.
use super::*;

pub(super) fn shader() -> Result<quickgui::CustomShader> {
    quickgui::CustomShader::new(include_str!("guide_grid.wgsl"))
        .map_err(|error| compositor::invalid(format!("Could not prepare grid guides: {error}")))
}
