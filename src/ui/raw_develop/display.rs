//! Tile full sensor previews to respect QuickGUI's per-image allocation limit.
use super::*;
use quickgui::Rect;

#[derive(Clone)]
pub(super) struct DisplayImage {
    width: u32,
    height: u32,
    tiles: Arc<Vec<Tile>>,
}
struct Tile {
    x: u32,
    y: u32,
    image: Image,
}
impl DisplayImage {
    pub(super) fn new(pixels: RgbaImage) -> Result<Self> {
        let (width, height) = pixels.dimensions();
        compositor::document::validate_size(width, height)?;
        let mut tiles = Vec::new();
        for y in (0..height).step_by(2048) {
            for x in (0..width).step_by(2048) {
                let tile = image::imageops::crop_imm(
                    &pixels,
                    x,
                    y,
                    (width - x).min(2048),
                    (height - y).min(2048),
                )
                .to_image();
                let image = Image::from_rgba(tile.width(), tile.height(), tile.into_raw())
                    .map_err(|e| {
                        invalid(format!(
                            "Could not display a RAW preview tile: {e}. Try Fit preview."
                        ))
                    })?;
                tiles.push(Tile { x, y, image });
            }
        }
        Ok(Self {
            width,
            height,
            tiles: Arc::new(tiles),
        })
    }
    pub(super) fn width(&self) -> u32 {
        self.width
    }
    pub(super) fn height(&self) -> u32 {
        self.height
    }
    pub(super) fn element(&self, rect: Rect) -> Element {
        let scale = [
            rect.width / self.width as f32,
            rect.height / self.height as f32,
        ];
        let mut container = div()
            .absolute()
            .left(rect.x)
            .top(rect.y)
            .size(rect.width, rect.height);
        for tile in self.tiles.iter() {
            container = container.child(
                quickgui::img(&tile.image)
                    .absolute()
                    .left(tile.x as f32 * scale[0])
                    .top(tile.y as f32 * scale[1])
                    .size(
                        tile.image.width() as f32 * scale[0],
                        tile.image.height() as f32 * scale[1],
                    ),
            );
        }
        container
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sensor_sized_preview_tiles_preserve_exact_pixels_and_dimensions() {
        let pixels = RgbaImage::from_fn(6000, 20, |x, y| {
            image::Rgba([(x % 255) as u8, y as u8, 75, 255])
        });
        let tiled = DisplayImage::new(pixels.clone()).unwrap();
        assert_eq!((tiled.width(), tiled.height()), (6000, 20));
        assert_eq!(tiled.tiles.len(), 3);
        for tile in tiled.tiles.iter() {
            for y in 0..tile.image.height() {
                for x in 0..tile.image.width() {
                    let offset = ((y * tile.image.width() + x) * 4) as usize;
                    assert_eq!(
                        &tile.image.rgba()[offset..offset + 4],
                        pixels.get_pixel(tile.x + x, tile.y + y).0
                    );
                }
            }
        }
    }
}
