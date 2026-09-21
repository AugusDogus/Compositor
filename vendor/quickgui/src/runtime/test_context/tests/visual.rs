use super::*;

struct EmptyVisualTestView;

impl View for EmptyVisualTestView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div()
    }
}

#[test]
fn oversized_visual_capture_fails_before_allocating_a_gpu() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(3_000.0, 32.0),
            EmptyVisualTestView,
        )
        .unwrap();
    assert!(matches!(
        cx.capture_screenshot(view.window_handle()),
        Err(TestAppError::Visual(
            crate::VisualTestError::InvalidDimensions
        ))
    ));
    assert!(cx.visual_renderer.is_none());
}

#[cfg(target_os = "macos")]
struct VisualTestView;

#[cfg(target_os = "macos")]
impl View for VisualTestView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(Color::rgb8(190, 30, 45))
            .child(
                div()
                    .id("visual-card")
                    .w(20.0)
                    .h(10.0)
                    .bg(Color::rgb8(20, 80, 210)),
            )
            .child(
                text("Visual")
                    .id("visual-label")
                    .text_sm()
                    .text_color(Color::WHITE),
            )
    }
}

#[cfg(target_os = "macos")]
#[test]
fn visual_context_uses_production_layout_and_offscreen_wgpu_capture() {
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::default().size(64.0, 48.0), VisualTestView)
        .unwrap();
    let window = view.window_handle();
    let mut visual = cx.visual(window).unwrap();
    visual
        .assert_element_bounds("visual-card", Rect::new(0.0, 0.0, 20.0, 10.0), 0.0)
        .unwrap();
    let label = visual.element_bounds("visual-label").unwrap();
    assert_eq!((label.x, label.y), (20.0, 0.0));
    assert!(label.width > 0.0 && label.height > 0.0);
    let first = visual.capture_screenshot().unwrap();
    let second = visual.capture_screenshot().unwrap();
    assert_eq!((first.width(), first.height()), (128, 96));
    assert_eq!(first.pixel(20, 10), Some([20, 80, 210, 255]));
    assert_eq!(first.pixel(127, 95), Some([190, 30, 45, 255]));
    assert_eq!(
        second
            .assert_matches(&first, crate::VisualTolerance::EXACT)
            .unwrap()
            .differing_pixels,
        0
    );
}

#[cfg(target_os = "macos")]
struct RightBorderVisualView;

#[cfg(target_os = "macos")]
impl View for RightBorderVisualView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex_row()
            .bg(Color::rgb8(237, 237, 238))
            .child(
                div()
                    .id("border-sidebar")
                    .w(20.0)
                    .h_full()
                    .flex_none()
                    .border_right(1.0, Color::rgb8(204, 204, 204)),
            )
            .child(
                div()
                    .id("border-content")
                    .flex_1()
                    .h_full()
                    .bg(Color::WHITE),
            )
    }
}

#[cfg(target_os = "macos")]
#[test]
fn right_border_stays_attached_to_its_fill_at_retina_scale() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(40.0, 20.0),
            RightBorderVisualView,
        )
        .unwrap();
    let mut visual = cx.visual(view.window_handle()).unwrap();
    visual
        .assert_element_bounds("border-sidebar", Rect::new(0.0, 0.0, 20.0, 20.0), 0.0)
        .unwrap();
    visual
        .assert_element_bounds("border-content", Rect::new(20.0, 0.0, 20.0, 20.0), 0.0)
        .unwrap();
    let snapshot = visual.capture_screenshot().unwrap();
    assert_eq!(snapshot.pixel(37, 20), Some([237, 237, 238, 255]));
    assert_eq!(snapshot.pixel(38, 20), Some([204, 204, 204, 255]));
    assert_eq!(snapshot.pixel(39, 20), Some([204, 204, 204, 255]));
    assert_eq!(snapshot.pixel(40, 20), Some([255, 255, 255, 255]));
}

#[cfg(target_os = "macos")]
struct WavyUnderlineVisualView;

#[cfg(target_os = "macos")]
impl View for WavyUnderlineVisualView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div().size_full().p_4().bg(Color::rgb8(8, 10, 14)).child(
            text("GPU WAVY UNDERLINE")
                .id("wavy-label")
                .text_size(24.0)
                .line_height(36.0)
                .text_color(Color::TRANSPARENT)
                .text_decoration_color(Color::rgb8(248, 113, 113))
                .text_decoration_2()
                .text_decoration_wavy()
                .whitespace_nowrap(),
        )
    }
}

#[cfg(target_os = "macos")]
#[test]
fn visual_wavy_underline_is_analytic_stable_and_not_a_solid_bar() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(280.0, 72.0),
            WavyUnderlineVisualView,
        )
        .unwrap();
    let mut visual = cx.visual(view.window_handle()).unwrap();
    let first = visual.capture_screenshot().unwrap();
    let second = visual.capture_screenshot().unwrap();
    assert_eq!(
        second
            .assert_matches(&first, crate::VisualTolerance::EXACT)
            .unwrap()
            .differing_pixels,
        0
    );

    let mut colored_pixels = 0_usize;
    let mut colored_rows = HashSet::new();
    let mut columns = HashMap::<u32, (u64, u32)>::new();
    for (index, pixel) in first.rgba().as_chunks::<4>().0.iter().enumerate() {
        if pixel[0] > 180
            && (70..=160).contains(&pixel[1])
            && (70..=160).contains(&pixel[2])
            && pixel[3] > 0
        {
            let x = index as u32 % first.width();
            let y = index as u32 / first.width();
            colored_pixels += 1;
            colored_rows.insert(y);
            let column = columns.entry(x).or_default();
            column.0 += u64::from(y);
            column.1 += 1;
        }
    }
    assert!(
        colored_pixels > 100,
        "the underline must paint a visible span"
    );
    assert!(
        colored_rows.len() >= 6,
        "a wavy underline must cover several physical rows"
    );
    let center_rows = columns
        .values()
        .filter(|(_, count)| *count > 0)
        .map(|(sum, count)| (sum / u64::from(*count)) as u32)
        .collect::<HashSet<_>>();
    assert!(
        center_rows.len() >= 4,
        "the underline center must vary across x instead of forming a solid bar"
    );
}

#[cfg(target_os = "macos")]
struct OpacityVisualView;

#[cfg(target_os = "macos")]
impl View for OpacityVisualView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let rich = || {
            crate::styled_text("RICH").with_highlights([(
                0..4,
                crate::HighlightStyle::default().color(Color::rgb8(248, 40, 72)),
            )])
        };
        div()
            .size_full()
            .flex_col()
            .bg(Color::BLACK)
            .child(
                div()
                    .id("opacity-reference")
                    .h(36.0)
                    .flex_none()
                    .text_size(24.0)
                    .line_height(32.0)
                    .child(rich()),
            )
            .child(
                div().h(36.0).flex_none().opacity(0.5).child(
                    div()
                        .id("opacity-target")
                        .size_full()
                        .opacity(0.5)
                        .hover(|style| style.opacity(1.0))
                        .transition(Duration::from_millis(100))
                        .text_size(24.0)
                        .line_height(32.0)
                        .child(rich()),
                ),
            )
    }
}

#[cfg(target_os = "macos")]
#[test]
fn subtree_opacity_multiplies_rich_text_and_transitions_without_reshaping() {
    fn maximum_red(snapshot: &crate::VisualSnapshot, bounds: Rect, scale: f32) -> u8 {
        let left = (bounds.x * scale).floor().max(0.0) as u32;
        let top = (bounds.y * scale).floor().max(0.0) as u32;
        let right = (bounds.right() * scale).ceil().min(snapshot.width() as f32) as u32;
        let bottom = (bounds.bottom() * scale)
            .ceil()
            .min(snapshot.height() as f32) as u32;
        (top..bottom)
            .flat_map(|y| (left..right).filter_map(move |x| snapshot.pixel(x, y)))
            .map(|pixel| pixel[0])
            .max()
            .unwrap_or(0)
    }

    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::default().size(96.0, 72.0), OpacityVisualView)
        .unwrap();
    let window = view.window_handle();
    let render_count = cx.render_count(window).unwrap();
    {
        let mut visual = cx.visual(window).unwrap();
        let reference_bounds = visual.element_bounds("opacity-reference").unwrap();
        let target_bounds = visual.element_bounds("opacity-target").unwrap();
        let scale = 2.0;

        let initial = visual.capture_screenshot().unwrap();
        let stable = visual.capture_screenshot().unwrap();
        stable
            .assert_matches(&initial, crate::VisualTolerance::EXACT)
            .unwrap();
        assert_eq!(
            visual
                .context
                .visual_renderer
                .as_ref()
                .unwrap()
                .last_reshaped_text_areas(),
            0
        );
        let reference_red = maximum_red(&initial, reference_bounds, scale);
        let initial_red = maximum_red(&initial, target_bounds, scale);
        assert!(reference_red > initial_red && initial_red > 40);

        assert!(visual.move_pointer(Point::new(12.0, 48.0)).unwrap());
        visual.capture_screenshot().unwrap();
        assert_eq!(
            visual
                .context
                .visual_renderer
                .as_ref()
                .unwrap()
                .last_reshaped_text_areas(),
            0
        );

        visual.advance_time(Duration::from_millis(50)).unwrap();
        let midpoint = visual.capture_screenshot().unwrap();
        let midpoint_red = maximum_red(&midpoint, target_bounds, scale);
        assert!(midpoint_red > initial_red);
        assert_eq!(
            visual
                .context
                .visual_renderer
                .as_ref()
                .unwrap()
                .last_reshaped_text_areas(),
            0
        );

        visual.advance_time(Duration::from_millis(50)).unwrap();
        let completed = visual.capture_screenshot().unwrap();
        let completed_red = maximum_red(&completed, target_bounds, scale);
        assert!(reference_red > completed_red && completed_red > midpoint_red);
        assert_eq!(
            visual
                .context
                .visual_renderer
                .as_ref()
                .unwrap()
                .last_reshaped_text_areas(),
            0
        );
    }
    assert_eq!(cx.render_count(window).unwrap(), render_count);
}

#[cfg(target_os = "macos")]
struct TextOverflowVisualView;

#[cfg(target_os = "macos")]
impl View for TextOverflowVisualView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex_col()
            .gap_2()
            .p_2()
            .bg(Color::rgb8(18, 19, 23))
            .text_color(Color::WHITE)
            .child(
                text("Unicode 🙂 alpha beta gamma delta epsilon zeta eta theta iota kappa")
                    .id("clamped-label")
                    .w(132.0)
                    .flex_none()
                    .text_sm()
                    .line_clamp(2)
                    .text_ellipsis(),
            )
            .child(
                text("/Users/example/a-very-long-directory/important-file.rs")
                    .id("middle-label")
                    .w(132.0)
                    .h(20.0)
                    .flex_none()
                    .text_sm()
                    .whitespace_nowrap()
                    .text_ellipsis_middle()
                    .overflow_hidden(),
            )
    }
}

#[cfg(target_os = "macos")]
#[test]
fn visual_text_overflow_clamps_layout_and_reuses_an_exact_frame() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(180.0, 100.0),
            TextOverflowVisualView,
        )
        .unwrap();
    let mut visual = cx.visual(view.window_handle()).unwrap();
    let clamped = visual.element_bounds("clamped-label").unwrap();
    assert_eq!((clamped.width, clamped.height), (132.0, 40.0));
    let middle = visual.element_bounds("middle-label").unwrap();
    assert_eq!((middle.width, middle.height), (132.0, 20.0));

    let first = visual.capture_screenshot().unwrap();
    let second = visual.capture_screenshot().unwrap();
    assert_eq!(
        second
            .assert_matches(&first, crate::VisualTolerance::EXACT)
            .unwrap()
            .differing_pixels,
        0
    );
}

#[cfg(target_os = "macos")]
struct TransitionVisualView;

#[cfg(target_os = "macos")]
impl View for TransitionVisualView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div()
            .id("transition-surface")
            .size_full()
            .bg(Color::BLACK)
            .hover(|style| style.bg(Color::WHITE).rounded(12.0))
            .transition(Duration::from_millis(100))
    }
}

#[cfg(target_os = "macos")]
#[test]
fn paint_only_hover_transitions_reverse_without_rebuilding_the_view() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(32.0, 32.0),
            TransitionVisualView,
        )
        .unwrap();
    let window = view.window_handle();
    let render_count = cx.render_count(window).unwrap();
    {
        let mut visual = cx.visual(window).unwrap();
        let initial = visual.capture_screenshot().unwrap();
        assert_eq!(initial.pixel(32, 32), Some([0, 0, 0, 255]));

        assert!(visual.move_pointer(Point::new(16.0, 16.0)).unwrap());
        let start = visual.capture_screenshot().unwrap();
        assert_eq!(start.pixel(32, 32), Some([0, 0, 0, 255]));

        visual.advance_time(Duration::from_millis(50)).unwrap();
        let midpoint = visual.capture_screenshot().unwrap();
        let midpoint_pixel = midpoint.pixel(32, 32).unwrap();
        assert!((186..=190).contains(&midpoint_pixel[0]));
        assert_eq!(midpoint_pixel[0], midpoint_pixel[1]);
        assert_eq!(midpoint_pixel[1], midpoint_pixel[2]);

        assert!(visual.move_pointer(Point::new(48.0, 48.0)).unwrap());
        let reversed = visual.capture_screenshot().unwrap();
        assert_eq!(reversed.pixel(32, 32), Some(midpoint_pixel));

        visual.advance_time(Duration::from_millis(100)).unwrap();
        let completed = visual.capture_screenshot().unwrap();
        assert_eq!(completed.pixel(32, 32), Some([0, 0, 0, 255]));
    }
    assert_eq!(cx.render_count(window).unwrap(), render_count);
}

#[cfg(target_os = "macos")]
struct DetachedTooltipAnimationVisualView;

#[cfg(target_os = "macos")]
impl View for DetachedTooltipAnimationVisualView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let tooltip = Tooltip::new(div().w(24.0).h(12.0).with_animation(
            "tooltip-fade",
            Animation::new(Duration::from_millis(100)),
            |element, phase| element.bg(Color::interpolate(Color::BLACK, Color::WHITE, phase)),
        ))
        .placement(AnchorPlacement::Bottom)
        .delay(Duration::ZERO)
        .gap(0.0)
        .viewport_margin(0.0);
        div()
            .size_full()
            .items_center()
            .justify_center()
            .bg(Color::BLACK)
            .child(
                div()
                    .id("detached-tooltip-trigger")
                    .w(24.0)
                    .h(12.0)
                    .tooltip(tooltip),
            )
    }
}

#[cfg(target_os = "macos")]
#[test]
fn detached_tooltip_motion_rebuilds_only_its_tree_and_drops_its_frame_source() {
    fn brightest(snapshot: &crate::VisualSnapshot) -> u8 {
        snapshot
            .rgba()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| pixel[0].max(pixel[1]).max(pixel[2]))
            .max()
            .unwrap_or(0)
    }

    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(64.0, 64.0),
            DetachedTooltipAnimationVisualView,
        )
        .unwrap();
    let window = view.window_handle();
    let render_count = cx.render_count(window).unwrap();
    {
        let mut visual = cx.visual(window).unwrap();
        let baseline = brightest(&visual.capture_screenshot().unwrap());
        assert!(baseline <= 4);
        assert!(visual.move_pointer(Point::new(32.0, 32.0)).unwrap());

        let start = visual.capture_screenshot().unwrap();
        assert_eq!(brightest(&start), baseline);
        assert!(
            visual
                .context
                .window(window)
                .unwrap()
                .ui
                .detached_animation_frame_requested()
        );

        visual.advance_time(Duration::from_millis(50)).unwrap();
        let midpoint = brightest(&visual.capture_screenshot().unwrap());
        assert!(
            (186..=190).contains(&midpoint),
            "unexpected tooltip midpoint brightness: {midpoint}"
        );

        visual.advance_time(Duration::from_millis(50)).unwrap();
        assert_eq!(brightest(&visual.capture_screenshot().unwrap()), 255);
        assert!(
            !visual
                .context
                .window(window)
                .unwrap()
                .ui
                .detached_animation_frame_requested()
        );

        assert!(visual.move_pointer(Point::new(80.0, 80.0)).unwrap());
        assert_eq!(brightest(&visual.capture_screenshot().unwrap()), baseline);
    }
    let ui = &cx.window(window).unwrap().ui;
    assert_eq!(ui.animation_counts().1, 0);
    assert_eq!(ui.next_animation_deadline(), None);
    assert!(!ui.detached_animation_frame_requested());
    assert_eq!(cx.render_count(window).unwrap(), render_count);
}

#[cfg(target_os = "macos")]
struct DetachedDragPreviewAnimationVisualView;

#[cfg(target_os = "macos")]
impl View for DetachedDragPreviewAnimationVisualView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div()
            .size_full()
            .items_center()
            .justify_center()
            .bg(Color::BLACK)
            .child(div().id("detached-drag-source").w(12.0).h(12.0))
    }
}

#[cfg(target_os = "macos")]
#[test]
fn detached_drag_preview_motion_drops_all_scheduling_when_cleared() {
    fn brightest(snapshot: &crate::VisualSnapshot) -> u8 {
        snapshot
            .rgba()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| pixel[0].max(pixel[1]).max(pixel[2]))
            .max()
            .unwrap_or(0)
    }

    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(64.0, 64.0),
            DetachedDragPreviewAnimationVisualView,
        )
        .unwrap();
    let window = view.window_handle();
    let render_count = cx.render_count(window).unwrap();
    {
        let mut visual = cx.visual(window).unwrap();
        let baseline = brightest(&visual.capture_screenshot().unwrap());
        assert!(baseline <= 4);
        let preview = div().w(16.0).h(12.0).with_animation(
            "drag-preview-fade",
            Animation::new(Duration::from_millis(100)),
            |element, phase| {
                element
                    .bg(Color::interpolate(Color::BLACK, Color::WHITE, phase))
                    .clickable()
            },
        );
        let now = visual.context.now();
        let mut renderer = visual.context.visual_renderer.take().unwrap();
        let installed = visual
            .context
            .window_mut(window)
            .unwrap()
            .ui
            .set_drag_preview(
                Some(preview),
                ElementId::named("detached-drag-source"),
                Point::new(32.0, 32.0),
                Point::new(16.0, 16.0),
                Some(Point::ZERO),
                &mut renderer,
                now,
            )
            .unwrap();
        visual.context.visual_renderer = Some(renderer);
        assert!(installed);

        assert_eq!(brightest(&visual.capture_screenshot().unwrap()), baseline);
        assert!(
            visual
                .context
                .window(window)
                .unwrap()
                .ui
                .detached_animation_frame_requested()
        );

        visual.advance_time(Duration::from_millis(50)).unwrap();
        let midpoint = brightest(&visual.capture_screenshot().unwrap());
        assert!((186..=190).contains(&midpoint));

        visual.advance_time(Duration::from_millis(50)).unwrap();
        assert_eq!(brightest(&visual.capture_screenshot().unwrap()), 255);
        assert!(
            visual
                .context
                .window_mut(window)
                .unwrap()
                .ui
                .clear_drag_preview()
        );
        assert_eq!(brightest(&visual.capture_screenshot().unwrap()), baseline);
    }
    let ui = &cx.window(window).unwrap().ui;
    assert_eq!(ui.animation_counts().1, 0);
    assert_eq!(ui.next_animation_deadline(), None);
    assert!(!ui.detached_animation_frame_requested());
    assert_eq!(cx.render_count(window).unwrap(), render_count);
}

#[cfg(target_os = "macos")]
struct VariableListVisualView {
    list: ListState,
}

#[cfg(target_os = "macos")]
impl View for VariableListVisualView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        self.list.set_viewport_size(100.0, 60.0);
        let rows = self
            .list
            .render_rows(self.list.visible_rows().range, |index| {
                div()
                    .id(ElementId::new(0x7000 + index as u64))
                    .h([20.0, 40.0, 30.0, 18.0, 26.0][index])
                    .bg(Color::rgb8(20 + index as u8 * 20, 80, 160))
            });
        div().size_full().child(
            div()
                .id("measured-list")
                .relative()
                .size(100.0, 60.0)
                .variable_virtual_scroll(&self.list)
                .child(rows),
        )
    }
}

#[cfg(target_os = "macos")]
#[test]
fn visual_context_converges_variable_list_measurements_without_view_or_idle_frames() {
    let list = ListState::new(5, 24.0).with_overscan(1);
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(100.0, 60.0),
            VariableListVisualView { list: list.clone() },
        )
        .unwrap();
    let window = view.window_handle();
    let initial_renders = cx.render_count(window).unwrap();
    let mut visual = cx.visual(window).unwrap();
    visual
        .assert_element_bounds(
            ElementId::new(0x7001),
            Rect::new(0.0, 20.0, 100.0, 40.0),
            0.0,
        )
        .unwrap();
    assert!(list.stats().measured_items >= 3);
    let converged_renders = cx.render_count(window).unwrap();
    assert_eq!(
        converged_renders, initial_renders,
        "mounted measurement convergence must not rerender the declarative view"
    );

    // Re-reading settled geometry performs layout on demand but does not rebuild the view.
    cx.element_bounds(window, ElementId::new(0x7001)).unwrap();
    assert_eq!(cx.render_count(window).unwrap(), converged_renders);
}

struct LoopView;

impl View for LoopView {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let focus = cx.focus_handle("loop");
        let listener = cx.action_listener("loop", |_view, _: &LoopForTest, cx| {
            cx.dispatch_action(LoopForTest);
        });
        div().track_focus(focus).auto_focus().on_action(listener)
    }
}

#[test]
fn recursive_effects_fail_at_a_bounded_turn_instead_of_hanging() {
    let (mut cx, view) = TestAppContext::new(LoopView).unwrap();
    assert!(matches!(
        cx.dispatch_action(view.window_handle(), LoopForTest),
        Err(TestAppError::EffectTurnLimit)
    ));
}

#[cfg(target_os = "macos")]
struct LinearGradientVisualView;

#[cfg(target_os = "macos")]
impl View for LinearGradientVisualView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div().size_full().id("gradient-band").bg_gradient(
            crate::Gradient::linear(
                crate::GradientDirection::ToRight,
                [Color::BLACK, Color::WHITE],
            )
            .color_space(crate::GradientColorSpace::Srgb),
        )
    }
}

#[cfg(target_os = "macos")]
#[test]
fn visual_linear_gradient_interpolates_monotonically_across_the_element() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(64.0, 8.0),
            LinearGradientVisualView,
        )
        .unwrap();
    let mut visual = cx.visual(view.window_handle()).unwrap();
    visual
        .assert_element_bounds("gradient-band", Rect::new(0.0, 0.0, 64.0, 8.0), 0.0)
        .unwrap();
    let snapshot = visual.capture_screenshot().unwrap();
    assert_eq!((snapshot.width(), snapshot.height()), (128, 16));

    let row = 8;
    let first = snapshot.pixel(0, row).unwrap();
    let last = snapshot.pixel(127, row).unwrap();
    assert!(
        first[0] < 12,
        "the first stop must stay near black: {first:?}"
    );
    assert!(
        last[0] > 243,
        "the last stop must stay near white: {last:?}"
    );
    // Encoded-sRGB interpolation reaches roughly half intensity at the midpoint.
    let middle = snapshot.pixel(64, row).unwrap();
    assert!(
        (i32::from(middle[0]) - 128).abs() <= 4,
        "the midpoint must be mid gray: {middle:?}"
    );
    let mut previous = 0_u8;
    for x in 0..snapshot.width() {
        let pixel = snapshot.pixel(x, row).unwrap();
        assert!(
            pixel[0] >= previous,
            "a left-to-right gradient must never darken at {x}"
        );
        assert_eq!(
            [pixel[1], pixel[2], pixel[3]],
            [pixel[0], pixel[0], 255],
            "a black-to-white ramp stays neutral and opaque at {x}"
        );
        previous = pixel[0];
    }
    // A second capture of the same settled scene is byte identical.
    let repeat = visual.capture_screenshot().unwrap();
    assert_eq!(
        repeat
            .assert_matches(&snapshot, crate::VisualTolerance::EXACT)
            .unwrap()
            .differing_pixels,
        0
    );
}

#[cfg(target_os = "macos")]
struct RadialConicGradientVisualView;

#[cfg(target_os = "macos")]
impl View for RadialConicGradientVisualView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex_row()
            .child(
                div()
                    .id("radial")
                    .w(32.0)
                    .h(32.0)
                    .flex_none()
                    .bg_radial_gradient_at(
                        crate::RadialGradientShape::Circle,
                        crate::GradientCenter::CENTER,
                        [Color::WHITE, Color::BLACK],
                    ),
            )
            .child(
                div()
                    .id("conic")
                    .w(32.0)
                    .h(32.0)
                    .flex_none()
                    .bg_conic_gradient(0.0, [Color::BLACK, Color::WHITE]),
            )
    }
}

#[cfg(target_os = "macos")]
#[test]
fn visual_radial_and_conic_gradients_use_distinct_geometry() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(64.0, 32.0),
            RadialConicGradientVisualView,
        )
        .unwrap();
    let mut visual = cx.visual(view.window_handle()).unwrap();
    visual
        .assert_element_bounds("radial", Rect::new(0.0, 0.0, 32.0, 32.0), 0.0)
        .unwrap();
    visual
        .assert_element_bounds("conic", Rect::new(32.0, 0.0, 32.0, 32.0), 0.0)
        .unwrap();
    let snapshot = visual.capture_screenshot().unwrap();

    // The radial gradient is brightest at its center and dark at the farthest corner.
    let center = snapshot.pixel(32, 32).unwrap();
    let corner = snapshot.pixel(1, 1).unwrap();
    assert!(center[0] > 240, "radial center must stay white: {center:?}");
    // Linear-light interpolation is the default, so the farthest corner is nearly black.
    assert!(corner[0] < 70, "radial corner must reach black: {corner:?}");
    let midway = snapshot.pixel(16, 16).unwrap();
    assert!(
        corner[0] < midway[0] && midway[0] < center[0],
        "a radial ramp must darken with distance: {center:?} {midway:?} {corner:?}"
    );
    // Distance, not axis, drives a circular radial gradient.
    let horizontal = snapshot.pixel(52, 32).unwrap();
    let vertical = snapshot.pixel(32, 12).unwrap();
    assert!(
        i32::from(horizontal[0]).abs_diff(i32::from(vertical[0])) <= 6,
        "a circle must be radially symmetric: {horizontal:?} {vertical:?}"
    );

    // The conic gradient sweeps clockwise from the top: up is the first stop, down the last.
    let up = snapshot.pixel(96, 12).unwrap();
    let down = snapshot.pixel(96, 52).unwrap();
    let right = snapshot.pixel(116, 32).unwrap();
    assert!(up[0] < 40, "the conic sweep must start dark: {up:?}");
    // Half a turn from the start is the midpoint of a two-stop sweep.
    assert!(
        down[0] > 150,
        "the conic sweep must advance half way opposite the start: {down:?}"
    );
    assert!(
        up[0] < right[0] && right[0] < down[0],
        "the conic sweep must advance clockwise: {up:?} {right:?} {down:?}"
    );
}

#[cfg(target_os = "macos")]
struct CornerRadiiVisualView;

#[cfg(target_os = "macos")]
impl View for CornerRadiiVisualView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div().size_full().bg(Color::WHITE).child(
            div()
                .id("corner-card")
                .size_full()
                .bg(Color::BLACK)
                .rounded_tl(16.0)
                .rounded_br(16.0),
        )
    }
}

#[cfg(target_os = "macos")]
#[test]
fn visual_per_corner_radii_round_only_the_declared_corners() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(32.0, 32.0),
            CornerRadiiVisualView,
        )
        .unwrap();
    let mut visual = cx.visual(view.window_handle()).unwrap();
    visual
        .assert_element_bounds("corner-card", Rect::new(0.0, 0.0, 32.0, 32.0), 0.0)
        .unwrap();
    let snapshot = visual.capture_screenshot().unwrap();
    assert_eq!((snapshot.width(), snapshot.height()), (64, 64));

    // Rounded corners reveal the white parent; square corners stay filled.
    assert_eq!(snapshot.pixel(1, 1), Some([255, 255, 255, 255]));
    assert_eq!(snapshot.pixel(62, 62), Some([255, 255, 255, 255]));
    // Sampled off the diagonal so the square corners are measured on their straight edges.
    assert_eq!(snapshot.pixel(62, 10), Some([0, 0, 0, 255]));
    assert_eq!(snapshot.pixel(10, 62), Some([0, 0, 0, 255]));
    // The rounded corners cut a visible arc rather than one antialiased pixel.
    assert_eq!(snapshot.pixel(6, 6), Some([255, 255, 255, 255]));
    assert_eq!(snapshot.pixel(57, 57), Some([255, 255, 255, 255]));
}

#[cfg(target_os = "macos")]
struct BorderStyleVisualView;

#[cfg(target_os = "macos")]
impl View for BorderStyleVisualView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(Color::BLACK)
            .flex_col()
            .child(
                div()
                    .id("solid-box")
                    .w_full()
                    .h(20.0)
                    .flex_none()
                    .border(2.0, Color::WHITE),
            )
            .child(
                div()
                    .id("dashed-box")
                    .w_full()
                    .h(20.0)
                    .flex_none()
                    .border(2.0, Color::WHITE)
                    .border_dashed(),
            )
            .child(
                div()
                    .id("dotted-box")
                    .w_full()
                    .h(20.0)
                    .flex_none()
                    .border(2.0, Color::WHITE)
                    .border_dotted(),
            )
    }
}

#[cfg(target_os = "macos")]
#[test]
fn visual_dashed_and_dotted_borders_leave_evenly_spaced_gaps() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(60.0, 60.0),
            BorderStyleVisualView,
        )
        .unwrap();
    let mut visual = cx.visual(view.window_handle()).unwrap();
    let snapshot = visual.capture_screenshot().unwrap();

    let bright_in_row = |row: u32| {
        (0..snapshot.width())
            .filter(|x| {
                snapshot
                    .pixel(*x, row)
                    .is_some_and(|pixel| pixel[0] > 200 && pixel[1] > 200 && pixel[2] > 200)
            })
            .count()
    };
    // The first physical row inside each box's top border.
    let solid = bright_in_row(1);
    let dashed = bright_in_row(41);
    let dotted = bright_in_row(81);
    assert_eq!(
        solid,
        snapshot.width() as usize,
        "a solid border is continuous"
    );
    assert!(
        (30..solid).contains(&dashed),
        "a dashed border must leave gaps: {dashed} of {solid}"
    );
    assert!(
        dotted < dashed,
        "dots are shorter than dashes: {dotted} vs {dashed}"
    );
    assert!(dotted > 10, "dots must still paint: {dotted}");
}

#[cfg(target_os = "macos")]
struct OutlineVisualView;

#[cfg(target_os = "macos")]
impl View for OutlineVisualView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div().size_full().bg(Color::WHITE).p(10.0).child(
            div()
                .id("outlined")
                .size_full()
                .bg(Color::BLACK)
                .outline(2.0, Color::rgb8(255, 0, 0))
                .outline_offset(2.0),
        )
    }
}

#[cfg(target_os = "macos")]
#[test]
fn visual_outlines_paint_outside_the_border_box_without_changing_layout() {
    let (mut cx, view) = Application::new()
        .into_test_context(WindowOptions::default().size(40.0, 40.0), OutlineVisualView)
        .unwrap();
    let mut visual = cx.visual(view.window_handle()).unwrap();
    // The outline never participates in layout.
    visual
        .assert_element_bounds("outlined", Rect::new(10.0, 10.0, 20.0, 20.0), 0.0)
        .unwrap();
    let snapshot = visual.capture_screenshot().unwrap();

    let row = 40;
    // Background, then the offset ring, then the gap, then the element itself.
    assert_eq!(snapshot.pixel(10, row), Some([255, 255, 255, 255]));
    assert_eq!(snapshot.pixel(13, row), Some([255, 0, 0, 255]));
    assert_eq!(snapshot.pixel(18, row), Some([255, 255, 255, 255]));
    assert_eq!(snapshot.pixel(25, row), Some([0, 0, 0, 255]));
    // The ring is symmetric on the opposite edge.
    assert_eq!(snapshot.pixel(66, row), Some([255, 0, 0, 255]));
}

#[cfg(target_os = "macos")]
fn quadrant_image() -> crate::Image {
    crate::Image::from_rgba(
        2,
        2,
        vec![
            255, 0, 0, 255, // top-left red
            0, 255, 0, 255, // top-right green
            0, 0, 255, 255, // bottom-left blue
            255, 255, 255, 255, // bottom-right white
        ],
    )
    .unwrap()
}

#[cfg(target_os = "macos")]
#[track_caller]
fn assert_pixel_near(snapshot: &crate::VisualSnapshot, x: u32, y: u32, expected: [u8; 4]) {
    let actual = snapshot
        .pixel(x, y)
        .expect("the pixel is inside the capture");
    for channel in 0..4 {
        assert!(
            actual[channel].abs_diff(expected[channel]) <= 8,
            "pixel ({x}, {y}) is {actual:?}, expected about {expected:?}"
        );
    }
}

#[cfg(target_os = "macos")]
struct BackgroundImageVisualView;

#[cfg(target_os = "macos")]
impl View for BackgroundImageVisualView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let image = quadrant_image();
        div()
            .size_full()
            .flex_row()
            .bg(Color::BLACK)
            .child(div().id("bg-scaled").w(17.0).h(17.0).flex_none().bg_image(
                image.clone(),
                crate::BackgroundSize::Fixed(17.0, 17.0),
                crate::BackgroundRepeat::NoRepeat,
                crate::BackgroundPosition::TOP_LEFT,
            ))
            .child(div().id("bg-tiled").w(16.0).h(16.0).flex_none().bg_image(
                image,
                crate::BackgroundSize::Fixed(3.0, 3.0),
                crate::BackgroundRepeat::Repeat,
                crate::BackgroundPosition::TOP_LEFT,
            ))
    }
}

#[cfg(target_os = "macos")]
#[test]
fn visual_background_images_scale_tile_and_stay_inside_the_element() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(35.0, 18.0),
            BackgroundImageVisualView,
        )
        .unwrap();
    let mut visual = cx.visual(view.window_handle()).unwrap();
    visual
        .assert_element_bounds("bg-scaled", Rect::new(0.0, 0.0, 17.0, 17.0), 0.0)
        .unwrap();
    visual
        .assert_element_bounds("bg-tiled", Rect::new(17.0, 0.0, 16.0, 16.0), 0.0)
        .unwrap();
    let snapshot = visual.capture_screenshot().unwrap();
    assert_eq!((snapshot.width(), snapshot.height()), (70, 36));

    // One tile stretched over the whole element keeps every source texel in its own quadrant.
    assert_pixel_near(&snapshot, 8, 8, [255, 0, 0, 255]);
    assert_pixel_near(&snapshot, 25, 8, [0, 255, 0, 255]);
    assert_pixel_near(&snapshot, 8, 25, [0, 0, 255, 255]);
    assert_pixel_near(&snapshot, 25, 25, [255, 255, 255, 255]);

    // Tiling repeats the same texel grid across the element without leaving it.
    assert_pixel_near(&snapshot, 35, 1, [255, 0, 0, 255]);
    assert_pixel_near(&snapshot, 38, 1, [0, 255, 0, 255]);
    assert_pixel_near(&snapshot, 35, 4, [0, 0, 255, 255]);
    assert_pixel_near(&snapshot, 41, 1, [255, 0, 0, 255]);
    assert_pixel_near(&snapshot, 44, 4, [255, 255, 255, 255]);
    // Nothing is painted past the element's own box.
    assert_eq!(snapshot.pixel(68, 35), Some([0, 0, 0, 255]));
    assert_eq!(snapshot.pixel(35, 35), Some([0, 0, 0, 255]));
}

#[cfg(target_os = "macos")]
struct RoundedBackgroundImageVisualView;

#[cfg(target_os = "macos")]
impl View for RoundedBackgroundImageVisualView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div().size_full().bg(Color::BLACK).child(
            div()
                .id("rounded-bg")
                .size_full()
                .rounded(8.0)
                .bg_image_cover(quadrant_image()),
        )
    }
}

#[cfg(target_os = "macos")]
#[test]
fn visual_background_images_respect_rounded_corners() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(16.0, 16.0),
            RoundedBackgroundImageVisualView,
        )
        .unwrap();
    let mut visual = cx.visual(view.window_handle()).unwrap();
    let snapshot = visual.capture_screenshot().unwrap();
    // The rounded corner masks the raster background back to the black parent.
    assert_eq!(snapshot.pixel(0, 0), Some([0, 0, 0, 255]));
    assert_eq!(snapshot.pixel(31, 31), Some([0, 0, 0, 255]));
    // A covering background still paints its source texels in the middle of the element.
    assert_pixel_near(&snapshot, 6, 6, [255, 0, 0, 255]);
    assert_pixel_near(&snapshot, 25, 25, [255, 255, 255, 255]);
}

#[cfg(target_os = "macos")]
struct RoundedBorderSeamVisualView;

#[cfg(target_os = "macos")]
impl View for RoundedBorderSeamVisualView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div().size_full().bg(Color::rgb8(220, 20, 20)).child(
            div()
                .id("seam-card")
                .size_full()
                .rounded(10.0)
                .bg(Color::WHITE)
                .border(4.0, Color::BLACK),
        )
    }
}

#[cfg(target_os = "macos")]
#[test]
fn visual_solid_rounded_borders_meet_their_fill_without_a_seam() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(40.0, 40.0),
            RoundedBorderSeamVisualView,
        )
        .unwrap();
    let mut visual = cx.visual(view.window_handle()).unwrap();
    let snapshot = visual.capture_screenshot().unwrap();

    // The red parent must never show through the boundary between the border and the fill.
    // The two outermost columns are the element's own antialiased outer edge.
    for x in 2..snapshot.width() - 2 {
        let pixel = snapshot.pixel(x, 40).unwrap();
        assert!(
            pixel[0].abs_diff(pixel[1]) <= 6 && pixel[1].abs_diff(pixel[2]) <= 6,
            "an opaque border and fill must stay neutral at x={x}: {pixel:?}"
        );
    }
    // The border and the fill are still both present on that row.
    assert_eq!(snapshot.pixel(2, 40), Some([0, 0, 0, 255]));
    assert_eq!(snapshot.pixel(40, 40), Some([255, 255, 255, 255]));
}

#[cfg(target_os = "macos")]
struct RoundedDashVisualView;

#[cfg(target_os = "macos")]
impl View for RoundedDashVisualView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div().size_full().bg(Color::BLACK).child(
            div()
                .id("dash-card")
                .size_full()
                .rounded(12.0)
                .bg_linear_gradient(
                    crate::GradientDirection::ToBottom,
                    [Color::rgb8(20, 20, 20), Color::rgb8(40, 40, 40)],
                )
                .border(3.0, Color::WHITE)
                .border_dashed(),
        )
    }
}

#[cfg(target_os = "macos")]
#[test]
fn visual_rounded_dashed_borders_wrap_the_corners_and_keep_gaps() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(60.0, 60.0),
            RoundedDashVisualView,
        )
        .unwrap();
    let mut visual = cx.visual(view.window_handle()).unwrap();
    let snapshot = visual.capture_screenshot().unwrap();

    let mut bright = 0_usize;
    let mut dark = 0_usize;
    // Walk the top border row and count lit and unlit samples.
    for x in 4..116 {
        let pixel = snapshot.pixel(x, 2).unwrap();
        if pixel[0] > 200 {
            bright += 1;
        } else if pixel[0] < 80 {
            dark += 1;
        }
    }
    assert!(bright > 20, "a dashed border must paint dashes: {bright}");
    assert!(dark > 20, "a dashed border must leave gaps: {dark}");

    // The rounded corners are part of the same evenly distributed pattern, so at least one
    // corner sample is lit and at least one is not.
    let corners = [
        snapshot.pixel(6, 6).unwrap(),
        snapshot.pixel(113, 6).unwrap(),
        snapshot.pixel(6, 113).unwrap(),
        snapshot.pixel(113, 113).unwrap(),
        snapshot.pixel(10, 4).unwrap(),
        snapshot.pixel(109, 4).unwrap(),
    ];
    assert!(
        corners.iter().any(|pixel| pixel[0] > 150),
        "dashes must continue around the corners: {corners:?}"
    );
}

#[cfg(target_os = "macos")]
struct ImageFilterVisualView;

#[cfg(target_os = "macos")]
impl View for ImageFilterVisualView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let tile = || {
            div().w(17.0).h(17.0).flex_none().bg_image(
                quadrant_image(),
                crate::BackgroundSize::Fixed(17.0, 17.0),
                crate::BackgroundRepeat::NoRepeat,
                crate::BackgroundPosition::TOP_LEFT,
            )
        };
        div()
            .size_full()
            .flex_row()
            .bg(Color::BLACK)
            .child(tile().id("filter-none"))
            .child(tile().id("filter-grayscale").grayscale(true))
            .child(tile().id("filter-invert").invert(1.0))
            .child(tile().id("filter-brightness").brightness(0.5))
    }
}

#[cfg(target_os = "macos")]
#[test]
fn visual_color_filters_apply_the_css_matrices_to_raster_content() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(68.0, 17.0),
            ImageFilterVisualView,
        )
        .unwrap();
    let mut visual = cx.visual(view.window_handle()).unwrap();
    visual
        .assert_element_bounds("filter-brightness", Rect::new(51.0, 0.0, 17.0, 17.0), 0.0)
        .unwrap();
    let snapshot = visual.capture_screenshot().unwrap();

    // The unfiltered source texel.
    assert_pixel_near(&snapshot, 8, 8, [255, 0, 0, 255]);
    // CSS grayscale is the luminance-preserving saturate(0) matrix on encoded sRGB.
    assert_pixel_near(&snapshot, 42, 8, [54, 54, 54, 255]);
    // Full inversion turns red into cyan.
    assert_pixel_near(&snapshot, 76, 8, [0, 255, 255, 255]);
    // Half brightness halves the encoded sRGB value.
    assert_pixel_near(&snapshot, 110, 8, [128, 0, 0, 255]);
    // Filters never leak outside their own element.
    assert_pixel_near(&snapshot, 8, 25, [0, 0, 255, 255]);
}

// ---------------------------------------------------------------------------------------------
// Compositing layers: transforms, subtree filters, backdrop effects, and blend modes.
//
// Every test here drives the same `Compositor::render_scene` pass sequencing the window renderer
// uses, through the headless offscreen target.
// ---------------------------------------------------------------------------------------------

#[cfg(target_os = "macos")]
struct RotatedBarVisualView {
    degrees: f32,
}

#[cfg(target_os = "macos")]
impl View for RotatedBarVisualView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div().size_full().bg(Color::WHITE).child(
            div()
                .id("bar")
                .absolute()
                .left(10.0)
                .top(15.0)
                .w(20.0)
                .h(10.0)
                .bg(Color::rgb8(220, 0, 0))
                .rotate_degrees(self.degrees),
        )
    }
}

#[cfg(target_os = "macos")]
#[test]
fn visual_rotation_moves_painted_pixels_without_moving_layout() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(40.0, 40.0),
            RotatedBarVisualView { degrees: 0.0 },
        )
        .unwrap();
    let upright = cx
        .visual(view.window_handle())
        .unwrap()
        .capture_screenshot()
        .unwrap();
    // Wide and short: filled across the middle row, empty above it.
    assert_pixel_near(&upright, 40, 40, [220, 0, 0, 255]);
    assert_pixel_near(&upright, 40, 24, [255, 255, 255, 255]);

    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(40.0, 40.0),
            RotatedBarVisualView { degrees: 90.0 },
        )
        .unwrap();
    let mut visual = cx.visual(view.window_handle()).unwrap();
    // Layout never moves: the element still reports its untransformed box.
    visual
        .assert_element_bounds("bar", Rect::new(10.0, 15.0, 20.0, 10.0), 0.0)
        .unwrap();
    let rotated = visual.capture_screenshot().unwrap();
    // A quarter turn about the element centre swaps the filled and empty samples.
    assert_pixel_near(&rotated, 24, 40, [255, 255, 255, 255]);
    assert_pixel_near(&rotated, 40, 24, [220, 0, 0, 255]);
}

#[cfg(target_os = "macos")]
struct HalfTurnVisualView {
    rotated: bool,
}

#[cfg(target_os = "macos")]
impl View for HalfTurnVisualView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let content = div()
            .size_full()
            .bg(Color::rgb8(250, 250, 250))
            .child(
                div()
                    .absolute()
                    .left(0.0)
                    .top(0.0)
                    .w(12.0)
                    .h(6.0)
                    .bg(Color::rgb8(20, 90, 220)),
            )
            .child(
                text("Ag")
                    .absolute()
                    .left(2.0)
                    .top(10.0)
                    .text_sm()
                    .text_color(Color::rgb8(10, 10, 10)),
            );
        if self.rotated {
            content.rotate_degrees(180.0)
        } else {
            content
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn visual_rotation_carries_glyphon_text_with_its_parent() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(32.0, 24.0),
            HalfTurnVisualView { rotated: false },
        )
        .unwrap();
    let upright = cx
        .visual(view.window_handle())
        .unwrap()
        .capture_screenshot()
        .unwrap();

    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(32.0, 24.0),
            HalfTurnVisualView { rotated: true },
        )
        .unwrap();
    let rotated = cx
        .visual(view.window_handle())
        .unwrap()
        .capture_screenshot()
        .unwrap();

    // A half turn about the centre of a full-window group maps every pixel centre onto another
    // pixel centre, so the rotated capture must be the point reflection of the upright one. Text
    // is rasterized by Glyphon into the group texture, so it travels with the shapes.
    let (width, height) = (rotated.width(), rotated.height());
    let mut differing = 0_u64;
    for y in 0..height {
        for x in 0..width {
            let expected = upright.pixel(width - 1 - x, height - 1 - y).unwrap();
            let actual = rotated.pixel(x, y).unwrap();
            if (0..4).any(|channel| actual[channel].abs_diff(expected[channel]) > 12) {
                differing += 1;
            }
        }
    }
    assert!(
        differing <= u64::from(width + height),
        "{differing} of {} pixels differ after a half turn",
        width * height
    );
    // The text really was drawn: below the blue rectangle the upright capture still has dark
    // glyph pixels, so the comparison above covered rasterized text and not only shapes.
    let dark_glyph_pixels = (20..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .filter(|(x, y)| upright.pixel(*x, *y).unwrap()[0] < 200)
        .count();
    assert!(
        dark_glyph_pixels > 0,
        "the upright capture has no dark text pixels below the shape"
    );
}

#[cfg(target_os = "macos")]
struct BlurVisualView {
    radius: f32,
}

#[cfg(target_os = "macos")]
impl View for BlurVisualView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div().size_full().bg(Color::WHITE).child(
            div()
                .id("blurred")
                .absolute()
                .left(0.0)
                .top(0.0)
                .w(20.0)
                .h(40.0)
                .bg(Color::BLACK)
                .blur(self.radius),
        )
    }
}

#[cfg(target_os = "macos")]
#[test]
fn visual_subtree_blur_softens_an_edge_over_a_bounded_support() {
    let sample = |radius: f32| {
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::default().size(40.0, 40.0),
                BlurVisualView { radius },
            )
            .unwrap();
        cx.visual(view.window_handle())
            .unwrap()
            .capture_screenshot()
            .unwrap()
    };
    let sharp = sample(0.0);
    // A hard edge: black up to x = 20 logical (40 physical), white after it.
    assert_pixel_near(&sharp, 38, 40, [0, 0, 0, 255]);
    assert_pixel_near(&sharp, 42, 40, [255, 255, 255, 255]);

    let blurred = sample(3.0);
    let left = blurred.pixel(38, 40).unwrap()[0];
    let right = blurred.pixel(42, 40).unwrap()[0];
    assert!(
        left > 8 && right < 247,
        "the blurred edge is still hard: left {left}, right {right}"
    );
    assert!(left < right, "the blur must brighten towards the outside");
    // Far outside the support the field is untouched.
    assert_pixel_near(&blurred, 78, 40, [255, 255, 255, 255]);
}

#[cfg(target_os = "macos")]
struct DropShadowVisualView;

#[cfg(target_os = "macos")]
impl View for DropShadowVisualView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div().size_full().bg(Color::WHITE).child(
            div()
                .id("card")
                .absolute()
                .left(8.0)
                .top(8.0)
                .w(16.0)
                .h(16.0)
                .bg(Color::rgb8(0, 0, 200))
                .drop_shadow(6.0, 6.0, 0.0, Color::rgb8(0, 160, 0)),
        )
    }
}

#[cfg(target_os = "macos")]
#[test]
fn visual_drop_shadow_follows_the_painted_alpha_behind_the_subtree() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(40.0, 40.0),
            DropShadowVisualView,
        )
        .unwrap();
    let snapshot = cx
        .visual(view.window_handle())
        .unwrap()
        .capture_screenshot()
        .unwrap();
    // The element itself is unchanged where it covers the shadow.
    assert_pixel_near(&snapshot, 32, 32, [0, 0, 200, 255]);
    // The offset, unblurred silhouette is painted behind it.
    assert_pixel_near(&snapshot, 56, 56, [0, 160, 0, 255]);
    // Neither the element nor its shadow leaks above and to the left.
    assert_pixel_near(&snapshot, 8, 8, [255, 255, 255, 255]);
}

#[cfg(target_os = "macos")]
struct BackdropVisualView {
    radius: f32,
}

#[cfg(target_os = "macos")]
impl View for BackdropVisualView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(Color::WHITE)
            .child(
                div()
                    .absolute()
                    .left(0.0)
                    .top(0.0)
                    .w(20.0)
                    .h(40.0)
                    .bg(Color::BLACK),
            )
            .child(
                div()
                    .id("glass")
                    .absolute()
                    .left(10.0)
                    .top(10.0)
                    .w(20.0)
                    .h(20.0)
                    .backdrop_blur(self.radius),
            )
    }
}

#[cfg(target_os = "macos")]
#[test]
fn visual_backdrop_blur_softens_only_what_is_painted_behind_it() {
    let sample = |radius: f32| {
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::default().size(40.0, 40.0),
                BackdropVisualView { radius },
            )
            .unwrap();
        cx.visual(view.window_handle())
            .unwrap()
            .capture_screenshot()
            .unwrap()
    };
    let plain = sample(0.0);
    assert_pixel_near(&plain, 38, 40, [0, 0, 0, 255]);
    assert_pixel_near(&plain, 42, 40, [255, 255, 255, 255]);

    let frosted = sample(3.0);
    let left = frosted.pixel(38, 40).unwrap()[0];
    let right = frosted.pixel(42, 40).unwrap()[0];
    assert!(
        left > 8 && right < 247,
        "the backdrop edge is still hard: left {left}, right {right}"
    );
    // Outside the element's own box the backdrop is untouched, even inside the blur support.
    assert_pixel_near(&frosted, 38, 8, [0, 0, 0, 255]);
    assert_pixel_near(&frosted, 42, 8, [255, 255, 255, 255]);
}

#[cfg(target_os = "macos")]
struct BlendVisualView {
    blend: crate::BlendMode,
}

#[cfg(target_os = "macos")]
impl View for BlendVisualView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div().size_full().bg(Color::rgb8(128, 128, 128)).child(
            div()
                .id("blended")
                .absolute()
                .left(0.0)
                .top(0.0)
                .w(20.0)
                .h(20.0)
                .bg(Color::rgb8(64, 192, 255))
                .blend_mode(self.blend),
        )
    }
}

#[cfg(target_os = "macos")]
#[test]
fn visual_blend_modes_evaluate_the_separable_css_formulas() {
    let sample = |blend: crate::BlendMode| {
        let (mut cx, view) = Application::new()
            .into_test_context(
                WindowOptions::default().size(40.0, 40.0),
                BlendVisualView { blend },
            )
            .unwrap();
        cx.visual(view.window_handle())
            .unwrap()
            .capture_screenshot()
            .unwrap()
            .pixel(20, 20)
            .unwrap()
    };
    // The source and the backdrop, in encoded sRGB, are (64, 192, 255) over (128, 128, 128).
    let expect = |mode: crate::BlendMode, blend: fn(f32, f32) -> f32| {
        let actual = sample(mode);
        for (channel, source) in [64.0_f32, 192.0, 255.0].into_iter().enumerate() {
            let source = source / 255.0;
            let backdrop = 128.0 / 255.0;
            let expected = (blend(source, backdrop) * 255.0).round() as i32;
            assert!(
                i32::from(actual[channel]).abs_diff(expected) <= 10,
                "{mode:?} channel {channel} is {} not about {expected}",
                actual[channel]
            );
        }
    };
    expect(crate::BlendMode::Normal, |source, _| source);
    expect(crate::BlendMode::Multiply, |source, backdrop| {
        source * backdrop
    });
    expect(crate::BlendMode::Screen, |source, backdrop| {
        source + backdrop - source * backdrop
    });
    expect(crate::BlendMode::Darken, |source, backdrop| {
        source.min(backdrop)
    });
    expect(crate::BlendMode::Lighten, |source, backdrop| {
        source.max(backdrop)
    });
    expect(crate::BlendMode::Difference, |source, backdrop| {
        (source - backdrop).abs()
    });
    expect(crate::BlendMode::Exclusion, |source, backdrop| {
        source + backdrop - 2.0 * source * backdrop
    });
    let hard_light = |source: f32, backdrop: f32| {
        if source <= 0.5 {
            2.0 * source * backdrop
        } else {
            1.0 - 2.0 * (1.0 - source) * (1.0 - backdrop)
        }
    };
    expect(crate::BlendMode::HardLight, hard_light);
    // Overlay is hard light with the operands swapped.
    expect(crate::BlendMode::Overlay, |source, backdrop| {
        if backdrop <= 0.5 {
            2.0 * source * backdrop
        } else {
            1.0 - 2.0 * (1.0 - source) * (1.0 - backdrop)
        }
    });
    expect(crate::BlendMode::ColorDodge, |source, backdrop| {
        if backdrop <= 0.0 {
            0.0
        } else if source >= 1.0 {
            1.0
        } else {
            (backdrop / (1.0 - source)).min(1.0)
        }
    });
    expect(crate::BlendMode::ColorBurn, |source, backdrop| {
        if backdrop >= 1.0 {
            1.0
        } else if source <= 0.0 {
            0.0
        } else {
            1.0 - ((1.0 - backdrop) / source).min(1.0)
        }
    });
}

#[cfg(target_os = "macos")]
struct SubtreeFilterVisualView;

#[cfg(target_os = "macos")]
impl View for SubtreeFilterVisualView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div().size_full().bg(Color::WHITE).child(
            div()
                .id("filtered")
                .absolute()
                .left(0.0)
                .top(0.0)
                .w(20.0)
                .h(20.0)
                .bg(Color::rgb8(220, 20, 20))
                .filters([crate::Filter::Grayscale(1.0), crate::Filter::Blur(0.001)]),
        )
    }
}

#[cfg(target_os = "macos")]
#[test]
fn visual_color_filters_reach_a_whole_subtree_once_it_is_a_group() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(40.0, 40.0),
            SubtreeFilterVisualView,
        )
        .unwrap();
    let snapshot = cx
        .visual(view.window_handle())
        .unwrap()
        .capture_screenshot()
        .unwrap();
    // A container background is not raster content, so without a group `grayscale` would not touch
    // it. The blur opens a group, and the whole chain then applies to the composited subtree.
    let pixel = snapshot.pixel(20, 20).unwrap();
    assert!(
        pixel[0].abs_diff(pixel[1]) <= 4 && pixel[1].abs_diff(pixel[2]) <= 4,
        "the subtree was not desaturated: {pixel:?}"
    );
    assert!(pixel[0] < 200, "the subtree lost its content: {pixel:?}");
}

/// A pointer target transformed inside its own compositing group.
///
/// Layout keeps the element at `(0, 0, 20, 10)` whatever the transform is. Paint and hit testing
/// both follow the transform, so pointer positions must be inverse-mapped through it.
#[cfg(target_os = "macos")]
struct TransformedPointerView {
    scaled: bool,
}

#[cfg(target_os = "macos")]
impl View for TransformedPointerView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let target = div()
            .id("target")
            .absolute()
            .left(0.0)
            .top(0.0)
            .w(20.0)
            .h(10.0)
            .bg(Color::rgb8(20, 20, 200))
            .cursor_pointer()
            .hover(|style| style.bg(Color::rgb8(200, 20, 20)));
        div().size_full().child(if self.scaled {
            target.scale_uniform(2.0)
        } else {
            target.rotate_degrees(90.0)
        })
    }
}

#[cfg(target_os = "macos")]
#[test]
fn pointer_positions_are_inverse_mapped_through_a_rotated_subtree() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(64.0, 48.0),
            TransformedPointerView { scaled: false },
        )
        .unwrap();
    let mut visual = cx.visual(view.window_handle()).unwrap();

    // Layout is untouched: the element still reports its declared box.
    visual
        .assert_element_bounds("target", Rect::new(0.0, 0.0, 20.0, 10.0), 0.0)
        .unwrap();

    // A quarter turn about the centre `(10, 5)` paints the target at `(5, -5, 10, 20)`.
    // Inside the rotated box but outside the layout box.
    assert_eq!(
        visual.cursor_style_at(Point::new(10.0, 12.0)).unwrap(),
        Some(CursorStyle::PointingHand)
    );
    // Inside the layout box but outside the rotated box.
    assert_eq!(visual.cursor_style_at(Point::new(18.0, 5.0)).unwrap(), None);
    // The fixed point of the rotation is inside both.
    assert_eq!(
        visual.cursor_style_at(Point::new(10.0, 5.0)).unwrap(),
        Some(CursorStyle::PointingHand)
    );

    // Hover follows the painted geometry too, and repaints the element's hover fill in place.
    assert!(visual.move_pointer(Point::new(10.0, 12.0)).unwrap());
    let hovered = visual.capture_screenshot().unwrap();
    assert_pixel_near(&hovered, 20, 24, [200, 20, 20, 255]);
    assert!(visual.move_pointer(Point::new(18.0, 5.0)).unwrap());
    let unhovered = visual.capture_screenshot().unwrap();
    assert_pixel_near(&unhovered, 20, 24, [20, 20, 200, 255]);
}

#[cfg(target_os = "macos")]
#[test]
fn pointer_positions_are_inverse_mapped_through_a_scaled_subtree() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(64.0, 48.0),
            TransformedPointerView { scaled: true },
        )
        .unwrap();
    let mut visual = cx.visual(view.window_handle()).unwrap();
    // Doubling about the centre `(10, 5)` grows the painted target to `(-10, -5, 40, 20)`.
    assert_eq!(
        visual.cursor_style_at(Point::new(28.0, 12.0)).unwrap(),
        Some(CursorStyle::PointingHand)
    );
    assert_eq!(
        visual.cursor_style_at(Point::new(34.0, 12.0)).unwrap(),
        None
    );
    assert_eq!(
        visual.cursor_style_at(Point::new(10.0, 18.0)).unwrap(),
        None
    );
}

#[cfg(target_os = "macos")]
struct NestedGroupVisualView;

#[cfg(target_os = "macos")]
impl View for NestedGroupVisualView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(Color::WHITE)
            .child(
                div()
                    .absolute()
                    .left(0.0)
                    .top(0.0)
                    .w(10.0)
                    .h(10.0)
                    .bg(Color::BLACK)
                    .blur(1.0),
            )
            .rotate_degrees(180.0)
    }
}

#[cfg(target_os = "macos")]
#[test]
fn visual_nested_groups_compose_recursively() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(40.0, 40.0),
            NestedGroupVisualView,
        )
        .unwrap();
    let snapshot = cx
        .visual(view.window_handle())
        .unwrap()
        .capture_screenshot()
        .unwrap();
    // The blurred child composites into its rotated parent's texture, and the parent's half turn
    // then carries it to the opposite corner.
    assert!(
        snapshot.pixel(70, 70).unwrap()[0] < 128,
        "the nested group did not land in the far corner: {:?}",
        snapshot.pixel(70, 70)
    );
    assert_pixel_near(&snapshot, 10, 10, [255, 255, 255, 255]);
}

struct TextShadowVisualView;

impl View for TextShadowVisualView {
    fn render(&mut self, _cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        div().size_full().bg(Color::rgb8(20, 20, 24)).p_4().child(
            text("Shadowed")
                .text_size(28.0)
                .text_color(Color::WHITE)
                .text_shadow(3.0, 3.0, 4.0, Color::rgb8(255, 40, 40)),
        )
    }
}

#[test]
fn visual_text_shadow_copies_render_through_the_gpu_text_system() {
    let (mut cx, view) = Application::new()
        .into_test_context(
            WindowOptions::default().size(240.0, 80.0),
            TextShadowVisualView,
        )
        .unwrap();
    let mut visual = cx.visual(view.window_handle()).unwrap();
    // Every blurred shadow sample is a paint-only copy of the run; the GPU text system must accept
    // all of them in one frame and keep the frame byte-identical when nothing changes.
    let first = visual.capture_screenshot().unwrap();
    let second = visual.capture_screenshot().unwrap();
    assert_eq!(
        second
            .assert_matches(&first, crate::VisualTolerance::EXACT)
            .unwrap()
            .differing_pixels,
        0
    );
    let shadow_pixels = first
        .rgba()
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[0] > 150 && pixel[1] < 120 && pixel[2] < 120)
        .count();
    assert!(
        shadow_pixels > 20,
        "the shadow copies must paint red pixels, got {shadow_pixels}"
    );
}
