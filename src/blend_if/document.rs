use crate::{Result, document::Document, invalid};

pub(crate) fn validate_document(document: &Document) -> Result<()> {
    for layer in &document.layers {
        if layer.blend_if.is_some() && (layer.is_group() || layer.is_adjustment()) {
            return Err(invalid(format!(
                "{}: Blend If requires a pixel, shape, text, or RAW layer. Rasterize the folder or adjustment before applying Blend If; its current settings are preserved.",
                layer.name
            )));
        }
    }
    Ok(())
}
