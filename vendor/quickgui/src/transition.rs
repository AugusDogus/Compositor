use std::{fmt, rc::Rc, time::Duration};

use bitflags::bitflags;

use crate::{MAX_ANIMATION_FPS, ease_in_out};

/// Maximum number of paint-only style transitions retained by one window.
pub const MAX_STYLE_TRANSITIONS_PER_WINDOW: usize = 4_096;

bitflags! {
    /// Paint properties selected by a [`Transition`].
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
    pub struct TransitionProperties: u8 {
        const BACKGROUND = 1 << 0;
        const BORDER_COLOR = 1 << 1;
        const BORDER_WIDTH = 1 << 2;
        const BORDER_RADIUS = 1 << 3;
        const TEXT_COLOR = 1 << 4;
        const BOX_SHADOW = 1 << 5;
        const OPACITY = 1 << 6;
        const TRANSFORM = 1 << 7;

        const COLORS = Self::BACKGROUND.bits()
            | Self::BORDER_COLOR.bits()
            | Self::TEXT_COLOR.bits();
        const ALL = Self::COLORS.bits()
            | Self::BORDER_WIDTH.bits()
            | Self::BORDER_RADIUS.bits()
            | Self::BOX_SHADOW.bits()
            | Self::OPACITY.bits()
            | Self::TRANSFORM.bits();
    }
}

/// A web-like transition for paint-only element style changes.
///
/// The default property set is [`TransitionProperties::ALL`]. Layout values remain explicit
/// declaration-time animations because changing them requires Taffy layout.
#[derive(Clone)]
pub struct Transition {
    pub duration: Duration,
    pub properties: TransitionProperties,
    pub easing: Rc<dyn Fn(f32) -> f32>,
    pub max_fps: Option<f32>,
}

impl Transition {
    pub fn new(duration: Duration) -> Self {
        Self {
            duration,
            properties: TransitionProperties::ALL,
            easing: Rc::new(ease_in_out),
            max_fps: None,
        }
    }

    pub fn colors(duration: Duration) -> Self {
        Self::new(duration).with_properties(TransitionProperties::COLORS)
    }

    pub fn with_properties(mut self, properties: TransitionProperties) -> Self {
        self.properties = properties;
        self
    }

    pub fn with_easing(mut self, easing: impl Fn(f32) -> f32 + 'static) -> Self {
        self.easing = Rc::new(easing);
        self
    }

    /// Limit repaint cadence while this transition is active.
    ///
    /// Non-finite and non-positive values are ignored. Effective timer cadence is capped at
    /// [`MAX_ANIMATION_FPS`](crate::MAX_ANIMATION_FPS).
    pub fn with_max_fps(mut self, max_fps: f32) -> Self {
        self.max_fps = Some(max_fps);
        self
    }

    pub(crate) fn eased(&self, phase: f32) -> f32 {
        let value = (self.easing)(phase);
        if value.is_finite() { value } else { phase }
    }

    pub(crate) fn frame_interval(&self) -> Option<Duration> {
        let fps = self.max_fps?;
        if !fps.is_finite() || fps <= 0.0 {
            return None;
        }
        Some(Duration::from_secs_f64(
            1.0 / f64::from(fps.min(MAX_ANIMATION_FPS)),
        ))
    }

    pub(crate) fn same_configuration(&self, other: &Self) -> bool {
        self.duration == other.duration
            && self.properties == other.properties
            && self.max_fps.map(f32::to_bits) == other.max_fps.map(f32::to_bits)
            && Rc::ptr_eq(&self.easing, &other.easing)
    }
}

impl From<Duration> for Transition {
    fn from(duration: Duration) -> Self {
        Self::new(duration)
    }
}

impl fmt::Debug for Transition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Transition")
            .field("duration", &self.duration)
            .field("properties", &self.properties)
            .field("max_fps", &self.max_fps)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_converts_to_an_all_property_transition() {
        let transition = Transition::from(Duration::from_millis(150));
        assert_eq!(transition.duration, Duration::from_millis(150));
        assert_eq!(transition.properties, TransitionProperties::ALL);
        assert!(
            transition
                .properties
                .contains(TransitionProperties::OPACITY)
        );
        assert!(!TransitionProperties::COLORS.contains(TransitionProperties::OPACITY));
    }

    #[test]
    fn invalid_fps_is_unthrottled_and_large_fps_is_bounded() {
        assert_eq!(
            Transition::new(Duration::from_secs(1))
                .with_max_fps(f32::NAN)
                .frame_interval(),
            None
        );
        assert_eq!(
            Transition::new(Duration::from_secs(1))
                .with_max_fps(10_000.0)
                .frame_interval(),
            Some(Duration::from_secs_f64(1.0 / f64::from(MAX_ANIMATION_FPS)))
        );
    }
}
