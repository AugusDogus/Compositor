use super::*;

use crate::{Direction, SnapAlign, SnapStrictness, TextDirection};

fn painted(root: Element, viewport: Size) -> UiTree {
    let mut tree = UiTree::new();
    let mut renderer = TestTextLayout;
    tree.set_root(root, viewport, 1.0, &mut renderer).unwrap();
    let mut scene = Scene::new();
    tree.paint_at(&mut scene, &mut renderer, Instant::now())
        .unwrap();
    tree
}

fn repaint(tree: &mut UiTree) {
    let mut renderer = TestTextLayout;
    let mut scene = Scene::new();
    tree.paint_at(&mut scene, &mut renderer, Instant::now())
        .unwrap();
}

#[test]
fn rtl_mirrors_row_children_inside_the_parent_content_box() {
    let tree = painted(
        div()
            .size(200.0, 60.0)
            .rtl()
            .px(10.0)
            .child(div().id("first").size(40.0, 20.0))
            .child(div().id("second").size(30.0, 20.0)),
        Size::new(200.0, 60.0),
    );

    // The content box spans 10..190. Left to right the children would sit at 10 and 50; mirrored
    // they occupy the same widths measured inward from the content box's right edge.
    assert_eq!(
        tree.element_bounds("first".into()),
        Some(Rect::new(150.0, 0.0, 40.0, 20.0))
    );
    assert_eq!(
        tree.element_bounds("second".into()),
        Some(Rect::new(120.0, 0.0, 30.0, 20.0))
    );
}

#[test]
fn direction_is_inherited_and_can_be_overridden_by_a_descendant() {
    let tree = painted(
        div().size(200.0, 60.0).rtl().child(
            div()
                .id("inherited")
                .size(100.0, 40.0)
                .child(div().id("nested").size(20.0, 10.0))
                .child(
                    div()
                        .id("restored")
                        .ltr()
                        .size(40.0, 10.0)
                        .child(div().id("inner").size(10.0, 10.0)),
                ),
        ),
        Size::new(200.0, 60.0),
    );

    // The inherited row is mirrored inside the 200 wide root.
    assert_eq!(
        tree.element_bounds("inherited".into()),
        Some(Rect::new(100.0, 0.0, 100.0, 40.0))
    );
    // Inside it, the first child is mirrored to the right edge and the second sits left of it.
    assert_eq!(
        tree.element_bounds("nested".into()),
        Some(Rect::new(180.0, 0.0, 20.0, 10.0))
    );
    assert_eq!(
        tree.element_bounds("restored".into()),
        Some(Rect::new(140.0, 0.0, 40.0, 10.0))
    );
    // `ltr()` restores left-to-right placement for that subtree only.
    assert_eq!(
        tree.element_bounds("inner".into()),
        Some(Rect::new(140.0, 0.0, 10.0, 10.0))
    );
}

#[test]
fn rtl_hit_testing_follows_mirrored_geometry() {
    let mut tree = UiTree::new();
    let mut renderer = TestTextLayout;
    tree.set_root(
        div()
            .size(200.0, 40.0)
            .rtl()
            .child(button().id("primary").size(60.0, 40.0))
            .child(button().id("secondary").size(60.0, 40.0)),
        Size::new(200.0, 40.0),
        1.0,
        &mut renderer,
    )
    .unwrap();
    let mut scene = Scene::new();
    tree.paint_at(&mut scene, &mut renderer, Instant::now())
        .unwrap();

    assert_eq!(
        tree.interactive_region_at(Point::new(170.0, 20.0))
            .map(|region| region.id),
        Some("primary".into())
    );
    assert_eq!(
        tree.interactive_region_at(Point::new(110.0, 20.0))
            .map(|region| region.id),
        Some("secondary".into())
    );
}

#[test]
fn logical_padding_and_border_resolve_against_the_element_direction() {
    let mut tree = UiTree::new();
    let mut renderer = TestTextLayout;
    tree.set_root(
        div()
            .size(200.0, 60.0)
            .child(
                div()
                    .id("ltr-box")
                    .size(100.0, 30.0)
                    .ps(12.0)
                    .pe(4.0)
                    .border_s(3.0)
                    .border_e(1.0),
            )
            .child(
                div()
                    .id("rtl-box")
                    .rtl()
                    .size(100.0, 30.0)
                    .ps(12.0)
                    .pe(4.0)
                    .border_s(3.0)
                    .border_e(1.0),
            ),
        Size::new(200.0, 60.0),
        1.0,
        &mut renderer,
    )
    .unwrap();

    let root = tree.root.as_ref().unwrap();
    let ltr = tree
        .taffy
        .layout(root.children[0].taffy_node.unwrap())
        .unwrap();
    let rtl = tree
        .taffy
        .layout(root.children[1].taffy_node.unwrap())
        .unwrap();
    assert_eq!((ltr.padding.left, ltr.padding.right), (12.0, 4.0));
    assert_eq!((ltr.border.left, ltr.border.right), (3.0, 1.0));
    assert_eq!((rtl.padding.left, rtl.padding.right), (4.0, 12.0));
    assert_eq!((rtl.border.left, rtl.border.right), (1.0, 3.0));
}

#[test]
fn logical_text_alignment_and_shaping_direction_resolve_per_subtree() {
    let mut tree = UiTree::new();
    let mut renderer = TestTextLayout;
    tree.set_root(
        div()
            .child(text("start").id("ltr-default"))
            .child(text("end").id("ltr-end").text_end())
            .child(div().rtl().child(text("start").id("rtl-default")))
            .child(
                div()
                    .rtl()
                    .child(text("end").id("rtl-end").text_end())
                    .child(text("fixed").id("rtl-left").text_left()),
            ),
        Size::new(200.0, 200.0),
        1.0,
        &mut renderer,
    )
    .unwrap();

    let root = tree.root.as_ref().unwrap();
    let ltr_default = &root.children[0];
    let ltr_end = &root.children[1];
    let rtl_default = &root.children[2].children[0];
    let rtl_end = &root.children[3].children[0];
    let rtl_left = &root.children[3].children[1];

    assert_eq!(ltr_default.resolved_typography.align, TextAlign::Left);
    assert_eq!(ltr_end.resolved_typography.align, TextAlign::Right);
    assert_eq!(rtl_default.resolved_typography.align, TextAlign::Right);
    assert_eq!(rtl_end.resolved_typography.align, TextAlign::Left);
    assert_eq!(rtl_left.resolved_typography.align, TextAlign::Left);

    assert_eq!(
        ltr_default.resolved_typography.direction,
        TextDirection::Auto
    );
    assert_eq!(
        rtl_default.resolved_typography.direction,
        TextDirection::Rtl
    );
    assert_eq!(rtl_default.resolved_direction, Direction::Rtl);
}

#[test]
fn rtl_horizontal_scrolling_starts_at_the_right_edge_and_advances_leftward() {
    let mut tree = painted(
        div()
            .id("scroller")
            .rtl()
            .size(100.0, 40.0)
            .overflow_x_scroll()
            .child(div().id("head").size(100.0, 40.0).flex_none())
            .child(div().id("tail").size(200.0, 40.0).flex_none()),
        Size::new(100.0, 40.0),
    );

    // At the initial offset the inline start edge is the container's right edge, so the first
    // child is fully visible on the right and the rest hangs off to the left.
    assert_eq!(
        tree.element_bounds("head".into()),
        Some(Rect::new(0.0, 0.0, 100.0, 40.0))
    );
    assert_eq!(
        tree.element_bounds("tail".into()),
        Some(Rect::new(-200.0, 0.0, 200.0, 40.0))
    );

    // A physical rightward wheel delta moves along the inline axis, revealing content to the left.
    let result = tree.scroll_at(
        Some(Point::new(50.0, 20.0)),
        Vector::new(80.0, 0.0),
        Instant::now(),
    );
    assert!(result.changed);
    assert_eq!(
        tree.scroll_offset("scroller".into()),
        Some(Vector::new(80.0, 0.0))
    );
    repaint(&mut tree);
    assert_eq!(
        tree.element_bounds("head".into()),
        Some(Rect::new(80.0, 0.0, 100.0, 40.0))
    );
}

#[test]
fn sticky_headers_pin_to_the_scroll_viewport_and_release_at_their_section_end() {
    let section = |id: &'static str, header: &'static str| {
        div().id(id).w(100.0).h(200.0).flex_col().flex_none().child(
            div()
                .id(header)
                .w(100.0)
                .h(20.0)
                .flex_none()
                .sticky_top(0.0),
        )
    };
    let mut tree = painted(
        div()
            .id("scroller")
            .size(100.0, 100.0)
            .overflow_y_scroll()
            .flex_col()
            .child(section("section-a", "header-a"))
            .child(section("section-b", "header-b")),
        Size::new(100.0, 100.0),
    );

    assert_eq!(
        tree.element_bounds("header-a".into()),
        Some(Rect::new(0.0, 0.0, 100.0, 20.0))
    );

    // Scrolling inside the first section keeps its header pinned to the viewport top.
    tree.scroll_at(
        Some(Point::new(50.0, 50.0)),
        Vector::new(0.0, -60.0),
        Instant::now(),
    );
    repaint(&mut tree);
    assert_eq!(
        tree.element_bounds("header-a".into()),
        Some(Rect::new(0.0, 0.0, 100.0, 20.0))
    );
    assert_eq!(
        tree.element_bounds("section-a".into()),
        Some(Rect::new(0.0, -60.0, 100.0, 200.0))
    );

    // Past the end of its section the header is pushed out with the section it belongs to.
    tree.scroll_at(
        Some(Point::new(50.0, 50.0)),
        Vector::new(0.0, -130.0),
        Instant::now(),
    );
    repaint(&mut tree);
    assert_eq!(
        tree.element_bounds("section-a".into()),
        Some(Rect::new(0.0, -190.0, 100.0, 200.0))
    );
    assert_eq!(
        tree.element_bounds("header-a".into()),
        Some(Rect::new(0.0, -10.0, 100.0, 20.0))
    );
    // The next section's header has taken over the pinned position.
    assert_eq!(
        tree.element_bounds("header-b".into()),
        Some(Rect::new(0.0, 10.0, 100.0, 20.0))
    );
}

#[test]
fn sticky_hit_regions_follow_the_pinned_position() {
    let mut tree = painted(
        div()
            .id("scroller")
            .size(100.0, 100.0)
            .overflow_y_scroll()
            .flex_col()
            .child(
                div()
                    .id("body")
                    .w(100.0)
                    .h(400.0)
                    .flex_none()
                    .flex_col()
                    .child(
                        button()
                            .id("pinned")
                            .w(100.0)
                            .h(20.0)
                            .flex_none()
                            .sticky_top(4.0),
                    ),
            ),
        Size::new(100.0, 100.0),
    );

    tree.scroll_at(
        Some(Point::new(50.0, 50.0)),
        Vector::new(0.0, -100.0),
        Instant::now(),
    );
    repaint(&mut tree);

    assert_eq!(
        tree.element_bounds("pinned".into()),
        Some(Rect::new(0.0, 4.0, 100.0, 20.0))
    );
    assert_eq!(
        tree.interactive_region_at(Point::new(50.0, 10.0))
            .map(|region| region.id),
        Some("pinned".into())
    );
}

#[test]
fn sticky_headers_paint_and_receive_input_above_later_rows() {
    let mut tree = painted(
        div()
            .id("scroller")
            .size(100.0, 100.0)
            .overflow_y_scroll()
            .flex_col()
            .child(
                div().h(300.0).flex_none().flex_col().children([
                    button()
                        .id("header")
                        .size(100.0, 28.0)
                        .flex_none()
                        .bg(Color::rgb8(200, 0, 0))
                        .sticky_top(0.0)
                        .child("Header"),
                    button()
                        .id("row")
                        .size(100.0, 60.0)
                        .flex_none()
                        .bg(Color::rgb8(0, 0, 200))
                        .child("Row"),
                ]),
            ),
        Size::new(100.0, 100.0),
    );
    tree.scroll_offsets
        .insert("scroller".into(), Vector::new(0.0, 20.0));
    let mut scene = Scene::new();
    tree.paint(&mut scene, &mut TestTextLayout).unwrap();
    let layer_for = |color| {
        scene
            .paint_layers()
            .iter()
            .find(|layer| layer.edge_quads().iter().any(|quad| quad.fill == color))
            .expect("colored element must be painted")
            .key()
    };
    assert!(layer_for(Color::rgb8(200, 0, 0)) > layer_for(Color::rgb8(0, 0, 200)));
    assert_eq!(
        tree.interactive_region_at(Point::new(50.0, 12.0))
            .map(|region| region.id),
        Some("header".into()),
    );
}

#[test]
fn sticky_declarations_are_hard_bounded_per_window() {
    let mut root = div().size(100.0, 100.0);
    for _ in 0..=MAX_STICKY_ELEMENTS_PER_WINDOW {
        root = root.child(div().w(10.0).h(10.0).sticky_top(0.0));
    }
    let mut tree = UiTree::new();
    let mut renderer = TestTextLayout;
    let error = tree
        .set_root(root, Size::new(100.0, 100.0), 1.0, &mut renderer)
        .unwrap_err();

    assert!(matches!(error, UiError::TooManyStickyElements));
}

fn snap_carousel() -> Element {
    div()
        .id("carousel")
        .size(100.0, 60.0)
        .overflow_x_scroll()
        .scroll_snap_x(SnapStrictness::Mandatory)
        .child(
            div()
                .id("page-0")
                .size(100.0, 60.0)
                .flex_none()
                .snap_align(SnapAlign::Start),
        )
        .child(
            div()
                .id("page-1")
                .size(100.0, 60.0)
                .flex_none()
                .snap_align(SnapAlign::Start),
        )
        .child(
            div()
                .id("page-2")
                .size(100.0, 60.0)
                .flex_none()
                .snap_align(SnapAlign::Start),
        )
}

#[test]
fn a_settled_wheel_gesture_snaps_to_the_nearest_position_on_an_exact_deadline() {
    let start = Instant::now();
    let mut tree = painted(snap_carousel(), Size::new(100.0, 60.0));

    // A short flick that stops between two pages.
    tree.scroll_at(Some(Point::new(50.0, 30.0)), Vector::new(-60.0, 0.0), start);
    assert_eq!(
        tree.scroll_offset("carousel".into()),
        Some(Vector::new(60.0, 0.0))
    );

    let deadline = tree
        .next_scroll_snap_deadline()
        .expect("a settle is pending");
    assert_eq!(deadline, start + SCROLL_SNAP_SETTLE_DELAY);
    // Nothing happens before the settle deadline, and no repaint is requested.
    assert!(!tree.advance_scroll_snap(deadline - Duration::from_millis(1)));
    assert_eq!(
        tree.scroll_offset("carousel".into()),
        Some(Vector::new(60.0, 0.0))
    );

    // Settling starts the bounded travel toward the nearest snap position.
    assert!(!tree.advance_scroll_snap(deadline));
    assert_eq!(tree.next_scroll_snap_deadline(), Some(deadline));

    // The travel is presented frame by frame and eases toward the target.
    let midpoint = deadline + SCROLL_SNAP_DURATION / 2;
    assert!(tree.advance_scroll_snap(midpoint));
    let sampled = tree.scroll_offset("carousel".into()).unwrap().x;
    assert!(
        sampled > 60.0 && sampled < 100.0,
        "the eased sample {sampled} must lie between the gesture and the snap position"
    );

    let animation_end = deadline + SCROLL_SNAP_DURATION;
    assert!(tree.advance_scroll_snap(animation_end));
    assert_eq!(
        tree.scroll_offset("carousel".into()),
        Some(Vector::new(100.0, 0.0))
    );
    // The window is settled again: no snap deadline survives the gesture.
    assert_eq!(tree.next_scroll_snap_deadline(), None);
    assert!(!tree.advance_scroll_snap(animation_end + Duration::from_secs(1)));
}

#[test]
fn a_momentum_end_phase_resolves_the_snap_without_waiting_for_the_settle_delay() {
    let start = Instant::now();
    let mut tree = painted(snap_carousel(), Size::new(100.0, 60.0));
    tree.set_animations_enabled(false, start);

    tree.scroll_at(Some(Point::new(50.0, 30.0)), Vector::new(-30.0, 0.0), start);
    assert_eq!(
        tree.scroll_offset("carousel".into()),
        Some(Vector::new(30.0, 0.0))
    );

    // With animation disabled the resolved position is applied immediately.
    assert!(tree.scroll_gesture_ended(start));
    assert_eq!(tree.scroll_offset("carousel".into()), Some(Vector::ZERO));
    assert_eq!(tree.next_scroll_snap_deadline(), None);
}

#[test]
fn proximity_snapping_only_captures_a_gesture_that_stopped_nearby() {
    let start = Instant::now();
    let mut tree = painted(
        div()
            .id("carousel")
            .size(100.0, 60.0)
            .overflow_x_scroll()
            .scroll_snap_x(SnapStrictness::Proximity)
            .child(
                div()
                    .id("page-0")
                    .size(100.0, 60.0)
                    .flex_none()
                    .snap_align(SnapAlign::Start),
            )
            .child(div().id("gap").size(600.0, 60.0).flex_none())
            .child(
                div()
                    .id("page-1")
                    .size(100.0, 60.0)
                    .flex_none()
                    .snap_align(SnapAlign::Start),
            ),
        Size::new(100.0, 60.0),
    );
    tree.set_animations_enabled(false, start);

    // 300 logical pixels from either snap position is outside the proximity window.
    tree.scroll_at(
        Some(Point::new(50.0, 30.0)),
        Vector::new(-300.0, 0.0),
        start,
    );
    assert!(!tree.scroll_gesture_ended(start));
    assert_eq!(
        tree.scroll_offset("carousel".into()),
        Some(Vector::new(300.0, 0.0))
    );

    // Stopping just short of the second page is inside it.
    tree.scroll_at(
        Some(Point::new(50.0, 30.0)),
        Vector::new(-370.0, 0.0),
        start,
    );
    assert!(tree.scroll_gesture_ended(start));
    assert_eq!(
        tree.scroll_offset("carousel".into()),
        Some(Vector::new(700.0, 0.0))
    );
}

#[test]
fn snap_stop_always_captures_a_gesture_that_would_fly_past_it() {
    let start = Instant::now();
    let mut tree = painted(
        div()
            .id("carousel")
            .size(100.0, 60.0)
            .overflow_y_scroll()
            .flex_col()
            .scroll_snap_y(SnapStrictness::Mandatory)
            .child(
                div()
                    .id("row-0")
                    .size(100.0, 60.0)
                    .flex_none()
                    .snap_align(SnapAlign::Start),
            )
            .child(
                div()
                    .id("row-1")
                    .size(100.0, 60.0)
                    .flex_none()
                    .snap_align(SnapAlign::Start)
                    .snap_stop_always(),
            )
            .child(
                div()
                    .id("row-2")
                    .size(100.0, 60.0)
                    .flex_none()
                    .snap_align(SnapAlign::Start),
            )
            .child(
                div()
                    .id("row-3")
                    .size(100.0, 60.0)
                    .flex_none()
                    .snap_align(SnapAlign::Start),
            ),
        Size::new(100.0, 60.0),
    );
    tree.set_animations_enabled(false, start);

    // A fling from the top all the way to the last row must stop on the blocking row instead.
    tree.scroll_at(
        Some(Point::new(50.0, 30.0)),
        Vector::new(0.0, -175.0),
        start,
    );
    assert!(tree.scroll_gesture_ended(start));
    assert_eq!(
        tree.scroll_offset("carousel".into()),
        Some(Vector::new(0.0, 60.0))
    );
}

#[test]
fn snap_geometry_and_animation_state_stay_bounded() {
    let tree = painted(snap_carousel(), Size::new(100.0, 60.0));

    assert_eq!(tree.scroll_snap_geometry.containers.len(), 1);
    assert_eq!(tree.scroll_snap_geometry.points.len(), 3);
    assert!(tree.scroll_snap_geometry.containers.len() <= MAX_SCROLL_SNAP_CONTAINERS_PER_WINDOW);
    assert!(tree.scroll_snap_geometry.points.len() <= MAX_SCROLL_SNAP_POINTS_PER_WINDOW);
    assert!(tree.scroll_snap.pending.is_none());
    assert!(tree.scroll_snap.animation.is_none());
}
