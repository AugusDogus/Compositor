use super::*;
use crate::{ElementUpdate, Transform2D};

struct ScopedView {
    visible: Entity<bool>,
    value: Entity<usize>,
    alternate: Entity<usize>,
    choose_alternate: Entity<bool>,
    color: Entity<Color>,
    label: Entity<Arc<str>>,
    runs: Rc<RefCell<[usize; 4]>>,
    clicked: usize,
    root_observes: bool,
}

impl Default for ScopedView {
    fn default() -> Self {
        Self {
            visible: Entity::new(true),
            value: Entity::new(1),
            alternate: Entity::new(7),
            choose_alternate: Entity::new(false),
            color: Entity::new(Color::BLACK),
            label: Entity::new(Arc::from("Before")),
            runs: Rc::new(RefCell::new([0; 4])),
            clicked: 0,
            root_observes: false,
        }
    }
}

impl View for ScopedView {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        if self.root_observes {
            cx.observe(&self.value, |_| ());
        }
        let visible = self.visible.clone();
        let value = self.value.clone();
        let alternate = self.alternate.clone();
        let choose = self.choose_alternate.clone();
        let runs = self.runs.clone();
        let panel = cx.component("panel", move |cx| {
            runs.borrow_mut()[0] += 1;
            let show = cx.observe(&visible, |show| *show);
            let mut panel = div().flex_col();
            if show {
                let value = value.clone();
                let alternate = alternate.clone();
                let choose = choose.clone();
                let runs = runs.clone();
                panel = panel.child(cx.component("child", move |cx| {
                    runs.borrow_mut()[1] += 1;
                    let chosen = if cx.observe(&choose, |value| *value) {
                        &alternate
                    } else {
                        &value
                    };
                    let value = cx.observe(chosen, |value| *value);
                    let click = cx.listener("child", move |view, _| view.clicked = value);
                    button()
                        .on_click(click)
                        .size(100.0, 30.0)
                        .child(value.to_string())
                }));
            }
            panel
        });
        let color = self.color.clone();
        let runs = self.runs.clone();
        let painted = cx.bind(div().id("painted").size(40.0, 40.0), move |cx| {
            runs.borrow_mut()[2] += 1;
            ElementUpdate::BackgroundColor {
                id: "painted".into(),
                color: cx.observe(&color, |color| *color),
            }
        });
        let label = self.label.clone();
        let runs = self.runs.clone();
        let label = cx.bind(text("").id("label"), move |cx| {
            runs.borrow_mut()[3] += 1;
            ElementUpdate::Text {
                id: "label".into(),
                content: cx.observe(&label, Clone::clone),
            }
        });
        let sibling = cx.listener("sibling", |view, _| view.clicked = 99);
        div()
            .flex_col()
            .child(panel)
            .child(painted)
            .child(label)
            .child(button().id("sibling").on_click(sibling).child("Sibling"))
    }
}

#[test]
fn scoped_entities_restart_only_the_observing_component_and_keep_sibling_listeners() {
    let (mut cx, view) = TestAppContext::new(ScopedView::default()).unwrap();
    let window = view.window_handle();
    let runs = cx.read(view, |view| view.runs.clone()).unwrap();
    assert_eq!(*runs.borrow(), [1, 1, 1, 1]);
    let sibling = cx.window(window).unwrap().listeners.clicks[&"sibling".into()].clone();
    cx.window_mut(window).unwrap().ui.take_work();
    cx.update(view, |view, cx| {
        view.value.update(cx, |value, _| *value = 3);
    })
    .unwrap();
    assert_eq!(cx.render_count(window).unwrap(), 1);
    assert_eq!(*runs.borrow(), [1, 2, 1, 1]);
    assert!(Arc::ptr_eq(
        &sibling,
        &cx.window(window).unwrap().listeners.clicks[&"sibling".into()]
    ));
    assert_eq!(
        cx.window_mut(window)
            .unwrap()
            .ui
            .take_work()
            .reconciled_nodes,
        2
    );
    cx.click(window, "child").unwrap();
    assert_eq!(cx.read(view, |view| view.clicked).unwrap(), 3);
    cx.click(window, "sibling").unwrap();
    assert_eq!(cx.read(view, |view| view.clicked).unwrap(), 99);
}

#[test]
fn value_bindings_separate_paint_from_intrinsic_layout() {
    let (mut cx, view) = TestAppContext::new(ScopedView::default()).unwrap();
    let window = view.window_handle();
    cx.element_bounds(window, "painted").unwrap();
    cx.window_mut(window).unwrap().ui.take_work();
    cx.update(view, |view, cx| {
        view.color.update(cx, |color, _| *color = Color::WHITE);
    })
    .unwrap();
    let work = cx.window_mut(window).unwrap().ui.take_work();
    assert_eq!(
        (
            work.reconciled_nodes,
            work.layout_passes,
            work.geometry_nodes
        ),
        (0, 0, 0)
    );
    cx.update(view, |view, cx| {
        view.label
            .update(cx, |label, _| *label = Arc::from("A longer label 🌍"));
    })
    .unwrap();
    let work = cx.window_mut(window).unwrap().ui.take_work();
    assert_eq!(work.reconciled_nodes, 0);
    assert!(work.layout_passes > 0);
    assert_eq!(cx.render_count(window).unwrap(), 1);
    assert_eq!(
        cx.read(view, |view| *view.runs.borrow()).unwrap(),
        [1, 1, 2, 2]
    );
    assert!(
        cx.accessibility_update(window)
            .unwrap()
            .nodes
            .iter()
            .any(|(_, node)| node.value() == Some("A longer label 🌍"))
    );
}

#[test]
fn conditional_dependencies_are_replaced_and_unmounted_scopes_are_disposed() {
    let (mut cx, view) = TestAppContext::new(ScopedView::default()).unwrap();
    let window = view.window_handle();
    cx.update(view, |view, cx| {
        view.choose_alternate.update(cx, |choose, _| *choose = true);
    })
    .unwrap();
    let counts = cx.read(view, |view| *view.runs.borrow()).unwrap();
    cx.update(view, |view, cx| {
        view.value.update(cx, |value, _| *value += 1);
    })
    .unwrap();
    assert_eq!(cx.read(view, |view| *view.runs.borrow()).unwrap(), counts);
    cx.update(view, |view, cx| {
        view.alternate.update(cx, |value, _| *value = 8);
    })
    .unwrap();
    assert!(
        cx.accessibility_update(window)
            .unwrap()
            .nodes
            .iter()
            .any(|(_, node)| node.label() == Some("8"))
    );
    cx.update(view, |view, cx| {
        view.visible.update(cx, |show, _| *show = false);
    })
    .unwrap();
    assert!(
        !cx.window(window)
            .unwrap()
            .listeners
            .clicks
            .contains_key(&"child".into())
    );
    let counts = cx.read(view, |view| *view.runs.borrow()).unwrap();
    cx.update(view, |view, cx| {
        view.alternate.update(cx, |value, _| *value += 1);
    })
    .unwrap();
    assert_eq!(cx.read(view, |view| *view.runs.borrow()).unwrap(), counts);
    assert_eq!(cx.render_count(window).unwrap(), 1);
}

#[test]
fn dirty_parents_coalesce_descendants_and_root_observation_keeps_full_render_semantics() {
    let (mut cx, view) = TestAppContext::new(ScopedView::default()).unwrap();
    let window = view.window_handle();
    cx.update(view, |view, cx| {
        cx.notify(&view.visible);
        view.value.update(cx, |value, _| *value = 2);
    })
    .unwrap();
    assert_eq!(
        cx.read(view, |view| *view.runs.borrow()).unwrap(),
        [2, 2, 1, 1]
    );
    assert_eq!(cx.render_count(window).unwrap(), 1);
    cx.update(view, |view, cx| {
        view.root_observes = true;
        cx.invalidate();
    })
    .unwrap();
    let before = cx.render_count(window).unwrap();
    cx.update(view, |view, cx| {
        view.value.update(cx, |value, _| *value = 4);
    })
    .unwrap();
    assert_eq!(cx.render_count(window).unwrap(), before + 1);
}

#[test]
fn retained_transform_updates_move_hits_without_layout() {
    let (mut cx, view) = TestAppContext::new(ScopedView::default()).unwrap();
    let window = view.window_handle();
    cx.prepare_retained_geometry(window).unwrap();
    let before = cx
        .window(window)
        .unwrap()
        .ui
        .element_bounds("child".into())
        .unwrap();
    cx.window_mut(window).unwrap().ui.take_work();
    assert!(
        cx.update_elements(
            window,
            &[ElementUpdate::Transform {
                id: "child".into(),
                transform: Transform2D::translate(15.0, 0.0),
            }]
        )
        .unwrap()
    );
    let after = cx
        .window(window)
        .unwrap()
        .ui
        .element_bounds("child".into())
        .unwrap();
    assert_eq!(after.x, before.x + 15.0);
    assert_eq!(
        cx.window_mut(window).unwrap().ui.take_work().layout_passes,
        0
    );
}

struct ScopedGlobal(usize);
impl Global for ScopedGlobal {}
struct GlobalScopeView(Rc<RefCell<usize>>);
impl View for GlobalScopeView {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let runs = self.0.clone();
        cx.component("global-scope", move |cx| {
            *runs.borrow_mut() += 1;
            text(cx.watch_global::<ScopedGlobal, _>(|value| value.0.to_string()))
        })
    }
}

#[test]
fn scoped_global_notifications_do_not_render_the_root() {
    let runs = Rc::new(RefCell::new(0));
    let (mut cx, view) = Application::new()
        .global(ScopedGlobal(1))
        .into_test_context(WindowOptions::default(), GlobalScopeView(runs.clone()))
        .unwrap();
    cx.update_global::<ScopedGlobal, _>(|value, _| value.0 = 2)
        .unwrap();
    assert_eq!(*runs.borrow(), 2);
    assert_eq!(cx.render_count(view.window_handle()).unwrap(), 1);
}

struct Producer;
struct Ping;
impl EventEmitter<Ping> for Producer {}
struct SubscriptionScopeView {
    visible: Entity<bool>,
    producer: Entity<Producer>,
    deliveries: usize,
}
impl View for SubscriptionScopeView {
    fn render(&mut self, cx: &mut ViewContext<'_, Self>) -> impl IntoElement {
        let visible = self.visible.clone();
        let producer = self.producer.clone();
        cx.component("subscription-scope", move |cx| {
            if cx.observe(&visible, |show| *show) {
                let producer = producer.clone();
                return div().child(cx.component("subscriber", move |cx| {
                    cx.subscribe(&producer, |view, _, _: &Ping, _| view.deliveries += 1)
                        .detach();
                    div()
                }));
            }
            div()
        })
    }
}

#[test]
fn detached_component_subscriptions_stop_after_unmount() {
    let (mut cx, view) = TestAppContext::new(SubscriptionScopeView {
        visible: Entity::new(true),
        producer: Entity::new(Producer),
        deliveries: 0,
    })
    .unwrap();
    cx.update(view, |view, cx| {
        view.producer.emit(cx, Ping);
    })
    .unwrap();
    assert_eq!(cx.read(view, |view| view.deliveries).unwrap(), 1);
    cx.update(view, |view, cx| {
        view.visible.update(cx, |show, _| *show = false);
    })
    .unwrap();
    cx.update(view, |view, cx| {
        view.producer.emit(cx, Ping);
    })
    .unwrap();
    assert_eq!(cx.read(view, |view| view.deliveries).unwrap(), 1);
    assert_eq!(
        cx.window(view.window_handle())
            .unwrap()
            .listeners
            .entity_subscription_count,
        0
    );
}

#[test]
fn external_structural_updates_require_the_scope_declaration_context() {
    let (mut cx, view) = TestAppContext::new(ScopedView::default()).unwrap();
    let window = view.window_handle();
    assert!(
        !cx.update_elements(
            window,
            &[ElementUpdate::Replace {
                id: "panel".into(),
                element: Box::new(div().id("panel")),
            }]
        )
        .unwrap()
    );
    assert!(cx.contains_element(window, "child").unwrap());
    assert_eq!(cx.render_count(window).unwrap(), 1);
}
