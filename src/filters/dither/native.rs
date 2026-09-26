use super::{Colors, Settings, Style, glyphs::Glyphs};
use crate::{Result, invalid};
use image::RgbaImage;

/// Matches DitherPixels.h. All indices, ranges and allocations are checked in the safe caller.
#[repr(C)]
struct Params {
    style: i32,
    levels: i32,
    diffusion: f32,
    density: f32,
    contrast: f32,
    cell: i32,
    angle: f32,
    light_on_dark: i32,
    original_colors: i32,
    dark: [u8; 3],
    light: [u8; 3],
    glyph_width: i32,
    glyph_height: i32,
    glyphs: *const u8,
    glyph_coverage: *const f32,
    glyph_count: i32,
}
unsafe extern "C" {
    fn dither_apply(
        rgba: *mut u8,
        width: usize,
        height: usize,
        stride: usize,
        params: *const Params,
    ) -> i32;
    fn dither_dots(
        rgba: *mut u8,
        width: usize,
        height: usize,
        stride: usize,
        block: i32,
        gap: *const u8,
    );
}
pub(super) fn apply(image: &mut RgbaImage, settings: Settings) -> Result<()> {
    let glyphs = if settings.style == Style::Ascii {
        Glyphs::render(settings.characters, settings.text_size)
    } else {
        Glyphs::empty()
    };
    let params = Params {
        style: settings.style as i32,
        levels: i32::from(settings.levels),
        diffusion: settings.diffusion / 100.,
        density: settings.density / 100.,
        contrast: settings.contrast / 100.,
        cell: i32::from(settings.cell_size),
        angle: settings.angle.to_radians(),
        light_on_dark: i32::from(settings.light_on_dark),
        original_colors: i32::from(settings.colors == Colors::Original),
        dark: if settings.colors == Colors::TwoColors {
            settings.dark
        } else {
            [0; 3]
        },
        light: if settings.colors == Colors::TwoColors {
            settings.light
        } else {
            [255; 3]
        },
        glyph_width: glyphs.width,
        glyph_height: glyphs.height,
        glyphs: glyphs.maps.as_ptr(),
        glyph_coverage: glyphs.coverage.as_ptr(),
        glyph_count: glyphs.coverage.len() as i32,
    };
    // SAFETY: The bounded settings and dimensions are validated by dither::apply. Buffers are
    // tightly packed, and the owned glyph maps/coverage remain alive throughout the synchronous call.
    let ok = unsafe {
        dither_apply(
            image.as_mut_ptr(),
            image.width() as usize,
            image.height() as usize,
            image.width() as usize * 4,
            &params,
        )
    };
    if ok == 0 {
        return Err(invalid(
            "Dither ran out of memory. Your layer is unchanged. Increase Pixel Size or resize the layer before trying again.",
        ));
    }
    Ok(())
}
pub(super) fn dots(image: &mut RgbaImage, settings: Settings) {
    let gap = if settings.colors == Colors::TwoColors {
        settings.dark
    } else {
        [0; 3]
    };
    // SAFETY: The image is tightly packed RGBA and the validated block is 1..=32; gap has 3 bytes.
    unsafe {
        dither_dots(
            image.as_mut_ptr(),
            image.width() as usize,
            image.height() as usize,
            image.width() as usize * 4,
            i32::from(settings.pixel_size),
            gap.as_ptr(),
        );
    }
}
