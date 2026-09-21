use std::{fmt, ops::RangeInclusive, rc::Rc, sync::Arc, time::Duration};

use crate::{
    Color, Element, ElementId, IntoElement, Point, Rect, Size, SpringAnimation, SpringTarget,
    Vector, spring::ElementSpring,
};

/// Maximum number of time-based stages retained by one animated element.
pub const MAX_ANIMATION_STAGES: usize = 64;

/// Maximum independently keyed declarative animations retained by one window.
pub const MAX_DECLARATIVE_ANIMATIONS_PER_WINDOW: usize = 4_096;

/// Highest exact timer cadence accepted by [`Animation::with_max_fps`].
///
/// Unthrottled animations still follow the display's presentation cadence. Capping application
/// timers prevents an accidental value such as one million FPS from creating a hot wake loop.
pub const MAX_ANIMATION_FPS: f32 = 240.0;

/// A duration-based animation that can be applied declaratively to an element.
///
/// Animations are one-shot and linear by default. The easing output may overshoot `0..=1`; only
/// non-finite values are rejected by the runtime.
#[derive(Clone)]
pub struct Animation {
    pub duration: Duration,
    pub oneshot: bool,
    pub synced: bool,
    pub easing: Rc<dyn Fn(f32) -> f32>,
    pub max_fps: Option<f32>,
}

impl Animation {
    pub fn new(duration: Duration) -> Self {
        Self {
            duration,
            oneshot: true,
            synced: false,
            easing: Rc::new(linear),
            max_fps: None,
        }
    }

    /// Repeat from the element-local start time.
    pub fn repeat(mut self) -> Self {
        self.oneshot = false;
        self
    }

    /// Repeat phase-locked to the application animation epoch.
    ///
    /// Elements mounted later therefore join an already running shared phase instead of starting
    /// at zero.
    pub fn repeat_synced(mut self) -> Self {
        self.oneshot = false;
        self.synced = true;
        self
    }

    pub fn with_easing(mut self, easing: impl Fn(f32) -> f32 + 'static) -> Self {
        self.easing = Rc::new(easing);
        self
    }

    /// Limit declaration rebuilds while this animation is active.
    ///
    /// Non-finite and non-positive values are ignored. Effective timer cadence is capped at
    /// [`MAX_ANIMATION_FPS`].
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
}

impl fmt::Debug for Animation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Animation")
            .field("duration", &self.duration)
            .field("oneshot", &self.oneshot)
            .field("synced", &self.synced)
            .field("max_fps", &self.max_fps)
            .finish_non_exhaustive()
    }
}

/// Potentially overshooting coordinate within an animation.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct AnimationPhase(pub f32);

impl AnimationPhase {
    pub fn clamp(self, range: RangeInclusive<f32>) -> Self {
        let (first, second) = range.into_inner();
        Self(self.0.clamp(first.min(second), first.max(second)))
    }

    pub fn interpolate<T: Interpolate>(self, from: T, to: T) -> T {
        T::interpolate(from, to, self.0)
    }

    pub fn interpolate_clamped<T: Interpolate>(self, from: T, to: T) -> T {
        T::interpolate(from, to, self.0.clamp(0.0, 1.0))
    }

    pub fn interpolate_between<T: Interpolate>(
        self,
        range: RangeInclusive<f32>,
        from: T,
        to: T,
    ) -> T {
        let (start, end) = range.into_inner();
        let phase = normalized_phase(self.0, start, end);
        T::interpolate(from, to, phase)
    }

    pub fn interpolate_between_clamped<T: Interpolate>(
        self,
        range: RangeInclusive<f32>,
        from: T,
        to: T,
    ) -> T {
        let (start, end) = range.into_inner();
        let phase = normalized_phase(self.0, start, end).clamp(0.0, 1.0);
        T::interpolate(from, to, phase)
    }
}

impl From<f32> for AnimationPhase {
    fn from(value: f32) -> Self {
        Self(value)
    }
}

impl From<bool> for AnimationPhase {
    fn from(value: bool) -> Self {
        Self(if value { 1.0 } else { 0.0 })
    }
}

/// A value supporting linear interpolation and extrapolation.
pub trait Interpolate: Sized {
    fn interpolate(from: Self, to: Self, phase: f32) -> Self;
}

impl Interpolate for f32 {
    fn interpolate(from: Self, to: Self, phase: f32) -> Self {
        from + (to - from) * phase
    }
}

impl Interpolate for Color {
    fn interpolate(from: Self, to: Self, phase: f32) -> Self {
        Self::linear(
            f32::interpolate(from.r, to.r, phase),
            f32::interpolate(from.g, to.g, phase),
            f32::interpolate(from.b, to.b, phase),
            f32::interpolate(from.a, to.a, phase),
        )
    }
}

impl Interpolate for Point {
    fn interpolate(from: Self, to: Self, phase: f32) -> Self {
        Self::new(
            f32::interpolate(from.x, to.x, phase),
            f32::interpolate(from.y, to.y, phase),
        )
    }
}

impl Interpolate for Size {
    fn interpolate(from: Self, to: Self, phase: f32) -> Self {
        Self::new(
            f32::interpolate(from.width, to.width, phase),
            f32::interpolate(from.height, to.height, phase),
        )
    }
}

impl Interpolate for Vector {
    fn interpolate(from: Self, to: Self, phase: f32) -> Self {
        Self::new(
            f32::interpolate(from.x, to.x, phase),
            f32::interpolate(from.y, to.y, phase),
        )
    }
}

impl Interpolate for Rect {
    fn interpolate(from: Self, to: Self, phase: f32) -> Self {
        Self::new(
            f32::interpolate(from.x, to.x, phase),
            f32::interpolate(from.y, to.y, phase),
            f32::interpolate(from.width, to.width, phase),
            f32::interpolate(from.height, to.height, phase),
        )
    }
}

pub(crate) type ElementAnimator = Rc<dyn Fn(Element, usize, f32) -> Element>;

#[derive(Clone)]
pub(crate) struct ElementAnimation {
    pub id: ElementId,
    pub stages: Arc<[Animation]>,
    pub animator: ElementAnimator,
}

impl fmt::Debug for ElementAnimation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ElementAnimation")
            .field("id", &self.id)
            .field("stages", &self.stages)
            .finish_non_exhaustive()
    }
}

/// GPUI-shaped extension methods for applying declaration-time animation values.
///
/// QuickGUI has one concrete [`Element`] type, so animator callbacks receive that normalized
/// element even when the input was another [`IntoElement`] value.
pub trait AnimationExt: IntoElement + Sized {
    fn with_animation(
        self,
        id: impl Into<ElementId>,
        animation: Animation,
        animator: impl Fn(Element, f32) -> Element + 'static,
    ) -> Element {
        self.with_animations(id, vec![animation], move |element, _, phase| {
            animator(element, phase)
        })
    }

    fn with_animations(
        self,
        id: impl Into<ElementId>,
        animations: Vec<Animation>,
        animator: impl Fn(Element, usize, f32) -> Element + 'static,
    ) -> Element {
        assert!(
            !animations.is_empty(),
            "an animated element requires at least one animation stage"
        );
        assert!(
            animations.len() <= MAX_ANIMATION_STAGES,
            "an animated element retains at most {MAX_ANIMATION_STAGES} stages"
        );
        let mut element = self.into_element();
        element.animation = Some(ElementAnimation {
            id: id.into(),
            stages: Arc::from(animations),
            animator: Rc::new(animator),
        });
        element
    }

    /// Drive an element with a retained, frame-rate-independent spring.
    ///
    /// Position and velocity survive target changes as long as the ID remains mounted.
    fn with_spring<T>(
        self,
        id: impl Into<ElementId>,
        animation: SpringAnimation<T>,
        animator: impl Fn(Element, T::Output) -> Element + 'static,
    ) -> Element
    where
        T: SpringTarget,
        T::Output: 'static,
    {
        let target = animation.target;
        let scalar_target = target.target();
        let mut element = self.into_element();
        element.spring = Some(ElementSpring {
            id: id.into(),
            config: animation.config,
            target: scalar_target,
            epsilon: animation.epsilon,
            initial: animation.initial,
            playback: animation.playback,
            animator: Rc::new(move |element, value| animator(element, target.resolve(value))),
        });
        element
    }
}

impl<E: IntoElement> AnimationExt for E {}

pub fn linear(delta: f32) -> f32 {
    delta
}

pub fn quadratic(delta: f32) -> f32 {
    delta * delta
}

pub fn ease_in_out(delta: f32) -> f32 {
    if delta < 0.5 {
        2.0 * delta * delta
    } else {
        let x = -2.0 * delta + 2.0;
        1.0 - x * x / 2.0
    }
}

pub fn ease_out_quint() -> impl Fn(f32) -> f32 {
    move |delta| 1.0 - (1.0 - delta).powi(5)
}

pub fn bounce(easing: impl Fn(f32) -> f32) -> impl Fn(f32) -> f32 {
    move |delta| {
        if delta < 0.5 {
            easing(delta * 2.0)
        } else {
            easing((1.0 - delta) * 2.0)
        }
    }
}

pub fn pulsating_between(min: f32, max: f32) -> impl Fn(f32) -> f32 {
    let range = max - min;
    move |delta| {
        let time = (delta * 2.0 * std::f32::consts::PI).sin();
        let breath = (time * time * time + time) / 2.0;
        min + ((breath + 1.0) / 2.0) * range
    }
}

fn normalized_phase(value: f32, start: f32, end: f32) -> f32 {
    if start == end {
        if value < start { 0.0 } else { 1.0 }
    } else {
        (value - start) / (end - start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_interpolate_and_extrapolate() {
        let phase = AnimationPhase(1.5);
        assert_eq!(phase.interpolate_between(1.0..=2.0, 10.0, 20.0), 15.0);
        assert_eq!(
            AnimationPhase(3.5).interpolate_between(2.0..=3.0, 10.0, 20.0),
            25.0
        );
        assert_eq!(
            AnimationPhase(3.5).interpolate_between_clamped(2.0..=3.0, 10.0, 20.0),
            20.0
        );
    }

    #[test]
    fn invalid_fps_is_unthrottled_and_large_fps_is_bounded() {
        assert_eq!(
            Animation::new(Duration::from_secs(1))
                .with_max_fps(0.0)
                .frame_interval(),
            None
        );
        assert_eq!(
            Animation::new(Duration::from_secs(1))
                .with_max_fps(f32::NAN)
                .frame_interval(),
            None
        );
        assert_eq!(
            Animation::new(Duration::from_secs(1))
                .with_max_fps(10_000.0)
                .frame_interval(),
            Some(Duration::from_secs_f64(1.0 / f64::from(MAX_ANIMATION_FPS)))
        );
    }
}
