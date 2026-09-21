//! Safe, dimension-checked access to the original application's portable pixel kernels.
use crate::{Result, document::validate_size, invalid};
use image::{GrayImage, RgbaImage};

unsafe extern "C" {
    fn wand_mask(
        rgba: *const u8,
        width: usize,
        height: usize,
        stride: usize,
        seed_x: usize,
        seed_y: usize,
        radius: usize,
        tolerance: i32,
        contiguous: i32,
        mask: *mut u8,
    ) -> std::ffi::c_long;
    fn content_fill(
        rgba: *mut u8,
        stride: usize,
        mask: *const u8,
        mask_stride: usize,
        width: i32,
        height: i32,
    ) -> i32;
    fn spot_heal(
        rgba: *mut u8,
        coverage: *const u8,
        width: usize,
        height: usize,
        stride: usize,
        opacity: f32,
        mode: i32,
        seed: u32,
    ) -> i32;
    fn noise_add(
        rgba: *mut u8,
        width: usize,
        height: usize,
        stride: usize,
        amount: f32,
        gaussian: i32,
        monochromatic: i32,
        seed: u32,
    );
    fn lens_distort(
        source: *const u8,
        destination: *mut u8,
        width: usize,
        height: usize,
        stride: usize,
        k: f64,
    );
}

pub(crate) fn premultiply(image: &RgbaImage) -> RgbaImage {
    let mut result = image.clone();
    for p in result.pixels_mut() {
        for i in 0..3 {
            p[i] = ((u16::from(p[i]) * u16::from(p[3]) + 127) / 255) as u8;
        }
    }
    result
}
pub(crate) fn unpremultiply(mut image: RgbaImage) -> RgbaImage {
    for p in image.pixels_mut() {
        if p[3] > 0 {
            for i in 0..3 {
                p[i] = ((u16::from(p[i]) * 255 + u16::from(p[3]) / 2) / u16::from(p[3])).min(255)
                    as u8;
            }
        }
    }
    image
}

pub(crate) fn fill(image: &RgbaImage, mask: &GrayImage) -> Result<RgbaImage> {
    validate_size(image.width(), image.height())?;
    if image.dimensions() != mask.dimensions() {
        return Err(invalid(
            "Content-aware fill coverage must match the image dimensions.",
        ));
    }
    let (w, h) = image.dimensions();
    let mut pixels = premultiply(image);
    // SAFETY: ImageBuffer owns tightly packed RGBA and grayscale buffers of the checked equal dimensions.
    // The 100-million-pixel cap keeps every C index and width/height conversion within i32.
    let result = unsafe {
        content_fill(
            pixels.as_mut_ptr(),
            w as usize * 4,
            mask.as_ptr(),
            w as usize,
            w as i32,
            h as i32,
        )
    };
    match result {
        1 => Ok(unpremultiply(pixels)),
        0 => Err(invalid(
            "No unselected opaque source patch is available. Select a smaller area with nearby texture.",
        )),
        _ => Err(invalid(
            "Content-aware fill ran out of memory. The current layer is preserved; try a smaller selection.",
        )),
    }
}

pub(crate) fn heal(
    image: &RgbaImage,
    mask: &GrayImage,
    mode: i32,
    opacity: f32,
) -> Result<RgbaImage> {
    validate_size(image.width(), image.height())?;
    if image.dimensions() != mask.dimensions()
        || !(0..=2).contains(&mode)
        || !(0. ..=1.).contains(&opacity)
    {
        return Err(invalid("Healing coverage, mode, or opacity is invalid."));
    }
    let (w, h) = image.dimensions();
    let mut pixels = premultiply(image);
    // SAFETY: Both buffers remain alive for the synchronous call and cover every index the C kernel uses.
    let result = unsafe {
        spot_heal(
            pixels.as_mut_ptr(),
            mask.as_ptr(),
            w as usize,
            h as usize,
            w as usize * 4,
            opacity,
            mode,
            0,
        )
    };
    if result == 0 {
        Ok(unpremultiply(pixels))
    } else {
        Err(invalid(
            "Spot healing ran out of memory. The original pixels are preserved.",
        ))
    }
}

pub(crate) fn noise(
    image: &RgbaImage,
    amount: f32,
    gaussian: bool,
    monochromatic: bool,
    seed: u32,
) -> Result<RgbaImage> {
    validate_size(image.width(), image.height())?;
    if !(0.1..=400.).contains(&amount) {
        return Err(invalid("Noise amount must be between 0.1 and 400%."));
    }
    let (w, h) = image.dimensions();
    let mut pixels = premultiply(image);
    // SAFETY: The mutable RGBA buffer has exactly height rows of width*4 bytes, with no aliasing.
    unsafe {
        noise_add(
            pixels.as_mut_ptr(),
            w as usize,
            h as usize,
            w as usize * 4,
            amount,
            i32::from(gaussian),
            i32::from(monochromatic),
            seed,
        );
    }
    Ok(unpremultiply(pixels))
}

pub(crate) fn lens(image: &RgbaImage, distortion: f64) -> Result<RgbaImage> {
    validate_size(image.width(), image.height())?;
    if !(-100. ..=100.).contains(&distortion) {
        return Err(invalid("Lens distortion must be between -100 and 100."));
    }
    let (w, h) = image.dimensions();
    let source = premultiply(image);
    let mut target = RgbaImage::new(w, h);
    // SAFETY: Source and target are distinct, equally sized packed buffers valid for the call.
    unsafe {
        lens_distort(
            source.as_ptr(),
            target.as_mut_ptr(),
            w as usize,
            h as usize,
            w as usize * 4,
            distortion / 100. * 0.35,
        );
    }
    Ok(unpremultiply(target))
}

pub(crate) fn wand(
    image: &RgbaImage,
    x: u32,
    y: u32,
    tolerance: u8,
    contiguous: bool,
    radius: usize,
) -> Result<GrayImage> {
    validate_size(image.width(), image.height())?;
    if radius > 2 {
        return Err(invalid(
            "Wand sampling supports point, 3 by 3, or 5 by 5 averages.",
        ));
    }
    let pixels = premultiply(image);
    let mut mask = GrayImage::new(image.width(), image.height());
    // SAFETY: Both tightly packed buffers have checked dimensions. The kernel checks the seed
    // before reading it and writes at most width * height coverage bytes.
    let count = unsafe {
        wand_mask(
            pixels.as_ptr(),
            image.width() as usize,
            image.height() as usize,
            image.width() as usize * 4,
            x as usize,
            y as usize,
            radius,
            i32::from(tolerance),
            i32::from(contiguous),
            mask.as_mut_ptr(),
        )
    };
    if count < 0 {
        return Err(invalid(
            "The magic wand could not allocate its selection. Try a smaller image.",
        ));
    }
    Ok(mask)
}
