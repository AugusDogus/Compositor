//! Native fallback for X-Trans and cameras outside rawler's decoder set.
use super::{DecodedRaw, RawMetadata};
use crate::{Result, document::MAX_SURFACE_PIXELS, invalid};
use image::Rgb32FImage;
use rawler::imgop::matrix::{multiply, pseudo_inverse};
use std::{
    ffi::{CStr, c_char, c_int, c_void},
    ptr,
};

#[repr(C)]
struct Surface {
    image: *mut c_void,
    pixels: *const u16,
    samples: usize,
    width: u32,
    height: u32,
    bits: u32,
    as_shot: [f32; 3],
    camera_to_rgb: [f32; 9],
    xyz_to_camera: [f32; 9],
    iso: f32,
    aperture: f32,
    shutter: f32,
    focal_length: f32,
    camera: [c_char; 256],
    lens: [c_char; 128],
}
impl Drop for Surface {
    fn drop(&mut self) {
        // SAFETY: The C bridge initializes this handle and clears it on failure;
        // this is the sole owner and the matching LibRaw allocator frees it.
        unsafe { compositor_raw_free(self) };
    }
}
unsafe extern "C" {
    fn compositor_raw_decode(
        bytes: *const u8,
        length: usize,
        max_pixels: u64,
        out: *mut Surface,
        error: *mut c_char,
        error_size: usize,
    ) -> c_int;
    fn compositor_raw_free(out: *mut Surface);
}

pub(super) fn decode(bytes: &[u8]) -> Result<DecodedRaw> {
    let mut surface = Surface {
        image: ptr::null_mut(),
        pixels: ptr::null(),
        samples: 0,
        width: 0,
        height: 0,
        bits: 0,
        as_shot: [0.; 3],
        camera_to_rgb: [0.; 9],
        xyz_to_camera: [0.; 9],
        iso: 0.,
        aperture: 0.,
        shutter: 0.,
        focal_length: 0.,
        camera: [0; 256],
        lens: [0; 128],
    };
    let mut error = [0 as c_char; 256];
    // SAFETY: Input and output buffers remain valid for this synchronous call.
    // The C boundary validates all allocation-driving geometry before unpacking.
    let code = unsafe {
        compositor_raw_decode(
            bytes.as_ptr(),
            bytes.len(),
            MAX_SURFACE_PIXELS,
            &mut surface,
            error.as_mut_ptr(),
            error.len(),
        )
    };
    if code != 0 {
        return Err(invalid(string(&error)));
    }
    crate::document::validate_size(surface.width, surface.height)?;
    if surface.pixels.is_null()
        || surface.samples != surface.width as usize * surface.height as usize * 3
    {
        return Err(invalid(
            "LibRaw returned invalid RGB storage. The source file is unchanged.",
        ));
    }
    let camera_to_rgb =
        std::array::from_fn(|row| std::array::from_fn(|col| surface.camera_to_rgb[row * 3 + col]));
    let mut xyz_to_camera =
        std::array::from_fn(|row| std::array::from_fn(|col| surface.xyz_to_camera[row * 3 + col]));
    if xyz_to_camera.iter().flatten().all(|v| *v == 0.) {
        xyz_to_camera = multiply(
            &pseudo_inverse(camera_to_rgb),
            &rawler::imgop::xyz::XYZ_TO_SRGB_D65,
        );
    }
    if !camera_to_rgb
        .iter()
        .flatten()
        .chain(xyz_to_camera.iter().flatten())
        .all(|v| v.is_finite())
        || camera_to_rgb.iter().all(|row| row.iter().all(|v| *v == 0.))
    {
        return Err(invalid(
            "LibRaw has no usable color calibration for this camera. Export a linear TIFF from camera software to import it.",
        ));
    }
    let green = surface.as_shot[1];
    let as_shot = surface.as_shot.map(|value| value / green);
    if !as_shot.iter().all(|v| v.is_finite() && *v > 0.) {
        return Err(invalid(
            "LibRaw could not recover this camera's white balance. Export a TIFF from camera software to import it.",
        ));
    }
    // SAFETY: The C bridge checks the exact allocation size and RGB16 layout;
    // Surface keeps the owning image alive throughout this borrowed slice.
    let pixels = unsafe { std::slice::from_raw_parts(surface.pixels, surface.samples) };
    let camera = Rgb32FImage::from_raw(
        surface.width,
        surface.height,
        pixels.iter().map(|v| f32::from(*v) / 65535.).collect(),
    )
    .ok_or_else(|| invalid("LibRaw's decoded dimensions do not match its RGB data."))?;
    let positive = |v: f32| (v.is_finite() && v > 0.).then_some(v);
    Ok(DecodedRaw {
        camera,
        as_shot,
        camera_to_rgb,
        xyz_to_camera,
        metadata: RawMetadata {
            camera: string(&surface.camera),
            lens: string(&surface.lens),
            iso: positive(surface.iso).map(|v| v.round() as u32),
            aperture: positive(surface.aperture),
            shutter: positive(surface.shutter),
            focal_length: positive(surface.focal_length),
            width: surface.width,
            height: surface.height,
            bits: surface.bits.min(32) as usize,
        },
    })
}
fn string(bytes: &[c_char]) -> String {
    let bytes: Vec<u8> = bytes.iter().map(|v| *v as u8).collect();
    CStr::from_bytes_until_nul(&bytes)
        .map_or_else(|_| String::new(), |s| s.to_string_lossy().trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_inputs_return_errors_without_a_surface() {
        for bytes in [&[][..], b"not a camera file", b"II*\0\xff\xff\xff\xff"] {
            assert!(decode(bytes).is_err());
        }
    }

    #[test]
    #[ignore = "Downloads CC0 camera samples with scripts/fetch-raw-fixtures-extra.sh"]
    fn real_xtrans_remains_linear_and_redevelopable() {
        let path = std::env::var_os("COMPOSITOR_XTRANS_TEST_PHOTO")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "target/fixtures/raw-external/fujifilm-xpro1.raf".into());
        let bytes = std::fs::read(&path).unwrap();
        let raw = super::super::decode(&bytes).unwrap();
        assert!(raw.metadata.camera.to_uppercase().contains("FUJIFILM"));
        assert_eq!(raw.camera.dimensions(), (4952, 3288));
        assert!(
            raw.camera
                .as_raw()
                .iter()
                .all(|v| v.is_finite() && *v >= 0. && *v <= 1.)
        );
        assert!(
            raw.camera
                .as_raw()
                .iter()
                .any(|v| (*v * 255. - (*v * 255.).round()).abs() > 0.1),
            "decoder reduced RAW precision to 8 bits"
        );
        let mut settings = super::super::DevelopSettings::default();
        let normal =
            super::super::render(&raw.preview(256), &settings, &Default::default()).unwrap();
        settings.exposure = 1.;
        let brighter =
            super::super::render(&raw.preview(256), &settings, &Default::default()).unwrap();
        assert_ne!(normal, brighter);
        assert!(
            normal
                .pixels()
                .any(|p| p[0].abs_diff(p[1]) > 10 || p[1].abs_diff(p[2]) > 10)
        );
    }
}
