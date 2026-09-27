use crate::{
    Result,
    bevel::{Lighting, Style},
    effects::LayerEffects,
};

pub(super) fn encode(
    parameters: &mut [u32; super::PARAMETER_WORDS],
    effects: &LayerEffects,
) -> Result<()> {
    let Some(settings) = &effects.bevel else {
        return Ok(());
    };
    let lighting = Lighting::new(settings)?;
    let start = super::gradient::PARAMETER_WORDS;
    for (index, value) in lighting
        .light
        .into_iter()
        .chain([lighting.slope_scale])
        .enumerate()
    {
        parameters[start + index] = value.to_bits();
    }
    let style: f32 = if settings.enabled {
        match settings.style {
            Style::Inner => 1.,
            Style::Outer => 2.,
            Style::Emboss => 3.,
        }
    } else {
        0.
    };
    parameters[start + 4] = style.to_bits();
    parameters[start + 5] = (settings.highlight_opacity as f32).to_bits();
    parameters[start + 6] = (settings.shadow_opacity as f32).to_bits();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        bevel::Settings,
        effects::{InnerGlowEffect, OuterGlowEffect, ShadowEffect, StrokeEffect},
        pattern,
    };
    use image::{Rgba, RgbaImage};

    #[test]
    #[ignore = "Requires a hardware Vulkan adapter"]
    fn bevel_gpu_matches_cpu_for_styles_endpoints_and_effect_combinations() {
        let mut engine = super::super::super::Engine::new().unwrap();
        for style in [Style::Inner, Style::Outer, Style::Emboss] {
            for (size, altitude, combined) in [
                (0., 30., false),
                (1e-30, 0., false),
                (0.02, 90., false),
                (3.5, 0., false),
                (3.5, 90., false),
                (3.5, 30., true),
            ] {
                let mut effects = LayerEffects {
                    bevel: Some(Box::new(Settings {
                        style,
                        size,
                        altitude,
                        angle: -145.,
                        depth: 183.,
                        ..Default::default()
                    })),
                    ..Default::default()
                };
                if combined {
                    effects.pattern_overlay = Some(Box::new(pattern::Overlay::new(
                        pattern::Pattern::from_pixels(
                            "Tile",
                            RgbaImage::from_pixel(2, 2, Rgba([91, 41, 210, 183])),
                        )
                        .unwrap(),
                    )));
                    effects.gradient_overlay = Some(Box::default());
                    effects.inner_glow = Some(InnerGlowEffect {
                        size: 2.3,
                        ..Default::default()
                    });
                    effects.outer_glow = Some(OuterGlowEffect {
                        size: 1.7,
                        ..Default::default()
                    });
                    effects.shadow = Some(ShadowEffect {
                        distance: 2.3,
                        blur: 1.2,
                        ..Default::default()
                    });
                    effects.inner_shadow = Some(ShadowEffect {
                        distance: 1.7,
                        blur: 0.7,
                        ..Default::default()
                    });
                    effects.stroke = Some(StrokeEffect {
                        size: 1.,
                        inside: true,
                        ..Default::default()
                    });
                }
                let inset = effects.margin();
                let image = RgbaImage::from_fn(11 + inset * 2, 9 + inset * 2, |x, y| {
                    if x >= inset && y >= inset && x < inset + 11 && y < inset + 9 {
                        Rgba([
                            73,
                            91,
                            121,
                            ((x - inset) * 19 + (y - inset) * 7).min(255) as u8,
                        ])
                    } else {
                        Rgba([0; 4])
                    }
                });
                let gpu = engine.effects(&image, &effects).unwrap().unwrap();
                let cpu = crate::effects::cpu::render(&image, &effects).unwrap();
                for (index, (a, b)) in gpu.as_raw().iter().zip(cpu.as_raw()).enumerate() {
                    assert!(
                        a.abs_diff(*b) <= 1,
                        "style={style:?},size={size},altitude={altitude},combined={combined},byte={index}: gpu={a},cpu={b}"
                    );
                }
            }
        }
    }
}
