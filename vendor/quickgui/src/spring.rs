use std::{fmt, rc::Rc};

use crate::{AnimationPhase, Element, ElementId};

const CRITICAL_DAMPING_TOLERANCE: f32 = 1.0e-4;
const DEFAULT_SPRING_EPSILON: f32 = 0.001;

/// Physical parameters of a damped harmonic oscillator.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpringConfig {
    pub stiffness: f32,
    pub damping: f32,
    pub mass: f32,
}

impl SpringConfig {
    pub const fn new(stiffness: f32, damping: f32, mass: f32) -> Self {
        Self {
            stiffness,
            damping,
            mass,
        }
    }

    pub fn is_valid(self) -> bool {
        self.stiffness.is_finite()
            && self.stiffness > 0.0
            && self.damping.is_finite()
            && self.damping >= 0.0
            && self.mass.is_finite()
            && self.mass > 0.0
    }

    /// Natural angular frequency and damping ratio `(omega_0, zeta)`.
    pub fn canonical(self) -> (f32, f32) {
        if !self.is_valid() {
            return (0.0, 0.0);
        }
        let natural_frequency = (self.stiffness / self.mass).sqrt();
        let damping_ratio = self.damping / (2.0 * (self.stiffness * self.mass).sqrt());
        (natural_frequency, damping_ratio)
    }

    /// Advance toward a fixed target using an analytic, frame-rate-independent solution.
    pub fn step(self, state: SpringState, target: f32, delta_time_seconds: f32) -> SpringState {
        if !self.is_valid()
            || !target.is_finite()
            || !state.position.is_finite()
            || !state.velocity.is_finite()
            || !delta_time_seconds.is_finite()
            || delta_time_seconds <= 0.0
        {
            return state;
        }
        let matrix = self.propagator(delta_time_seconds);
        let displacement = state.position - target;
        let next = SpringState {
            position: target + matrix[0][0] * displacement + matrix[0][1] * state.velocity,
            velocity: matrix[1][0] * displacement + matrix[1][1] * state.velocity,
        };
        if next.position.is_finite() && next.velocity.is_finite() {
            next
        } else {
            SpringState {
                position: target,
                velocity: 0.0,
            }
        }
    }

    /// Exact state-transition matrix for a constant target.
    pub fn propagator(self, delta_time_seconds: f32) -> [[f32; 2]; 2] {
        if !self.is_valid() || !delta_time_seconds.is_finite() || delta_time_seconds <= 0.0 {
            return [[1.0, 0.0], [0.0, 1.0]];
        }
        let (omega, damping_ratio) = self.canonical();
        if damping_ratio < 1.0 - CRITICAL_DAMPING_TOLERANCE {
            let decay = damping_ratio * omega;
            let damped = omega * (1.0 - damping_ratio * damping_ratio).sqrt();
            let exponential = (-decay * delta_time_seconds).exp();
            let (sine, cosine) = (damped * delta_time_seconds).sin_cos();
            let sine_over_frequency = sine / damped;
            [
                [
                    exponential * (cosine + decay * sine_over_frequency),
                    exponential * sine_over_frequency,
                ],
                [
                    -exponential * omega * omega * sine_over_frequency,
                    exponential * (cosine - decay * sine_over_frequency),
                ],
            ]
        } else if damping_ratio > 1.0 + CRITICAL_DAMPING_TOLERANCE {
            let root = (damping_ratio * damping_ratio - 1.0).sqrt();
            let root_sum = damping_ratio + root;
            let slow = -omega / root_sum;
            let fast = -omega * root_sum;
            let denominator = slow - fast;
            let slow_exponential = (slow * delta_time_seconds).exp();
            let fast_exponential = (fast * delta_time_seconds).exp();
            [
                [
                    (-fast * slow_exponential + slow * fast_exponential) / denominator,
                    (slow_exponential - fast_exponential) / denominator,
                ],
                [
                    slow * fast * (fast_exponential - slow_exponential) / denominator,
                    (slow * slow_exponential - fast * fast_exponential) / denominator,
                ],
            ]
        } else {
            let exponential = (-omega * delta_time_seconds).exp();
            [
                [
                    exponential * (1.0 + omega * delta_time_seconds),
                    exponential * delta_time_seconds,
                ],
                [
                    -exponential * omega * omega * delta_time_seconds,
                    exponential * (1.0 - omega * delta_time_seconds),
                ],
            ]
        }
    }

    pub fn is_settled(self, state: SpringState, target: f32, epsilon: f32) -> bool {
        if !self.is_valid() || !epsilon.is_finite() || epsilon < 0.0 {
            return false;
        }
        let (omega, _) = self.canonical();
        (state.position - target).abs() <= epsilon && state.velocity.abs() <= epsilon * omega
    }
}

impl Default for SpringConfig {
    fn default() -> Self {
        Self::new(170.0, 26.0, 1.0)
    }
}

/// Instantaneous spring position and velocity.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpringState {
    pub position: f32,
    pub velocity: f32,
}

/// Controls how a retained spring advances and resolves its presentation value.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SpringPlayback {
    #[default]
    Running,
    Paused,
    Stopped,
    Completed,
    Cancelled,
}

/// A value that can be targeted by a one-dimensional spring.
pub trait SpringTarget: 'static {
    type Output;

    fn target(&self) -> f32;
    fn resolve(&self, value: f32) -> Self::Output;
}

impl SpringTarget for f32 {
    type Output = f32;

    fn target(&self) -> f32 {
        *self
    }

    fn resolve(&self, value: f32) -> Self::Output {
        value
    }
}

impl SpringTarget for bool {
    type Output = AnimationPhase;

    fn target(&self) -> f32 {
        if *self { 1.0 } else { 0.0 }
    }

    fn resolve(&self, value: f32) -> Self::Output {
        AnimationPhase(value)
    }
}

impl SpringTarget for AnimationPhase {
    type Output = AnimationPhase;

    fn target(&self) -> f32 {
        self.0
    }

    fn resolve(&self, value: f32) -> Self::Output {
        Self(value)
    }
}

/// Stateful spring animation targeting a scalar value or projected phase.
#[derive(Clone, Debug)]
pub struct SpringAnimation<T = ()> {
    pub(crate) config: SpringConfig,
    pub(crate) target: T,
    pub(crate) epsilon: f32,
    pub(crate) initial: Option<f32>,
    pub(crate) playback: SpringPlayback,
}

impl SpringAnimation<()> {
    pub fn new(config: SpringConfig) -> Self {
        Self {
            config,
            target: (),
            epsilon: DEFAULT_SPRING_EPSILON,
            initial: None,
            playback: SpringPlayback::Running,
        }
    }

    pub fn to<T: SpringTarget>(self, target: T) -> SpringAnimation<T> {
        SpringAnimation {
            config: self.config,
            target,
            epsilon: self.epsilon,
            initial: self.initial,
            playback: self.playback,
        }
    }
}

impl<T> SpringAnimation<T> {
    pub fn with_epsilon(mut self, epsilon: f32) -> Self {
        self.epsilon = epsilon;
        self
    }

    pub fn playback(mut self, playback: SpringPlayback) -> Self {
        self.playback = playback;
        self
    }
}

impl<T: SpringTarget> SpringAnimation<T> {
    pub fn from(mut self, initial: T) -> Self {
        self.initial = Some(initial.target());
        self
    }
}

pub(crate) type SpringAnimator = Rc<dyn Fn(Element, f32) -> Element>;

#[derive(Clone)]
pub(crate) struct ElementSpring {
    pub id: ElementId,
    pub config: SpringConfig,
    pub target: f32,
    pub epsilon: f32,
    pub initial: Option<f32>,
    pub playback: SpringPlayback,
    pub animator: SpringAnimator,
}

impl fmt::Debug for ElementSpring {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ElementSpring")
            .field("id", &self.id)
            .field("config", &self.config)
            .field("target", &self.target)
            .field("epsilon", &self.epsilon)
            .field("initial", &self.initial)
            .field("playback", &self.playback)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analytic_steps_compose_across_damping_regimes() {
        let state = SpringState {
            position: -3.0,
            velocity: 5.0,
        };
        for damping in [4.0, 20.0, 40.0] {
            let config = SpringConfig::new(100.0, damping, 1.0);
            let split = config.step(config.step(state, 7.0, 0.013), 7.0, 0.021);
            let direct = config.step(state, 7.0, 0.034);
            assert!((split.position - direct.position).abs() < 2.0e-4);
            assert!((split.velocity - direct.velocity).abs() < 2.0e-4);
        }
    }

    #[test]
    fn invalid_springs_never_create_non_finite_state() {
        let state = SpringState {
            position: 2.0,
            velocity: 3.0,
        };
        assert_eq!(
            SpringConfig::new(0.0, 1.0, 1.0).step(state, 8.0, 1.0),
            state
        );
        assert_eq!(
            SpringConfig::new(1.0, 1.0, 0.0).step(state, 8.0, 1.0),
            state
        );
    }
}
