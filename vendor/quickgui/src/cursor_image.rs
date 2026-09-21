use crate::{Image, image::ImageId};

/// A window-level override for the cursor selected by element hit testing.
#[derive(Clone, Debug)]
pub enum CursorOverride {
    System(crate::CursorStyle),
    Image(CursorImage),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CursorOverrideId {
    System(crate::CursorStyle),
    Image(CursorImageId),
}

impl CursorOverride {
    pub fn id(&self) -> CursorOverrideId {
        match self {
            Self::System(style) => CursorOverrideId::System(*style),
            Self::Image(image) => CursorOverrideId::Image(image.id()),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum NativeCursorAlpha {
    Straight,
    Premultiplied,
}

/// Identity of an immutable cursor image and its physical-pixel hot spot.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CursorImageId {
    image: ImageId,
    hotspot: [u16; 2],
}

/// A validated native cursor bitmap. Pixels and hot spot are in physical pixels.
/// Render at the window's backing scale to keep the logical cursor size constant.
#[derive(Clone, Debug)]
pub struct CursorImage {
    image: Image,
    hotspot: [u16; 2],
}

#[derive(Debug, thiserror::Error)]
pub enum CursorImageError {
    #[error("cursor image is {width} by {height} pixels; each dimension must be at most 2048")]
    TooLarge { width: u32, height: u32 },
    #[error("cursor hot spot ({x}, {y}) lies outside its {width} by {height} image")]
    HotspotOutOfBounds {
        x: u16,
        y: u16,
        width: u32,
        height: u32,
    },
}

impl CursorImage {
    pub fn new(image: Image, hotspot: [u16; 2]) -> Result<Self, CursorImageError> {
        let (width, height) = (image.width(), image.height());
        if width > 2048 || height > 2048 {
            return Err(CursorImageError::TooLarge { width, height });
        }
        let [x, y] = hotspot;
        if u32::from(x) >= width || u32::from(y) >= height {
            return Err(CursorImageError::HotspotOutOfBounds {
                x,
                y,
                width,
                height,
            });
        }
        Ok(Self { image, hotspot })
    }

    pub fn id(&self) -> CursorImageId {
        CursorImageId {
            image: self.image.id(),
            hotspot: self.hotspot,
        }
    }

    pub fn image(&self) -> &Image {
        &self.image
    }
    pub fn hotspot(&self) -> [u16; 2] {
        self.hotspot
    }

    pub(crate) fn native_source(
        &self,
        alpha: NativeCursorAlpha,
    ) -> Result<winit::window::CustomCursorSource, winit::window::BadImage> {
        winit::window::CustomCursor::from_rgba(
            self.native_rgba(alpha),
            self.image.width() as u16,
            self.image.height() as u16,
            self.hotspot[0],
            self.hotspot[1],
        )
    }

    fn native_rgba(&self, alpha: NativeCursorAlpha) -> Vec<u8> {
        let mut pixels = self.image.rgba().to_vec();
        // quickgui-winit 0.1.5 premultiplies Wayland buffers but forwards X11 pixels
        // directly to XcursorImageLoadCursor, whose ARGB pixels must be premultiplied.
        if matches!(alpha, NativeCursorAlpha::Premultiplied) {
            for pixel in pixels.chunks_exact_mut(4) {
                for channel in 0..3 {
                    pixel[channel] =
                        ((u16::from(pixel[channel]) * u16::from(pixel[3]) + 127) / 255) as u8;
                }
            }
        }
        pixels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_images_validate_bounds_and_keep_identity_when_cloned() {
        let image = Image::from_rgba(2, 2, vec![255; 16]).unwrap();
        assert!(matches!(
            CursorImage::new(image.clone(), [2, 0]),
            Err(CursorImageError::HotspotOutOfBounds { .. })
        ));
        let cursor = CursorImage::new(image.clone(), [1, 1]).unwrap();
        assert_eq!(cursor.id(), cursor.clone().id());
        assert_ne!(cursor.id(), CursorImage::new(image, [0, 0]).unwrap().id());
        assert!(cursor.native_source(NativeCursorAlpha::Straight).is_ok());
        let wide = Image::from_rgba(2049, 1, vec![0; 2049 * 4]).unwrap();
        assert!(matches!(
            CursorImage::new(wide, [0, 0]),
            Err(CursorImageError::TooLarge { .. })
        ));
    }

    #[test]
    fn cursor_images_keep_wayland_straight_alpha_and_encode_x11_premultiplied_alpha() {
        let image = Image::from_rgba(1, 1, vec![200, 100, 50, 128]).unwrap();
        let cursor = CursorImage::new(image, [0, 0]).unwrap();
        assert_eq!(
            cursor.native_rgba(NativeCursorAlpha::Straight),
            [200, 100, 50, 128]
        );
        assert_eq!(
            cursor.native_rgba(NativeCursorAlpha::Premultiplied),
            [100, 50, 25, 128]
        );
        assert_eq!(cursor.image().rgba(), [200, 100, 50, 128]);
    }
}
