use super::*;
impl Library {
    pub(super) fn pixels(&self) -> usize {
        self.tips.iter().map(|e| e.brush.pixel_count()).sum()
    }
}
impl Editor {
    pub(in crate::ui) fn install_brush_tip(&mut self, tip: Tip) -> Result<()> {
        self.install_brush_tips(vec![tip])
    }
    pub(in crate::ui) fn install_brush_tips(&mut self, tips: Vec<Tip>) -> Result<()> {
        self.install_sampled_brushes(
            tips.into_iter()
                .map(|tip| Sampled::new(Arc::new(tip)))
                .collect(),
        )
    }
    pub(in crate::ui) fn install_brush_hose(
        &mut self,
        hose: compositor::brush::sampled::gih::Hose,
    ) -> Result<()> {
        self.install_sampled_brushes(vec![Sampled::from_hose(Arc::new(hose))])
    }
    fn install_sampled_brushes(&mut self, brushes: Vec<Sampled>) -> Result<()> {
        let mut additions: Vec<Entry> = Vec::new();
        let mut selected = None;
        let mut pixels = self.brush_presets.pixels();
        for brush in brushes {
            if let Some(existing) =
                self.brush_presets.tips.iter().chain(&additions).find(|e| {
                    match (e.brush.hose(), brush.hose()) {
                        (Some(a), Some(b)) => a == b,
                        (None, None) => e.brush.tip() == brush.tip(),
                        _ => false,
                    }
                })
            {
                selected = Some(existing.brush.clone());
                continue;
            }
            pixels += brush.pixel_count();
            if self.brush_presets.tips.len() + additions.len() >= MAX_TIPS
                || pixels > MAX_TOTAL_PIXELS
            {
                return Err(invalid(
                    "The session brush library is full (32 brushes or 16 million pixels). No brushes were added. Unload a brush in Brush Tips, then retry the import.",
                ));
            }
            let thumb = image::DynamicImage::ImageLuma8(brush.tip().pixels().clone())
                .thumbnail(40, 40)
                .into_luma8();
            let rgba = image::RgbaImage::from_fn(thumb.width(), thumb.height(), |x, y| {
                image::Rgba([230, 230, 230, thumb[(x, y)][0]])
            });
            let thumbnail = Image::from_rgba(rgba.width(), rgba.height(), rgba.into_raw())
                .map_err(|e| {
                    invalid(format!(
                        "Could not display the imported tip: {e}. No brushes were added."
                    ))
                })?;
            selected = Some(brush.clone());
            additions.push(Entry { brush, thumbnail });
        }
        let count = additions.len();
        self.brush_presets.tips.extend(additions);
        if let Some(brush) = selected {
            self.tools.brush_shape = Shape::Sampled(brush);
        }
        self.open_brush_tips();
        self.status = format!(
            "{count} brushes added for this session. Painting uses the foreground color and current brush size."
        );
        Ok(())
    }
    pub(super) fn unload_brush_tip(&mut self) {
        let Shape::Sampled(sampled) = &self.tools.brush_shape else {
            return;
        };
        let removed = sampled.clone();
        self.brush_presets
            .tips
            .retain(|entry| !entry.brush.same_source(&removed));
        for tools in self
            .tabs
            .iter_mut()
            .map(|tab| &mut tab.parked_tools)
            .chain(std::iter::once(&mut self.tools))
        {
            if matches!(&tools.brush_shape, Shape::Sampled(sampled) if sampled.same_source(&removed))
            {
                tools.brush_shape = Shape::Round;
            }
        }
        self.open_brush_tips();
        self.status = "Brush tip unloaded. Projects using it now use the round tip; painted pixels are unchanged.".into();
    }
}
