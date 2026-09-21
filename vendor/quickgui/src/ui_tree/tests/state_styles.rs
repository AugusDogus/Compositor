use super::*;

/// The box an element paints at `x`, if it paints one at all: an unrevealed member has a
/// transparent fill and no border, so the scene holds no quad for it.
fn quad_at(scene: &Scene, x: f32) -> Option<&EdgeQuad> {
    scene
        .edge_quads()
        .iter()
        .find(|quad| quad.rect.x <= x && x < quad.rect.x + quad.rect.width)
}

fn fill_at(scene: &Scene, x: f32) -> Option<Color> {
    quad_at(scene, x).map(|quad| quad.fill)
}

/// The fill of the box painted exactly at `rect`, for nested boxes that share an `x`.
fn fill_of(scene: &Scene, rect: Rect) -> Option<Color> {
    scene
        .edge_quads()
        .iter()
        .find(|quad| quad.rect == rect)
        .map(|quad| quad.fill)
}

#[test]
fn group_focus_tracks_keyboard_modality_without_changing_layout_or_tab_order() {
    let control = ElementId::new(910);
    let next = ElementId::new(911);
    let accent = Color::rgb8(89, 147, 211);
    let root = div().size(200.0, 40.0).flex_row().children([
        button()
            .id(control)
            .size(100.0, 40.0)
            .clickable()
            .group()
            .children([
                div()
                    .size(20.0, 20.0)
                    .group_focus(|s| s.bg(accent))
                    .children((0..64).map(|_| div())),
                // A nested group must not inherit the outer control's focus.
                div()
                    .size(20.0, 20.0)
                    .group()
                    .child(div().size(20.0, 20.0).group_focus(|s| s.bg(accent))),
            ]),
        button().id(next).size(100.0, 40.0).clickable(),
    ]);
    let mut tree = UiTree::new();
    let mut renderer = TestTextLayout;
    let mut scene = Scene::new();
    tree.set_root(root, Size::new(200.0, 40.0), 1.0, &mut renderer)
        .unwrap();
    tree.paint(&mut scene, &mut renderer).unwrap();
    let now = Instant::now();
    let point = Point::new(70.0, 20.0);
    tree.pointer_button(Some(point), true, false, now, &mut renderer);
    tree.pointer_button(Some(point), false, false, now, &mut renderer);
    assert_eq!(tree.focused(), Some(control));
    let geometry = tree.element_bounds(control);
    for (reveal, expected) in [(false, 0), (true, 1)] {
        if reveal {
            assert!(tree.reveal_focus());
        }
        // A second paint exercises cached subtrees too.
        for _ in 0..2 {
            scene.clear(Color::TRANSPARENT);
            tree.paint(&mut scene, &mut renderer).unwrap();
            assert_eq!(
                scene
                    .edge_quads()
                    .iter()
                    .filter(|q| q.fill == accent)
                    .count(),
                expected
            );
            assert_eq!(tree.element_bounds(control), geometry);
        }
    }
    assert!(tree.focus_next(false));
    assert_eq!(tree.focused(), Some(next));
    scene.clear(Color::TRANSPARENT);
    tree.paint(&mut scene, &mut renderer).unwrap();
    assert!(!scene.edge_quads().iter().any(|q| q.fill == accent));
    assert!(tree.focus_next(true));
    assert_eq!(tree.focused(), Some(control));
    tree.pointer_button(Some(point), true, false, now, &mut renderer);
    tree.pointer_button(Some(point), false, false, now, &mut renderer);
    scene.clear(Color::TRANSPARENT);
    tree.paint(&mut scene, &mut renderer).unwrap();
    assert!(!scene.edge_quads().iter().any(|q| q.fill == accent));
}

#[test]
fn group_hover_paints_members_while_their_group_is_hovered() {
    let row = ElementId::new(120);
    let action = ElementId::new(121);
    let revealed = Color::rgb8(37, 99, 235);
    let own_hover = Color::rgb8(220, 38, 38);
    let declaration = div()
        .id(row)
        .size(200.0, 40.0)
        .flex_row()
        .group()
        .children([
            div().size(160.0, 40.0).flex_none(),
            button()
                .id(action)
                .size(40.0, 40.0)
                .flex_none()
                .clickable()
                .group_hover(|style| style.bg(revealed).rounded(8.0))
                .hover(|style| style.bg(own_hover)),
        ]);
    let viewport = Size::new(200.0, 40.0);
    let now = Instant::now();
    let mut tree = UiTree::new();
    let mut renderer = TestTextLayout;
    let mut scene = Scene::new();

    tree.set_root(declaration, viewport, 1.0, &mut renderer)
        .unwrap();
    tree.paint_at(&mut scene, &mut renderer, now).unwrap();
    // Nothing is hovered: the action paints no fill at all.
    assert!(quad_at(&scene, 180.0).is_none());
    // The group registers a stateful hit region even though it paints nothing stateful itself.
    assert!(
        tree.hit_regions
            .iter()
            .any(|region| region.id == row && region.stateful)
    );

    // Over the plain label, well away from the action: the group is hovered, so the action
    // reveals itself with the group-hover fill and radius.
    assert!(tree.pointer_moved(Point::new(40.0, 20.0), &mut renderer));
    assert!(tree.hovered.contains(&row));
    assert!(!tree.hovered.contains(&action));
    scene.clear(Color::TRANSPARENT);
    tree.paint_at(&mut scene, &mut renderer, now).unwrap();
    let quad = quad_at(&scene, 180.0).expect("a revealed action paints its group-hover fill");
    assert_eq!(quad.fill, revealed);
    assert_eq!(quad.radius, Corners::all(8.0));

    // Over the action itself: its own hover wins the fill it declares, and the group-hover radius
    // it does not declare stays, exactly as `.group:hover .action` and `.action:hover` cascade.
    assert!(tree.pointer_moved(Point::new(180.0, 20.0), &mut renderer));
    assert!(tree.hovered.contains(&row));
    assert!(tree.hovered.contains(&action));
    scene.clear(Color::TRANSPARENT);
    tree.paint_at(&mut scene, &mut renderer, now).unwrap();
    let quad = quad_at(&scene, 180.0).expect("a hovered action paints its own fill");
    assert_eq!(quad.fill, own_hover);
    assert_eq!(quad.radius, Corners::all(8.0));

    // The pointer leaves the window: the reveal is withdrawn again.
    assert!(tree.pointer_left());
    scene.clear(Color::TRANSPARENT);
    tree.paint_at(&mut scene, &mut renderer, now).unwrap();
    assert!(quad_at(&scene, 180.0).is_none());
}

#[test]
fn group_hover_follows_the_nearest_group_and_moves_transforms() {
    let outer = ElementId::new(130);
    let inner = ElementId::new(131);
    let member = ElementId::new(132);
    let revealed = Color::rgb8(16, 185, 129);
    let declaration = div()
        .id(outer)
        .size(200.0, 40.0)
        .flex_row()
        .group()
        .children([
            div().size(100.0, 40.0).flex_none(),
            div().id(inner).size(100.0, 40.0).flex_row().group().child(
                div()
                    .id(member)
                    .size(40.0, 40.0)
                    .flex_none()
                    .group_hover(|style| style.bg(revealed).translate(0.0, -4.0)),
            ),
        ]);
    let viewport = Size::new(200.0, 40.0);
    let now = Instant::now();
    let mut tree = UiTree::new();
    let mut renderer = TestTextLayout;
    let mut scene = Scene::new();

    tree.set_root(declaration, viewport, 1.0, &mut renderer)
        .unwrap();
    tree.paint_at(&mut scene, &mut renderer, now).unwrap();

    // Inside the outer group but outside the inner one: the member's nearest group is not
    // hovered, so nothing reveals.
    assert!(tree.pointer_moved(Point::new(50.0, 20.0), &mut renderer));
    assert!(tree.hovered.contains(&outer));
    assert!(!tree.hovered.contains(&inner));
    scene.clear(Color::TRANSPARENT);
    tree.paint_at(&mut scene, &mut renderer, now).unwrap();
    assert!(quad_at(&scene, 120.0).is_none());

    // Inside the inner group, away from the member: it reveals, lifted by its paint-only
    // translation, and its reported bounds follow the painted pixels.
    assert!(tree.pointer_moved(Point::new(170.0, 20.0), &mut renderer));
    assert!(tree.hovered.contains(&inner));
    scene.clear(Color::TRANSPARENT);
    tree.paint_at(&mut scene, &mut renderer, now).unwrap();
    let quad = quad_at(&scene, 120.0).expect("the member reveals inside its nearest group");
    assert_eq!(quad.fill, revealed);
    assert_eq!(quad.rect.y, -4.0);
    assert_eq!(
        tree.element_bounds(member),
        Some(Rect::new(100.0, -4.0, 40.0, 40.0))
    );
}

#[test]
fn named_group_hover_follows_the_named_ancestor_past_nearer_groups() {
    let sidebar = ElementId::new(140);
    let row = ElementId::new(141);
    let direct = ElementId::new(142);
    let named_member = ElementId::new(143);
    let nearest_member = ElementId::new(144);
    let stray = ElementId::new(145);
    let direct_color = Color::rgb8(14, 165, 233);
    let sidebar_color = Color::rgb8(99, 102, 241);
    let row_color = Color::rgb8(245, 158, 11);
    let declaration = div()
        .id(sidebar)
        .size(200.0, 40.0)
        .flex_row()
        .group_named("sidebar")
        .children([
            // A member naming no group follows the nearest one, even when that group is named.
            div()
                .id(direct)
                .size(80.0, 40.0)
                .flex_none()
                .group_hover(|style| style.bg(direct_color)),
            div()
                .id(row)
                .size(120.0, 40.0)
                .flex_row()
                .group()
                .children([
                    div()
                        .id(named_member)
                        .size(40.0, 40.0)
                        .flex_none()
                        .group_hover_named("sidebar", |style| style.bg(sidebar_color)),
                    div()
                        .id(nearest_member)
                        .size(40.0, 40.0)
                        .flex_none()
                        .group_hover(|style| style.bg(row_color)),
                    div()
                        .id(stray)
                        .size(40.0, 40.0)
                        .flex_none()
                        .group_hover_named("toolbar", |style| style.bg(Color::WHITE)),
                ]),
        ]);
    let viewport = Size::new(200.0, 40.0);
    let now = Instant::now();
    let mut tree = UiTree::new();
    let mut renderer = TestTextLayout;
    let mut scene = Scene::new();

    tree.set_root(declaration, viewport, 1.0, &mut renderer)
        .unwrap();
    tree.paint_at(&mut scene, &mut renderer, now).unwrap();

    // Over the sidebar's own member, outside the row: the sidebar is hovered and the row is not,
    // so the direct member and the row member naming the sidebar reveal while the row member
    // following its nearest group does not.
    assert!(tree.pointer_moved(Point::new(40.0, 20.0), &mut renderer));
    assert!(tree.hovered.contains(&sidebar));
    assert!(!tree.hovered.contains(&row));
    scene.clear(Color::TRANSPARENT);
    tree.paint_at(&mut scene, &mut renderer, now).unwrap();
    assert_eq!(fill_at(&scene, 40.0), Some(direct_color));
    assert_eq!(fill_at(&scene, 100.0), Some(sidebar_color));
    assert!(quad_at(&scene, 140.0).is_none());
    assert!(quad_at(&scene, 180.0).is_none());

    // Inside the row: both groups are hovered, every member follows its own group, and the member
    // naming a group no ancestor carries still paints nothing.
    assert!(tree.pointer_moved(Point::new(180.0, 20.0), &mut renderer));
    assert!(tree.hovered.contains(&row));
    scene.clear(Color::TRANSPARENT);
    tree.paint_at(&mut scene, &mut renderer, now).unwrap();
    assert_eq!(fill_at(&scene, 40.0), Some(direct_color));
    assert_eq!(fill_at(&scene, 100.0), Some(sidebar_color));
    assert_eq!(fill_at(&scene, 140.0), Some(row_color));
    assert!(quad_at(&scene, 180.0).is_none());
}

#[test]
fn several_group_styles_layer_in_declaration_order() {
    let sidebar = ElementId::new(150);
    let row = ElementId::new(151);
    let member = ElementId::new(152);
    let sidebar_color = Color::rgb8(99, 102, 241);
    let row_color = Color::rgb8(245, 158, 11);
    let declaration = div()
        .id(sidebar)
        .size(200.0, 40.0)
        .flex_row()
        .group_named("sidebar")
        .children([
            div().size(80.0, 40.0).flex_none(),
            div().id(row).size(120.0, 40.0).flex_row().group().child(
                // One member follows two groups: the sidebar sets a fill and a radius, the
                // nearer row only a fill.
                div()
                    .id(member)
                    .size(40.0, 40.0)
                    .flex_none()
                    .group_hover_named("sidebar", |style| style.bg(sidebar_color).rounded(6.0))
                    .group_hover(|style| style.bg(row_color)),
            ),
        ]);
    let viewport = Size::new(200.0, 40.0);
    let now = Instant::now();
    let mut tree = UiTree::new();
    let mut renderer = TestTextLayout;
    let mut scene = Scene::new();

    tree.set_root(declaration, viewport, 1.0, &mut renderer)
        .unwrap();
    tree.paint_at(&mut scene, &mut renderer, now).unwrap();

    // Only the sidebar is hovered: its entry alone paints.
    assert!(tree.pointer_moved(Point::new(40.0, 20.0), &mut renderer));
    scene.clear(Color::TRANSPARENT);
    tree.paint_at(&mut scene, &mut renderer, now).unwrap();
    let quad = quad_at(&scene, 100.0).expect("the sidebar entry paints");
    assert_eq!(quad.fill, sidebar_color);
    assert_eq!(quad.radius, Corners::all(6.0));

    // Both are hovered: the later row entry wins the fill and the sidebar's radius stays, as two
    // matching CSS rules of equal specificity cascade.
    assert!(tree.pointer_moved(Point::new(170.0, 20.0), &mut renderer));
    scene.clear(Color::TRANSPARENT);
    tree.paint_at(&mut scene, &mut renderer, now).unwrap();
    let quad = quad_at(&scene, 100.0).expect("both entries paint");
    assert_eq!(quad.fill, row_color);
    assert_eq!(quad.radius, Corners::all(6.0));
}

#[test]
fn group_active_follows_a_held_press_inside_the_group() {
    let row = ElementId::new(160);
    let trigger = ElementId::new(161);
    let member = ElementId::new(162);
    let hover_color = Color::rgb8(59, 130, 246);
    let active_color = Color::rgb8(29, 78, 216);
    let declaration = div()
        .id(row)
        .size(200.0, 40.0)
        .flex_row()
        .group()
        .children([
            button()
                .id(trigger)
                .size(80.0, 40.0)
                .flex_none()
                .clickable(),
            // `group_active` declared after `group_hover` wins over it while the press is held.
            div()
                .id(member)
                .size(40.0, 40.0)
                .flex_none()
                .group_hover(|style| style.bg(hover_color))
                .group_active(|style| style.bg(active_color)),
        ]);
    let viewport = Size::new(200.0, 40.0);
    let now = Instant::now();
    let mut tree = UiTree::new();
    let mut renderer = TestTextLayout;
    let mut scene = Scene::new();

    tree.set_root(declaration, viewport, 1.0, &mut renderer)
        .unwrap();
    tree.paint_at(&mut scene, &mut renderer, now).unwrap();

    // Hovering the trigger hovers the group.
    let on_trigger = Point::new(40.0, 20.0);
    assert!(tree.pointer_moved(on_trigger, &mut renderer));
    scene.clear(Color::TRANSPARENT);
    tree.paint_at(&mut scene, &mut renderer, now).unwrap();
    assert_eq!(fill_at(&scene, 100.0), Some(hover_color));

    // Pressing the trigger makes the group active: the press is inside it, not on it.
    let result = tree.pointer_button(Some(on_trigger), true, false, now, &mut renderer);
    assert!(result.repaint);
    assert_eq!(tree.pressed, Some(trigger));
    scene.clear(Color::TRANSPARENT);
    tree.paint_at(&mut scene, &mut renderer, now).unwrap();
    assert_eq!(fill_at(&scene, 100.0), Some(active_color));

    // Releasing ends the press and the hover entry paints again.
    tree.pointer_button(Some(on_trigger), false, false, now, &mut renderer);
    assert_eq!(tree.pressed, None);
    scene.clear(Color::TRANSPARENT);
    tree.paint_at(&mut scene, &mut renderer, now).unwrap();
    assert_eq!(fill_at(&scene, 100.0), Some(hover_color));
}

#[test]
fn focus_within_paints_the_focused_element_and_every_ancestor() {
    let left = ElementId::new(170);
    let left_button = ElementId::new(171);
    let right = ElementId::new(172);
    let right_button = ElementId::new(173);
    let left_color = Color::rgb8(14, 165, 233);
    let right_color = Color::rgb8(244, 63, 94);
    let own_color = Color::rgb8(34, 197, 94);
    let declaration = div().size(200.0, 40.0).flex_row().children([
        div()
            .id(left)
            .size(100.0, 40.0)
            .flex_row()
            .focus_within(|style| style.bg(left_color))
            .child(
                button()
                    .id(left_button)
                    .size(60.0, 40.0)
                    .flex_none()
                    .clickable(),
            ),
        div()
            .id(right)
            .size(100.0, 40.0)
            .flex_row()
            .focus_within(|style| style.bg(right_color))
            .child(
                button()
                    .id(right_button)
                    .size(60.0, 40.0)
                    .flex_none()
                    .clickable()
                    .focus_within(|style| style.bg(own_color)),
            ),
    ]);
    let viewport = Size::new(200.0, 40.0);
    let now = Instant::now();
    let mut tree = UiTree::new();
    let mut renderer = TestTextLayout;
    let mut scene = Scene::new();

    tree.set_root(declaration, viewport, 1.0, &mut renderer)
        .unwrap();
    tree.paint_at(&mut scene, &mut renderer, now).unwrap();
    assert!(quad_at(&scene, 80.0).is_none());
    assert!(quad_at(&scene, 180.0).is_none());

    // Programmatic focus keeps focus invisible, so the `focus` variant stays off while
    // `focus_within` follows the focus itself up through the ancestors.
    assert!(tree.focus(left_button));
    scene.clear(Color::TRANSPARENT);
    tree.paint_at(&mut scene, &mut renderer, now).unwrap();
    assert_eq!(fill_at(&scene, 80.0), Some(left_color));
    assert!(quad_at(&scene, 180.0).is_none());

    // Moving focus moves the highlight, and the focused element counts as within itself.
    assert!(tree.focus(right_button));
    scene.clear(Color::TRANSPARENT);
    tree.paint_at(&mut scene, &mut renderer, now).unwrap();
    assert!(quad_at(&scene, 80.0).is_none());
    assert_eq!(
        fill_of(&scene, Rect::new(100.0, 0.0, 100.0, 40.0)),
        Some(right_color)
    );
    assert_eq!(
        fill_of(&scene, Rect::new(100.0, 0.0, 60.0, 40.0)),
        Some(own_color)
    );
}

#[test]
fn selected_paints_while_the_element_is_selected_and_keeps_its_paint_under_hover() {
    let row = ElementId::new(150);
    let chosen = Color::rgb8(37, 99, 235);
    let hover = Color::rgb8(220, 38, 38);
    let declaration = |selected: bool| {
        div()
            .id(row)
            .size(200.0, 40.0)
            .clickable()
            .selected(selected)
            .selected_style(|style| style.bg(chosen))
            .hover(|style| style.bg(hover))
    };
    let viewport = Size::new(200.0, 40.0);
    let now = Instant::now();
    let mut tree = UiTree::new();
    let mut renderer = TestTextLayout;
    let mut scene = Scene::new();

    tree.set_root(declaration(false), viewport, 1.0, &mut renderer)
        .unwrap();
    tree.paint_at(&mut scene, &mut renderer, now).unwrap();
    // Neither selected nor hovered: the row paints no box at all.
    assert!(quad_at(&scene, 100.0).is_none());

    // Hovered: the ordinary hover fill.
    assert!(tree.pointer_moved(Point::new(100.0, 20.0), &mut renderer));
    scene.clear(Color::TRANSPARENT);
    tree.paint_at(&mut scene, &mut renderer, now).unwrap();
    assert_eq!(fill_at(&scene, 100.0), Some(hover));

    // Selected under the same pointer: the selected fill wins over the hover beneath it, as a
    // native list row keeps its selection colour while the pointer rests on it.
    tree.set_root(declaration(true), viewport, 1.0, &mut renderer)
        .unwrap();
    tree.pointer_moved(Point::new(100.0, 20.0), &mut renderer);
    assert!(tree.hovered.contains(&row));
    scene.clear(Color::TRANSPARENT);
    tree.paint_at(&mut scene, &mut renderer, now).unwrap();
    assert_eq!(fill_at(&scene, 100.0), Some(chosen));

    // The pointer leaves: the selection paint stays, because it follows the flag, not the pointer.
    assert!(tree.pointer_left());
    scene.clear(Color::TRANSPARENT);
    tree.paint_at(&mut scene, &mut renderer, now).unwrap();
    assert_eq!(fill_at(&scene, 100.0), Some(chosen));
}
