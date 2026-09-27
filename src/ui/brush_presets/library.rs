use super::*;
impl Library {
    pub(super) fn pixels(&self) -> usize {
        self.tips.iter().map(|e| e.tip.pixels().len()).sum()
    }
}
impl Editor {
    pub(in crate::ui) fn install_brush_tip(&mut self, tip: Tip) -> Result<()> {
        self.install_brush_tips(vec![tip])
    }
    pub(in crate::ui) fn install_brush_tips(&mut self, tips: Vec<Tip>) -> Result<()> {
        let mut additions: Vec<Entry> = Vec::new();
        let mut selected = None;
        let mut pixels = self.brush_presets.pixels();
        for tip in tips {
            if let Some(existing) = self
                .brush_presets
                .tips
                .iter()
                .chain(&additions)
                .find(|e| *e.tip == tip)
            {
                selected = Some(existing.tip.clone());
                continue;
            }
            pixels += tip.pixels().len();
            if self.brush_presets.tips.len() + additions.len() >= MAX_TIPS
                || pixels > MAX_TOTAL_PIXELS
            {
                return Err(invalid(
                    "The session brush library is full (32 tips or 16 million pixels). No tips were added. Unload a tip in Brush Tips, then retry the import.",
                ));
            }
            let thumb = image::DynamicImage::ImageLuma8(tip.pixels().clone())
                .thumbnail(40, 40)
                .into_luma8();
            let rgba = image::RgbaImage::from_fn(thumb.width(), thumb.height(), |x, y| {
                image::Rgba([230, 230, 230, thumb[(x, y)][0]])
            });
            let thumbnail = Image::from_rgba(rgba.width(), rgba.height(), rgba.into_raw())
                .map_err(|e| {
                    invalid(format!(
                        "Could not display the imported tip: {e}. No tips were added."
                    ))
                })?;
            let tip = Arc::new(tip);
            selected = Some(tip.clone());
            additions.push(Entry { tip, thumbnail });
        }
        let count = additions.len();
        self.brush_presets.tips.extend(additions);
        if let Some(tip) = selected {
            self.tools.brush_shape = Shape::Sampled(Sampled::new(tip));
        }
        self.open_brush_tips();
        self.status = format!(
            "{count} brush tips added for this session. Painting uses the foreground color and current brush size."
        );
        Ok(())
    }
    pub(super) fn unload_brush_tip(&mut self) {
        let Shape::Sampled(sampled) = &self.tools.brush_shape else {
            return;
        };
        let tip = sampled.tip().clone();
        self.brush_presets
            .tips
            .retain(|entry| !Arc::ptr_eq(&entry.tip, &tip));
        for tools in self
            .tabs
            .iter_mut()
            .map(|tab| &mut tab.parked_tools)
            .chain(std::iter::once(&mut self.tools))
        {
            if matches!(&tools.brush_shape, Shape::Sampled(sampled) if Arc::ptr_eq(sampled.tip(), &tip))
            {
                tools.brush_shape = Shape::Round;
            }
        }
        self.open_brush_tips();
        self.status = "Brush tip unloaded. Projects using it now use the round tip; painted pixels are unchanged.".into();
    }
}
