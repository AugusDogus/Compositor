use std::{
    collections::HashSet,
    fmt,
    fs::File,
    io::{BufReader, Cursor},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use web_time::Duration;

use image_codecs::{
    AnimationDecoder as _, ImageDecoder as _, ImageFormat, Limits, Rgba,
    codecs::{gif::GifDecoder, webp::WebPDecoder},
    metadata::LoopCount,
};

use crate::{
    Image, ImageError, Size,
    image::{MAX_DECODED_IMAGE_BYTES, MAX_ENCODED_IMAGE_BYTES, MAX_IMAGE_DIMENSION},
};

/// Maximum frames retained by one animated image.
pub const MAX_ANIMATION_FRAMES: usize = 256;
/// Maximum decoded RGBA bytes retained by one animated image.
pub const MAX_ANIMATED_IMAGE_BYTES: u64 = MAX_DECODED_IMAGE_BYTES;
/// Fast frame delays are clamped to one 60 Hz display interval.
pub const MIN_ANIMATION_FRAME_DURATION: Duration = Duration::from_micros(16_667);

static NEXT_ANIMATED_IMAGE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AnimationRepeat {
    #[default]
    Infinite,
    Finite(u32),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnimatedImageFrame {
    image: Image,
    duration: Duration,
}

impl AnimatedImageFrame {
    pub fn new(image: Image, duration: Duration) -> Self {
        Self { image, duration }
    }

    pub fn image(&self) -> &Image {
        &self.image
    }

    pub fn duration(&self) -> Duration {
        self.duration
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct AnimatedImageId(u64);

#[derive(Clone)]
pub struct AnimatedImage(Arc<AnimatedImageData>);

struct AnimatedImageData {
    id: AnimatedImageId,
    frames: Arc<[AnimatedImageFrame]>,
    frame_ends: Arc<[Duration]>,
    total_duration: Duration,
    repeat: AnimationRepeat,
    byte_len: u64,
}

impl AnimatedImage {
    /// Create an infinitely repeating animated image from decoded frames.
    pub fn new(frames: impl IntoIterator<Item = AnimatedImageFrame>) -> Result<Self, ImageError> {
        Self::with_repeat(frames, AnimationRepeat::Infinite)
    }

    /// Create an animated image with an explicit repeat policy.
    pub fn with_repeat(
        frames: impl IntoIterator<Item = AnimatedImageFrame>,
        repeat: AnimationRepeat,
    ) -> Result<Self, ImageError> {
        if repeat == AnimationRepeat::Finite(0) {
            return Err(ImageError::ZeroAnimationIterations);
        }
        let mut frames = frames.into_iter().collect::<Vec<_>>();
        if frames.is_empty() {
            return Err(ImageError::EmptyAnimation);
        }
        if frames.len() > MAX_ANIMATION_FRAMES {
            return Err(ImageError::TooManyAnimationFrames {
                frames: frames.len(),
                maximum: MAX_ANIMATION_FRAMES,
            });
        }

        let mut frame_ends = Vec::with_capacity(frames.len());
        let mut total_duration = Duration::ZERO;
        let mut byte_len = 0_u64;
        let mut image_ids = HashSet::with_capacity(frames.len());
        for frame in &mut frames {
            frame.duration = frame.duration.max(MIN_ANIMATION_FRAME_DURATION);
            total_duration = total_duration.saturating_add(frame.duration);
            if image_ids.insert(frame.image.id()) {
                byte_len = byte_len.saturating_add(frame.image.byte_len() as u64);
            }
            if byte_len > MAX_ANIMATED_IMAGE_BYTES {
                return Err(ImageError::AnimationTooLarge {
                    bytes: byte_len,
                    maximum: MAX_ANIMATED_IMAGE_BYTES,
                });
            }
            frame_ends.push(total_duration);
        }

        Ok(Self(Arc::new(AnimatedImageData {
            id: AnimatedImageId(NEXT_ANIMATED_IMAGE_ID.fetch_add(1, Ordering::Relaxed)),
            frames: frames.into(),
            frame_ends: frame_ends.into(),
            total_duration,
            repeat,
            byte_len,
        })))
    }

    /// Decode an animated GIF or WebP from memory.
    pub fn decode(encoded: impl AsRef<[u8]>) -> Result<Self, ImageError> {
        match ImageAsset::decode(encoded.as_ref())? {
            ImageAsset::Animated(animation) => Ok(animation),
            ImageAsset::Static(_) => Err(ImageError::NotAnimated),
        }
    }

    /// Decode an animated GIF or WebP file with encoded and decoded size limits.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ImageError> {
        match ImageAsset::open(path.as_ref())? {
            ImageAsset::Animated(animation) => Ok(animation),
            ImageAsset::Static(_) => Err(ImageError::NotAnimated),
        }
    }

    pub fn frame_count(&self) -> usize {
        self.0.frames.len()
    }

    pub fn frame(&self, index: usize) -> Option<&AnimatedImageFrame> {
        self.0.frames.get(index)
    }

    pub fn size(&self) -> Size {
        self.0.frames[0].image.size()
    }

    pub fn repeat(&self) -> AnimationRepeat {
        self.0.repeat
    }

    pub fn byte_len(&self) -> u64 {
        self.0.byte_len
    }

    pub fn total_duration(&self) -> Duration {
        self.0.total_duration
    }

    pub(crate) fn id(&self) -> AnimatedImageId {
        self.0.id
    }

    pub(crate) fn frame_index_at(&self, elapsed: Duration) -> (usize, bool) {
        let total_nanos = self.total_duration().as_nanos();
        debug_assert!(total_nanos > 0);
        let elapsed_nanos = elapsed.as_nanos();
        if let AnimationRepeat::Finite(iterations) = self.repeat()
            && elapsed_nanos >= total_nanos.saturating_mul(u128::from(iterations))
        {
            return (self.frame_count() - 1, true);
        }
        let phase = duration_from_nanos(elapsed_nanos % total_nanos);
        let index = self.0.frame_ends.partition_point(|end| *end <= phase);
        (index.min(self.frame_count() - 1), false)
    }

    pub(crate) fn remaining_in_frame(&self, elapsed: Duration, index: usize) -> Duration {
        let total_nanos = self.total_duration().as_nanos();
        let phase_nanos = elapsed.as_nanos() % total_nanos;
        let end_nanos = self.0.frame_ends[index].as_nanos();
        duration_from_nanos(end_nanos.saturating_sub(phase_nanos))
    }
}

impl fmt::Debug for AnimatedImage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AnimatedImage")
            .field("frames", &self.frame_count())
            .field("bytes", &self.byte_len())
            .field("duration", &self.total_duration())
            .field("repeat", &self.repeat())
            .finish()
    }
}

impl PartialEq for AnimatedImage {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

impl Eq for AnimatedImage {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ImageAsset {
    Static(Image),
    Animated(AnimatedImage),
}

impl ImageAsset {
    pub(crate) fn decode(encoded: &[u8]) -> Result<Self, ImageError> {
        if encoded.len() as u64 > MAX_ENCODED_IMAGE_BYTES {
            return Err(ImageError::EncodedTooLarge {
                bytes: encoded.len() as u64,
                maximum: MAX_ENCODED_IMAGE_BYTES,
            });
        }
        match image_codecs::guess_format(encoded).map_err(ImageError::Decode)? {
            ImageFormat::Gif => decode_gif(Cursor::new(encoded)).map(Self::Animated),
            ImageFormat::WebP => decode_webp(Cursor::new(encoded)),
            _ => Image::decode(encoded).map(Self::Static),
        }
    }

    pub(crate) fn open(path: &Path) -> Result<Self, ImageError> {
        let metadata = std::fs::metadata(path).map_err(|source| ImageError::Open {
            path: path.to_path_buf(),
            source,
        })?;
        if metadata.len() > MAX_ENCODED_IMAGE_BYTES {
            return Err(ImageError::EncodedTooLarge {
                bytes: metadata.len(),
                maximum: MAX_ENCODED_IMAGE_BYTES,
            });
        }

        let reader = image_codecs::ImageReader::open(path)
            .map_err(|source| ImageError::Open {
                path: path.to_path_buf(),
                source,
            })?
            .with_guessed_format()
            .map_err(ImageError::Inspect)?;
        match reader.format() {
            Some(ImageFormat::Gif) => open_file(path).and_then(decode_gif).map(Self::Animated),
            Some(ImageFormat::WebP) => open_file(path).and_then(decode_webp),
            _ => Image::open(path).map(Self::Static),
        }
    }

    pub(crate) fn byte_len(&self) -> u64 {
        match self {
            Self::Static(image) => image.byte_len() as u64,
            Self::Animated(animation) => animation.byte_len(),
        }
    }
}

fn open_file(path: &Path) -> Result<BufReader<File>, ImageError> {
    File::open(path)
        .map(BufReader::new)
        .map_err(|source| ImageError::Open {
            path: path.to_path_buf(),
            source,
        })
}

fn decode_gif<R: std::io::BufRead + std::io::Seek>(reader: R) -> Result<AnimatedImage, ImageError> {
    let mut decoder = GifDecoder::new(reader).map_err(ImageError::Decode)?;
    decoder
        .set_limits(animation_limits())
        .map_err(ImageError::Decode)?;
    let repeat = map_repeat(decoder.loop_count());
    decode_frames(decoder.into_frames(), repeat)
}

fn decode_webp<R: std::io::BufRead + std::io::Seek>(reader: R) -> Result<ImageAsset, ImageError> {
    let mut decoder = WebPDecoder::new(reader).map_err(ImageError::Decode)?;
    decoder
        .set_limits(animation_limits())
        .map_err(ImageError::Decode)?;
    if !decoder.has_animation() {
        let dynamic = image_codecs::DynamicImage::from_decoder(decoder)
            .map_err(ImageError::Decode)?
            .into_rgba8();
        return Image::from_rgba(dynamic.width(), dynamic.height(), dynamic.into_raw())
            .map(ImageAsset::Static);
    }
    decoder
        .set_background_color(Rgba([0, 0, 0, 0]))
        .map_err(ImageError::Decode)?;
    let repeat = map_repeat(decoder.loop_count());
    decode_frames(decoder.into_frames(), repeat).map(ImageAsset::Animated)
}

fn decode_frames(
    frames: impl Iterator<Item = Result<image_codecs::Frame, image_codecs::ImageError>>,
    repeat: AnimationRepeat,
) -> Result<AnimatedImage, ImageError> {
    let mut decoded = Vec::new();
    let mut last_error = None;
    for (index, frame) in frames.enumerate() {
        if index >= MAX_ANIMATION_FRAMES {
            return Err(ImageError::TooManyAnimationFrames {
                frames: index + 1,
                maximum: MAX_ANIMATION_FRAMES,
            });
        }
        let frame = match frame {
            Ok(frame) => frame,
            Err(error) => {
                tracing::debug!(%error, index, "skipping an undecodable animation frame");
                last_error = Some(error);
                continue;
            }
        };
        let duration = frame_delay(frame.delay());
        let buffer = frame.into_buffer();
        let image = Image::from_rgba(buffer.width(), buffer.height(), buffer.into_raw())?;
        decoded.push(AnimatedImageFrame::new(image, duration));
    }
    if decoded.is_empty() {
        return match last_error {
            Some(error) => Err(ImageError::Decode(error)),
            None => Err(ImageError::EmptyAnimation),
        };
    }
    AnimatedImage::with_repeat(decoded, repeat)
}

fn animation_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_DECODED_IMAGE_BYTES);
    limits
}

fn map_repeat(repeat: LoopCount) -> AnimationRepeat {
    match repeat {
        LoopCount::Infinite => AnimationRepeat::Infinite,
        LoopCount::Finite(iterations) => AnimationRepeat::Finite(iterations.get()),
    }
}

fn frame_delay(delay: image_codecs::Delay) -> Duration {
    let (numerator, denominator) = delay.numer_denom_ms();
    let denominator = u64::from(denominator.max(1));
    let microseconds = u64::from(numerator)
        .saturating_mul(1_000)
        .saturating_add(denominator - 1)
        / denominator;
    Duration::from_micros(microseconds).max(MIN_ANIMATION_FRAME_DURATION)
}

fn duration_from_nanos(nanoseconds: u128) -> Duration {
    let seconds = (nanoseconds / 1_000_000_000).min(u128::from(u64::MAX)) as u64;
    let subsecond = (nanoseconds % 1_000_000_000) as u32;
    Duration::new(seconds, subsecond)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image_codecs::{
        Delay, ExtendedColorType, Frame as CodecFrame, ImageBuffer,
        codecs::{
            gif::{GifEncoder, Repeat},
            webp::WebPEncoder,
        },
    };

    fn frame(color: u8, duration: Duration) -> AnimatedImageFrame {
        AnimatedImageFrame::new(
            Image::from_rgba(1, 1, vec![color, 0, 0, 255]).unwrap(),
            duration,
        )
    }

    #[test]
    fn animation_requires_a_bounded_nonempty_frame_set() {
        assert!(matches!(
            AnimatedImage::new(Vec::new()).unwrap_err(),
            ImageError::EmptyAnimation
        ));
        let frames = (0..=MAX_ANIMATION_FRAMES)
            .map(|_| frame(0, Duration::from_millis(20)))
            .collect::<Vec<_>>();
        assert!(matches!(
            AnimatedImage::new(frames).unwrap_err(),
            ImageError::TooManyAnimationFrames { .. }
        ));
        assert!(matches!(
            AnimatedImage::with_repeat(
                [frame(0, Duration::from_millis(20))],
                AnimationRepeat::Finite(0),
            )
            .unwrap_err(),
            ImageError::ZeroAnimationIterations
        ));
    }

    #[test]
    fn short_delays_are_clamped_and_frame_lookup_wraps() {
        let animation = AnimatedImage::new([
            frame(1, Duration::ZERO),
            frame(2, Duration::from_millis(30)),
        ])
        .unwrap();
        assert_eq!(
            animation.frame(0).unwrap().duration(),
            MIN_ANIMATION_FRAME_DURATION
        );
        assert_eq!(animation.frame_index_at(Duration::ZERO), (0, false));
        assert_eq!(
            animation.frame_index_at(MIN_ANIMATION_FRAME_DURATION),
            (1, false)
        );
        assert_eq!(
            animation.frame_index_at(animation.total_duration()),
            (0, false)
        );
    }

    #[test]
    fn finite_animation_stops_on_its_last_frame() {
        let animation = AnimatedImage::with_repeat(
            [
                frame(1, Duration::from_millis(20)),
                frame(2, Duration::from_millis(20)),
            ],
            AnimationRepeat::Finite(2),
        )
        .unwrap();
        assert_eq!(
            animation.frame_index_at(Duration::from_millis(80)),
            (1, true)
        );
    }

    #[test]
    fn gif_decoding_preserves_frames_delays_repeat_and_path_loading() {
        let mut encoded = Vec::new();
        {
            let mut encoder = GifEncoder::new(&mut encoded);
            encoder.set_repeat(Repeat::Finite(2)).unwrap();
            encoder
                .encode_frame(CodecFrame::from_parts(
                    ImageBuffer::from_pixel(1, 1, Rgba([10, 20, 30, 255])),
                    0,
                    0,
                    Delay::from_numer_denom_ms(40, 1),
                ))
                .unwrap();
            encoder
                .encode_frame(CodecFrame::from_parts(
                    ImageBuffer::from_pixel(1, 1, Rgba([90, 80, 70, 255])),
                    0,
                    0,
                    Delay::from_numer_denom_ms(60, 1),
                ))
                .unwrap();
        }

        let animation = AnimatedImage::decode(&encoded).unwrap();
        assert_eq!(animation.frame_count(), 2);
        assert_eq!(
            animation.frame(0).unwrap().duration(),
            Duration::from_millis(40)
        );
        assert_eq!(
            animation.frame(1).unwrap().duration(),
            Duration::from_millis(60)
        );
        assert_eq!(animation.repeat(), AnimationRepeat::Finite(2));

        let path = std::env::temp_dir().join(format!(
            "quickgui-animation-{}-{}.gif",
            std::process::id(),
            NEXT_ANIMATED_IMAGE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, encoded).unwrap();
        let from_path = AnimatedImage::open(&path).unwrap();
        let _ = std::fs::remove_file(path);
        assert_eq!(from_path.frame_count(), 2);
    }

    #[test]
    fn animated_webp_decoding_preserves_frames_delays_and_repeat() {
        let encoded = animated_webp([
            ([255, 0, 0, 255], Duration::from_millis(40)),
            ([0, 0, 255, 255], Duration::from_millis(60)),
        ]);

        let animation = AnimatedImage::decode(encoded).unwrap();
        assert_eq!(animation.frame_count(), 2);
        assert_eq!(animation.size(), Size::new(2.0, 2.0));
        assert_eq!(
            animation.frame(0).unwrap().duration(),
            Duration::from_millis(40)
        );
        assert_eq!(
            animation.frame(1).unwrap().duration(),
            Duration::from_millis(60)
        );
        assert_eq!(animation.repeat(), AnimationRepeat::Finite(2));
    }

    fn animated_webp(frames: [([u8; 4], Duration); 2]) -> Vec<u8> {
        let mut body = Vec::new();
        write_webp_chunk(&mut body, b"VP8X", &[0x12, 0, 0, 0, 1, 0, 0, 1, 0, 0]);
        write_webp_chunk(&mut body, b"ANIM", &[0, 0, 0, 0, 2, 0]);

        for (color, duration) in frames {
            let mut pixels = Vec::with_capacity(16);
            for _ in 0..4 {
                pixels.extend_from_slice(&color);
            }
            let mut still = Vec::new();
            WebPEncoder::new_lossless(&mut still)
                .encode(&pixels, 2, 2, ExtendedColorType::Rgba8)
                .unwrap();
            assert_eq!(&still[..4], b"RIFF");
            assert_eq!(&still[8..12], b"WEBP");

            let mut frame = vec![0; 6];
            frame.extend_from_slice(&[1, 0, 0, 1, 0, 0]);
            let millis = u32::try_from(duration.as_millis()).unwrap();
            frame.extend_from_slice(&millis.to_le_bytes()[..3]);
            frame.push(0);
            frame.extend_from_slice(&still[12..]);
            write_webp_chunk(&mut body, b"ANMF", &frame);
        }

        let mut encoded = b"RIFF".to_vec();
        encoded.extend_from_slice(&u32::try_from(body.len() + 4).unwrap().to_le_bytes());
        encoded.extend_from_slice(b"WEBP");
        encoded.extend_from_slice(&body);
        encoded
    }

    fn write_webp_chunk(output: &mut Vec<u8>, kind: &[u8; 4], payload: &[u8]) {
        output.extend_from_slice(kind);
        output.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_le_bytes());
        output.extend_from_slice(payload);
        if !payload.len().is_multiple_of(2) {
            output.push(0);
        }
    }
}
