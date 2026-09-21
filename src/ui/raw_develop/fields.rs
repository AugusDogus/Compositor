use super::*;

// Keep a representable margin above the validated 1% crop minimum.
const CROP_GAP: f32 = 0.010001;

macro_rules! scalar_fields {
    ($($variant:ident => $field:ident),* $(,)?) => {
        #[derive(Clone,Copy)]
        pub(super) enum Field { $($variant,)* Hsl(usize,usize), Mix(usize), Shadow(usize), Highlight(usize), Perspective(usize), Crop(usize), Mask(usize,MaskField) }
        impl Field {
            pub(super) fn value(self,s:&DevelopSettings)->f32 {match self {
                $(Self::$variant=>s.$field,)*
                Self::Hsl(b,c)=>s.hsl[b][c],Self::Mix(c)=>s.bw_mix[c],Self::Shadow(c)=>s.shadow_tone[c],Self::Highlight(c)=>s.highlight_tone[c],Self::Perspective(c)=>s.perspective[c],Self::Crop(c)=>s.crop[c],
                Self::Mask(i,f)=>s.overlays.get(i).map_or(0.,|m|match f {MaskField::Exposure=>m.exposure,MaskField::Warmth=>m.warmth,MaskField::Saturation=>m.saturation,MaskField::Radius=>m.radius,MaskField::Feather=>m.feather}),
            }}
            pub(super) fn set(self,s:&mut DevelopSettings,value:f32) {
                if matches!(self, Self::Temperature) { s.white_balance = raw::WhiteBalance::Temperature; }
                match self {
                $(Self::$variant=>s.$field=value,)*
                Self::Hsl(b,c)=>s.hsl[b][c]=value,Self::Mix(c)=>s.bw_mix[c]=value,Self::Shadow(c)=>s.shadow_tone[c]=value,Self::Highlight(c)=>s.highlight_tone[c]=value,Self::Perspective(c)=>s.perspective[c]=value,
                Self::Crop(c)=>s.crop[c]=match c {0=>value.min(s.crop[2]-CROP_GAP),1=>value.min(s.crop[3]-CROP_GAP),2=>value.max(s.crop[0]+CROP_GAP),_=>value.max(s.crop[1]+CROP_GAP)},
                Self::Mask(i,f)=>if let Some(m)=s.overlays.get_mut(i) {match f {MaskField::Exposure=>m.exposure=value,MaskField::Warmth=>m.warmth=value,MaskField::Saturation=>m.saturation=value,MaskField::Radius=>m.radius=value,MaskField::Feather=>m.feather=value}},
            }}
        }
    }
}
scalar_fields! {Temperature=>temperature,Tint=>tint,Exposure=>exposure,Brightness=>brightness,Contrast=>contrast,Highlights=>highlights,Shadows=>shadows,Whites=>whites,Blacks=>blacks,Saturation=>saturation,Vibrance=>vibrance,Clarity=>clarity,Texture=>texture,Dehaze=>dehaze,ToneBalance=>tone_balance,LuminanceNoise=>luminance_noise,ColorNoise=>color_noise,Sharpen=>sharpen,SharpenRadius=>sharpen_radius,SharpenThreshold=>sharpen_threshold,Distortion=>distortion,ChromaticRed=>chromatic_red,ChromaticBlue=>chromatic_blue,Defringe=>defringe,Vignette=>vignette,Rotation=>rotation}
#[derive(Clone, Copy)]
pub(super) enum MaskField {
    Exposure,
    Warmth,
    Saturation,
    Radius,
    Feather,
}

pub(super) struct NumericDraft {
    id: String,
    label: String,
    text: String,
    field: Field,
    range: (f32, f32),
}

impl Develop {
    pub(super) fn finish_numeric_input(&mut self) -> bool {
        let Some(draft) = self.numeric_draft.take() else {
            return true;
        };
        if self.committing {
            self.numeric_draft = Some(draft);
            return false;
        }
        match draft.text.trim().parse::<f32>() {
            Ok(value) if value.is_finite() => {
                self.edit(|settings| {
                    draft
                        .field
                        .set(settings, value.clamp(draft.range.0, draft.range.1))
                });
                true
            }
            _ => {
                self.error = Some(format!(
                    "Enter a number for {} before continuing. The previous setting is unchanged.",
                    draft.label
                ));
                self.numeric_draft = Some(draft);
                false
            }
        }
    }
}

impl Editor {
    pub(in crate::ui) fn sync_raw_input(&mut self, cx: &ViewContext<'_, Self>) {
        if let Some(d) = &mut self.develop
            && d.numeric_draft
                .as_ref()
                .is_some_and(|draft| !cx.is_focused(quickgui::FocusHandle::new(draft.id.clone())))
        {
            d.finish_numeric_input();
        }
    }
    pub(super) fn raw_field(
        &self,
        cx: &mut ViewContext<'_, Self>,
        label: &str,
        field: Field,
        range: (f32, f32),
    ) -> Element {
        let value = self
            .develop
            .as_ref()
            .map_or(0., |d| field.value(&d.settings));
        let id = format!("raw-field-{label}");
        let slider_id = format!("raw-slider-{label}");
        let displayed = self
            .develop
            .as_ref()
            .and_then(|d| d.numeric_draft.as_ref())
            .filter(|draft| draft.id == id)
            .map_or_else(|| format!("{value:.2}"), |draft| draft.text.clone());
        let pointer = cx.pointer_listener(slider_id.clone(), move |this, event, cx| {
            use quickgui::PointerPhase;
            let Some(d) = &mut this.develop else {
                return;
            };
            if d.committing || event.button != quickgui::MouseButton::Left {
                return;
            }
            if event.phase == PointerPhase::Down {
                d.finish_numeric_input();
                d.begin_gesture();
            }
            if matches!(event.phase, PointerPhase::Down | PointerPhase::Move) {
                let fraction = ((event.local_position.x - 6.) / (event.size.width - 12.).max(1.))
                    .clamp(0., 1.);
                d.edit(|s| field.set(s, range.0 + fraction * (range.1 - range.0)));
            }
            if matches!(event.phase, PointerPhase::Up | PointerPhase::Cancel) {
                d.finish_gesture(event.phase == PointerPhase::Cancel);
            }
            cx.invalidate();
        });
        let input_id = id.clone();
        let input_label = label.to_owned();
        let input = cx.input_listener(id.clone(), move |this, input, cx| {
            if let Some(d) = &mut this.develop
                && !d.committing
            {
                if d.numeric_draft
                    .as_ref()
                    .is_some_and(|draft| draft.id != input_id)
                {
                    d.finish_numeric_input();
                }
                d.numeric_draft = Some(NumericDraft {
                    id: input_id.clone(),
                    label: input_label.clone(),
                    text: input.into(),
                    field,
                    range,
                });
            }
            cx.invalidate();
        });
        let input_key = cx.key_down_listener(id.clone(), |this, event, cx| {
            if matches!(event.key, Key::Enter | Key::Escape) {
                let finished = if let Some(d) = &mut this.develop {
                    if event.key == Key::Escape {
                        d.numeric_draft = None;
                        true
                    } else {
                        d.finish_numeric_input()
                    }
                } else {
                    true
                };
                if finished {
                    cx.focus(quickgui::FocusHandle::new("workspace"));
                }
                cx.prevent_default();
                cx.invalidate();
            }
            // Preserve native text editing and prevent Develop's Escape/Undo handler.
            cx.stop_propagation();
        });
        let key = cx.key_down_listener(slider_id.clone(), move |this, event, cx| {
            let direction = match event.key {
                Key::ArrowLeft | Key::ArrowDown => -1.,
                Key::ArrowRight | Key::ArrowUp => 1.,
                _ => {
                    cx.propagate();
                    return;
                }
            };
            if let Some(d) = &mut this.develop {
                let value = (field.value(&d.settings) + direction * (range.1 - range.0) / 100.)
                    .clamp(range.0, range.1);
                d.edit(|s| field.set(s, value));
            }
            cx.prevent_default();
            cx.invalidate();
        });
        let slider = div()
            .id(slider_id)
            .focusable()
            .accessibility_label(label.to_owned())
            .h(22.)
            .w(260.)
            .relative()
            .on_pointer(pointer)
            .on_key_down(key)
            .child(
                div()
                    .absolute()
                    .left(6.)
                    .top(9.)
                    .w(248.)
                    .h(4.)
                    .rounded(2.)
                    .bg(Color::rgb8(72, 77, 84)),
            )
            .child(
                div()
                    .absolute()
                    .left(248. * ((value - range.0) / (range.1 - range.0)).clamp(0., 1.))
                    .top(5.)
                    .size(12., 12.)
                    .rounded(6.)
                    .bg(Color::rgb8(70, 148, 235)),
            );
        div()
            .flex_col()
            .gap(2.)
            .flex_shrink_0()
            .child(
                div()
                    .flex_row()
                    .items_center()
                    .child(text(label.to_owned()).text_size(12.).flex_1())
                    .child(
                        Self::text_field(displayed)
                            .id(id)
                            .w(72.)
                            .h(25.)
                            .text_size(12.)
                            .on_input(input)
                            .on_key_down(input_key),
                    ),
            )
            .child(slider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crop_controls_remain_valid_at_both_minimum_extent_limits() {
        for (edge, value) in [(0, 0.99), (1, 0.99), (2, 0.01), (3, 0.01)] {
            let mut settings = DevelopSettings::default();
            Field::Crop(edge).set(&mut settings, value);
            settings.validate().unwrap();
            assert!(settings.crop[2] - settings.crop[0] >= 0.01);
            assert!(settings.crop[3] - settings.crop[1] >= 0.01);
        }
    }
    #[test]
    fn temperature_edit_activates_temperature_balance_and_keeps_tint() {
        let mut settings = DevelopSettings {
            tint: 17.,
            ..Default::default()
        };
        assert_eq!(settings.white_balance, raw::WhiteBalance::AsShot);
        Field::Temperature.set(&mut settings, 5500.);
        assert_eq!(settings.white_balance, raw::WhiteBalance::Temperature);
        assert_eq!(settings.temperature, 5500.);
        assert_eq!(settings.tint, 17.);
    }
}
