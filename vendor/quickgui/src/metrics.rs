use web_time::{Duration, Instant};

/// Work performed between presented frames. Durations are elapsed time for each phase, while
/// [`FrameMetrics::cpu_time`] measures application-thread CPU time across the frame as a whole.
/// Mutation time includes retained updates received before the redraw callback starts.
#[derive(Clone, Copy, Debug, Default)]
pub struct PipelineMetrics {
    pub mutation_time: Duration,
    pub declaration_time: Duration,
    pub reconciliation_time: Duration,
    pub layout_time: Duration,
    pub geometry_time: Duration,
    pub paint_time: Duration,
    pub accessibility_time: Duration,
    /// Renderer preparation, upload, submission, and any presentation wait.
    pub render_time: Duration,
    pub reconciled_nodes: usize,
    pub layout_passes: usize,
    pub measured_nodes: usize,
    pub geometry_nodes: usize,
    pub painted_nodes: usize,
    pub reused_subtrees: usize,
    pub cached_paint_bytes: usize,
}

/// Renderer work submitted for the most recently completed frame.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RenderStats {
    /// Bytes written to retained primitive GPU buffers this frame, excluding texture uploads.
    pub uploaded_buffer_bytes: u64,
    pub buffer_write_calls: usize,
    pub reused_buffers: usize,
    /// Bounded CPU shadows used to compare rotating physical GPU buffers.
    pub upload_shadow_bytes: usize,
    pub quads: usize,
    /// Analytic drop and inset shadows submitted with the instanced shape draw.
    pub shadows: usize,
    pub images: usize,
    /// Images uploaded to a GPU texture during this frame.
    pub image_uploads: usize,
    /// Decoded RGBA bytes retained in the renderer's bounded texture cache.
    pub gpu_image_cache_bytes: u64,
    /// Decoded RGBA bytes retained by asynchronous image resources on the CPU.
    pub cpu_image_cache_bytes: u64,
    pub image_resource_entries: usize,
    pub image_resources_loading: usize,
    pub image_resources_failed: usize,
    pub animated_images: usize,
    pub active_animations: usize,
    pub svgs: usize,
    /// SVG masks rasterized and uploaded during this frame.
    pub svg_rasterizations: usize,
    /// One-channel alpha bytes retained in the renderer's bounded SVG cache.
    pub gpu_svg_cache_bytes: u64,
    pub paths: usize,
    /// De-indexed tessellated path vertices uploaded during this frame.
    pub path_vertices: usize,
    /// Visible paths omitted because the bounded per-frame GPU path budget was exhausted.
    pub skipped_paths: usize,
    /// Visible application-WGSL rectangles submitted through the instanced custom pipeline.
    pub custom_shader_instances: usize,
    /// New custom shader pipelines compiled during this frame.
    pub custom_shader_compilations: usize,
    /// Custom pipelines retained by this window's bounded cache.
    pub cached_custom_shader_pipelines: usize,
    /// Visible custom rectangles omitted by the per-frame shader or instance limits.
    pub skipped_custom_shader_instances: usize,
    pub text_areas: usize,
    pub draw_calls: usize,
    /// Text buffers whose content, metrics, or wrapping changed this frame.
    pub reshaped_text_areas: usize,
    /// Stable element-addressed text buffers retained by this window after eviction.
    pub retained_text_areas: usize,
    /// Content/style-addressed text layouts retained by this window after eviction.
    pub retained_text_layouts: usize,
    /// Glyphon renderers retained for painter-order-separated text batches.
    pub retained_text_renderers: usize,
    pub cached_text_areas: usize,
    /// Compositing groups drawn from their own offscreen texture this frame.
    pub compositing_layers: usize,
    /// Offscreen group passes recorded this frame.
    pub layer_passes: usize,
    /// Stable group textures composited without redrawing their content.
    pub reused_compositing_layers: usize,
    /// Separable Gaussian passes recorded for subtree, drop-shadow, and backdrop blurs.
    pub blur_passes: usize,
    /// Offscreen compositing bytes retained by this window, bounded by
    /// [`MAX_LAYER_TEXTURE_BYTES`](crate::MAX_LAYER_TEXTURE_BYTES).
    pub layer_texture_bytes: u64,
    /// Declared layer effects painted without their effect because a bound was reached.
    pub skipped_layer_effects: usize,
}

/// Lightweight CPU-side frame telemetry. It is intentionally allocation-free.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameMetrics {
    pub frame_number: u64,
    /// CPU time consumed by the application thread while preparing and submitting this frame.
    /// Unix targets use the monotonic per-thread CPU clock, so FIFO presentation waits are not
    /// counted as work. Other targets currently fall back to elapsed wall time.
    pub cpu_time: Duration,
    pub smoothed_cpu_time: Duration,
    /// Elapsed wall time spent preparing and submitting this frame, including any surface wait.
    pub frame_time: Duration,
    pub smoothed_frame_time: Duration,
    pub render: RenderStats,
    pub pipeline: PipelineMetrics,
}

impl FrameMetrics {
    pub fn cpu_milliseconds(self) -> f64 {
        self.cpu_time.as_secs_f64() * 1_000.0
    }

    pub fn smoothed_cpu_milliseconds(self) -> f64 {
        self.smoothed_cpu_time.as_secs_f64() * 1_000.0
    }

    pub fn frame_milliseconds(self) -> f64 {
        self.frame_time.as_secs_f64() * 1_000.0
    }

    pub fn smoothed_frame_milliseconds(self) -> f64 {
        self.smoothed_frame_time.as_secs_f64() * 1_000.0
    }
}

#[derive(Debug, Default)]
pub(crate) struct MetricsTracker {
    metrics: FrameMetrics,
}

impl MetricsTracker {
    pub fn current(&self) -> FrameMetrics {
        self.metrics
    }

    pub fn record(
        &mut self,
        elapsed: FrameElapsed,
        render: RenderStats,
        pipeline: PipelineMetrics,
    ) {
        let (smoothed_cpu_time, smoothed_frame_time) = if self.metrics.frame_number == 0 {
            (elapsed.cpu_time, elapsed.frame_time)
        } else {
            // An exponential moving average settles quickly without storing a sample ring.
            (
                self.metrics.smoothed_cpu_time.mul_f64(0.9) + elapsed.cpu_time.mul_f64(0.1),
                self.metrics.smoothed_frame_time.mul_f64(0.9) + elapsed.frame_time.mul_f64(0.1),
            )
        };
        self.metrics = FrameMetrics {
            frame_number: self.metrics.frame_number + 1,
            cpu_time: elapsed.cpu_time,
            smoothed_cpu_time,
            frame_time: elapsed.frame_time,
            smoothed_frame_time,
            render,
            pipeline,
        };
    }
}

pub(crate) struct FrameTimer {
    wall_time: Instant,
    thread_cpu_time: Option<Duration>,
}

pub(crate) struct FrameElapsed {
    cpu_time: Duration,
    frame_time: Duration,
}

impl FrameTimer {
    pub(crate) fn start() -> Self {
        Self {
            wall_time: Instant::now(),
            thread_cpu_time: thread_cpu_time(),
        }
    }

    pub(crate) fn elapsed(self) -> FrameElapsed {
        let frame_time = self.wall_time.elapsed();
        let cpu_time = self
            .thread_cpu_time
            .zip(thread_cpu_time())
            .and_then(|(started, finished)| finished.checked_sub(started))
            .unwrap_or(frame_time);
        FrameElapsed {
            cpu_time,
            frame_time,
        }
    }
}

#[cfg(unix)]
fn thread_cpu_time() -> Option<Duration> {
    let mut timestamp = std::mem::MaybeUninit::<libc::timespec>::uninit();
    // SAFETY: `timestamp` points to writable storage for one `timespec`; clock_gettime initializes
    // it on success and does not retain the pointer.
    if unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, timestamp.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: a zero return from clock_gettime guarantees that it initialized the timespec.
    let timestamp = unsafe { timestamp.assume_init() };
    let seconds = u64::try_from(timestamp.tv_sec).ok()?;
    let nanoseconds = u32::try_from(timestamp.tv_nsec).ok()?;
    (nanoseconds < 1_000_000_000).then(|| Duration::new(seconds, nanoseconds))
}

#[cfg(not(unix))]
fn thread_cpu_time() -> Option<Duration> {
    None
}
