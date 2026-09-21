use super::*;

impl AnimationPlayback {
    pub(super) fn new(asset: AnimatedImage, now: Instant) -> Self {
        Self {
            asset_id: asset.id(),
            asset,
            frame_index: 0,
            elapsed: Duration::ZERO,
            last_advanced_at: now,
            active: false,
            seen: false,
            completed: false,
        }
    }

    pub(super) fn activate(&mut self, now: Instant, enabled: bool) {
        self.seen = true;
        if enabled && !self.completed && self.asset.frame_count() > 1 && !self.active {
            self.active = true;
            self.last_advanced_at = now;
        }
    }

    pub(super) fn advance(&mut self, now: Instant) -> bool {
        if !self.active || self.completed {
            return false;
        }
        self.elapsed = self
            .elapsed
            .saturating_add(now.saturating_duration_since(self.last_advanced_at));
        self.last_advanced_at = now;
        let previous = self.frame_index;
        (self.frame_index, self.completed) = self.asset.frame_index_at(self.elapsed);
        if self.completed {
            self.active = false;
        }
        self.frame_index != previous
    }

    pub(super) fn finish_visibility(&mut self) {
        if !self.seen {
            self.active = false;
        }
    }

    pub(super) fn deadline(&self) -> Option<Instant> {
        (self.active && !self.completed && self.asset.frame_count() > 1)
            .then(|| {
                self.last_advanced_at.checked_add(
                    self.asset
                        .remaining_in_frame(self.elapsed, self.frame_index),
                )
            })
            .flatten()
    }
}

impl DeclarativeAnimationPlayback {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            animation_ix: 0,
            elapsed: Duration::ZERO,
            last_advanced_at: now,
            next_frame_at: None,
            scheduled_stage: 0,
            last_value: 0.0,
            completed: false,
            active: false,
        }
    }

    pub(super) fn pause(&mut self, now: Instant) {
        if self.active && !self.completed {
            self.elapsed = self
                .elapsed
                .saturating_add(now.saturating_duration_since(self.last_advanced_at));
        }
        self.last_advanced_at = now;
        self.next_frame_at = None;
        self.active = false;
    }

    pub(super) fn resume(&mut self, now: Instant) {
        self.last_advanced_at = now;
        self.next_frame_at = None;
    }

    pub(super) fn sample(
        &mut self,
        stages: &[Animation],
        now: Instant,
        animation_epoch: Instant,
        enabled: bool,
        reduce_motion: bool,
    ) -> DeclarativeAnimationSample {
        debug_assert!(!stages.is_empty());
        if self.animation_ix >= stages.len() {
            self.animation_ix = stages.len() - 1;
            self.elapsed = Duration::ZERO;
            self.completed = false;
            self.next_frame_at = None;
        }

        if reduce_motion {
            let animation_ix = stages.len() - 1;
            let phase = if stages[animation_ix].oneshot {
                1.0
            } else {
                0.0
            };
            self.active = false;
            self.next_frame_at = None;
            self.last_value = stages[animation_ix].eased(phase);
            return DeclarativeAnimationSample {
                animation_ix,
                value: self.last_value,
                request_frame: false,
                deadline: None,
            };
        }

        if !enabled {
            return DeclarativeAnimationSample {
                animation_ix: self.animation_ix,
                value: self.last_value,
                request_frame: false,
                deadline: None,
            };
        }

        if !self.completed {
            self.elapsed = self
                .elapsed
                .saturating_add(now.saturating_duration_since(self.last_advanced_at));
        }
        self.last_advanced_at = now;

        let (animation_ix, phase, active) = self.resolve_phase(stages, now, animation_epoch);
        let stage = &stages[animation_ix];
        self.last_value = stage.eased(phase);
        self.active = active;

        let (request_frame, deadline) = if active {
            if let Some(interval) = stage.frame_interval() {
                if self.scheduled_stage != animation_ix
                    || self.next_frame_at.is_none_or(|deadline| deadline <= now)
                {
                    let delay = if stage.oneshot {
                        interval.min(stage.duration.saturating_sub(self.elapsed))
                    } else {
                        interval
                    };
                    self.next_frame_at = now.checked_add(delay);
                    self.scheduled_stage = animation_ix;
                }
                (false, self.next_frame_at)
            } else {
                self.next_frame_at = None;
                (true, None)
            }
        } else {
            self.next_frame_at = None;
            (false, None)
        };

        DeclarativeAnimationSample {
            animation_ix,
            value: self.last_value,
            request_frame,
            deadline,
        }
    }

    pub(super) fn resolve_phase(
        &mut self,
        stages: &[Animation],
        now: Instant,
        animation_epoch: Instant,
    ) -> (usize, f32, bool) {
        if self.completed {
            return (self.animation_ix, 1.0, false);
        }

        loop {
            let stage = &stages[self.animation_ix];
            if stage.duration.is_zero() {
                if stage.oneshot && self.animation_ix + 1 < stages.len() {
                    self.animation_ix += 1;
                    self.elapsed = Duration::ZERO;
                    self.next_frame_at = None;
                    continue;
                }
                self.completed = stage.oneshot;
                return (
                    self.animation_ix,
                    if stage.oneshot { 1.0 } else { 0.0 },
                    false,
                );
            }

            if !stage.oneshot && stage.synced {
                let elapsed = now.saturating_duration_since(animation_epoch);
                let remainder = elapsed.as_nanos() % stage.duration.as_nanos();
                let phase = remainder as f64 / stage.duration.as_nanos() as f64;
                return (self.animation_ix, phase as f32, true);
            }

            if stage.oneshot {
                if self.elapsed >= stage.duration {
                    if self.animation_ix + 1 < stages.len() {
                        self.elapsed = self.elapsed.saturating_sub(stage.duration);
                        self.animation_ix += 1;
                        self.next_frame_at = None;
                        continue;
                    }
                    self.elapsed = stage.duration;
                    self.completed = true;
                    return (self.animation_ix, 1.0, false);
                }
                return (
                    self.animation_ix,
                    self.elapsed.as_secs_f32() / stage.duration.as_secs_f32(),
                    true,
                );
            }

            let remainder = self.elapsed.as_nanos() % stage.duration.as_nanos();
            let phase = remainder as f64 / stage.duration.as_nanos() as f64;
            return (self.animation_ix, phase as f32, true);
        }
    }
}

impl DeclarativeSpringPlayback {
    pub(super) fn new(declaration: &ElementSpring, now: Instant) -> Self {
        let target = finite_spring_value(declaration.target, 0.0);
        let initial = declaration
            .initial
            .filter(|value| value.is_finite())
            .unwrap_or(target);
        Self {
            state: SpringState {
                position: initial,
                velocity: 0.0,
            },
            target,
            config: declaration.config,
            initial,
            playback: declaration.playback,
            updated_at: now,
            active: false,
        }
    }

    pub(super) fn pause(&mut self, now: Instant) {
        self.advance_previous_target(now);
        self.updated_at = now;
        self.active = false;
    }

    pub(super) fn resume(&mut self, now: Instant) {
        self.updated_at = now;
    }

    pub(super) fn sample(
        &mut self,
        declaration: &ElementSpring,
        now: Instant,
        enabled: bool,
        reduce_motion: bool,
    ) -> DeclarativeSpringSample {
        self.advance_previous_target(now);
        self.updated_at = now;
        self.config = declaration.config;
        self.target = finite_spring_value(declaration.target, self.target);
        self.playback = declaration.playback;

        if reduce_motion {
            self.state = SpringState {
                position: self.target,
                velocity: 0.0,
            };
            self.active = false;
            return DeclarativeSpringSample {
                value: self.state.position,
                request_frame: false,
            };
        }

        if !enabled {
            self.active = false;
            return DeclarativeSpringSample {
                value: self.state.position,
                request_frame: false,
            };
        }

        let epsilon = sane_spring_epsilon(declaration.epsilon);
        self.active = match self.playback {
            SpringPlayback::Running if self.config.is_valid() => {
                let settled = self.config.is_settled(self.state, self.target, epsilon);
                if settled {
                    self.state = SpringState {
                        position: self.target,
                        velocity: 0.0,
                    };
                }
                !settled
            }
            SpringPlayback::Running => {
                self.state = SpringState {
                    position: self.target,
                    velocity: 0.0,
                };
                false
            }
            SpringPlayback::Paused => false,
            SpringPlayback::Stopped => {
                self.state.velocity = 0.0;
                false
            }
            SpringPlayback::Completed => {
                self.state = SpringState {
                    position: self.target,
                    velocity: 0.0,
                };
                false
            }
            SpringPlayback::Cancelled => {
                self.state = SpringState {
                    position: self.initial,
                    velocity: 0.0,
                };
                false
            }
        };

        DeclarativeSpringSample {
            value: self.state.position,
            request_frame: self.active,
        }
    }

    pub(super) fn advance_previous_target(&mut self, now: Instant) {
        if !self.active || self.playback != SpringPlayback::Running || !self.config.is_valid() {
            return;
        }
        let elapsed = now.saturating_duration_since(self.updated_at);
        let seconds = elapsed.as_secs_f64().min(60.0) as f32;
        self.state = self.config.step(self.state, self.target, seconds);
    }
}

impl StyleTransitionPlayback {
    pub(super) fn new(target: TransitionPaintStyle, config: &Transition, now: Instant) -> Self {
        Self {
            from: target,
            current: target,
            target,
            config: config.clone(),
            elapsed: Duration::ZERO,
            last_advanced_at: now,
            next_frame_at: None,
            in_progress: false,
            active: false,
        }
    }

    pub(super) fn sample(
        &mut self,
        target: TransitionPaintStyle,
        config: &Transition,
        now: Instant,
        enabled: bool,
        reduce_motion: bool,
    ) -> TransitionPaintStyle {
        self.advance_to(now);
        let target_changed = self.target != target;
        let properties_changed = self.config.properties != config.properties;
        let cadence_changed =
            self.config.max_fps.map(f32::to_bits) != config.max_fps.map(f32::to_bits);
        let configuration_changed = !self.config.same_configuration(config);
        if target_changed || properties_changed {
            self.from = self.current;
            self.target = target;
            self.elapsed = Duration::ZERO;
            self.next_frame_at = None;
        }
        if configuration_changed {
            self.config = config.clone();
        }
        if cadence_changed {
            self.next_frame_at = None;
        }

        if reduce_motion || self.config.duration.is_zero() || self.config.properties.is_empty() {
            self.finish();
            return self.current;
        }

        if target_changed || properties_changed {
            self.in_progress = self.from.differs_for(self.target, self.config.properties);
            if !self.in_progress {
                self.finish();
                return self.current;
            }
        }

        if !enabled {
            self.active = false;
            self.next_frame_at = None;
            self.last_advanced_at = now;
            return self.current;
        }

        if self.in_progress {
            self.active = true;
            self.resolve_current();
        }
        if self.active {
            if let Some(interval) = self.config.frame_interval() {
                if self.next_frame_at.is_none_or(|deadline| deadline <= now) {
                    let remaining = self.config.duration.saturating_sub(self.elapsed);
                    self.next_frame_at = now.checked_add(interval.min(remaining));
                }
            } else {
                self.next_frame_at = None;
            }
        }
        self.current
    }

    pub(super) fn advance_to(&mut self, now: Instant) {
        if self.active && self.in_progress {
            self.elapsed = self
                .elapsed
                .saturating_add(now.saturating_duration_since(self.last_advanced_at));
            self.resolve_current();
        }
        self.last_advanced_at = now;
    }

    pub(super) fn resolve_current(&mut self) {
        if self.config.duration.is_zero() || self.elapsed >= self.config.duration {
            self.finish();
            return;
        }
        let phase = self.elapsed.as_secs_f32() / self.config.duration.as_secs_f32();
        self.current = TransitionPaintStyle::interpolate(
            self.from,
            self.target,
            self.config.eased(phase),
            self.config.properties,
        );
    }

    pub(super) fn finish(&mut self) {
        self.current = self.target;
        self.elapsed = self.config.duration;
        self.next_frame_at = None;
        self.in_progress = false;
        self.active = false;
    }

    pub(super) fn pause(&mut self, now: Instant) {
        self.advance_to(now);
        self.active = false;
        self.next_frame_at = None;
    }

    pub(super) fn resume(&mut self, now: Instant) {
        self.last_advanced_at = now;
        self.next_frame_at = None;
        self.active = self.in_progress;
    }

    pub(super) fn deadline_due(&mut self, now: Instant) -> bool {
        if self.active && self.next_frame_at.is_some_and(|deadline| deadline <= now) {
            self.next_frame_at = None;
            true
        } else {
            false
        }
    }

    pub(super) fn deadline(&self) -> Option<Instant> {
        (self.active && self.in_progress)
            .then_some(self.next_frame_at)
            .flatten()
    }

    pub(super) fn requests_frame(&self) -> bool {
        self.active && self.in_progress && self.config.frame_interval().is_none()
    }
}

impl StyleTransitionPaintContext<'_> {
    pub(super) fn sample(
        &mut self,
        id: ElementId,
        target: TransitionPaintStyle,
        config: &Transition,
    ) -> TransitionPaintStyle {
        let Some(playbacks) = self.playbacks.as_deref_mut() else {
            return target;
        };
        let playback = playbacks
            .entry(id)
            .or_insert_with(|| StyleTransitionPlayback::new(target, config, self.now));
        let value = playback.sample(target, config, self.now, self.enabled, self.reduce_motion);
        *self.request_frame |= playback.requests_frame();
        value
    }
}

pub(super) fn interpolate_transition_transform(
    from: Transform2D,
    to: Transform2D,
    phase: f32,
) -> Transform2D {
    if phase == 0.0 {
        return from;
    }
    if phase == 1.0 {
        return to;
    }
    let lerp = |from, to| f32::interpolate(from, to, phase);
    // Translation needs no decomposition and stays on the paint path without a group texture.
    let matrix_lerp = || {
        Transform2D::new(
            lerp(from.a, to.a),
            lerp(from.b, to.b),
            lerp(from.c, to.c),
            lerp(from.d, to.d),
            lerp(from.tx, to.tx),
            lerp(from.ty, to.ty),
        )
    };
    if (from.a, from.b, from.c, from.d) == (to.a, to.b, to.c, to.d) {
        return matrix_lerp();
    }
    // Separate rotation from scale/shear so a turning element does not shrink at the midpoint.
    // Singular endpoints have no unique rotation, so interpolate their matrix components.
    let decompose = |matrix: Transform2D| {
        let scale_x = matrix.a.hypot(matrix.b);
        (scale_x > f32::EPSILON).then(|| {
            (
                scale_x,
                matrix.determinant() / scale_x,
                (matrix.a * matrix.c + matrix.b * matrix.d) / scale_x,
                matrix.b.atan2(matrix.a),
            )
        })
    };
    let (Some((from_x, from_y, from_shear, from_angle)), Some((to_x, to_y, to_shear, to_angle))) =
        (decompose(from), decompose(to))
    else {
        return matrix_lerp();
    };
    let angle_delta = (to_angle - from_angle + std::f32::consts::PI)
        .rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    let (sin, cos) = (from_angle + phase * angle_delta).sin_cos();
    let scale_x = lerp(from_x, to_x);
    let scale_y = lerp(from_y, to_y);
    let shear = lerp(from_shear, to_shear);
    Transform2D::new(
        cos * scale_x,
        sin * scale_x,
        cos * shear - sin * scale_y,
        sin * shear + cos * scale_y,
        lerp(from.tx, to.tx),
        lerp(from.ty, to.ty),
    )
}

pub(super) fn sane_transition_color(color: Color, fallback: Color) -> Color {
    if [color.r, color.g, color.b, color.a]
        .into_iter()
        .all(f32::is_finite)
    {
        color
    } else {
        fallback
    }
}

pub(super) fn sane_transition_shadow(shadow: BoxShadow) -> BoxShadow {
    BoxShadow::new(
        shadow.offset().x,
        shadow.offset().y,
        sane_transition_color(shadow.color(), Color::TRANSPARENT),
    )
    .blur_radius(shadow.blur())
    .spread_radius(shadow.spread())
    .inset(shadow.is_inset())
}

pub(super) fn neutral_transition_shadow(inset: bool) -> BoxShadow {
    BoxShadow::new(0.0, 0.0, Color::TRANSPARENT).inset(inset)
}

pub(super) fn interpolate_transition_shadow(
    from: BoxShadow,
    to: BoxShadow,
    phase: f32,
) -> BoxShadow {
    if from.is_inset() != to.is_inset() {
        return if phase < 0.5 { from } else { to };
    }
    BoxShadow::new(
        f32::interpolate(from.offset().x, to.offset().x, phase),
        f32::interpolate(from.offset().y, to.offset().y, phase),
        Color::interpolate_premultiplied(from.color(), to.color(), phase),
    )
    .blur_radius(f32::interpolate(from.blur(), to.blur(), phase))
    .spread_radius(f32::interpolate(from.spread(), to.spread(), phase))
    .inset(from.is_inset())
}

pub(super) fn sane_spring_epsilon(epsilon: f32) -> f32 {
    if epsilon.is_finite() && epsilon >= 0.0 {
        epsilon
    } else {
        0.001
    }
}

pub(super) fn finite_spring_value(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

impl DetachedMotionState {
    pub(super) fn new(mut template: Element, animation_epoch: Instant) -> Self {
        sanitize_detached_element(&mut template, true);
        Self {
            template,
            animations: HashMap::with_capacity(4),
            time_animation_ids: HashSet::with_capacity(4),
            springs: HashMap::with_capacity(4),
            spring_ids: HashSet::with_capacity(4),
            motion_ids: HashSet::with_capacity(4),
            frame_requested: false,
            deadline: None,
            needs_resolve: true,
            animation_epoch,
        }
    }

    #[cfg(test)]
    pub(super) fn resolve(
        &mut self,
        now: Instant,
        enabled: bool,
        reduce_motion: bool,
    ) -> Result<Element, UiError> {
        let root = self.begin_resolution(now, enabled, reduce_motion)?;
        self.finish_resolution();
        Ok(root)
    }

    pub(super) fn begin_resolution(
        &mut self,
        now: Instant,
        enabled: bool,
        reduce_motion: bool,
    ) -> Result<Element, UiError> {
        let mut root = self.template.clone();
        self.motion_ids.clear();
        self.time_animation_ids.clear();
        self.spring_ids.clear();
        self.frame_requested = false;
        self.deadline = None;
        let mut resolved_motion_ids = Vec::new();
        resolve_declarative_animations(
            &mut root,
            &mut self.animations,
            &mut self.motion_ids,
            &mut self.time_animation_ids,
            &mut self.springs,
            &mut self.spring_ids,
            &mut self.frame_requested,
            &mut self.deadline,
            now,
            self.animation_epoch,
            enabled,
            reduce_motion,
            &mut resolved_motion_ids,
        )?;
        // Animators may construct fresh interactive descendants. Detached surfaces stay strictly
        // pointer-passive even when their resolved shape changes from frame to frame.
        sanitize_detached_element(&mut root, false);
        self.needs_resolve = false;
        Ok(root)
    }

    pub(super) fn finish_resolution(&mut self) {
        self.animations
            .retain(|id, _| self.time_animation_ids.contains(id));
        self.springs.retain(|id, _| self.spring_ids.contains(id));
    }

    pub(super) fn pause(&mut self, now: Instant) {
        for playback in self.animations.values_mut() {
            playback.pause(now);
        }
        for playback in self.springs.values_mut() {
            playback.pause(now);
        }
        self.frame_requested = false;
        self.deadline = None;
    }

    pub(super) fn resume(&mut self, now: Instant) {
        for playback in self.animations.values_mut() {
            playback.resume(now);
        }
        for playback in self.springs.values_mut() {
            playback.resume(now);
        }
        self.frame_requested = false;
        self.deadline = None;
        self.needs_resolve = !self.animations.is_empty() || !self.springs.is_empty();
    }

    pub(super) fn mark_needs_resolve(&mut self) {
        self.needs_resolve |= !self.animations.is_empty() || !self.springs.is_empty();
    }

    pub(super) fn deadline_due(&mut self, now: Instant) -> bool {
        if self.deadline.is_some_and(|deadline| deadline <= now) {
            self.deadline = None;
            self.needs_resolve = true;
            true
        } else {
            false
        }
    }

    pub(super) fn active_count(&self) -> usize {
        self.animations
            .values()
            .filter(|playback| playback.active)
            .count()
            + self
                .springs
                .values()
                .filter(|playback| playback.active)
                .count()
    }
}

impl DetachedTree {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        template: Element,
        root_seed: ElementId,
        viewport: Size,
        scale_factor: f32,
        renderer: &mut impl TextLayoutEngine,
        now: Instant,
        animation_epoch: Instant,
        animations_enabled: bool,
        reduce_motion: bool,
    ) -> Result<Self, UiError> {
        let mut tree = Self {
            motion: DetachedMotionState::new(template, animation_epoch),
            root: crate::div(),
            taffy: TaffyTree::with_capacity(32),
            root_node: None,
            root_seed,
            seen_ids: HashSet::with_capacity(32),
            input_ids: HashSet::with_capacity(4),
            animation_ids: HashSet::with_capacity(4),
            scroll_offsets: HashMap::new(),
            scroll_end_states: HashMap::new(),
            natural_bounds: HashMap::new(),
            paint_bounds: HashMap::with_capacity(32),
            text_inputs: HashMap::new(),
            animations: HashMap::new(),
        };
        tree.refresh(
            viewport,
            scale_factor,
            renderer,
            now,
            animations_enabled,
            reduce_motion,
        )?;
        Ok(tree)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn refresh(
        &mut self,
        viewport: Size,
        scale_factor: f32,
        renderer: &mut impl TextLayoutEngine,
        now: Instant,
        animations_enabled: bool,
        reduce_motion: bool,
    ) -> Result<bool, UiError> {
        if !self.motion.needs_resolve {
            return Ok(false);
        }
        let mut root = self
            .motion
            .begin_resolution(now, animations_enabled, reduce_motion)?;
        let mut final_root_node = None;
        let mut converged = false;
        for _ in 0..=MAX_CONTAINER_QUERY_DEPTH {
            self.taffy.clear();
            self.seen_ids.clear();
            self.seen_ids
                .insert(ElementId::new(ACCESSIBILITY_ROOT_ID.0));
            validate_container_query_limits(&root)?;
            validate_sticky_limits(&root)?;
            let inherited = TextStyle::default();
            let root_node = build_layout_node(
                &mut self.taffy,
                &mut self.seen_ids,
                &mut root,
                self.root_seed,
                0,
                &inherited,
                false,
                Direction::Ltr,
            )?;
            compute_detached_layout(&mut self.taffy, root_node, viewport, scale_factor, renderer)?;
            compute_container_query_child_layouts(
                &root,
                &mut self.taffy,
                scale_factor,
                renderer,
                None,
            )?;

            let resolution = {
                let mut prepare = |_: &mut Element| {};
                let motion = &mut self.motion;
                let mut context = ContainerQueryResolveContext {
                    taffy: &self.taffy,
                    prepare: &mut prepare,
                    animations: &mut motion.animations,
                    motion_ids: &mut motion.motion_ids,
                    time_animation_ids: &mut motion.time_animation_ids,
                    springs: &mut motion.springs,
                    spring_ids: &mut motion.spring_ids,
                    request_frame: &mut motion.frame_requested,
                    deadline: &mut motion.deadline,
                    now,
                    animation_epoch: motion.animation_epoch,
                    enabled: animations_enabled,
                    reduce_motion,
                    sanitize_detached: true,
                };
                context.resolve(&mut root)?
            };
            if !resolution.changed {
                final_root_node = Some(root_node);
                converged = true;
                break;
            }
        }
        if !converged {
            return Err(UiError::ContainerQueryDidNotConverge);
        }
        self.motion.finish_resolution();
        let root_node = final_root_node.expect("a converged detached declaration has a root node");

        self.input_ids.clear();
        sync_text_inputs(&root, &mut self.text_inputs, &mut self.input_ids);
        self.text_inputs.retain(|id, _| self.input_ids.contains(id));
        self.animation_ids.clear();
        sync_animations(&root, &mut self.animations, &mut self.animation_ids, now);
        self.animations
            .retain(|id, _| self.animation_ids.contains(id));
        let mut displayed_ids = HashSet::with_capacity(self.seen_ids.len());
        collect_displayed_ids(&root, &mut displayed_ids);
        self.scroll_offsets
            .retain(|id, _| displayed_ids.contains(id));
        self.scroll_end_states
            .retain(|id, _| displayed_ids.contains(id));
        report_variable_list_layout_measurements(&root, &self.taffy)?;
        self.natural_bounds.clear();
        self.root = root;
        self.root_node = Some(root_node);
        Ok(true)
    }

    pub(super) fn layout(
        &mut self,
        viewport: Size,
        scale_factor: f32,
        renderer: &mut impl TextLayoutEngine,
    ) -> Result<(), UiError> {
        if self.motion.needs_resolve {
            return Ok(());
        }
        if let Some(root_node) = self.root_node {
            compute_detached_layout(&mut self.taffy, root_node, viewport, scale_factor, renderer)?;
            compute_container_query_child_layouts(
                &self.root,
                &mut self.taffy,
                scale_factor,
                renderer,
                None,
            )?;
            if container_queries_need_resolution(&self.root, &self.taffy)? {
                self.motion.needs_resolve = true;
            }
            report_variable_list_layout_measurements(&self.root, &self.taffy)?;
        }
        Ok(())
    }

    pub(super) fn root_layout(&self) -> Result<&taffy::tree::Layout, UiError> {
        let root = self
            .root_node
            .expect("a detached tree is laid out before it is painted");
        Ok(self.taffy.layout(root)?)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn paint(
        &mut self,
        origin: Point,
        layer: PaintLayerKey,
        scene: &mut Scene,
        renderer: &mut impl TextLayoutEngine,
        scale_factor: f32,
        animations_enabled: bool,
        reduce_motion: bool,
        paint_time: Instant,
        viewport: Rect,
        source_order: &mut usize,
    ) -> Result<(), UiError> {
        self.natural_bounds.clear();
        for playback in self.animations.values_mut() {
            playback.seen = false;
        }
        // Detached overlay trees never declare scroll-snap containers, so this scratch geometry
        // stays empty and never allocates.
        let mut snap = ScrollSnapGeometry::default();
        let frame = LayoutFrame::root(origin, viewport);
        collect_layout_bounds(
            &self.root,
            &self.taffy,
            &mut self.scroll_offsets,
            &mut self.scroll_end_states,
            &mut self.natural_bounds,
            &mut snap,
            None,
            frame,
        )?;
        debug_assert!(snap.is_empty());

        self.paint_bounds.clear();
        let hovered = HashSet::new();
        let mut hit_regions = Vec::new();
        let mut scroll_regions = Vec::new();
        let mut scrollbar_states = HashMap::new();
        let mut dismiss_regions = Vec::new();
        #[cfg(target_os = "macos")]
        let mut native_views = Vec::new();
        let mut text_input_regions = Vec::new();
        let selectable_text_indices = HashMap::new();
        let mut selectable_text_regions = Vec::new();
        let mut transition_frame_requested = false;
        let mut transition_context = StyleTransitionPaintContext {
            playbacks: None,
            request_frame: &mut transition_frame_requested,
            enabled: animations_enabled,
            reduce_motion,
            now: paint_time,
        };
        let result = paint_element(
            &self.root,
            &self.taffy,
            &self.natural_bounds,
            &mut self.paint_bounds,
            &mut self.scroll_offsets,
            &hovered,
            None,
            None,
            None,
            None,
            &[],
            &[],
            None,
            scale_factor,
            scene,
            renderer,
            &mut self.text_inputs,
            &mut self.animations,
            animations_enabled,
            paint_time,
            &mut transition_context,
            &mut hit_regions,
            &mut scroll_regions,
            &mut scrollbar_states,
            &mut dismiss_regions,
            #[cfg(target_os = "macos")]
            &mut native_views,
            &mut text_input_regions,
            &selectable_text_indices,
            &mut selectable_text_regions,
            None,
            frame,
            viewport,
            viewport,
            layer,
            source_order,
            None,
            &mut crate::PipelineMetrics::default(),
            true,
            &mut paint_cache::PaintCache::default(),
            true,
        );
        for playback in self.animations.values_mut() {
            playback.finish_visibility();
        }
        // An unthrottled declaration samples once per presented paint. Marking the next sample
        // here avoids resolving a newly created tooltip twice inside its first paint.
        self.motion.needs_resolve |= self.motion.frame_requested;
        result
    }

    pub(super) fn pause(&mut self, now: Instant) {
        self.motion.pause(now);
        for playback in self.animations.values_mut() {
            playback.advance(now);
            playback.active = false;
        }
    }

    pub(super) fn resume(&mut self, now: Instant) {
        self.motion.resume(now);
    }

    pub(super) fn advance_animations(&mut self, now: Instant) -> bool {
        let mut changed = self.motion.deadline_due(now);
        for playback in self.animations.values_mut() {
            changed |= playback.advance(now);
        }
        changed
    }

    pub(super) fn next_animation_deadline(&self) -> Option<Instant> {
        self.animations
            .values()
            .filter_map(AnimationPlayback::deadline)
            .chain(self.motion.deadline)
            .min()
    }

    pub(super) fn animation_counts(&self) -> (usize, usize) {
        (
            self.animations.len(),
            self.animations
                .values()
                .filter(|playback| playback.active)
                .count()
                + self.motion.active_count(),
        )
    }
}

impl DragPreview {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn paint(
        &mut self,
        scene: &mut Scene,
        renderer: &mut impl TextLayoutEngine,
        scale_factor: f32,
        animations_enabled: bool,
        reduce_motion: bool,
        paint_time: Instant,
        viewport: Rect,
        source_order: &mut usize,
    ) -> Result<(), UiError> {
        self.tree.refresh(
            Size::new(viewport.width, viewport.height),
            scale_factor,
            renderer,
            paint_time,
            animations_enabled,
            reduce_motion,
        )?;
        let origin = Point::new(
            self.position.x - self.cursor_offset.x,
            self.position.y - self.cursor_offset.y,
        );
        self.tree.paint(
            origin,
            PaintLayerKey {
                plane: crate::ScenePlane::Overlay,
                z_index: i16::MAX,
                group: 0,
            },
            scene,
            renderer,
            scale_factor,
            animations_enabled,
            reduce_motion,
            paint_time,
            viewport,
            source_order,
        )
    }
}

impl TooltipOverlay {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        target: ElementId,
        tooltip: &Tooltip,
        viewport: Size,
        scale_factor: f32,
        renderer: &mut impl TextLayoutEngine,
        now: Instant,
        animation_epoch: Instant,
        animations_enabled: bool,
        reduce_motion: bool,
    ) -> Result<Self, UiError> {
        let tree = DetachedTree::new(
            tooltip.content.as_ref().clone(),
            ElementId::new(0x7474_6970_6f6f_6c00),
            viewport,
            scale_factor,
            renderer,
            now,
            animation_epoch,
            animations_enabled,
            reduce_motion,
        )?;
        Ok(Self {
            target,
            content_identity: tooltip.content_identity(),
            placement: tooltip.placement,
            gap: tooltip.gap,
            viewport_margin: tooltip.viewport_margin,
            tree,
        })
    }

    pub(super) fn matches(&self, target: ElementId, tooltip: &Tooltip) -> bool {
        self.target == target && self.content_identity == tooltip.content_identity()
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn paint(
        &mut self,
        anchor: Rect,
        scene: &mut Scene,
        renderer: &mut impl TextLayoutEngine,
        scale_factor: f32,
        animations_enabled: bool,
        reduce_motion: bool,
        paint_time: Instant,
        viewport: Rect,
        source_order: &mut usize,
    ) -> Result<(), UiError> {
        self.tree.refresh(
            Size::new(viewport.width, viewport.height),
            scale_factor,
            renderer,
            paint_time,
            animations_enabled,
            reduce_motion,
        )?;
        let layout = *self.tree.root_layout()?;
        let placed = place_anchored(
            anchor,
            Size::new(layout.size.width, layout.size.height),
            viewport,
            self.placement,
            self.gap,
            self.viewport_margin,
        );
        let origin = Point::new(placed.x - layout.location.x, placed.y - layout.location.y);
        self.tree.paint(
            origin,
            PaintLayerKey {
                plane: crate::ScenePlane::Overlay,
                z_index: i16::MAX - 1,
                group: 0,
            },
            scene,
            renderer,
            scale_factor,
            animations_enabled,
            reduce_motion,
            paint_time,
            viewport,
            source_order,
        )
    }
}

/// How long a scroll container may keep receiving wheel deltas before it is considered settled.
///
/// Platforms that report gesture phases resolve at the momentum end phase instead; this bounded
/// delay is the fallback for plain wheels that carry no phase.
pub(crate) const SCROLL_SNAP_SETTLE_DELAY: Duration = Duration::from_millis(90);

/// Duration of the animation that carries a settled scroll container to its snap position.
pub(crate) const SCROLL_SNAP_DURATION: Duration = Duration::from_millis(220);

/// One scroll gesture waiting to be resolved into a snap position.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PendingScrollSnap {
    pub(crate) container: ElementId,
    /// Offset the gesture started from, used to honor `snap_stop_always` children.
    pub(crate) gesture_start: Vector,
    pub(crate) settle_at: Instant,
}

/// The in-flight animation carrying one container to its snap position.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollSnapAnimation {
    pub(crate) container: ElementId,
    pub(crate) from: Vector,
    pub(crate) to: Vector,
    pub(crate) start: Instant,
    pub(crate) end: Instant,
}

/// Retained scroll-snap state for one window.
///
/// A window has exactly one pointer, so it can only be mid-gesture in one container at a time:
/// this holds at most one pending settle and one running animation, and both are dropped the
/// moment they resolve, so a settled window keeps no timer alive.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ScrollSnapState {
    pub(crate) pending: Option<PendingScrollSnap>,
    pub(crate) animation: Option<ScrollSnapAnimation>,
}

impl ScrollSnapState {
    /// The next instant snapping must run.
    ///
    /// A pending gesture wakes exactly once, at its settle deadline. A running travel must present
    /// every frame until it lands, so it reports its own start instant, which is always already
    /// due; the travel clears itself at its end and leaves no deadline behind.
    pub(crate) fn deadline(&self) -> Option<Instant> {
        match (self.pending, self.animation) {
            (Some(pending), Some(animation)) => Some(pending.settle_at.min(animation.start)),
            (Some(pending), None) => Some(pending.settle_at),
            (None, Some(animation)) => Some(animation.start),
            (None, None) => None,
        }
    }
}

/// Smooth, decelerating progress for a snap animation.
fn scroll_snap_easing(phase: f32) -> f32 {
    let phase = phase.clamp(0.0, 1.0);
    1.0 - (1.0 - phase).powi(3)
}

impl UiTree {
    /// Note that a snapping container received scroll input and arm its settle deadline.
    ///
    /// The first call of a gesture records the offset the gesture began from so a
    /// `snap_stop_always` child can capture a fling that would otherwise pass over it.
    pub(super) fn arm_scroll_snap(
        &mut self,
        container: ElementId,
        gesture_start: Vector,
        now: Instant,
    ) {
        let gesture_start = self
            .scroll_snap
            .pending
            .filter(|pending| pending.container == container)
            .map_or(gesture_start, |pending| pending.gesture_start);
        self.scroll_snap.animation = None;
        self.scroll_snap.pending = Some(PendingScrollSnap {
            container,
            gesture_start,
            settle_at: now.checked_add(SCROLL_SNAP_SETTLE_DELAY).unwrap_or(now),
        });
    }

    /// Resolve any pending snap immediately, as at a native momentum end phase.
    ///
    /// Returns true when the retained scroll offsets changed and the window must repaint.
    pub fn scroll_gesture_ended(&mut self, now: Instant) -> bool {
        if let Some(pending) = &mut self.scroll_snap.pending {
            pending.settle_at = now;
        }
        self.advance_scroll_snap(now)
    }

    /// Snap the container that owns `id` to its nearest position without waiting.
    ///
    /// This is the keyboard and programmatic-scroll entry point: it starts the same bounded
    /// animation a gesture would, from the offset the caller scrolled to.
    pub fn snap_scroll_container(&mut self, id: ElementId, now: Instant) -> bool {
        let gesture_start = self.scroll_offsets.get(&id).copied().unwrap_or_default();
        self.scroll_snap.pending = Some(PendingScrollSnap {
            container: id,
            gesture_start,
            settle_at: now,
        });
        self.scroll_snap.animation = None;
        self.advance_scroll_snap(now)
    }

    /// The next instant at which scroll snapping must run, if any.
    pub(crate) fn next_scroll_snap_deadline(&self) -> Option<Instant> {
        self.scroll_snap.deadline()
    }

    /// Advance pending settles and the running snap animation.
    ///
    /// Returns true when a retained scroll offset changed. Both the pending settle and the
    /// animation clear themselves once resolved, so no idle source survives the gesture.
    pub(crate) fn advance_scroll_snap(&mut self, now: Instant) -> bool {
        let mut changed = false;
        if let Some(pending) = self
            .scroll_snap
            .pending
            .filter(|pending| pending.settle_at <= now)
        {
            self.scroll_snap.pending = None;
            changed |= self.begin_scroll_snap(pending, now);
        }
        if let Some(animation) = self.scroll_snap.animation {
            let offset = self.scroll_offsets.entry(animation.container).or_default();
            let total = animation
                .end
                .saturating_duration_since(animation.start)
                .as_secs_f32();
            let phase = if total <= 0.0 {
                1.0
            } else {
                (now.saturating_duration_since(animation.start).as_secs_f32() / total)
                    .clamp(0.0, 1.0)
            };
            let eased = scroll_snap_easing(phase);
            let next = Vector::new(
                animation.from.x + (animation.to.x - animation.from.x) * eased,
                animation.from.y + (animation.to.y - animation.from.y) * eased,
            );
            if *offset != next {
                *offset = next;
                changed = true;
            }
            if phase >= 1.0 {
                self.scroll_snap.animation = None;
            }
        }
        changed
    }

    /// Resolve one settled gesture into a snap target and start moving toward it.
    fn begin_scroll_snap(&mut self, pending: PendingScrollSnap, now: Instant) -> bool {
        let from = self
            .scroll_offsets
            .get(&pending.container)
            .copied()
            .unwrap_or_default();
        let Some(target) =
            self.scroll_snap_geometry
                .resolve(pending.container, pending.gesture_start, from)
        else {
            return false;
        };
        if !self.animations_enabled || self.reduce_motion {
            self.scroll_offsets.insert(pending.container, target);
            return true;
        }
        self.scroll_snap.animation = Some(ScrollSnapAnimation {
            container: pending.container,
            from,
            to: target,
            start: now,
            end: now.checked_add(SCROLL_SNAP_DURATION).unwrap_or(now),
        });
        false
    }
}
